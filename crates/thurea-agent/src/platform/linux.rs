//! Linux: family DNS over TLS through systemd-resolved (or a managed resolv.conf), the
//! hosts-file blocklist, managed policies for Chrome/Chromium/Brave/Edge/Firefox, and a
//! systemd service that keeps it all in place.

use super::{file_check, hosts_check, merge_hosts, run, write_if_changed};
use thurea_core::status::dns_verdict;
use thurea_core::{Check, Policy, Report, State};
use std::path::{Path, PathBuf};

pub const PLATFORM: &str = "linux";

const HOSTS: &str = "/etc/hosts";
const RESOLVED_DROPIN: &str = "/etc/systemd/resolved.conf.d/90-thurea.conf";
const NM_DROPIN: &str = "/etc/NetworkManager/conf.d/90-thurea.conf";
const RESOLV_CONF: &str = "/etc/resolv.conf";
const UNIT: &str = "/etc/systemd/system/thurea.service";
const BIN: &str = "/usr/local/bin/thurea";

/// Chromium-family managed policy directories (written whether or not the browser is
/// installed, so a browser installed later is locked from its first start).
const CHROMIUM_DIRS: &[(&str, &str)] = &[
    ("chrome", "/etc/opt/chrome/policies/managed"),
    ("chromium", "/etc/chromium/policies/managed"),
    ("chromium-opt", "/etc/opt/chromium/policies/managed"),
    ("brave", "/etc/brave/policies/managed"),
    ("edge", "/etc/opt/edge/policies/managed"),
    ("vivaldi", "/etc/opt/vivaldi/policies/managed"),
];
const FIREFOX: &str = "/etc/firefox/policies/policies.json";

fn parent_pid() -> Option<u32> {
    // /proc/self/stat: "pid (comm) state ppid ..."; comm may contain spaces.
    let s = std::fs::read_to_string("/proc/self/stat").ok()?;
    s.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

pub fn is_admin() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("Uid:")).map(|l| l.split_whitespace().nth(2) == Some("0")))
        .unwrap_or(false)
}

pub fn lock_down_private_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| format!("{}: {e}", dir.display()))
}

pub fn lock_down_private_file(f: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o600)).map_err(|e| format!("{}: {e}", f.display()))
}

fn resolved_active() -> bool {
    run("systemctl", &["is-active", "--quiet", "systemd-resolved"]).is_ok()
}

fn resolved_dropin(p: &Policy) -> String {
    let servers: Vec<String> = p.family_dns.servers.iter().map(|s| format!("{s}#{}", p.family_dns.host)).collect();
    format!(
        "# Managed by Thurea. Family DNS over TLS for the whole computer.\n[Resolve]\nDNS={}\nFallbackDNS=\nDNSOverTLS=yes\nDomains=~.\n",
        servers.join(" ")
    )
}

fn resolv_conf(p: &Policy) -> String {
    let mut s = String::from("# Managed by Thurea.\n");
    for ip in &p.family_dns.servers {
        s.push_str(&format!("nameserver {ip}\n"));
    }
    s
}

fn chromium_json(p: &Policy) -> Vec<u8> {
    serde_json::to_vec_pretty(&p.chromium_policy()).unwrap()
}

fn firefox_json(p: &Policy) -> Vec<u8> {
    serde_json::to_vec_pretty(&p.firefox_policy()).unwrap()
}

fn chromium_files() -> impl Iterator<Item = (&'static str, PathBuf)> {
    CHROMIUM_DIRS.iter().map(|(n, d)| (*n, Path::new(d).join("thurea.json")))
}

pub fn apply(p: &Policy) -> Report {
    let mut r = Report::default();

    // 1. DNS.
    if resolved_active() {
        match write_if_changed(Path::new(RESOLVED_DROPIN), resolved_dropin(p).as_bytes()) {
            Ok(changed) => {
                // NetworkManager would otherwise hand each Wi-Fi's own DNS to resolved.
                if Path::new("/etc/NetworkManager").is_dir()
                    && write_if_changed(Path::new(NM_DROPIN), b"# Managed by Thurea.\n[main]\ndns=none\n").unwrap_or(false)
                {
                    let _ = run("systemctl", &["reload", "NetworkManager"]);
                }
                if changed {
                    let _ = run("systemctl", &["restart", "systemd-resolved"]);
                }
                // Drop per-link servers that were already pushed (Wi-Fi DHCP etc.).
                for link in links_with_foreign_dns(p) {
                    let _ = run("resolvectl", &["revert", &link]);
                }
            }
            Err(e) => r.push(Check::degraded("dns", format!("Can't set family DNS: {e}"))),
        }
    } else {
        let is_link = std::fs::symlink_metadata(RESOLV_CONF).map(|m| m.file_type().is_symlink()).unwrap_or(false);
        if is_link {
            let _ = std::fs::remove_file(RESOLV_CONF);
        }
        if let Err(e) = write_if_changed(Path::new(RESOLV_CONF), resolv_conf(p).as_bytes()) {
            r.push(Check::degraded("dns", format!("Can't set family DNS: {e}")));
        }
    }

    // 2. Hosts file.
    if let Ok(cur) = std::fs::read_to_string(HOSTS) {
        let _ = write_if_changed(Path::new(HOSTS), merge_hosts(&cur, Some(p), "\n").as_bytes());
    }

    // 3. Browsers.
    let cj = chromium_json(p);
    for (_, f) in chromium_files() {
        let _ = write_if_changed(&f, &cj);
    }
    let _ = write_if_changed(Path::new(FIREFOX), &firefox_json(p));

    // Report what is now actually in force (write errors show up here too).
    let mut checked = check(p);
    checked.checks.splice(0..0, r.checks);
    checked
}

/// `resolvectl dns` lines: "Global: 1.1.1.3#family.cloudflare-dns.com ..." and
/// "Link 3 (wlan0): 192.168.1.1". Returns (scope, servers).
fn resolvectl_dns() -> Option<Vec<(String, Vec<String>)>> {
    let out = run("resolvectl", &["dns"]).ok()?;
    Some(
        out.lines()
            .filter_map(|l| {
                let (scope, rest) = l.split_once("):").map(|(a, b)| (format!("{a})"), b)).or_else(|| l.split_once(':').map(|(a, b)| (a.to_string(), b)))?;
                let servers = rest.split_whitespace().map(|s| s.split('#').next().unwrap_or(s).to_string()).collect();
                Some((scope.trim().to_string(), servers))
            })
            .collect(),
    )
}

fn links_with_foreign_dns(p: &Policy) -> Vec<String> {
    resolvectl_dns()
        .unwrap_or_default()
        .into_iter()
        .filter(|(scope, servers)| scope.starts_with("Link") && dns_verdict(servers, &p.family_dns.servers).0 == State::Degraded)
        .filter_map(|(scope, _)| scope.split_once('(').map(|(_, n)| n.trim_end_matches(')').to_string()))
        .collect()
}

fn dns_check(p: &Policy) -> Check {
    if resolved_active() {
        let Some(scopes) = resolvectl_dns() else { return Check::unknown("dns", "Can't read DNS settings (resolvectl failed)") };
        let global = scopes.iter().find(|(s, _)| s == "Global").map(|(_, v)| v.clone()).unwrap_or_default();
        match dns_verdict(&global, &p.family_dns.servers) {
            (State::Ok, _) => {}
            (State::Unknown, _) => return Check::degraded("dns", "Family DNS is not set"),
            (_, bad) => return Check::degraded("dns", format!("DNS uses {}, which isn't filtered", bad.unwrap_or_default())),
        }
        for (scope, servers) in &scopes {
            if let (State::Degraded, Some(bad)) = dns_verdict(servers, &p.family_dns.servers) {
                if scope != "Global" {
                    return Check::degraded("dns", format!("{scope} uses DNS {bad}, which isn't filtered"));
                }
            }
        }
        let dot = std::fs::read_to_string(RESOLVED_DROPIN).map(|s| s.contains("DNSOverTLS=yes")).unwrap_or(false);
        if dot {
            Check::ok("dns", "Family DNS locked on (encrypted)")
        } else {
            Check::degraded("dns", "Family DNS is set but not encrypted")
        }
    } else {
        let servers: Vec<String> = std::fs::read_to_string(RESOLV_CONF)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().strip_prefix("nameserver").map(|s| s.trim().to_string()))
            .collect();
        match dns_verdict(&servers, &p.family_dns.servers) {
            (State::Ok, _) => Check::ok("dns", "Family DNS locked on"),
            (State::Unknown, _) => Check::unknown("dns", "No DNS servers found in /etc/resolv.conf"),
            (_, bad) => Check::degraded("dns", format!("DNS uses {}, which isn't filtered", bad.unwrap_or_default())),
        }
    }
}

pub fn check(p: &Policy) -> Report {
    let mut r = Report::default();
    r.push(dns_check(p));
    r.push(hosts_check(Path::new(HOSTS), p, "\n"));
    let cj = chromium_json(p);
    let bad: Vec<&str> = chromium_files().filter(|(_, f)| std::fs::read(f).ok().as_deref() != Some(&cj[..])).map(|(n, _)| n).collect();
    r.push(if bad.is_empty() {
        Check::ok("chromium", "Chrome, Chromium, Brave, Edge, Vivaldi locked: family DNS, SafeSearch, no private windows")
    } else {
        Check::degraded("chromium", format!("Browser policy missing or changed: {}", bad.join(", ")))
    });
    r.push(file_check("firefox", "Firefox", Path::new(FIREFOX), &firefox_json(p)));
    r.push(if Path::new("/run/systemd/system").is_dir() {
        match run("systemctl", &["is-enabled", "thurea"]) {
            Ok(s) if s.trim() == "enabled" => Check::ok("service", "Thurea starts with the computer"),
            _ => Check::degraded("service", "Thurea is not set to start with the computer (run `thurea install`)"),
        }
    } else if parent_pid() == Some(1) {
        // No systemd (runit, OpenRC, a minimal VM): the init system runs Thurea directly.
        Check::ok("service", "Thurea is run by the system's init")
    } else {
        Check::unknown("service", "No systemd here: make sure your init system starts `thurea run`")
    });
    r
}

pub fn install() -> Result<String, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if me != Path::new(BIN) {
        std::fs::copy(&me, BIN).map_err(|e| format!("{BIN}: {e}"))?;
    }
    let unit = format!(
        "[Unit]\nDescription=Thurea family protection\nAfter=network-online.target\nWants=network-online.target\n\n\
         [Service]\nExecStart={BIN} run\nRestart=always\nRestartSec=5\n\n[Install]\nWantedBy=multi-user.target\n"
    );
    write_if_changed(Path::new(UNIT), unit.as_bytes()).map_err(|e| e.to_string())?;
    run("systemctl", &["daemon-reload"])?;
    run("systemctl", &["enable", "--now", "thurea"])?;
    let _ = run("systemctl", &["restart", "thurea"]);
    // Mesh traffic: open the port if a firewall is managing this machine.
    if run("firewall-cmd", &["--state"]).is_ok() {
        let _ = run("firewall-cmd", &["--permanent", "--add-port=47823/udp"]);
        let _ = run("firewall-cmd", &["--reload"]);
    } else if run("ufw", &["status"]).map(|s| s.contains("active")).unwrap_or(false) {
        let _ = run("ufw", &["allow", "47823/udp"]);
    }
    Ok(format!("Installed {BIN} and enabled thurea.service"))
}

/// Removes every rule Thurea set, so the computer can be recovered.
pub fn uninstall() -> Result<String, String> {
    let _ = run("systemctl", &["disable", "--now", "thurea"]);
    for f in [UNIT, RESOLVED_DROPIN, NM_DROPIN, FIREFOX] {
        let _ = std::fs::remove_file(f);
    }
    for (_, f) in chromium_files() {
        let _ = std::fs::remove_file(f);
    }
    if let Ok(cur) = std::fs::read_to_string(HOSTS) {
        let _ = std::fs::write(HOSTS, merge_hosts(&cur, None, "\n"));
    }
    if std::fs::read_to_string(RESOLV_CONF).map(|s| s.starts_with("# Managed by Thurea")).unwrap_or(false) {
        let _ = std::fs::remove_file(RESOLV_CONF);
        let _ = std::os::unix::fs::symlink("/run/systemd/resolve/stub-resolv.conf", RESOLV_CONF);
    }
    let _ = run("systemctl", &["daemon-reload"]);
    let _ = run("systemctl", &["restart", "systemd-resolved"]);
    let _ = run("systemctl", &["reload", "NetworkManager"]);
    Ok("Thurea's rules were removed. Config is kept in /etc/thurea.".into())
}

/// Desktop notification for every signed-in user (the daemon runs as root, so it reaches
/// each user's session bus directly).
pub fn notify(text: &str, urgent: bool) {
    eprintln!("thurea: {text}");
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    let Ok(dirs) = std::fs::read_dir("/run/user") else { return };
    for d in dirs.flatten() {
        let uid = d.file_name().to_string_lossy().into_owned();
        if !d.path().join("bus").exists() {
            continue;
        }
        let Some(user) = passwd.lines().map(|l| l.split(':').collect::<Vec<_>>()).find(|f| f.get(2) == Some(&uid.as_str())).map(|f| f[0].to_string()) else { continue };
        let bus = format!("DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{uid}/bus");
        let _ = std::process::Command::new("runuser")
            .args(["-u", &user, "--", "env", &bus, "notify-send", "-a", "Thurea", "-u", if urgent { "critical" } else { "normal" }, "Thurea", text])
            .status();
    }
}

/// The submit folder: anyone may drop files (sticky, world-writable), nobody can list them.
pub fn open_submit_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o1733)).map_err(|e| e.to_string())
}

/// After joining a household: (re)start the package's mesh service, if Thurea is installed.
pub fn start_mesh_service() -> String {
    if Path::new("/usr/lib/systemd/system/thurea-mesh.service").exists() && run("systemctl", &["restart", "thurea-mesh.service"]).is_ok() {
        "Thurea's household service is running.".into()
    } else {
        "Next: sudo thurea install (runs Thurea at startup)".into()
    }
}

// ── Children's accounts (Thurea family) ──

/// People's accounts on this computer.
pub fn accounts() -> Vec<String> {
    thurea_core::desktop::parse_passwd(&std::fs::read_to_string("/etc/passwd").unwrap_or_default())
}

/// Login sessions and whether each is in use right now.
pub fn sessions() -> Vec<thurea_core::desktop::Session> {
    let list = run("loginctl", &["list-sessions", "--no-legend"]).unwrap_or_default();
    thurea_core::desktop::parse_loginctl_sessions(&list)
        .into_iter()
        .map(|(id, user)| {
            let show = run("loginctl", &["show-session", &id, "-p", "Active", "-p", "LockedHint", "-p", "IdleHint", "-p", "Class"]).unwrap_or_default();
            thurea_core::desktop::Session { id, user: user.to_ascii_lowercase(), active: thurea_core::desktop::parse_loginctl_show(&show) }
        })
        .collect()
}

/// Programs a user is running: (pid, program path).
pub fn running(user: &str) -> Vec<(String, String)> {
    run("ps", &["-u", user, "-o", "pid=,args="])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let (pid, args) = l.split_once(' ')?;
            Some((pid.to_string(), args.trim().split_whitespace().next()?.to_string()))
        })
        .collect()
}

/// Closes a user's program (every process of it).
pub fn close(user: &str, program: &str) {
    for (pid, p) in running(user) {
        if p == program {
            let _ = run("kill", &["-TERM", &pid]);
        }
    }
}

pub fn lock_session(id: &str) {
    let _ = run("loginctl", &["lock-session", id]);
}

/// An account that can become root can't be held by Thurea.
pub fn is_admin_account(user: &str) -> bool {
    run("id", &["-nG", user]).unwrap_or_default().split_whitespace().any(|g| matches!(g, "wheel" | "sudo" | "admin" | "root"))
}

/// Local day and minute ("YYYY-MM-DD", minutes after midnight), in this computer's time zone.
pub fn local_clock() -> Option<(String, u16)> {
    let out = run("date", &["+%F %H %M"]).ok()?;
    let f: Vec<&str> = out.split_whitespace().collect();
    Some((f.first()?.to_string(), f.get(1)?.parse::<u16>().ok()? * 60 + f.get(2)?.parse::<u16>().ok()?))
}
