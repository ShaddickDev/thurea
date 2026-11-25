//! The browser extension's view of Thurea, asked through `thurea family-host` (the extension's
//! native messaging helper): the same protection status, verse of the day and settings as the
//! tray apps, and the tray apps' actions (set up a phone, family, re-apply). Actions go to this
//! computer's own tray, so there is one phone setup and one family center everywhere:
//! the Omarchy bar plugin, thurea-tray's page (opened in a tab of the browser that asked),
//! or the Windows tray.
//!
//! Requests (JSON): {"type":"home"} · {"type":"prefs","key":"verses"|"animation","on":bool} ·
//! {"type":"open","what":"main"|"phone"|"reset"|"family"|"reapply"}.
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};
#[cfg(not(windows))]
use std::time::{Duration, Instant};

pub fn reply(msg: &Value) -> Value {
    match msg["type"].as_str() {
        Some("home") => home(),
        Some("prefs") => {
            let key = msg["key"].as_str().unwrap_or("");
            if !matches!(key, "verses" | "animation") {
                return json!({ "ok": false, "error": "unknown setting" });
            }
            match crate::prefs::set(key, msg["on"].as_bool().unwrap_or(true)) {
                Ok(()) => json!({ "ok": true }),
                Err(e) => json!({ "ok": false, "error": e }),
            }
        }
        Some("open") => open(msg["what"].as_str().unwrap_or("main")),
        _ => json!({ "ok": false, "error": "unknown request" }),
    }
}

fn home() -> Value {
    let v = thurea_core::verses::today(crate::prefs::utc_offset());
    let (abbr, notice) = thurea_core::verses::translation();
    json!({
        "status": crate::platform::desktop_status(),
        "verse": { "ref": v.reference, "text": v.text, "translation": abbr, "notice": notice },
        "prefs": Value::Object(crate::prefs::load()),
        "tray": tray_kind(),
    })
}

fn on_omarchy() -> bool {
    !cfg!(windows) && (std::env::var_os("OMARCHY_PATH").is_some() || std::path::Path::new("/usr/share/omarchy/shell").is_dir())
}

fn tray_kind() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if on_omarchy() {
        "omarchy"
    } else {
        "tray"
    }
}

fn ok() -> Value {
    json!({ "ok": true })
}

fn fail(e: impl Into<String>) -> Value {
    json!({ "ok": false, "error": e.into() })
}

fn me() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("thurea"))
}

/// Starts a program that outlives this helper (the browser ends the helper once it has replied).
fn spawn(program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> std::io::Result<()> {
    let mut c = Command::new(program);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED: u32 = 0x0000_0008; // DETACHED_PROCESS
        const NEW_GROUP: u32 = 0x0000_0200; // CREATE_NEW_PROCESS_GROUP
        const BREAKAWAY: u32 = 0x0100_0000; // CREATE_BREAKAWAY_FROM_JOB
        // Out of the browser's job if it allows that; else inside it.
        if c.creation_flags(DETACHED | NEW_GROUP | BREAKAWAY).spawn().is_ok() {
            return Ok(());
        }
        c.creation_flags(DETACHED | NEW_GROUP);
    }
    c.spawn().map(|_| ())
}

fn open(what: &str) -> Value {
    match what {
        "family" => match spawn(me(), &["control-center"]) {
            Ok(()) => ok(),
            Err(e) => fail(format!("Couldn't open the family center: {e}")),
        },
        "reapply" => reapply(),
        _ => open_tray(what),
    }
}

#[cfg(windows)]
fn open_tray(what: &str) -> Value {
    let tray = me().with_file_name("ThureaTray.exe");
    // A running tray takes the request and this copy exits at once.
    let args: &[&str] = if matches!(what, "phone" | "reset") { &["--keep-phone"] } else { &[] };
    match spawn(&tray, args) {
        Ok(()) => ok(),
        Err(e) => fail(format!("Couldn't open Thurea: {e}")),
    }
}

#[cfg(not(windows))]
fn open_tray(what: &str) -> Value {
    if on_omarchy() {
        let method = match what {
            "phone" => "keepPhone",
            "reset" => "setupPhone",
            _ => "open",
        };
        let shown = Command::new("omarchy-shell")
            .args(["thurea.omarchy", method])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        return if shown { ok() } else { fail("Thurea isn't in the Omarchy bar. Add it with: thurea-omarchy install") };
    }
    // thurea-tray: its page, in a tab of the browser that asked. Start the tray if it isn't running.
    let anchor = match what {
        "phone" => "#keep",
        "reset" => "#reset",
        _ => "",
    };
    let url = tray_url().or_else(|| {
        spawn("thurea-tray", &[]).ok()?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(150));
            if let Some(u) = tray_url() {
                return Some(u);
            }
        }
        None
    });
    match url {
        Some(u) => json!({ "ok": true, "url": format!("{u}{anchor}") }),
        None => fail("Thurea's tray didn't start. Is Thurea installed?"),
    }
}

/// The running thurea-tray's page (it writes the address there, readable by this user only).
#[cfg(not(windows))]
fn tray_url() -> Option<String> {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let u = std::fs::read_to_string(base.join("thurea-tray.url")).ok()?.trim().to_string();
    let port: u16 = u.strip_prefix("http://127.0.0.1:")?.split('/').next()?.parse().ok()?;
    // Left over from a tray that has gone? Nobody answers there.
    std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)).ok()?;
    Some(u)
}

fn reapply() -> Value {
    #[cfg(windows)]
    let r = spawn(
        "powershell.exe",
        &["-NoProfile", "-WindowStyle", "Hidden", "-Command", "Start-Process schtasks.exe -ArgumentList '/run','/tn','\\Thurea\\Thurea' -Verb RunAs -WindowStyle Hidden"],
    );
    #[cfg(not(windows))]
    let r = spawn("pkexec", &["systemctl", "start", "thurea.service"]);
    match r {
        Ok(()) => json!({ "ok": true, "message": "Re-applying: confirm as administrator if asked. Thurea also re-applies by itself every 5 minutes." }),
        Err(e) => fail(format!("Couldn't re-apply: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_unknown_requests_and_settings() {
        assert_eq!(reply(&json!({ "type": "nope" }))["ok"], false);
        assert_eq!(reply(&json!({ "type": "prefs", "key": "anything", "on": true }))["ok"], false);
    }

    #[test]
    fn home_has_what_the_trays_show() {
        let h = reply(&json!({ "type": "home" }));
        assert!(h["verse"]["text"].as_str().is_some_and(|t| !t.is_empty()));
        assert!(h["prefs"].is_object());
        assert!(["windows", "omarchy", "tray"].contains(&h["tray"].as_str().unwrap()));
    }
}
