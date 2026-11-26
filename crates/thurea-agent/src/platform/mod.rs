//! Enforcement on the desktop. Each OS module provides the same functions:
//!
//! * `apply(policy)`  — put every rule in place (idempotent; cheap when nothing changed);
//! * `check(policy)`  — what is actually in force right now, as evidence-based checks;
//! * `install()` / `uninstall()` — run Thurea at startup / remove everything Thurea set;
//! * `notify(text)`   — show an alert to whoever is using the computer.

// The hosts and policy-file helpers are Linux's; on Windows Thurea.ps1 manages those files.
#![cfg_attr(windows, allow(dead_code))]

use thurea_core::Policy;
use std::path::Path;
use std::process::Command;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

pub const HOSTS_BEGIN: &str = "# >>> Thurea (managed: edits here are replaced) >>>";
pub const HOSTS_END: &str = "# <<< Thurea <<<";

/// Replaces Thurea's block in a hosts file, keeping everything else exactly as it was.
pub fn merge_hosts(existing: &str, policy: Option<&Policy>, newline: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in existing.lines() {
        let t = line.trim();
        if t == HOSTS_BEGIN {
            inside = true;
        } else if t == HOSTS_END {
            inside = false;
        } else if !inside {
            out.push_str(line);
            out.push_str(newline);
        }
    }
    if let Some(p) = policy {
        if !out.is_empty() && !out.ends_with(&format!("{newline}{newline}")) {
            out.push_str(newline);
        }
        out.push_str(HOSTS_BEGIN);
        out.push_str(newline);
        for l in p.hosts_entries().lines() {
            out.push_str(l);
            out.push_str(newline);
        }
        out.push_str(HOSTS_END);
        out.push_str(newline);
    }
    out
}

/// Where Thurea's enforcement (thurea-apply on Linux, Thurea.ps1 on Windows) reports what it
/// found on its last run.
pub fn desktop_status_path() -> std::path::PathBuf {
    #[cfg(windows)]
    return std::env::var_os("ProgramData").map(std::path::PathBuf::from).unwrap_or_else(|| r"C:\ProgramData".into()).join("Thurea").join("status.json");
    #[cfg(not(windows))]
    return std::env::var_os("THUREA_STATUS_FILE").map(Into::into).unwrap_or_else(|| "/run/thurea/status.json".into());
}

/// This computer's protection for the tray apps: the enforcement's last report, marked
/// unprotected if it's missing or older than 15 minutes (the check runs every 5).
pub fn desktop_status() -> serde_json::Value {
    let raw = std::fs::read_to_string(desktop_status_path()).ok();
    let mut v: serde_json::Value = raw.as_deref().and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok()).unwrap_or(serde_json::json!({}));
    if !v.is_object() {
        v = serde_json::json!({});
    }
    let at = v["epoch"].as_u64().unwrap_or(0);
    let age = thurea_core::now().saturating_sub(at);
    let (state, line) = if raw.is_none() || at == 0 {
        ("unknown", "Thurea isn't reporting (not installed, or not running)".to_string())
    } else if age > 15 * 60 {
        ("stale", format!("Not checked for {} minutes", age / 60))
    } else if v["protected"].as_bool() == Some(true) {
        ("ok", "This computer is protected".to_string())
    } else if only_extension_missing(&v) {
        // Everything is in place, but a browser hasn't installed the extension (yet): browsers
        // pick up a new policy when they restart.
        let n = v["extensionMissing"].as_array().map_or(0, Vec::len).max(1);
        ("degraded", format!("The Thurea extension isn't in {n} browser profile{} yet: close and reopen that browser", if n == 1 { "" } else { "s" }))
    } else {
        ("degraded", "Not fully protected: Thurea is putting it back".to_string())
    };
    v["state"] = state.into();
    v["line"] = line.into();
    v["age"] = age.into();
    v
}

/// The only failing check is "the extension is installed in every browser profile".
fn only_extension_missing(v: &serde_json::Value) -> bool {
    let Some(checks) = v["checks"].as_object() else { return false };
    checks.get("extensionInstalled").and_then(|c| c.as_bool()) == Some(false) && checks.iter().all(|(k, c)| k == "extensionInstalled" || c.as_bool() != Some(false))
}

/// Writes only when the content differs. Returns true if it wrote.
pub fn write_if_changed(path: &Path, content: &[u8]) -> std::io::Result<bool> {
    if std::fs::read(path).ok().as_deref() == Some(content) {
        return Ok(false);
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("thurea-tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

/// Runs a command, returning stdout, or an error with stderr.
pub fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd).args(args).output().map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("{cmd} {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Checks the managed hosts block matches the policy.
pub fn hosts_check(path: &Path, policy: &Policy, newline: &str) -> thurea_core::Check {
    use thurea_core::Check;
    match std::fs::read_to_string(path) {
        Ok(s) if merge_hosts(&s, Some(policy), newline) == s => Check::ok("hosts", format!("{} sites blocked in the hosts file", policy.blocklist.len())),
        Ok(_) => Check::degraded("hosts", "Hosts file blocklist is missing or was changed"),
        Err(e) => Check::unknown("hosts", format!("Can't read the hosts file: {e}")),
    }
}

/// Checks a managed browser-policy file holds exactly what Thurea wrote.
pub fn file_check(id: &str, label: &str, path: &Path, want: &[u8]) -> thurea_core::Check {
    use thurea_core::Check;
    match std::fs::read(path) {
        Ok(b) if b == want => Check::ok(id, format!("{label} locked")),
        Ok(_) => Check::degraded(id, format!("{label} policy was changed")),
        Err(_) => Check::degraded(id, format!("{label} policy is missing")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_merge_is_idempotent_and_reversible() {
        let mut p = Policy::default();
        p.blocklist = vec!["example.org".into()];
        let orig = "127.0.0.1 localhost\n::1 localhost\n";
        let once = merge_hosts(orig, Some(&p), "\n");
        assert!(once.starts_with(orig));
        assert!(once.contains("0.0.0.0 www.example.org\n"));
        assert_eq!(merge_hosts(&once, Some(&p), "\n"), once);
        // a tampered block is replaced, user lines survive
        let tampered = once.replace("0.0.0.0 example.org\n", "") + "10.0.0.2 nas\n";
        let fixed = merge_hosts(&tampered, Some(&p), "\n");
        assert!(fixed.contains("0.0.0.0 example.org\n") && fixed.contains("10.0.0.2 nas"));
        // removal restores the original
        assert_eq!(merge_hosts(&once, None, "\n").trim_end(), orig.trim_end());
    }

    #[test]
    fn a_missing_extension_is_named() {
        let only = serde_json::json!({ "checks": { "dns": true, "extensionInstalled": false } });
        assert!(only_extension_missing(&only));
        let more = serde_json::json!({ "checks": { "dns": false, "extensionInstalled": false } });
        assert!(!only_extension_missing(&more));
        assert!(!only_extension_missing(&serde_json::json!({ "checks": { "dns": true } })));
    }

    #[test]
    fn hosts_merge_windows_newlines() {
        let mut p = Policy::default();
        p.blocklist = vec!["example.org".into()];
        let orig = "# hosts\r\n127.0.0.1 localhost\r\n";
        let once = merge_hosts(orig, Some(&p), "\r\n");
        assert!(once.contains("\r\n0.0.0.0 example.org\r\n"));
        assert_eq!(merge_hosts(&once, Some(&p), "\r\n"), once);
    }
}
