//! `thurea adopt-phone`: put Thurea on a phone that's already in use, without resetting it. Apps,
//! photos and settings stay.
//!
//! Android only lets an app become the device owner on a phone with no accounts on it. So, as
//! android/phone-setup.sh does over USB, this:
//!   1. connects to the phone: by USB if one is plugged in, otherwise wirelessly. The computer
//!      shows a QR code for Android's own "Wireless debugging → Pair device with QR code";
//!   2. installs Thurea;
//!   3. has Thurea's account helper sign the accounts out (you confirm on the phone, and enter
//!      your PIN there if asked; Thurea never types passwords);
//!   4. makes Thurea the device owner;
//!   5. signs the accounts back in (Thurea fills in the email; you do the password / passkey);
//!   6. switches Wireless debugging off again.
//!
//! It needs `adb`: Android's open-source tools (Arch: `android-tools`, Apache-2.0).
//!
//! With `--tray` it prints the line format the tray apps read (`m\t<0/1 row>` for the QR,
//! `ready\t...`, then `event\t<kind>\t<detail>`), like `thurea setup-phone --tray`.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PKG: &str = "com.thurea.guard";

struct Out {
    tray: bool,
}

impl Out {
    fn line(&self, fields: &[&str]) {
        if self.tray {
            let clean: Vec<String> = fields.iter().map(|f| f.replace(['\t', '\n', '\r'], " ")).collect();
            println!("{}", clean.join("\t"));
        } else {
            match fields {
                ["event", _, detail @ ..] => println!("{}", detail.join(" ")),
                ["error", msg @ ..] => eprintln!("thurea: {}", msg.join(" ")),
                _ => {}
            }
        }
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
    fn event(&self, kind: &str, detail: &str) {
        self.line(&["event", kind, detail]);
    }
}

/// adb: THUREA_ADB, then the PATH, then the usual Android SDK places.
pub fn find_adb() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("THUREA_ADB").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let exe = if cfg!(windows) { "adb.exe" } else { "adb" };
    if let Some(paths) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&paths) {
            if d.join(exe).is_file() {
                return Some(d.join(exe));
            }
        }
    }
    let mut c = Vec::new();
    #[cfg(windows)]
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        c.push(PathBuf::from(l).join(r"Android\Sdk\platform-tools\adb.exe"));
    }
    if let Some(h) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        c.push(PathBuf::from(h).join("Android/Sdk/platform-tools").join(exe));
    }
    c.into_iter().find(|p| p.is_file())
}

struct Adb {
    bin: PathBuf,
    serial: Option<String>,
}

impl Adb {
    fn raw(&self, args: &[&str]) -> (bool, String) {
        let mut c = Command::new(&self.bin);
        if let Some(s) = &self.serial {
            c.args(["-s", s]);
        }
        match c.args(args).stdin(Stdio::null()).output() {
            Ok(o) => (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)).replace('\r', "")),
            Err(e) => (false, e.to_string()),
        }
    }
    fn sh(&self, cmd: &str) -> String {
        self.raw(&["shell", cmd]).1.trim().to_string()
    }
}

/// A service in `adb mdns services`: (name, type, "ip:port").
pub fn parse_mdns(text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let ty = f.iter().position(|x| x.starts_with("_adb"))?;
            let addr = f.last()?.to_string();
            (ty > 0 && addr.contains(':') && addr != f[ty]).then(|| (f[..ty].join(" "), f[ty].trim_end_matches('.').to_string(), addr))
        })
        .collect()
}

/// Accounts as "type name" lines, from `dumpsys account`.
pub fn parse_accounts(text: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = text
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("Account {name=")?;
            let (name, ty) = rest.split_once(", type=")?;
            Some((ty.trim_end_matches('}').to_string(), name.to_string()))
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The QR code Android's "Pair device with QR code" scans: the pairing service's name and its
/// password, in Wi-Fi QR syntax (what Android Studio shows).
/// Another app that owns the phone or its profile, from `dpm list-owners`. Android writes it
/// "User 0: admin=com.x/.Admin,DeviceOwner" (Android 14 and later) or "... Device Owner" (older).
fn other_owner(owners: &str) -> Option<String> {
    owners.lines().find_map(|l| {
        let squashed: String = l.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
        if l.contains(PKG) || !(squashed.contains("deviceowner") || squashed.contains("profileowner")) {
            return None;
        }
        let admin = l.split("admin=").nth(1).unwrap_or(l).split([',', ' ', '/']).next().unwrap_or("").trim();
        Some(if admin.is_empty() { l.trim().to_string() } else { admin.to_string() })
    })
}

pub fn pairing_qr(name: &str, password: &str) -> String {
    format!("WIFI:T:ADB;S:{name};P:{password};;")
}

fn random_text(n: usize, alphabet: &[u8]) -> String {
    let mut b = vec![0u8; n];
    let _ = getrandom::getrandom(&mut b);
    b.iter().map(|x| alphabet[*x as usize % alphabet.len()] as char).collect()
}

/// Devices adb already has, in the "device" state (USB, or paired earlier).
fn devices(adb: &Adb) -> Vec<String> {
    adb.raw(&["devices"]).1.lines().skip(1).filter_map(|l| l.strip_suffix("\tdevice").map(str::to_string)).collect()
}

pub fn run(apk: Option<PathBuf>, tray: bool) -> Result<String, String> {
    let out = Out { tray };
    let r = run_inner(&out, apk);
    if let Err(e) = &r {
        out.line(&["error", e]);
    }
    r
}

fn run_inner(out: &Out, apk: Option<PathBuf>) -> Result<String, String> {
    let bin = find_adb().ok_or(if cfg!(windows) {
        "adb isn't on this PC. Install Android's platform tools, then try again."
    } else {
        "adb isn't installed. Install it with: sudo pacman -S android-tools"
    })?;
    let apk = crate::setup_phone::find_apk(apk)?;
    let mut adb = Adb { bin, serial: None };
    let _ = adb.raw(&["start-server"]);

    // ── 1. Connect: a phone already on USB, or wireless pairing by QR ──
    let before: Vec<String> = devices(&adb);
    let name = format!("thurea-{}", random_text(6, b"abcdefghjkmnpqrstuvwxyz23456789"));
    let password = random_text(10, b"0123456789");
    let qr = thurea_core::qr::QrCode::encode(pairing_qr(&name, &password).as_bytes(), thurea_core::qr::Ecc::High).ok_or("QR code too long")?;
    if out.tray {
        for row in qr.to_matrix() {
            out.line(&["m", &row]);
        }
        out.line(&["ready", "adopt"]);
    } else {
        print!("{}", qr.to_terminal());
        println!("\nOn the phone: Settings → About phone → tap Build number 7 times (Developer options).");
        println!("Then Settings → Developer options → Wireless debugging: on → Pair device with QR code, and scan this.");
        println!("(Or plug the phone in by USB with USB debugging on.) Waiting…\n");
    }
    let start = Instant::now();
    let mut paired_ip: Option<String> = None;
    let serial = loop {
        if start.elapsed() > Duration::from_secs(20 * 60) {
            return Err("No phone after 20 minutes. Start again when the phone is ready.".into());
        }
        // A phone plugged in by USB (or connected earlier) needs no pairing.
        if let Some(d) = devices(&adb).into_iter().find(|d| !before.contains(d) || (before.len() == 1 && !d.contains(':'))) {
            out.event("connected", &d);
            break d;
        }
        let services = parse_mdns(&adb.raw(&["mdns", "services"]).1);
        if paired_ip.is_none() {
            if let Some((_, _, addr)) = services.iter().find(|(n, t, _)| n == &name && t.contains("pairing")) {
                out.event("found", addr);
                let (ok, msg) = adb.raw(&["pair", addr, &password]);
                if !ok || !msg.to_lowercase().contains("success") {
                    return Err(format!("Pairing with the phone failed: {}", msg.trim()));
                }
                out.event("paired", "");
                paired_ip = addr.rsplit_once(':').map(|(ip, _)| ip.to_string());
            }
        }
        if let Some(ip) = &paired_ip {
            if let Some((_, _, addr)) = services.iter().find(|(_, t, a)| t.contains("tls-connect") && a.rsplit_once(':').map(|x| x.0) == Some(ip.as_str())) {
                let _ = adb.raw(&["connect", addr]);
            }
            if let Some(d) = devices(&adb).into_iter().find(|d| d.starts_with(&format!("{ip}:"))) {
                out.event("connected", &d);
                break d;
            }
        }
        std::thread::sleep(Duration::from_millis(1500));
    };
    adb.serial = Some(serial);

    // ── 2. Checks: nothing is changed if one fails ──
    let model = format!("{} {}", adb.sh("getprop ro.product.manufacturer"), adb.sh("getprop ro.product.model"));
    out.event("phone", model.trim());
    let sdk: u32 = adb.sh("getprop ro.build.version.sdk").parse().unwrap_or(0);
    if sdk < 29 {
        return Err("Android 10 or newer is needed.".into());
    }
    let owners = adb.sh("dpm list-owners");
    let already = owners.contains(PKG);
    // Before anything is signed out: a phone another app manages can't take Thurea as its owner.
    if !already {
        if let Some(other) = other_owner(&owners) {
            return Err(format!("Another app already manages this phone ({other}). Nothing was changed."));
        }
    }
    let users = adb.sh("pm list users").matches("UserInfo{").count();
    if !already && users > 1 {
        return Err(format!(
            "The phone has {users} users or profiles (Secure Folder, Dual Messenger, a work profile or a guest). Remove the extra ones, then try again."
        ));
    }

    // ── 3. Install Thurea while still signed in ──
    out.event("installing", "");
    let verify = adb.sh("settings get global verifier_verify_adb_installs");
    let _ = adb.sh("settings put global verifier_verify_adb_installs 0");
    let (ok, msg) = adb.raw(&["install", "-r", &apk.to_string_lossy()]);
    if verify == "null" || verify.is_empty() {
        let _ = adb.sh("settings delete global verifier_verify_adb_installs");
    } else {
        let _ = adb.sh(&format!("settings put global verifier_verify_adb_installs {verify}"));
    }
    if !ok {
        return Err(format!(
            "Installing Thurea failed: {}. On a Samsung, check Settings → Security and privacy → Auto Blocker is off.",
            msg.lines().last().unwrap_or("").trim()
        ));
    }
    let _ = adb.sh(&format!("dumpsys deviceidle whitelist +{PKG}"));
    let _ = adb.sh(&format!("appops set {PKG} ACCESS_RESTRICTED_SETTINGS allow"));
    let _ = adb.sh(&format!("pm grant {PKG} android.permission.WRITE_SECURE_SETTINGS"));
    out.event("installed", "");
    let helper = |mode: &str, email: Option<&str>| {
        let mut c = format!("am start -f 0x10008000 -n {PKG}/.Remote --es account_mode {mode}");
        if let Some(e) = email {
            c.push_str(&format!(" --es email '{}'", e.replace('\'', "")));
        }
        let _ = adb.sh(&c);
    };

    if already {
        let _ = adb.sh(&format!("am start -n {PKG}/.MainActivity"));
        let _ = adb.sh("settings put global adb_wifi_enabled 0");
        out.event("done", "updated");
        return Ok("Thurea was already in charge of this phone; it's updated.".into());
    }

    // ── 4. Accounts out for a moment (Android requires it once) ──
    let accounts = parse_accounts(&adb.sh("dumpsys account"));
    save_list(&accounts);
    if !accounts.is_empty() {
        out.event("signout", &accounts.iter().map(|a| a.1.clone()).collect::<Vec<_>>().join(", "));
        helper("remove", None);
        let t = Instant::now();
        loop {
            let left = parse_accounts(&adb.sh("dumpsys account"));
            if left.is_empty() {
                break;
            }
            if t.elapsed() > Duration::from_secs(20 * 60) {
                helper("off", None);
                return Err("Accounts are still on the phone after 20 minutes. Nothing was changed; sign back in to any you removed.".into());
            }
            out.event("signout-wait", &left.iter().map(|a| a.1.clone()).collect::<Vec<_>>().join(", "));
            std::thread::sleep(Duration::from_secs(3));
        }
    }

    // ── 5. Device owner ──
    // Right after installing, Android sometimes isn't ready to accept the new admin yet: retry.
    let (mut owner, mut msg) = (false, String::new());
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(3));
        }
        let (_, m) = adb.raw(&["shell", &format!("dpm set-device-owner {PKG}/.AdminReceiver")]);
        owner = m.contains("Success") || adb.sh("dpm list-owners").contains(PKG);
        msg = reason(&m);
        if owner {
            break;
        }
    }
    if owner {
        out.event("owner", "");
    } else {
        out.event("owner-failed", &msg);
    }

    // ── 6. Accounts back in ──
    let _ = adb.sh(&format!("am start -n {PKG}/.MainActivity"));
    std::thread::sleep(Duration::from_secs(2));
    let mut later = Vec::new();
    for (ty, name) in &accounts {
        let signed_in = |adb: &Adb| parse_accounts(&adb.sh("dumpsys account")).iter().any(|(t, n)| t == ty && n == name);
        if signed_in(&adb) {
            continue;
        }
        match ty.as_str() {
            "com.google" => {
                helper("add", Some(name));
                out.event("signin", name);
            }
            "com.osp.app.signin" => {
                let _ = adb.sh("am start -a android.settings.ADD_ACCOUNT_SETTINGS");
                out.event("signin-samsung", name);
            }
            _ => {
                later.push(name.clone()); // app accounts (WhatsApp, Signal…) come back when the app is opened
                continue;
            }
        }
        let t = Instant::now();
        while !signed_in(&adb) && t.elapsed() < Duration::from_secs(10 * 60) {
            std::thread::sleep(Duration::from_secs(3));
        }
        out.event(if signed_in(&adb) { "signedin" } else { "signin-later" }, name);
    }
    helper("off", None);
    if !owner {
        return Err(format!("Thurea couldn't become the device owner ({msg}). Your accounts are back; try again."));
    }
    clear_list();
    if !later.is_empty() {
        out.event("apps", &later.join(", "));
    }
    // Wireless debugging was only for this: switch it off again.
    let _ = adb.sh("settings put global adb_wifi_enabled 0");
    out.event("done", "");
    Ok("Thurea is in charge of the phone, and your accounts are back.".into())
}

/// The useful line of an Android error (its exception message, not the stack trace).
fn reason(out: &str) -> String {
    let line = out
        .lines()
        .map(str::trim)
        .find(|l| l.contains("Exception: ") || l.starts_with("Error"))
        .or_else(|| out.lines().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or("no answer from the phone");
    line.rsplit_once("Exception: ").map(|x| x.1).unwrap_or(line).to_string()
}

/// The accounts that were signed out, kept on this computer until they're back (so an
/// interrupted run still knows what to restore).
fn list_path() -> PathBuf {
    crate::prefs::path().with_file_name("accounts-to-readd")
}
fn save_list(a: &[(String, String)]) {
    if a.is_empty() {
        return;
    }
    let p = list_path();
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let _ = std::fs::write(&p, a.iter().map(|(t, n)| format!("{t} {n}\n")).collect::<String>());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
}
fn clear_list() {
    let _ = std::fs::remove_file(list_path());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_adb_mdns_services() {
        let t = "List of discovered mdns services\nadb-R58N123-AbCdEf\t_adb-tls-connect._tcp\t192.168.1.40:37011\nthurea-abc234\t_adb-tls-pairing._tcp\t192.168.1.40:41235\n";
        let s = parse_mdns(t);
        assert_eq!(s.len(), 2);
        assert_eq!(s[1], ("thurea-abc234".into(), "_adb-tls-pairing._tcp".into(), "192.168.1.40:41235".into()));
        assert!(parse_mdns("List of discovered mdns services\n\n").is_empty());
    }

    #[test]
    fn sees_another_owner_in_every_format() {
        // A Samsung S24 Ultra on Android 17 (no space), and older Androids.
        assert_eq!(other_owner("User 0: admin=com.example.guard/.AdminReceiver,DeviceOwner").as_deref(), Some("com.example.guard"));
        assert_eq!(other_owner("User 0: admin=com.x.mdm/.Admin,Device Owner,Affiliated").as_deref(), Some("com.x.mdm"));
        assert_eq!(other_owner("User 10: admin=com.work/.Admin,ProfileOwner").as_deref(), Some("com.work"));
        assert_eq!(other_owner(&format!("User 0: admin={PKG}/.AdminReceiver,DeviceOwner")), None);
        assert_eq!(other_owner("no owners have been set"), None);
        assert_eq!(other_owner(""), None);
    }

    #[test]
    fn android_errors_are_readable() {
        let t = "Exception occurred while executing 'set-device-owner':\njava.lang.IllegalStateException: Not allowed to set the device owner because there are already some accounts on the device.\n\tat com.android.server.devicepolicy.X(Y.java:1)\n\tat android.os.Binder.execTransact(Binder.java:1275)\n";
        assert_eq!(reason(t), "Not allowed to set the device owner because there are already some accounts on the device.");
    }

    #[test]
    fn reads_accounts() {
        let t = "Accounts: 2\n    Account {name=sam@gmail.com, type=com.google}\n    Account {name=Sam, type=com.whatsapp}\n    Account {name=sam@gmail.com, type=com.google}\n";
        assert_eq!(parse_accounts(t), vec![("com.google".into(), "sam@gmail.com".into()), ("com.whatsapp".into(), "Sam".into())]);
    }

    #[test]
    fn pairing_code_is_what_android_scans() {
        assert_eq!(pairing_qr("thurea-abc234", "0123456789"), "WIFI:T:ADB;S:thurea-abc234;P:0123456789;;");
    }
}
