//! Windows: family DNS on every active adapter with DNS-over-HTTPS (no plain fallback),
//! the hosts-file blocklist, Chrome/Edge/Brave policies in the registry, Firefox's
//! policies.json, and a SYSTEM task that starts Thurea with Windows.

use super::{file_check, hosts_check, merge_hosts, run, write_if_changed};
use thurea_core::status::dns_verdict;
use thurea_core::{Check, Policy, Report, State};
use std::path::{Path, PathBuf};

pub const PLATFORM: &str = "windows";
const TASK: &str = "Thurea";

/// HKLM policy keys of the Chromium browsers.
const CHROMIUM_KEYS: &[(&str, &str)] = &[
    ("Chrome", r"SOFTWARE\Policies\Google\Chrome"),
    ("Edge", r"SOFTWARE\Policies\Microsoft\Edge"),
    ("Brave", r"SOFTWARE\Policies\BraveSoftware\Brave"),
];

fn system_root() -> PathBuf {
    std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| r"C:\Windows".into())
}
fn hosts() -> PathBuf {
    system_root().join(r"System32\drivers\etc\hosts")
}
fn install_dir() -> PathBuf {
    std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| r"C:\Program Files".into()).join("Thurea")
}
fn firefox_policy_files() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|p| PathBuf::from(p).join("Mozilla Firefox"))
        .filter(|d| d.is_dir())
        .map(|d| d.join("distribution").join("policies.json"))
        .collect()
}

pub fn is_admin() -> bool {
    run("net", &["session"]).is_ok()
}

/// Only SYSTEM and Administrators may read the household key.
pub fn lock_down_private_dir(dir: &Path) -> Result<(), String> {
    let d = dir.to_string_lossy();
    run("icacls", &[&d, "/inheritance:r", "/grant:r", "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F"]).map(|_| ())
}
pub fn lock_down_private_file(_f: &Path) -> Result<(), String> {
    Ok(()) // inherits the locked-down directory's ACL
}

fn powershell(script: &str) -> Result<String, String> {
    run("powershell.exe", &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
}

/// (adapter, servers) for every connected adapter.
fn adapter_dns() -> Result<Vec<(String, Vec<String>)>, String> {
    let out = powershell(
        "Get-NetAdapter | Where-Object Status -eq 'Up' | ForEach-Object { $a=$_; \
         $s=(Get-DnsClientServerAddress -InterfaceIndex $a.ifIndex -ErrorAction SilentlyContinue | ForEach-Object ServerAddresses) -join ','; \
         \"$($a.Name)|$s\" }",
    )?;
    Ok(out
        .lines()
        .filter_map(|l| l.trim().split_once('|'))
        .map(|(n, s)| (n.to_string(), s.split(',').filter(|x| !x.is_empty()).map(String::from).collect()))
        .collect())
}

fn apply_dns(p: &Policy) -> Result<(), String> {
    let all: Vec<String> = p.family_dns.servers.iter().map(|s| format!("'{s}'")).collect();
    let script = format!(
        "$s=@({all}); $t='{doh}'; \
         foreach($ip in $s){{ if(Get-Command Add-DnsClientDohServerAddress -ErrorAction SilentlyContinue){{ \
           if(Get-DnsClientDohServerAddress -ServerAddress $ip -ErrorAction SilentlyContinue){{ Set-DnsClientDohServerAddress -ServerAddress $ip -DohTemplate $t -AllowFallbackToUdp $false -AutoUpgrade $true | Out-Null }} \
           else {{ Add-DnsClientDohServerAddress -ServerAddress $ip -DohTemplate $t -AllowFallbackToUdp $false -AutoUpgrade $true | Out-Null }} }} }}; \
         Get-NetAdapter | Where-Object Status -eq 'Up' | ForEach-Object {{ Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ServerAddresses $s }}; \
         Clear-DnsClientCache",
        all = all.join(","),
        doh = p.doh_template()
    );
    powershell(&script).map(|_| ())
}

fn dns_check(p: &Policy) -> Check {
    let adapters = match adapter_dns() {
        Ok(a) => a,
        Err(e) => return Check::unknown("dns", format!("Can't read DNS settings: {e}")),
    };
    if adapters.is_empty() {
        return Check::unknown("dns", "No connected network adapters");
    }
    for (name, servers) in &adapters {
        match dns_verdict(servers, &p.family_dns.servers) {
            (State::Ok, _) => {}
            (State::Unknown, _) => return Check::degraded("dns", format!("{name} has no DNS set (uses the router's)")),
            (_, bad) => return Check::degraded("dns", format!("{name} uses DNS {}, which isn't filtered", bad.unwrap_or_default())),
        }
    }
    Check::ok("dns", "Family DNS locked on every adapter (encrypted)")
}

/// A .reg file for one browser's policy key.
fn reg_file(key: &str, p: &Policy) -> String {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut s = format!("Windows Registry Editor Version 5.00\r\n\r\n[-HKEY_LOCAL_MACHINE\\{key}\\URLBlocklist]\r\n\r\n[HKEY_LOCAL_MACHINE\\{key}]\r\n");
    let pol = p.chromium_policy();
    for (k, v) in pol.as_object().unwrap() {
        match v {
            serde_json::Value::String(x) => s.push_str(&format!("\"{k}\"=\"{}\"\r\n", esc(x))),
            serde_json::Value::Bool(b) => s.push_str(&format!("\"{k}\"=dword:{:08x}\r\n", *b as u32)),
            serde_json::Value::Number(n) => s.push_str(&format!("\"{k}\"=dword:{:08x}\r\n", n.as_u64().unwrap_or(0))),
            _ => {}
        }
    }
    s.push_str(&format!("\r\n[HKEY_LOCAL_MACHINE\\{key}\\URLBlocklist]\r\n"));
    for (i, d) in p.blocklist.iter().enumerate() {
        s.push_str(&format!("\"{}\"=\"{}\"\r\n", i + 1, esc(d)));
    }
    s
}

fn utf16(s: &str) -> Vec<u8> {
    let mut b = vec![0xFF, 0xFE];
    for u in s.encode_utf16() {
        b.extend_from_slice(&u.to_le_bytes());
    }
    b
}

/// Registry state of a browser key that we compare against (values Thurea sets).
fn chromium_ok(key: &str, p: &Policy) -> bool {
    let full = format!(r"HKLM\{key}");
    let Ok(main) = run("reg", &["query", &full]) else { return false };
    let doh = main.lines().any(|l| l.contains("DnsOverHttpsTemplates") && l.contains(&p.doh_template()));
    let n = run("reg", &["query", &format!(r"{full}\URLBlocklist")]).map(|o| o.lines().filter(|l| l.contains("REG_SZ")).count()).unwrap_or(0);
    doh && n == p.blocklist.len()
}

pub fn apply(p: &Policy) -> Report {
    let mut r = Report::default();

    if dns_check(p).state != State::Ok {
        if let Err(e) = apply_dns(p) {
            r.push(Check::degraded("dns", format!("Can't set family DNS: {e}")));
        }
    }

    let h = hosts();
    if let Ok(cur) = std::fs::read_to_string(&h) {
        let _ = write_if_changed(&h, merge_hosts(&cur, Some(p), "\r\n").as_bytes());
    }

    for (_, key) in CHROMIUM_KEYS {
        if !chromium_ok(key, p) {
            let f = std::env::temp_dir().join("thurea-policy.reg");
            if std::fs::write(&f, utf16(&reg_file(key, p))).is_ok() {
                let _ = run("reg", &["import", &f.to_string_lossy()]);
            }
            let _ = std::fs::remove_file(f);
        }
    }

    let ff = serde_json::to_vec_pretty(&p.firefox_policy()).unwrap();
    for f in firefox_policy_files() {
        let _ = write_if_changed(&f, &ff);
    }

    let mut checked = check(p);
    checked.checks.splice(0..0, r.checks);
    checked
}

pub fn check(p: &Policy) -> Report {
    let mut r = Report::default();
    r.push(dns_check(p));
    r.push(hosts_check(&hosts(), p, "\r\n"));
    let bad: Vec<&str> = CHROMIUM_KEYS.iter().filter(|(_, k)| !chromium_ok(k, p)).map(|(n, _)| *n).collect();
    r.push(if bad.is_empty() {
        Check::ok("chromium", "Chrome, Edge, Brave locked: family DNS, SafeSearch, no private windows")
    } else {
        Check::degraded("chromium", format!("Browser policy missing or changed: {}", bad.join(", ")))
    });
    let ff = serde_json::to_vec_pretty(&p.firefox_policy()).unwrap();
    for f in firefox_policy_files() {
        r.push(file_check("firefox", "Firefox", &f, &ff));
    }
    r.push(match run("schtasks", &["/Query", "/TN", TASK]) {
        Ok(_) => Check::ok("service", "Thurea starts with Windows"),
        Err(_) => Check::degraded("service", "Thurea is not set to start with Windows (run `thurea install`)"),
    });
    r
}

pub fn install() -> Result<String, String> {
    let dir = install_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let bin = dir.join("thurea.exe");
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if me != bin {
        // A running copy can't be overwritten: stop it first.
        let _ = run("schtasks", &["/End", "/TN", TASK]);
        std::fs::copy(&me, &bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    }
    let tr = format!("\"{}\" run", bin.display());
    run("schtasks", &["/Create", "/F", "/TN", TASK, "/SC", "ONSTART", "/RU", "SYSTEM", "/RL", "HIGHEST", "/TR", &tr])?;
    // Restart if it stops, and keep running on battery.
    let _ = powershell(&format!(
        "$s=New-ScheduledTaskSettingsSet -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit 0; Set-ScheduledTask -TaskName '{TASK}' -Settings $s | Out-Null"
    ));
    let _ = run("netsh", &["advfirewall", "firewall", "add", "rule", "name=Thurea mesh", "dir=in", "action=allow", "protocol=UDP", "localport=47823"]);
    run("schtasks", &["/Run", "/TN", TASK])?;
    Ok(format!("Installed {} and the Thurea startup task", bin.display()))
}

pub fn uninstall() -> Result<String, String> {
    let _ = run("schtasks", &["/End", "/TN", TASK]);
    let _ = run("schtasks", &["/Delete", "/F", "/TN", TASK]);
    for (_, key) in CHROMIUM_KEYS {
        let _ = run("reg", &["delete", &format!(r"HKLM\{key}"), "/f"]);
    }
    for f in firefox_policy_files() {
        let _ = std::fs::remove_file(f);
    }
    let h = hosts();
    if let Ok(cur) = std::fs::read_to_string(&h) {
        let _ = std::fs::write(&h, merge_hosts(&cur, None, "\r\n"));
    }
    let _ = powershell("Get-NetAdapter | Where-Object Status -eq 'Up' | ForEach-Object { Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ResetServerAddresses }");
    let _ = run("netsh", &["advfirewall", "firewall", "delete", "rule", "name=Thurea mesh"]);
    Ok("Thurea's rules were removed. Config is kept in ProgramData\\Thurea.".into())
}

/// A message box on every signed-in session (msg.exe; not present on Windows Home).
pub fn notify(text: &str, _urgent: bool) {
    eprintln!("thurea: {text}");
    let _ = std::process::Command::new("msg").args(["*", "/TIME:120", &format!("Thurea: {text}")]).status();
}
