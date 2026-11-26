//! `thurea control-center`: the parents' control center, a page in a browser window served
//! from this computer only (127.0.0.1, a random port, a one-time token). The same page opens from
//! the Omarchy bar, the Linux tray, the Windows tray and the browser extension's Thurea tab. It
//! runs as the parent — no admin — and changes go through [`crate::family`] (signed with the
//! parent's own key).
//!
//! `thurea control-center` is a launcher: it shows the window that is already open, or starts
//! the page's server in the background (`thurea control-center --serve`) and opens a window on
//! it, then returns. So every launcher (a Quickshell `Process`, the Windows tray, a terminal) gets
//! control back at once, and asking again always brings the window back.
//!
//! Guarded against other websites: every request needs the token, the Host must be this
//! address, and changes need a custom header (a page elsewhere can't send it without a CORS
//! preflight, which is never answered).

use crate::family;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PAGE: &str = include_str!("control.html");
/// Closes itself after this long without the page asking for anything (the open page asks
/// every few seconds, so this is "30 minutes after the window was closed").
const IDLE: Duration = Duration::from_secs(30 * 60);
/// What `/api/ping` answers, so a launcher knows the port is still ours.
const PING: &str = "thurea-control-center";

fn session_file() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = base.join("Thurea");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("control-center.json")
    }
    #[cfg(not(windows))]
    {
        // XDG_RUNTIME_DIR is per user; without it (su, some minimal desktops) the shared temp
        // folder, with the user's name so two people on one computer don't collide.
        match std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_dir()) {
            Some(d) => d.join("thurea-control-center.json"),
            None => {
                let user = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_else(|_| "user".into());
                let user: String = user.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')).collect();
                std::env::temp_dir().join(format!("thurea-control-center-{user}.json"))
            }
        }
    }
}

fn write_session(port: u16, token: &str) {
    let path = session_file();
    let body = json!({ "port": port, "token": token, "pid": std::process::id() }).to_string();
    let _ = std::fs::remove_file(&path);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600); // the token is the key to the page: this user only, from the start
    }
    if let Ok(mut f) = o.open(&path) {
        let _ = f.write_all(body.as_bytes());
    }
}

/// The URL of a control center that is already running for this user, if it answers.
fn running() -> Option<String> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(session_file()).ok()?).ok()?;
    let (port, token) = (v["port"].as_u64()? as u16, v["token"].as_str()?.to_string());
    let mut c = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(500)).ok()?;
    let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = c.set_write_timeout(Some(Duration::from_secs(2)));
    write!(c, "GET /api/ping?t={token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").ok()?;
    let mut out = String::new();
    let _ = c.read_to_string(&mut out);
    out.contains(PING).then(|| format!("http://127.0.0.1:{port}/?t={token}"))
}

pub fn run(args: &[String]) -> Result<String, String> {
    if args.iter().any(|a| a == "--serve") {
        return serve();
    }
    let url = match running() {
        Some(u) => u,
        None => start()?,
    };
    let shown = args.iter().any(|a| a == "--no-browser") || std::env::var_os("THUREA_NO_BROWSER").is_some() || !open(&url);
    if shown {
        Ok(format!("Thurea's family control center is open at:\n  {url}\n(open it in a browser on this computer; it closes 30 minutes after the page is closed)"))
    } else {
        Ok(String::new())
    }
}

/// Starts the server in the background and waits until it answers.
fn start() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    crate::browser::spawn(&exe, &["control-center", "--serve"]).map_err(|e| format!("couldn't start the control center: {e}"))?;
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
        if let Some(u) = running() {
            return Ok(u);
        }
    }
    Err("the control center didn't start (try `thurea control-center --serve` in a terminal to see why)".into())
}

/// The server itself: runs until nobody has used the page for [`IDLE`].
fn serve() -> Result<String, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = thurea_core::crypto::hex(&thurea_core::crypto::new_key()[..16]);
    write_session(port, &token);
    eprintln!("Thurea control center: http://127.0.0.1:{port}/?t={token}");
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let last = Arc::new(Mutex::new(Instant::now()));
    // One change at a time: each reads the policy, signs a newer one and hands it over.
    let changes = Arc::new(Mutex::new(()));
    let token = Arc::new(token);
    while last.lock().map(|l| l.elapsed() < IDLE).unwrap_or(false) {
        match listener.accept() {
            Ok((c, _)) => {
                let (last, changes, token) = (last.clone(), changes.clone(), token.clone());
                // A thread each: browsers open spare connections that send nothing, and one of
                // those must never hold up the page (or a slow admin prompt hold up the rest).
                std::thread::spawn(move || {
                    let _ = c.set_nonblocking(false);
                    let _ = c.set_read_timeout(Some(Duration::from_secs(15)));
                    if handle(c, port, &token, &changes) {
                        if let Ok(mut l) = last.lock() {
                            *l = Instant::now();
                        }
                    }
                });
            }
            Err(_) => std::thread::sleep(Duration::from_millis(40)),
        }
    }
    // Only remove the session file if it is still ours.
    let sf = session_file();
    if std::fs::read_to_string(&sf).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).is_some_and(|v| v["pid"] == std::process::id()) {
        let _ = std::fs::remove_file(sf);
    }
    Ok(String::new())
}

// ── Opening a window ──

fn quiet(c: &mut Command) -> &mut Command {
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0); // the browser outlives whoever asked for the window
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

/// Started and still running, or handed the URL to a running copy of itself: true.
fn launched(c: &mut Command) -> bool {
    let Ok(mut child) = quiet(c).spawn() else { return false };
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(1500) {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return false,
        }
    }
    true
}

/// A window of its own if a Chromium-family browser is there (an app window, no tabs or
/// address bar), else the default browser. False if nothing could show it.
fn open(url: &str) -> bool {
    let app = format!("--app={url}");
    #[cfg(windows)]
    {
        // Edge is on every Windows 10 / 11; the others if someone removed it.
        let mut bases = Vec::new();
        for v in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
            if let Some(b) = std::env::var_os(v) {
                bases.push(PathBuf::from(b));
            }
        }
        for rel in [r"Microsoft\Edge\Application\msedge.exe", r"Google\Chrome\Application\chrome.exe", r"BraveSoftware\Brave-Browser\Application\brave.exe"] {
            for b in &bases {
                let exe = b.join(rel);
                if exe.is_file() && launched(Command::new(&exe).arg(&app)) {
                    return true;
                }
            }
        }
        launched(Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", url]))
            || launched(Command::new("explorer.exe").arg(url))
    }
    #[cfg(target_os = "macos")]
    {
        for b in ["Google Chrome", "Microsoft Edge", "Brave Browser", "Chromium"] {
            if launched(Command::new("open").args(["-na", b, "--args", &app])) {
                return true;
            }
        }
        launched(Command::new("open").arg(url))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let display = std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some();
        if display {
            // Names differ by distro: Arch / Omarchy `chromium`, `brave`; Debian, Ubuntu and
            // Fedora `chromium` or `chromium-browser`, `brave-browser`, `google-chrome-stable`.
            for b in [
                "chromium", "chromium-browser", "google-chrome-stable", "google-chrome", "brave", "brave-browser",
                "microsoft-edge-stable", "microsoft-edge", "vivaldi-stable", "vivaldi",
            ] {
                if in_path(b) && launched(Command::new(b).arg(&app)) {
                    return true;
                }
            }
        }
        for (b, args) in [("xdg-open", vec![url]), ("gio", vec!["open", url]), ("sensible-browser", vec![url]), ("x-www-browser", vec![url]), ("firefox", vec![url])] {
            if in_path(b) && launched(Command::new(b).args(args)) {
                return true;
            }
        }
        false
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn in_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

// ── The page and its API ──

fn respond(mut c: TcpStream, code: &str, ctype: &str, body: &[u8]) {
    let _ = write!(
        c,
        "HTTP/1.1 {code}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Frame-Options: DENY\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'\r\n\
         Referrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = c.write_all(body);
}

/// Serves one request; true if it was a genuine one (keeps the server alive).
fn handle(c: TcpStream, port: u16, token: &str, changes: &Mutex<()>) -> bool {
    let mut r = BufReader::new(match c.try_clone() { Ok(x) => x, Err(_) => return false });
    let mut line = String::new();
    if r.read_line(&mut line).is_err() {
        return false;
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let (mut host, mut len, mut marked) = (String::new(), 0usize, false);
    for _ in 0..100 {
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
    let ok_host = host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}");
    if !ok_host || !ok_token {
        respond(c, "403 Forbidden", "text/plain", b"forbidden");
        return false;
    }
    match (method, path) {
        ("GET", "/") => respond(c, "200 OK", "text/html; charset=utf-8", PAGE.replace("{{TOKEN}}", token).as_bytes()),
        ("GET", "/api/ping") => {
            respond(c, "200 OK", "text/plain", PING.as_bytes());
            return false; // a launcher checking, not the page
        }
        ("GET", "/api/state") => {
            let mut v = family::json();
            v["now"] = json!(thurea_core::now());
            respond(c, "200 OK", "application/json", v.to_string().as_bytes())
        }
        ("POST", "/api/do") if marked => {
            let mut body = vec![0u8; len];
            let _ = r.read_exact(&mut body);
            let v: Value = serde_json::from_slice(&body).unwrap_or_default();
            let out = {
                let _one = changes.lock();
                match act(&v) {
                    Ok(m) => json!({ "ok": true, "msg": m }),
                    Err(e) => json!({ "ok": false, "msg": e }),
                }
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

/// A person's name as Thurea takes it: 1–40 characters, nothing a command line could misread.
fn name_ok(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 || name.contains(['\'', '"', '`', '$', '\\', '%', '&', '|', '<', '>', ';', '\n', '\r']) || name.starts_with('-') {
        return Err("use a name of up to 40 letters, digits and spaces".into());
    }
    Ok(name)
}

/// HOST:PORT, HOST a name or an address ([…] for IPv6).
fn relay_ok(addr: &str) -> bool {
    let Some((host, port)) = addr.rsplit_once(':') else { return false };
    let (host, v6) = match host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        Some(h) => (h, true),
        None => (host, false),
    };
    port.parse::<u16>().is_ok_and(|p| p > 0)
        && !host.is_empty()
        && (v6 || !host.contains(':'))
        && host.len() <= 253
        && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
}

fn act(v: &Value) -> Result<String, String> {
    match s(v, "action") {
        "create" => {
            let name = name_ok(s(v, "name"))?;
            elevated(&["family", "create", name]).map(|_| format!("Family started — {name} is a parent."))
        }
        "join" => family::join(name_ok(s(v, "name"))?),
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
            let dev = s(v, "device").to_string();
            let name = family::device_name(&dev);
            let child = if s(v, "role") == "child" { Some(name_ok(s(v, "child"))?.to_string()) } else { None };
            let role = if child.is_some() { thurea_core::family::Role::Child } else { thurea_core::family::Role::Adult };
            family::change(move |p| p.family.set_role(&dev, &name, role, child.as_deref()))
        }
        "revoke" => {
            let (child, item) = (s(v, "child").to_string(), s(v, "item").to_string());
            family::change(move |p| {
                if let Some(r) = p.family.children.get_mut(&child) {
                    r.revoke(&item)
                }
            })
        }
        "relay" => {
            let addr = s(v, "addr").trim();
            if !addr.is_empty() && !relay_ok(addr) {
                return Err("a relay address looks like relay.example.org:47830".into());
            }
            family::relay(addr)
        }
        "account" => {
            let child = s(v, "child").trim();
            let child = if child.is_empty() || child == "-" { "" } else { name_ok(child)? };
            family::account(s(v, "device"), s(v, "user"), child)
        }
        "screen" => family::screen(s(v, "child"), s(v, "what"), s(v, "a"), s(v, "b")),
        "code" => {
            let child = name_ok(s(v, "child"))?;
            let minutes = v["minutes"].as_u64().unwrap_or(0).to_string();
            elevated(&["family", "code", child, &minutes])
        }
        _ => Err("unknown action".into()),
    }
}

/// Runs `thurea ARGS…` as admin, once, asking for it the platform's way (UAC on Windows,
/// polkit on Linux). Returns what it printed.
fn elevated(args: &[&str]) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let shown = || format!("sudo thurea {}", args.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.to_string() }).collect::<Vec<_>>().join(" "));
    #[cfg(windows)]
    {
        // Start-Process can't hand back what the admin copy printed: it writes it to a file.
        let out = std::env::temp_dir().join(format!("thurea-{}.json", thurea_core::crypto::hex(&thurea_core::crypto::new_key()[..8])));
        let q = |a: &str| format!("'\"{}\"'", a.replace('\'', "''"));
        let mut list: Vec<String> = args.iter().map(|a| q(a)).collect();
        list.push("'--out'".into());
        list.push(q(&out.display().to_string()));
        let script = format!(
            "try {{ $p = Start-Process -FilePath '{}' -ArgumentList {} -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ErrorAction Stop; exit $p.ExitCode }} catch {{ exit 1223 }}",
            exe.display().to_string().replace('\'', "''"),
            list.join(",")
        );
        let mut c = Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script]);
        let status = quiet(&mut c).status().map_err(|e| format!("couldn't ask for admin permission: {e}"))?;
        let result: Option<Value> = std::fs::read_to_string(&out).ok().and_then(|s| serde_json::from_str(&s).ok());
        let _ = std::fs::remove_file(&out);
        if let Some(r) = result {
            let msg = r["msg"].as_str().unwrap_or("").trim().to_string();
            return if r["ok"] == true { Ok(msg) } else { Err(msg) };
        }
        Err(if status.code() == Some(1223) {
            "admin permission wasn't given".into()
        } else {
            format!("that needs admin permission: open a terminal as administrator and run {}", shown().replace("sudo ", ""))
        })
    }
    #[cfg(not(windows))]
    {
        let pkexec = ["/usr/bin/pkexec", "/bin/pkexec", "/usr/local/bin/pkexec"].into_iter().find(|p| std::path::Path::new(p).is_file());
        let Some(pkexec) = pkexec else {
            return Err(format!("this computer can't ask for admin permission in a window (no polkit): in a terminal, run {}", shown()));
        };
        let out = Command::new(pkexec).arg(&exe).args(args).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        match out.status.code() {
            Some(0) => Ok(stdout),
            // 126: the prompt was dismissed or the password was wrong. 127: no polkit agent.
            Some(126) => Err("admin permission wasn't given".into()),
            Some(127) if stderr.contains("Not authorized") || stderr.contains("authentication agent") || stderr.is_empty() => {
                Err(format!("no admin prompt is available on this desktop: in a terminal, run {}", shown()))
            }
            _ => Err(if stderr.is_empty() { format!("that didn't work: in a terminal, run {}", shown()) } else { stderr.trim_start_matches("Error: ").to_string() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(name_ok("Mum").is_ok());
        assert!(name_ok("Anna Lena").is_ok());
        assert!(name_ok("Zoë").is_ok());
        for bad in ["", "  ", "a\"b", "x'y", "$(id)", "a;b", "--help", "a&b", &"x".repeat(41)] {
            assert!(name_ok(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn relays() {
        for good in ["relay.example.org:47830", "203.0.113.5:47830", "[2001:db8::1]:47830"] {
            assert!(relay_ok(good), "{good}");
        }
        for bad in ["relay.example.org", "relay.example.org:47830relay.example.org:47830", "a b:1", ":47830", "x:0"] {
            assert!(!relay_ok(bad), "{bad}");
        }
    }
}
