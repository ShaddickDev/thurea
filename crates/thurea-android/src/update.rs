//! Thurea's own update check on Android: what to do with a release manifest whose signature the
//! Java side has already checked against Thurea's release key (`Updater.verified`, Android's own
//! RSA), and whether the downloaded APK matches the checksum in that signed manifest.
//!
//! Java still does the network, the signature check and the install, which need Android's APIs.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::io::Read;

pub const BASE: &str = "https://github.com/ShaddickDev/thurea/releases";

/// "1.1.0" vs "1.0.12": number by number. A part's text after its leading digits is ignored
/// ("12-rc" counts as 12) and a part with no leading digits counts as 0.
pub fn compare(a: &str, b: &str) -> Ordering {
    let parts = |s: &str| s.trim().split('.').map(num).collect::<Vec<u64>>();
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q {
            return p.cmp(&q);
        }
    }
    Ordering::Equal
}

fn num(s: &str) -> u64 {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(0)
}

fn failed(why: &str) -> Value {
    json!({"action": "error", "note": format!("Update check failed: {why}")})
}

/// What to do with a signed release manifest when this phone has Thurea `have`:
/// `{"action":"current","note"}`, `{"action":"error","note"}`, or
/// `{"action":"download","version","url","sha256"}` with the APK's download address.
pub fn plan(manifest: &str, have: &str) -> Value {
    let Ok(m) = serde_json::from_str::<Value>(manifest) else { return failed("the release manifest isn't valid JSON.") };
    let Some(latest) = m["version"].as_str() else { return failed("the release manifest has no version.") };
    // The version and file name go into a URL: only plain version numbers and file names.
    if latest.is_empty() || !latest.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return failed("the release manifest's version isn't a version number.");
    }
    if compare(latest, have) != Ordering::Greater {
        return json!({"action": "current", "note": format!("Thurea {have} is up to date.")});
    }
    let f = &m["files"]["android"];
    let (Some(name), Some(sha)) = (f["name"].as_str(), f["sha256"].as_str()) else {
        return failed("the release has no Android download.");
    };
    if name.is_empty() || name.starts_with('.') || !name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return failed("the release's Android file name isn't a plain file name.");
    }
    if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return failed("the release's Android checksum isn't a SHA-256.");
    }
    json!({
        "action": "download",
        "version": latest,
        "url": format!("{BASE}/download/v{latest}/{name}"),
        "sha256": sha.to_ascii_lowercase(),
    })
}

/// The file's SHA-256 in lowercase hex, or None if it can't be read.
pub fn sha256_file(path: &str) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Does the downloaded file match the signed manifest's checksum?
pub fn file_matches(path: &str, sha256: &str) -> bool {
    sha256.len() == 64 && sha256_file(path).is_some_and(|h| h.eq_ignore_ascii_case(sha256))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(compare("1.1.0", "1.0.12"), Ordering::Greater);
        assert_eq!(compare("1.0.7", "1.0.7"), Ordering::Equal);
        assert_eq!(compare("1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare("1.0.10", "1.0.9"), Ordering::Greater);
        assert_eq!(compare("1.0.7", "1.0.8"), Ordering::Less);
        assert_eq!(compare(" 1.0.8 ", "1.0.7-debug"), Ordering::Greater);
        assert_eq!(compare("1.0.8", "0"), Ordering::Greater);
        assert_eq!(compare("1.x.0", "1.0.0"), Ordering::Equal);
    }

    const SHA: &str = "aa1f3e5b0c2d4e6f8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7";

    fn manifest(version: &str, name: &str, sha: &str) -> String {
        json!({"version": version, "files": {"android": {"name": name, "sha256": sha}}}).to_string()
    }

    #[test]
    fn plans() {
        let p = plan(&manifest("1.0.8", "thurea-android.apk", SHA), "1.0.7");
        assert_eq!(p["action"], "download");
        assert_eq!(p["version"], "1.0.8");
        assert_eq!(p["url"], "https://github.com/ShaddickDev/thurea/releases/download/v1.0.8/thurea-android.apk");
        assert_eq!(p["sha256"], SHA);
        assert_eq!(plan(&manifest("1.0.8", "thurea-android.apk", &SHA.to_uppercase()), "1.0.7")["sha256"], SHA);

        let p = plan(&manifest("1.0.7", "thurea-android.apk", SHA), "1.0.7");
        assert_eq!(p["action"], "current");
        assert_eq!(p["note"], "Thurea 1.0.7 is up to date.");
        assert_eq!(plan(&manifest("1.0.6", "thurea-android.apk", SHA), "1.0.7")["action"], "current");

        for bad in [
            "not json".to_string(),
            json!({"files": {}}).to_string(),
            manifest("1.0.8/../../x", "thurea-android.apk", SHA),
            manifest("", "thurea-android.apk", SHA),
            manifest("1.0.8", "../evil.apk", SHA),
            manifest("1.0.8", "a/b.apk", SHA),
            manifest("1.0.8", ".apk", SHA),
            manifest("1.0.8", "thurea-android.apk", "abc"),
            json!({"version": "1.0.8", "files": {}}).to_string(),
        ] {
            let p = plan(&bad, "1.0.7");
            assert_eq!(p["action"], "error", "{bad}");
            assert!(p["note"].as_str().unwrap().starts_with("Update check failed: "));
        }
    }

    #[test]
    fn checksums() {
        let path = std::env::temp_dir().join(format!("thurea-update-test-{}", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        let p = path.to_str().unwrap();
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(sha256_file(p).as_deref(), Some(abc));
        assert!(file_matches(p, abc));
        assert!(file_matches(p, &abc.to_uppercase()));
        assert!(!file_matches(p, SHA));
        assert!(!file_matches(p, ""));
        let _ = std::fs::remove_file(&path);
        assert!(!file_matches(p, abc));
        assert_eq!(sha256_file(p), None);
    }
}
