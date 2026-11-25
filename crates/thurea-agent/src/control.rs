//! `thurea control-center`: the parents' control center, a page in a browser window served
//! from this computer only (127.0.0.1, a random port, a one-time token). The same page opens from
//! the Omarchy bar, the Linux tray and the Windows tray. It runs as the parent — no admin — and
//! changes go through [`crate::family`] (signed with the parent's own key).
//!
//! Guarded against other websites: every request needs the token, the Host must be this
//! address, and changes need a custom header (a page elsewhere can't send it without a CORS
//! preflight, which is never answered).

use crate::family;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::time::{Duration, Instant};

const PAGE: &str = include_str!("control.html");
/// Closes itself after this long without the page asking for anything.
const IDLE: Duration = Duration::from_secs(30 * 60);

fn session_file() -> std::path::PathBuf {
    let base = std::env::var_os(if cfg!(windows) { "LOCALAPPDATA" } else { "XDG_RUNTIME_DIR" })
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("thurea-control-center.json")
}

fn open(url: &str) {
    if std::env::var_os("THUREA_NO_BROWSER").is_some() {
        return; // tests / headless
    }
    #[cfg(windows)]
    {
        let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    }
    #[cfg(not(windows))]
    {
        // A window of its own if a Chromium-family browser is there, else the default browser.
        for b in ["chromium", "google-chrome-stable", "brave", "google-chrome"] {
            if Command::new(b).arg(format!("--app={url}")).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().is_ok() {
                return;
            }
        }
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
}

pub fn run() -> Result<String, String> {
    // Already open? Just show it again.
    if let Some(v) = std::fs::read_to_string(session_file()).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        if let (Some(port), Some(t)) = (v["port"].as_u64(), v["token"].as_str()) {
            if TcpStream::connect_timeout(&([127, 0, 0, 1], port as u16).into(), Duration::from_millis(300)).is_ok() {
                open(&format!("http://127.0.0.1:{port}/?t={t}"));
                return Ok(format!("Control center: http://127.0.0.1:{port}/"));
            }
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = thurea_core::crypto::hex(&thurea_core::crypto::new_key()[..16]);
    let sf = session_file();
    let _ = std::fs::write(&sf, json!({ "port": port, "token": token }).to_string());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&sf, std::fs::Permissions::from_mode(0o600));
    }
    let url = format!("http://127.0.0.1:{port}/?t={token}");
    open(&url);
    eprintln!("Thurea control center: {url}");
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let mut last = Instant::now();
    while last.elapsed() < IDLE {
        match listener.accept() {
            Ok((c, _)) => {
                let _ = c.set_nonblocking(false);
                let _ = c.set_read_timeout(Some(Duration::from_secs(10)));
                if handle(c, port, &token) {
                    last = Instant::now();
                }
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let _ = std::fs::remove_file(sf);
    Ok(String::new())
}

fn respond(mut c: TcpStream, code: &str, ctype: &str, body: &[u8]) {
    let _ = write!(
        c,
        "HTTP/1.1 {code}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Frame-Options: DENY\r\n\
         Content-Security-Policy: default-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'\r\n\
         Referrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = c.write_all(body);
}

/// Serves one request; true if it was a genuine one (keeps the server alive).
fn handle(c: TcpStream, port: u16, token: &str) -> bool {
    let mut r = BufReader::new(match c.try_clone() { Ok(x) => x, Err(_) => return false });
    let mut line = String::new();
    if r.read_line(&mut line).is_err() {
        return false;
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let (mut host, mut len, mut marked) = (String::new(), 0usize, false);
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).is_err() || h.trim().is_empty() {
            break;
        }
        let (k, v) = h.split_once(':').unwrap_or((&h, ""));
        match k.trim().to_ascii_lowercase().as_str() {
            "host" => host = v.trim().to_string(),
            "content-length" => len = v.trim().parse().unwrap_or(0).min(64 * 1024),
            "x-thurea" => marked = v.trim() == "1",
            _ => {}
        }
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let ok_token = query.split('&').any(|kv| kv == format!("t={token}"));
    if host != format!("127.0.0.1:{port}") || !ok_token {
        respond(c, "403 Forbidden", "text/plain", b"forbidden");
        return false;
    }
    match (method, path) {
        ("GET", "/") => respond(c, "200 OK", "text/html; charset=utf-8", PAGE.replace("{{TOKEN}}", token).as_bytes()),
        ("GET", "/api/state") => respond(c, "200 OK", "application/json", family::json().to_string().as_bytes()),
        ("POST", "/api/do") if marked => {
            let mut body = vec![0u8; len];
            let _ = r.read_exact(&mut body);
            let v: Value = serde_json::from_slice(&body).unwrap_or_default();
            let out = match act(&v) {
                Ok(m) => json!({ "ok": true, "msg": m }),
                Err(e) => json!({ "ok": false, "msg": e }),
            };
            respond(c, "200 OK", "application/json", out.to_string().as_bytes());
        }
        _ => respond(c, "404 Not Found", "text/plain", b"not found"),
    }
    true
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn act(v: &Value) -> Result<String, String> {
    match s(v, "action") {
        "create" => elevated_create(s(v, "name")),
        "join" => family::join(s(v, "name")),
        "answer" => family::answer(s(v, "id"), v["approve"] == true, v["minutes"].as_u64().filter(|m| *m > 0)),
        "category" => family::category(s(v, "child"), s(v, "category"), v["allow"] == true),
        "flag" => {
            let (child, flag, on) = (s(v, "child").to_string(), s(v, "flag").to_string(), v["on"] == true);
            if !matches!(flag.as_str(), "hold_new_apps" | "block_debugging") {
                return Err("unknown setting".into());
            }
            family::change(move |p| {
                let r = p.family.children.entry(child).or_default();
                if flag == "hold_new_apps" { r.hold_new_apps = on } else { r.block_debugging = on }
            })
        }
        "role" => {
            let (dev, child) = (s(v, "device").to_string(), s(v, "child").trim().to_string());
            let name = family::device_name(&dev);
            let role = if s(v, "role") == "child" && !child.is_empty() { thurea_core::family::Role::Child } else { thurea_core::family::Role::Adult };
            family::change(move |p| p.family.set_role(&dev, &name, role, Some(child.as_str()).filter(|c| !c.is_empty())))
        }
        "revoke" => {
            let (child, item) = (s(v, "child").to_string(), s(v, "item").to_string());
            family::change(move |p| {
                if let Some(r) = p.family.children.get_mut(&child) {
                    r.revoke(&item)
                }
            })
        }
        "relay" => family::relay(s(v, "addr")),
        _ => Err("unknown action".into()),
    }
}

/// Starting a family needs admin once: ask for it (polkit on Linux, UAC on Windows).
fn elevated_create(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 40 || name.contains(['\'', '"', '`', '$', '\\']) {
        return Err("enter your name (letters and spaces)".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &format!("$p = Start-Process -FilePath '{}' -ArgumentList 'family','create','\"{name}\"' -Verb RunAs -Wait -PassThru -WindowStyle Hidden; exit $p.ExitCode", exe.display())])
        .output();
    #[cfg(not(windows))]
    let out = Command::new("pkexec").arg(&exe).args(["family", "create", name]).output();
    let out = out.map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(format!("Family started — {name} is a parent."))
    } else {
        let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if e.is_empty() { "admin permission wasn't given".into() } else { e })
    }
}
