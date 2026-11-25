//! `thurea-tray`: Thurea's tray icon on Linux desktops other than Omarchy (KDE, GNOME with the
//! AppIndicator extension, Xfce, Sway with a tray...). On Omarchy the Thurea bar plugin does this
//! job, so the tray exits at once there.
//!
//! The icon shows whether this computer is protected. Its window is a page on this computer
//! only (127.0.0.1, at a random secret path) opened in the browser: the logo drawing itself,
//! the verse of the day, protection details, the phone-setup QR code and the settings. Every
//! answer comes from the `thurea` command, exactly as the Omarchy plugin gets it.
//!
//!   thurea-tray            run the tray (started at login by /etc/xdg/autostart)
//!   thurea-tray --force    run it on Omarchy too
//!   thurea-tray --open     run it and open the window

mod icon;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PAGE: &str = include_str!("page.html");

#[derive(Default)]
struct Phone {
    child: Option<Child>,
    pending: Vec<String>,
    rows: Vec<String>,
    ready: bool,
    household: bool,
    wifi: String,
    event: Option<(String, String)>,
    error: String,
    generation: u64,
}

struct App {
    base: String,
    phone: Mutex<Phone>,
}

fn thurea(args: &[&str]) -> Option<String> {
    let o = Command::new("thurea").args(args).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn thurea_json(args: &[&str]) -> serde_json::Value {
    thurea(args).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({}))
}

fn on_omarchy() -> bool {
    std::env::var_os("OMARCHY_PATH").is_some() || std::path::Path::new("/usr/share/omarchy/shell").is_dir()
}

fn open_window(app: &App, anchor: &str) {
    let _ = Command::new("xdg-open").arg(format!("{}{anchor}", app.base)).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

fn set_pref(key: &str, on: bool) {
    let _ = thurea(&["prefs", "set", key, if on { "on" } else { "off" }]);
}

// ── Phone setup: `thurea setup-phone --tray`, read line by line ──

fn stop_phone(app: &App) {
    let mut p = app.phone.lock().unwrap();
    if let Some(mut c) = p.child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    p.generation += 1;
}

/// `keep`: a phone in use, nothing erased (`thurea adopt-phone`); otherwise a new or
/// factory-reset phone (`thurea setup-phone`).
fn start_phone(app: &Arc<App>, wifi: bool, keep: bool) {
    stop_phone(app);
    let mut cmd = Command::new("thurea");
    if keep {
        cmd.args(["adopt-phone", "--tray"]);
    } else {
        cmd.args(["setup-phone", "--tray"]);
        if wifi {
            cmd.arg("--wifi-current");
        }
    }
    let mut p = app.phone.lock().unwrap();
    let generation = p.generation;
    *p = Phone { generation, ..Default::default() };
    match cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Err(e) => p.error = format!("Couldn't start Thurea's phone setup: {e}"),
        Ok(mut child) => {
            let out = child.stdout.take().unwrap();
            let err = child.stderr.take().unwrap();
            p.child = Some(child);
            let app1 = app.clone();
            std::thread::spawn(move || {
                let app = app1;
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    let mut p = app.phone.lock().unwrap();
                    if p.generation != generation {
                        return;
                    }
                    let f: Vec<&str> = line.split('\t').collect();
                    match f.as_slice() {
                        ["m", row] if !row.is_empty() && row.bytes().all(|b| b == b'0' || b == b'1') => p.pending.push(row.to_string()),
                        ["ready", rest @ ..] => {
                            let rows = std::mem::take(&mut p.pending);
                            if !rows.is_empty() && rows.len() == rows[0].len() {
                                p.rows = rows;
                            } else {
                                p.error = "The setup code didn't come out right. Try again.".into();
                            }
                            p.ready = true;
                            p.household = rest.get(2) == Some(&"household");
                            p.wifi = rest.get(3).unwrap_or(&"").to_string();
                        }
                        ["event", kind, detail @ ..] => p.event = Some((kind.to_string(), detail.first().unwrap_or(&"").to_string())),
                        ["error", msg @ ..] => p.error = msg.join(" "),
                        _ => {}
                    }
                }
            });
            let app2 = Arc::clone(&app);
            std::thread::spawn(move || {
                let mut s = String::new();
                let _ = BufReader::new(err).take(64 * 1024).read_to_string(&mut s);
                let mut p = app2.phone.lock().unwrap();
                if p.generation == generation && !p.ready && p.error.is_empty() && !s.trim().is_empty() {
                    p.error = s.trim().lines().last().unwrap_or("").to_string();
                }
            });
        }
    }
}

fn phone_json(app: &App) -> serde_json::Value {
    let p = app.phone.lock().unwrap();
    serde_json::json!({
        "rows": p.rows, "ready": p.ready, "household": p.household, "wifi": p.wifi, "error": p.error,
        "event": p.event.as_ref().map(|(k, d)| vec![k.clone(), d.clone()]),
    })
}

// ── The window's page and its API (127.0.0.1 only, secret path) ──

fn respond(mut c: TcpStream, code: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {code}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
Content-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; connect-src 'self'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = c.write_all(head.as_bytes()).and_then(|_| c.write_all(body));
}

fn handle(app: &Arc<App>, token: &str, port: u16, c: TcpStream) {
    let _ = c.set_read_timeout(Some(Duration::from_secs(10)));
    let mut r = BufReader::new(c.try_clone().unwrap());
    let mut first = String::new();
    if r.by_ref().take(4096).read_line(&mut first).is_err() {
        return;
    }
    let mut host_ok = false;
    loop {
        let mut h = String::new();
        match r.by_ref().take(8192).read_line(&mut h) {
            Ok(0) | Err(_) => break,
            Ok(_) if h.trim().is_empty() => break,
            Ok(_) => {
                // Only this computer, by address: a web page elsewhere can't reach it through a
                // DNS name pointed at 127.0.0.1.
                let l = h.to_ascii_lowercase();
                if let Some(v) = l.strip_prefix("host:") {
                    host_ok = matches!(v.trim(), x if x == format!("127.0.0.1:{port}"));
                }
            }
        }
    }
    let mut parts = first.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let prefix = format!("/{token}/");
    let Some(rest) = target.strip_prefix(&prefix).filter(|_| host_ok) else {
        return respond(c, "404 Not Found", "text/plain", b"");
    };
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let arg = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(&format!("{k}="))).map(str::to_string);
    let json = |c: TcpStream, v: serde_json::Value| respond(c, "200 OK", "application/json", v.to_string().as_bytes());
    match (method, path) {
        ("GET", "") => respond(c, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        ("GET", "api/state") => json(c, serde_json::json!({
            "status": thurea_json(&["desktop-status"]), "verse": thurea_json(&["verse", "--json"]), "prefs": thurea_json(&["prefs", "--json"]),
        })),
        ("POST", "api/prefs") => {
            for k in ["verses", "animation"] {
                if let Some(v) = arg(k) {
                    set_pref(k, v == "on");
                }
            }
            json(c, serde_json::json!({ "ok": true }))
        }
        ("POST", "api/phone/start") => {
            start_phone(app, arg("wifi").as_deref() != Some("0"), arg("mode").as_deref() == Some("keep"));
            json(c, serde_json::json!({ "ok": true }))
        }
        ("GET", "api/phone") => json(c, phone_json(app)),
        ("POST", "api/phone/stop") => {
            stop_phone(app);
            json(c, serde_json::json!({ "ok": true }))
        }
        ("POST", "api/reapply") => {
            let ok = Command::new("pkexec").args(["systemctl", "start", "thurea.service"]).status().is_ok_and(|s| s.success());
            json(c, serde_json::json!({ "ok": ok }))
        }
        _ => respond(c, "404 Not Found", "text/plain", b""),
    }
}

fn serve() -> Result<Arc<App>, String> {
    let l = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = l.local_addr().map_err(|e| e.to_string())?.port();
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).map_err(|e| e.to_string())?;
    let token: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let app = Arc::new(App { base: format!("http://127.0.0.1:{port}/{token}/"), phone: Mutex::new(Phone::default()) });
    let a = app.clone();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let (a, t) = (a.clone(), token.clone());
            std::thread::spawn(move || handle(&a, &t, port, c));
        }
    });
    Ok(app)
}

// ── The tray icon ──

#[cfg(target_os = "linux")]
mod tray {
    use super::*;
    use ksni::blocking::TrayMethods;

    pub struct ThureaTray {
        pub app: Arc<App>,
        pub state: String,
        pub line: String,
        pub verse: String,
        pub prefs: serde_json::Value,
    }

    impl ThureaTray {
        pub fn refresh(&mut self) {
            let s = thurea_json(&["desktop-status"]);
            self.state = s["state"].as_str().unwrap_or("unknown").to_string();
            self.line = s["line"].as_str().unwrap_or("Thurea isn't reporting").to_string();
            let v = thurea_json(&["verse", "--json"]);
            self.verse = match (v["text"].as_str(), v["ref"].as_str()) {
                (Some(t), Some(r)) => format!("\u{201c}{t}\u{201d} ({r}, WEB)"),
                _ => String::new(),
            };
            self.prefs = thurea_json(&["prefs", "--json"]);
        }
        fn pref(&self, k: &str) -> bool {
            self.prefs[k].as_bool() != Some(false)
        }
    }

    impl ksni::Tray for ThureaTray {
        fn id(&self) -> String {
            "thurea".into()
        }
        fn title(&self) -> String {
            "Thurea".into()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            let warn = self.state != "ok";
            [22, 32, 48, 64].iter().map(|&n| ksni::Icon { width: n as i32, height: n as i32, data: icon::pixmap(n, warn) }).collect()
        }
        fn tool_tip(&self) -> ksni::ToolTip {
            let mut d = self.line.clone();
            if self.pref("verses") && !self.verse.is_empty() {
                d.push_str("\n\n");
                d.push_str(&self.verse);
            }
            ksni::ToolTip { title: "Thurea".into(), description: d, ..Default::default() }
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            open_window(&self.app, "");
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::*;
            let mut m: Vec<ksni::MenuItem<Self>> = vec![
                StandardItem { label: self.line.clone(), enabled: false, ..Default::default() }.into(),
                MenuItem::Separator,
                StandardItem { label: "Open Thurea".into(), activate: Box::new(|t: &mut Self| open_window(&t.app, "")), ..Default::default() }.into(),
                StandardItem {
                    label: "Family…".into(),
                    activate: Box::new(|_| {
                        let _ = Command::new("thurea").arg("control-center").stdout(Stdio::null()).stderr(Stdio::null()).spawn();
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem { label: "Set up a phone…".into(), activate: Box::new(|t: &mut Self| open_window(&t.app, "#phone")), ..Default::default() }.into(),
                StandardItem {
                    label: "Re-apply protection now".into(),
                    activate: Box::new(|_| {
                        let _ = Command::new("pkexec").args(["systemctl", "start", "thurea.service"]).spawn();
                    }),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                CheckmarkItem {
                    label: "Verse of the day".into(),
                    checked: self.pref("verses"),
                    activate: Box::new(|t: &mut Self| {
                        set_pref("verses", !t.pref("verses"));
                        t.refresh();
                    }),
                    ..Default::default()
                }
                .into(),
                CheckmarkItem {
                    label: "Opening animation".into(),
                    checked: self.pref("animation"),
                    activate: Box::new(|t: &mut Self| {
                        set_pref("animation", !t.pref("animation"));
                        t.refresh();
                    }),
                    ..Default::default()
                }
                .into(),
            ];
            if self.pref("verses") && !self.verse.is_empty() {
                // Menus show one line per item: the verse, a few words per line.
                let mut lines = vec![MenuItem::Separator];
                let mut cur = String::new();
                for w in self.verse.split_whitespace() {
                    if cur.len() + w.len() > 48 {
                        lines.push(StandardItem { label: std::mem::take(&mut cur), enabled: false, ..Default::default() }.into());
                    }
                    if !cur.is_empty() {
                        cur.push(' ');
                    }
                    cur.push_str(w);
                }
                lines.push(StandardItem { label: cur, enabled: false, ..Default::default() }.into());
                m.splice(2..2, lines);
            }
            m
        }
    }

    pub fn run(app: Arc<App>) -> Result<(), String> {
        let mut t = ThureaTray { app, state: String::new(), line: String::new(), verse: String::new(), prefs: serde_json::json!({}) };
        t.refresh();
        let handle = t.spawn().map_err(|e| format!("no system tray available: {e}"))?;
        loop {
            std::thread::sleep(Duration::from_secs(60));
            if handle.update(|t: &mut ThureaTray| t.refresh()).is_none() {
                return Ok(());
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if on_omarchy() && !args.iter().any(|a| a == "--force") {
        return; // the Thurea bar plugin is Thurea's tray on Omarchy
    }
    let app = match serve() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("thurea-tray: {e}");
            std::process::exit(1);
        }
    };
    if args.iter().any(|a| a == "--print-url") {
        println!("{}", app.base);
        let _ = std::io::stdout().flush();
    }
    if args.iter().any(|a| a == "--open") {
        open_window(&app, "");
    }
    #[cfg(target_os = "linux")]
    if let Err(e) = tray::run(app) {
        eprintln!("thurea-tray: {e}");
        std::process::exit(1);
    }
}
