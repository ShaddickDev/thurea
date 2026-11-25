//! The tray apps' per-user settings, shared by the Omarchy plugin, the Linux tray and the
//! Windows tray: `~/.config/thurea/prefs.json` (Linux), `%APPDATA%\Thurea\prefs.json` (Windows).
//!
//!   verses     show the verse of the day (default on)
//!   animation  draw the Thurea logo when the app opens (default on)

use std::path::PathBuf;

pub const KEYS: &[(&str, bool)] = &[("verses", true), ("animation", true)];

pub fn path() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| ".".into()).join("Thurea");
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| ".".into())
        .join("thurea");
    base.join("prefs.json")
}

pub fn load() -> serde_json::Map<String, serde_json::Value> {
    let mut m: serde_json::Map<String, serde_json::Value> = std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    for (k, d) in KEYS {
        if !m.get(*k).is_some_and(|v| v.is_boolean()) {
            m.insert((*k).into(), (*d).into());
        }
    }
    m
}

pub fn get(key: &str) -> bool {
    load().get(key).and_then(|v| v.as_bool()).unwrap_or(true)
}

pub fn set(key: &str, on: bool) -> Result<(), String> {
    if !KEYS.iter().any(|(k, _)| *k == key) {
        return Err(format!("unknown setting {key:?} (settings: {})", KEYS.iter().map(|k| k.0).collect::<Vec<_>>().join(", ")));
    }
    let mut m = load();
    m.insert(key.into(), on.into());
    let p = path();
    std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&m).unwrap()).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, p).map_err(|e| e.to_string())
}

/// Seconds east of UTC for today's date ("verse of the day" changes at local midnight).
pub fn utc_offset() -> i64 {
    #[cfg(unix)]
    if let Ok(o) = std::process::Command::new("date").arg("+%z").output() {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if s.len() == 5 {
            let sign = if s.starts_with('-') { -1 } else { 1 };
            if let (Ok(h), Ok(m)) = (s[1..3].parse::<i64>(), s[3..5].parse::<i64>()) {
                return sign * (h * 3600 + m * 60);
            }
        }
    }
    0
}
