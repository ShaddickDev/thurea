//! `thurea setup-phone`: set up a new (or factory-reset) Android phone with one QR scan.
//!
//! The computer shows a QR code for Android's own setup ("tap the welcome screen 6 times").
//! It tells the phone to:
//!   1. download Thurea from this computer (a one-time link on the local network);
//!   2. install it only if the file's SHA-256 matches, so a swapped file is refused;
//!   3. make Thurea the device owner, so protection is on before the home screen appears;
//!   4. join this household (key + this computer's address, in the admin extras).
//! The command then waits until the phone checks in on the household mesh.
//!
//! The QR holds the household key: it's shown on screen only, never saved unless you ask
//! (`--save`), and the download link stops working when the command ends.

use crate::config::{self, Config};
use thurea_core::crypto;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const HTTP_PORT: u16 = 47824;
const GIVE_UP: Duration = Duration::from_secs(45 * 60);

pub struct Options {
    pub apk: Option<PathBuf>,
    pub address: Option<IpAddr>,
    pub port: u16,
    pub name: Option<String>,
    pub wifi: Option<(String, String)>,
    pub save: Option<PathBuf>,
    /// Make the QR and stop (for scripts and tests): don't serve or wait.
    pub print_only: bool,
    /// For the tray apps / Omarchy plugin: machine-readable lines on stdout instead of text
    /// (`m\t<0/1 row>` for the QR, then `ready\t...`, then `event\t<kind>\t<detail>`).
    pub tray: bool,
}

/// One line for the tray (tabs and newlines in values would break the format).
fn tray_line(fields: &[&str]) {
    let clean: Vec<String> = fields.iter().map(|f| f.replace(['\t', '\n', '\r'], " ")).collect();
    println!("{}", clean.join("\t"));
    let _ = std::io::stdout().flush();
}

/// The Wi-Fi this computer is connected to, and its password, from NetworkManager (Linux), so
/// the phone can join it from the QR code. None if not on Wi-Fi or the password isn't readable.
pub fn current_wifi() -> Option<(String, String)> {
    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("nmcli").args(["-t", "-f", "NAME,TYPE,DEVICE", "connection", "show", "--active"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        // nmcli -t escapes ':' in values as '\:'.
        let name = text.lines().find_map(|l| {
            let f: Vec<String> = l.replace("\\:", "\u{1}").split(':').map(|x| x.replace('\u{1}', ":")).collect();
            (f.len() >= 2 && f[1] == "802-11-wireless").then(|| f[0].clone())
        })?;
        let get = |field: &str| {
            std::process::Command::new("nmcli").args(["-s", "-g", field, "connection", "show", &name]).output().ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        let ssid = get("802-11-wireless.ssid").filter(|s| !s.is_empty()).unwrap_or_else(|| name.clone());
        let pass = get("802-11-wireless-security.psk").unwrap_or_default();
        let key_mgmt = get("802-11-wireless-security.key-mgmt").unwrap_or_default();
        if !key_mgmt.is_empty() && pass.is_empty() {
            return None; // secured, but the password isn't readable by this user
        }
        Some((ssid, pass))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

pub fn find_apk(given: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(p) = given.or_else(|| std::env::var_os("THUREA_APK").map(PathBuf::from)) {
        return if p.is_file() { Ok(p) } else { Err(format!("{}: not found", p.display())) };
    }
    let mut c = Vec::new();
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        c.push(d.join("Thurea.apk"));
        c.push(d.join("android").join("Thurea.apk")); // Windows: Program Files\Thurea\android
        c.push(d.join("../share/thurea/Thurea.apk"));
    }
    #[cfg(unix)]
    c.push("/usr/share/thurea/Thurea.apk".into());
    #[cfg(unix)]
    if let Some(h) = std::env::var_os("HOME") {
        c.push(PathBuf::from(h).join(".local/share/thurea/Thurea.apk"));
    }
    c.push(config::state_dir().join("Thurea.apk"));
    c.into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| format!("Thurea.apk not found — pass --apk PATH (it's built by android/build.sh) or put it in {}", config::state_dir().display()))
}

/// Whether a phone on the home network could reach this address: private ranges only, and not
/// a VPN's own tunnel address (a VPN's /32 in 10.x or 100.64/10 isn't the home network).
fn is_home_lan(ip: std::net::Ipv4Addr, iface: &str) -> bool {
    let vpn = ["tun", "tap", "wg", "proton", "tailscale", "zt", "nordlynx", "mullvad", "ppp", "ipsec", "docker", "br-", "virbr", "veth", "lo"];
    ip.is_private() && !vpn.iter().any(|p| iface.starts_with(p))
}

/// The address other devices on this network reach this computer at: the home-network
/// interface's address (Linux: from `ip`, skipping VPN tunnels), else the interface of the
/// default route (a UDP "connect" only picks it; nothing is sent).
fn lan_address() -> Result<IpAddr, String> {
    #[cfg(target_os = "linux")]
    if let Ok(o) = std::process::Command::new("ip").args(["-4", "-o", "addr", "show", "scope", "global"]).output() {
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        let mut found: Vec<(u8, std::net::Ipv4Addr)> = Vec::new();
        for l in text.lines() {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 4 || f[2] != "inet" {
                continue;
            }
            let (Some(iface), Ok(ip)) = (Some(f[1]), f[3].split('/').next().unwrap_or("").parse::<std::net::Ipv4Addr>()) else { continue };
            if is_home_lan(ip, iface) {
                // Wi-Fi and Ethernet first; a /32 or point-to-point address last.
                let rank = if iface.starts_with('w') || iface.starts_with('e') { 0 } else { 1 };
                found.push((rank, ip));
            }
        }
        found.sort();
        if let Some((_, ip)) = found.first() {
            return Ok(IpAddr::V4(*ip));
        }
    }
    let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    s.connect("10.255.255.254:9").map_err(|_| "this computer has no network — pass --address IP".to_string())?;
    let ip = s.local_addr().map_err(|e| e.to_string())?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        return Err("couldn't tell this computer's network address — pass --address IP".into());
    }
    Ok(ip)
}

/// Android's provisioning QR (a JSON object of `android.app.extra.PROVISIONING_*` keys).
/// Without a household (`cfg` None) the phone still gets Thurea as its device owner; it just
/// isn't joined to a household mesh (it can be later, with `thurea pair-android`).
pub fn provisioning_json(cfg: Option<&Config>, url: &str, apk_sha256: &[u8; 32], peers: &str, name: Option<&str>, wifi: Option<&(String, String)>) -> String {
    let mut extras = match cfg {
        Some(cfg) => serde_json::json!({ "thurea_key": cfg.key, "thurea_peers": peers }),
        None => serde_json::json!({}),
    };
    if let Some(n) = name {
        extras["thurea_name"] = n.into();
    }
    let k = |s: &str| format!("android.app.extra.PROVISIONING_{s}");
    let mut j = serde_json::Map::new();
    j.insert(k("DEVICE_ADMIN_COMPONENT_NAME"), "com.thurea.guard/.AdminReceiver".into());
    j.insert(k("DEVICE_ADMIN_PACKAGE_DOWNLOAD_LOCATION"), url.into());
    j.insert(k("DEVICE_ADMIN_PACKAGE_CHECKSUM"), crypto::base64url(apk_sha256).into());
    j.insert(k("LEAVE_ALL_SYSTEM_APPS_ENABLED"), true.into());
    j.insert(k("SKIP_EDUCATION_SCREENS"), true.into());
    j.insert(k("ADMIN_EXTRAS_BUNDLE"), extras);
    if let Some((ssid, pass)) = wifi {
        j.insert(k("WIFI_SSID"), ssid.as_str().into());
        j.insert(k("WIFI_SECURITY_TYPE"), if pass.is_empty() { "NONE" } else { "WPA" }.into());
        if !pass.is_empty() {
            j.insert(k("WIFI_PASSWORD"), pass.as_str().into());
        }
    }
    serde_json::Value::Object(j).to_string()
}

enum Served {
    Started(SocketAddr),
    Done(SocketAddr),
    Failed(SocketAddr, String),
}

fn serve(listener: TcpListener, path: String, apk: Vec<u8>, tx: mpsc::Sender<Served>) {
    let apk = std::sync::Arc::new(apk);
    for conn in listener.incoming().flatten() {
        let (apk, path, tx) = (apk.clone(), path.clone(), tx.clone());
        std::thread::spawn(move || handle(conn, &path, &apk, &tx));
    }
}

fn handle(mut c: TcpStream, path: &str, apk: &[u8], tx: &mpsc::Sender<Served>) {
    let Ok(peer) = c.peer_addr() else { return };
    let _ = c.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = c.set_write_timeout(Some(Duration::from_secs(60)));
    let mut r = BufReader::new(c.try_clone().expect("socket clone"));
    let mut line = String::new();
    if r.by_ref().take(4096).read_line(&mut line).is_err() {
        return;
    }
    loop {
        let mut h = String::new();
        match r.by_ref().take(8192).read_line(&mut h) {
            Ok(0) | Err(_) => break,
            Ok(_) if h.trim().is_empty() => break,
            Ok(_) => {}
        }
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    // The random path is what keeps the link private: compare it in constant time.
    let same = target.len() == path.len() && target.bytes().zip(path.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0;
    let ok = (method == "GET" || method == "HEAD") && same;
    if !ok {
        let _ = c.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    }
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.android.package-archive\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        apk.len()
    );
    if c.write_all(head.as_bytes()).is_err() || method == "HEAD" {
        return;
    }
    let _ = tx.send(Served::Started(peer));
    match c.write_all(apk).and_then(|_| c.flush()) {
        Ok(()) => {
            let _ = tx.send(Served::Done(peer));
        }
        Err(e) => {
            let _ = tx.send(Served::Failed(peer, e.to_string()));
        }
    }
}

#[derive(serde::Deserialize)]
struct StatePeer {
    #[serde(default)]
    id: String,
    name: String,
    platform: String,
    state: thurea_core::State,
    #[serde(default)]
    policy_version: u64,
}

#[derive(serde::Deserialize)]
struct StateFile {
    updated: u64,
    #[serde(default)]
    peers: Vec<StatePeer>,
}

fn read_state() -> Option<StateFile> {
    serde_json::from_str(&std::fs::read_to_string(config::state_path()).ok()?).ok()
}

fn service_running() -> bool {
    read_state().is_some_and(|s| thurea_core::now().saturating_sub(s.updated) < 180)
}

pub fn run(o: Options) -> Result<String, String> {
    let tray = o.tray;
    match run_inner(o) {
        Err(e) if tray => {
            tray_line(&["error", &e]);
            Err(e)
        }
        r => r,
    }
}

fn run_inner(o: Options) -> Result<String, String> {
    // The household key is readable by root/Administrators only. The tray apps run as the
    // user, so they set phones up without it (the phone can join the household later).
    let household = Config::load().ok();
    let cfg = household.as_ref();
    let apk_path = find_apk(o.apk)?;
    let apk = std::fs::read(&apk_path).map_err(|e| format!("{}: {e}", apk_path.display()))?;
    if apk.len() < 4 || &apk[..2] != b"PK" {
        return Err(format!("{} isn't an APK", apk_path.display()));
    }
    let sum = crypto::sha256(&apk);
    let ip = match o.address {
        Some(a) => a,
        None => lan_address()?,
    };
    let host = match ip {
        IpAddr::V6(v6) => format!("[{v6}]"),
        IpAddr::V4(v4) => v4.to_string(),
    };
    let path = format!("/{}{}/Thurea.apk", crypto::new_device_id(), crypto::new_device_id());
    let url = format!("http://{host}:{}{path}", o.port);
    let mut peers = Vec::new();
    if let Some(cfg) = cfg {
        peers.push(format!("{host}:{}", cfg.port));
        peers.extend(cfg.peers.iter().cloned());
    }
    let json = provisioning_json(cfg, &url, &sum, &peers.join(","), o.name.as_deref(), o.wifi.as_ref());
    let qr = thurea_core::qr::QrCode::encode(json.as_bytes(), thurea_core::qr::Ecc::Low).ok_or("the setup code is too long for a QR code")?;

    if let Some(p) = &o.save {
        let data = if p.extension().is_some_and(|e| e == "pgm") { qr.to_pgm(8) } else { qr.to_svg().into_bytes() };
        std::fs::write(p, data).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    if o.print_only {
        return Ok(json);
    }

    let listener = TcpListener::bind(SocketAddr::new(if ip.is_ipv6() { "::".parse().unwrap() } else { "0.0.0.0".parse().unwrap() }, o.port))
        .map_err(|e| format!("can't open port {} for the download: {e} (pass --port N)", o.port))?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || serve(listener, path, apk, tx));

    let known: std::collections::HashSet<String> = read_state().map(|s| s.peers.into_iter().map(|p| p.id).collect()).unwrap_or_default();
    if o.tray {
        return tray_loop(&rx, &qr, &host, o.port, household.is_some(), o.wifi.as_ref().map(|w| w.0.as_str()), &known);
    }
    print!("{}", qr.to_terminal());
    let wifi_step = if o.wifi.is_some() { " (it joins Wi-Fi from the code)" } else { " (connect it to Wi-Fi if it asks)" };
    println!();
    println!("Set up the phone (new, or factory reset — resetting erases it):");
    println!("  1. On its first Welcome screen, tap the same spot 6 times. A QR scanner opens{wifi_step}.");
    println!("  2. Scan the code above. The phone downloads Thurea from this computer, checks it,");
    println!("     and makes Thurea its device owner: protection is on before the home screen.");
    match (&o.name, household.is_some()) {
        (Some(n), true) => println!("  3. It joins this household by itself, as \"{n}\"."),
        (None, true) => println!("  3. It joins this household by itself."),
        (_, false) => println!("  3. (No household on this computer, or not run as root/Administrator: the phone isn't\n     joined to one. It's fully protected on its own; join it later with `thurea pair-android`.)"),
    }
    println!();
    println!("The phone must be on the same network as this computer ({host}).");
    println!("If the download doesn't start, check that the firewall allows inbound TCP port {}.", o.port);
    #[cfg(windows)]
    if o.save.is_none() {
        println!("If the QR code is garbled, use Windows Terminal or rerun with --save qr.svg and open the image.");
    }
    if household.is_some() {
        println!("This code holds the household key: don't photograph or share it. Ctrl+C to stop.");
    } else {
        println!("Ctrl+C to stop.");
    }
    println!();
    if household.is_some() && !service_running() {
        println!("Note: Thurea isn't running on this computer, so it can't see the phone check in. Run `sudo thurea install` first to see that.\n");
    }

    let start = Instant::now();
    let mut downloaded_at: Option<Instant> = None;
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Served::Started(a)) => println!("… {} is downloading Thurea", a.ip()),
            Ok(Served::Done(a)) => {
                println!("✓ {} downloaded Thurea ({:.1} MB). The phone is installing it.", a.ip(), apk_len_mb(&apk_path));
                downloaded_at.get_or_insert_with(Instant::now);
            }
            Ok(Served::Failed(a, e)) => println!("! The download to {} broke off ({e}); the phone will retry.", a.ip()),
            Err(_) => {}
        }
        if let Some(s) = read_state() {
            if let Some(p) = s.peers.iter().find(|p| p.platform == "android" && !p.id.is_empty() && !known.contains(&p.id)) {
                return Ok(format!(
                    "✓ \"{}\" joined the household ({}, policy v{}). It's set up — the household keeps an eye on it from now on.",
                    p.name,
                    match p.state {
                        thurea_core::State::Ok => "protected",
                        thurea_core::State::Unknown => "still checking its protection",
                        thurea_core::State::Degraded => "NOT fully protected yet — open Thurea on it",
                    },
                    p.policy_version
                ));
            }
        }
        if let Some(t) = downloaded_at {
            if household.is_none() && t.elapsed() > Duration::from_secs(60) {
                return Ok("✓ Thurea is on the phone and is setting itself up as the device owner.".into());
            }
            if !service_running() && t.elapsed() > Duration::from_secs(120) {
                return Ok("Thurea is on the phone. Start Thurea here (sudo thurea install) to see it join the household.".into());
            }
            if t.elapsed() > Duration::from_secs(15 * 60) {
                return Err("the phone downloaded Thurea but hasn't checked in after 15 minutes. Open Thurea on it, and check this computer's firewall allows UDP port 47823".into());
            }
        }
        if start.elapsed() > GIVE_UP {
            return Err("gave up after 45 minutes with no phone. Run it again when the phone is ready".into());
        }
    }
}

/// Tray mode: the QR as 0/1 rows, then progress events until the phone has Thurea (or joined
/// the household), or the tray closes the panel (which ends this process).
fn tray_loop(rx: &mpsc::Receiver<Served>, qr: &thurea_core::qr::QrCode, host: &str, port: u16, household: bool, wifi: Option<&str>,
             known: &std::collections::HashSet<String>) -> Result<String, String> {
    for row in qr.to_matrix() {
        tray_line(&["m", &row]);
    }
    tray_line(&["ready", host, &port.to_string(), if household { "household" } else { "standalone" }, wifi.unwrap_or("")]);
    let start = Instant::now();
    let mut done_at: Option<Instant> = None;
    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Served::Started(a)) => tray_line(&["event", "downloading", &a.ip().to_string()]),
            Ok(Served::Done(a)) => {
                tray_line(&["event", "downloaded", &a.ip().to_string()]);
                done_at.get_or_insert_with(Instant::now);
            }
            Ok(Served::Failed(a, e)) => tray_line(&["event", "retry", &format!("{}: {e}", a.ip())]),
            Err(_) => {}
        }
        if household {
            if let Some(p) = read_state().and_then(|s| s.peers.into_iter().find(|p| p.platform == "android" && !p.id.is_empty() && !known.contains(&p.id))) {
                tray_line(&["event", "joined", &p.name]);
                return Ok(String::new());
            }
        }
        // Without a household there's nothing to wait for once the phone has the app.
        if done_at.is_some_and(|t| t.elapsed() > Duration::from_secs(if household { 15 * 60 } else { 90 })) {
            tray_line(&["event", "finished", ""]);
            return Ok(String::new());
        }
        if start.elapsed() > GIVE_UP {
            tray_line(&["event", "expired", ""]);
            return Ok(String::new());
        }
    }
}

fn apk_len_mb(p: &Path) -> f64 {
    std::fs::metadata(p).map(|m| m.len() as f64 / 1e6).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provisioning_code_has_what_android_needs() {
        let cfg = Config::new("Laptop".into(), &[7u8; 32]);
        let j = provisioning_json(Some(&cfg), "http://192.168.1.5:47824/x/Thurea.apk", &[0u8; 32], "192.168.1.5:47823", Some("Sam's phone"), None);
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        let k = |s: &str| v[format!("android.app.extra.PROVISIONING_{s}")].clone();
        assert_eq!(k("DEVICE_ADMIN_COMPONENT_NAME"), "com.thurea.guard/.AdminReceiver");
        assert_eq!(k("DEVICE_ADMIN_PACKAGE_CHECKSUM"), "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
        assert_eq!(k("ADMIN_EXTRAS_BUNDLE")["thurea_key"], cfg.key.as_str());
        assert_eq!(k("ADMIN_EXTRAS_BUNDLE")["thurea_name"], "Sam's phone");
        assert!(k("WIFI_SSID").is_null());
        assert!(thurea_core::qr::QrCode::encode(j.as_bytes(), thurea_core::qr::Ecc::Low).unwrap().version <= 20);
    }

    #[test]
    fn vpn_tunnels_are_not_the_home_network() {
        assert!(is_home_lan("192.168.1.164".parse().unwrap(), "wlp2s0"));
        assert!(!is_home_lan("10.2.0.2".parse().unwrap(), "proton0"));
        assert!(!is_home_lan("100.101.1.2".parse().unwrap(), "tailscale0"));
        assert!(!is_home_lan("8.8.8.8".parse().unwrap(), "eth0"));
    }

    #[test]
    fn standalone_code_has_no_household() {
        let wifi = ("Home".to_string(), "pa:ss".to_string());
        let j = provisioning_json(None, "http://192.168.1.5:47824/x/Thurea.apk", &[0u8; 32], "", None, Some(&wifi));
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        let k = |s: &str| v[format!("android.app.extra.PROVISIONING_{s}")].clone();
        assert_eq!(k("ADMIN_EXTRAS_BUNDLE"), serde_json::json!({}));
        assert_eq!(k("WIFI_SSID"), "Home");
        assert_eq!(k("WIFI_PASSWORD"), "pa:ss");
        assert_eq!(k("DEVICE_ADMIN_COMPONENT_NAME"), "com.thurea.guard/.AdminReceiver");
    }

    #[test]
    fn serves_only_the_secret_path() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || serve(l, "/abc123/Thurea.apk".into(), b"PKdata".to_vec(), tx));
        let get = |p: &str| {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(s, "GET {p} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        assert!(get("/abc124/Thurea.apk").starts_with("HTTP/1.1 404"));
        assert!(get("/").starts_with("HTTP/1.1 404"));
        let ok = get("/abc123/Thurea.apk");
        assert!(ok.starts_with("HTTP/1.1 200") && ok.ends_with("\r\n\r\nPKdata"), "{ok}");
        assert!(matches!(rx.recv_timeout(Duration::from_secs(5)), Ok(Served::Started(_))));
        assert!(matches!(rx.recv_timeout(Duration::from_secs(5)), Ok(Served::Done(_))));
    }
}
