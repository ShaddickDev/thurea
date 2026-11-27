//! Windows. Thurea.ps1 (windows/thurea) enforces the rules; this module asks it to and reads its
//! report, and provides what the household mesh and children's accounts need from Windows.

use super::run;
use thurea_core::{Check, Policy, Report};
use std::path::{Path, PathBuf};

pub const PLATFORM: &str = "windows";

fn system_root() -> PathBuf {
    std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| r"C:\Windows".into())
}
fn install_dir() -> PathBuf {
    std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| r"C:\Program Files".into()).join("Thurea")
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

// ── Enforcement: Thurea.ps1 is the only enforcer on Windows ──
//
// Thurea for Windows is enforced by Thurea.ps1 (its "\Thurea\Thurea" task, every 5 minutes and at
// startup). It never changes DNS. thurea.exe used to have its own enforcement here too, with a
// different idea of the rules (the full hosts file, which stalls Windows' DNS Client for minutes;
// no DNS fallback; its own startup task), and the two would undo each other. Now thurea.exe only
// asks Thurea.ps1 and reads what it reports, so there is exactly one set of rules on the PC.

/// Thurea.ps1 as installed, or (before installing) the one shipped next to thurea.exe.
fn enforcer() -> Option<PathBuf> {
    let installed = install_dir().join("Thurea.ps1");
    if installed.is_file() {
        return Some(installed);
    }
    let me = std::env::current_exe().ok()?;
    let beside = me.parent()?.join("Thurea.ps1");
    beside.is_file().then_some(beside)
}

fn run_enforcer(arg: &str) -> Result<String, String> {
    let ps1 = enforcer().ok_or("Thurea.ps1 isn't installed: run Thurea\\Install Thurea.cmd from thurea-windows.zip")?;
    let ps = system_root().join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    run(&ps.to_string_lossy(), &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", &ps1.to_string_lossy(), arg])
}

/// Readable names for Thurea.ps1's checks (status.json).
fn check_label(id: &str) -> &str {
    match id {
        "hosts" => "hosts file block list",
        "browsers" => "browser policies",
        "managed" => "browsers accept the Thurea extension",
        "extensionServer" => "extension update server",
        "extensionInstalled" => "Thurea extension installed in every browser",
        "edgeRemoved" => "Edge removed",
        "tasks" => "Thurea's tasks",
        "uninstallBlock" => "uninstall block",
        other => other,
    }
}

/// The report from Thurea.ps1's last run (status.json), one check per item.
pub fn check(_p: &Policy) -> Report {
    let v = super::desktop_status();
    let mut r = Report::default();
    match v["checks"].as_object() {
        Some(m) if v["state"] != "stale" && v["state"] != "unknown" => {
            for (id, ok) in m {
                let label = check_label(id);
                r.push(if ok.as_bool() == Some(true) { Check::ok(id, label.to_string()) } else { Check::degraded(id, format!("Not in place: {label}")) });
            }
        }
        _ => r.push(Check::unknown("service", v["line"].as_str().unwrap_or("Thurea isn't reporting").to_string())),
    }
    r
}

/// Asks Thurea.ps1 to put every rule back now, then reports.
pub fn apply(p: &Policy) -> Report {
    let mut r = Report::default();
    if let Err(e) = run_enforcer("-Apply") {
        r.push(Check::degraded("service", format!("Thurea.ps1 -Apply failed: {e}")));
    }
    let mut checked = check(p);
    checked.checks.splice(0..0, r.checks);
    checked
}

/// Installs Thurea for Windows (Thurea.ps1 -Install: files, tasks, rules).
pub fn install() -> Result<String, String> {
    run_enforcer("-Install").map(|out| format!("{}\nThurea for Windows is installed", out.trim_end()))
}

/// Thurea for Windows is meant to stay (its tasks put each other back), so thurea.exe doesn't
/// take it apart: half-removed rules would be restored within 5 minutes anyway.
pub fn uninstall() -> Result<String, String> {
    Err("Thurea for Windows can't be removed with thurea.exe. See docs/windows.md, \"Removing Thurea\".".into())
}

/// A message box on every signed-in session (msg.exe; not present on Windows Home).
pub fn notify(text: &str, _urgent: bool) {
    eprintln!("thurea: {text}");
    let _ = std::process::Command::new("msg").args(["*", "/TIME:120", &format!("Thurea: {text}")]).status();
}

/// The submit folder: every user may add files (the service checks parents' signatures).
pub fn open_submit_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let _ = std::process::Command::new("icacls").arg(dir).args(["/grant", "*S-1-5-32-545:(OI)(CI)(W,RD)"]).output();
    Ok(())
}

/// After joining a household: start the mesh task, if Thurea is installed.
pub fn start_mesh_service() -> String {
    match powershell("Start-ScheduledTask -TaskPath '\\Thurea\\' -TaskName 'Thurea Mesh' -ErrorAction Stop") {
        Ok(_) => "Thurea's household service is running.".into(),
        Err(_) => "Next: thurea install (runs Thurea at startup)".into(),
    }
}

// ── Children's accounts (Thurea family) ──

pub fn accounts() -> Vec<String> {
    powershell("Get-LocalUser | Where-Object Enabled | ForEach-Object { $_.Name }")
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty() && !matches!(l.as_str(), "administrator" | "guest" | "defaultaccount" | "wdagutilityaccount"))
        .collect()
}

pub fn sessions() -> Vec<thurea_core::desktop::Session> {
    // `query user` exits 1 when nobody is logged in.
    let out = std::process::Command::new("query").arg("user").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    thurea_core::desktop::parse_query_user(&out)
}

pub fn running(user: &str) -> Vec<(String, String)> {
    run("tasklist", &["/FI", &format!("USERNAME eq {user}"), "/FO", "CSV", "/NH"])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split("\",\"").collect();
            Some((f.get(1)?.to_string(), f.first()?.trim_start_matches('"').to_string()))
        })
        .collect()
}

pub fn close(user: &str, program: &str) {
    let _ = run("taskkill", &["/F", "/FI", &format!("USERNAME eq {user}"), "/IM", program]);
}

/// Disconnecting a session shows the lock screen (the user signs in again to continue).
pub fn lock_session(id: &str) {
    let _ = run("tsdiscon", &[id]);
}

pub fn is_admin_account(user: &str) -> bool {
    run("net", &["localgroup", "Administrators"]).unwrap_or_default().lines().any(|l| l.trim().eq_ignore_ascii_case(user))
}

pub fn local_clock() -> Option<(String, u16)> {
    let out = powershell("Get-Date -Format 'yyyy-MM-dd HH mm'").ok()?;
    let f: Vec<&str> = out.split_whitespace().collect();
    Some((f.first()?.to_string(), f.get(1)?.parse::<u16>().ok()? * 60 + f.get(2)?.parse::<u16>().ok()?))
}
