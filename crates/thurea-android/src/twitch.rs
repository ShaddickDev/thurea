//! Patching Twitch on the phone: which ReVanced patches release to use, whether its signature is
//! ReVanced's own, which Twitch version to patch, and whether a Twitch APK is signed by Twitch.
//!
//! Java does the downloads, runs the patcher and installs the result; every decision about what
//! to trust is made here, so it is the same code the tests run.
//!
//! Trust:
//! - The patches (`patches-<version>.rvp`) must come with ReVanced's detached OpenPGP signature
//!   (`.rvp.asc`, published with every release) made by ReVanced's key, pinned in
//!   `keys/revanced-patches.asc`.
//! - The Twitch APK can come from anywhere (it's fetched from APKMirror): it is used only if its
//!   signing certificate is Twitch's own, pinned in [`TWITCH_CERTS`]. APK signatures cover the
//!   whole file, so a mirror can't change a byte of it without that check failing.

use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use serde_json::{json, Value};
use std::cmp::Ordering;

/// Twitch's package. ReVanced's Twitch keeps it (no rename), so the patched app replaces the
/// official one.
pub const PACKAGE: &str = "tv.twitch.android.app";

/// Where ReVanced's patches releases are downloaded from: nothing else is accepted.
pub const PATCHES_DOWNLOADS: &str = "https://github.com/ReVanced/revanced-patches/releases/download/";

/// ReVanced's public key, which signs every patches release (`.rvp.asc`).
pub const REVANCED_KEY: &str = include_str!("../keys/revanced-patches.asc");

/// SHA-256 of Twitch's APK signing certificate(s), lower-case hex.
pub const TWITCH_CERTS: &[&str] = &[include_str!("../keys/twitch-cert.sha256").trim_ascii()];

/// A plain version number ("25.3.0"): it goes into download addresses and file names.
fn plain(v: &str) -> bool {
    !v.is_empty() && v.len() <= 32 && v.chars().all(|c| c.is_ascii_digit() || c == '.') && !v.starts_with('.') && !v.contains("..")
}

/// `{"tag", "rvp", "asc"}` for a GitHub "latest release" answer from ReVanced's patches: the tag
/// without its "v", and the download addresses of `patches-<tag>.rvp` and its signature.
/// `{}` for a draft, a pre-release, a tag that isn't a plain version, or missing files.
pub fn patches_release(body: &str) -> Value {
    let Ok(r) = serde_json::from_str::<Value>(body) else { return json!({}) };
    if r["draft"].as_bool() == Some(true) || r["prerelease"].as_bool() == Some(true) {
        return json!({});
    }
    let Some(tag) = r["tag_name"].as_str() else { return json!({}) };
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    if !plain(tag) {
        return json!({});
    }
    let rvp = format!("{PATCHES_DOWNLOADS}v{tag}/patches-{tag}.rvp");
    let asc = format!("{rvp}.asc");
    let urls: Vec<&str> = r["assets"].as_array().into_iter().flatten().filter_map(|a| a["browser_download_url"].as_str()).collect();
    if urls.contains(&rvp.as_str()) && urls.contains(&asc.as_str()) {
        json!({"tag": tag, "rvp": rvp, "asc": asc})
    } else {
        json!({})
    }
}

/// Is `asc` (an armored detached signature) a valid signature over `data` by `key` (an armored
/// public key) or one of its subkeys? Err says why not.
pub fn verify_with(key: &str, data: &[u8], asc: &str) -> Result<(), String> {
    let (key, _) = SignedPublicKey::from_string(key).map_err(|e| format!("the pinned key can't be read: {e}"))?;
    key.verify_bindings().map_err(|e| format!("the pinned key isn't valid: {e}"))?;
    let (sig, _) = DetachedSignature::from_string(asc).map_err(|_| "the signature file isn't an OpenPGP signature".to_string())?;
    if sig.verify(&key.primary_key, data).is_ok() {
        return Ok(());
    }
    for sub in &key.public_subkeys {
        if sub.verify_bindings(&key.primary_key).is_ok() && sig.verify(&sub.key, data).is_ok() {
            return Ok(());
        }
    }
    Err("the signature isn't ReVanced's".into())
}

/// Is the patches file at `path` signed by ReVanced (`asc`: the release's `.rvp.asc`)?
pub fn verify_patches(path: &str, asc: &str) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| format!("can't read the patches: {e}"))?;
    verify_with(REVANCED_KEY, &data, asc)
}

/// Is one of `certs` (SHA-256 hex of an APK's signing certificates) one of `pinned`?
pub fn cert_in(certs: &[String], pinned: &[&str]) -> bool {
    certs.iter().any(|c| pinned.iter().any(|p| c.trim().eq_ignore_ascii_case(p)))
}

/// Is an APK signed by Twitch? `certs_json`: its signing certificates' SHA-256s, a JSON array.
pub fn genuine_twitch(certs_json: &str) -> bool {
    let certs: Vec<String> = serde_json::from_str(certs_json).unwrap_or_default();
    cert_in(&certs, TWITCH_CERTS)
}

/// The newest of `versions` that is a plain version number.
pub fn newest(versions: &[String]) -> Option<String> {
    versions.iter().filter(|v| plain(v)).max_by(|a, b| crate::update::compare(a, b)).cloned()
}

/// What to do about the patched Twitch.
///
/// `have`: what Thurea has patched and pinned now, `{"patches", "twitch"}` (empty strings, or `{}`,
/// when nothing is). `tag`: the newest verified patches release. `compatible`: the Twitch
/// versions those patches name, a JSON array (from the patcher).
///
/// `{"action":"current","note"}`, `{"action":"error","note"}`, or
/// `{"action":"patch","patches","twitch","fresh"}`: patch Twitch `twitch` with patches `patches`;
/// `fresh` when it would be older than the installed Twitch (Android can't install an older
/// version on top, so the old one is removed first and Twitch asks you to sign in again).
pub fn plan(have: &str, tag: &str, compatible: &str) -> Value {
    let have: Value = serde_json::from_str(have).unwrap_or_default();
    let (have_patches, have_twitch) = (have["patches"].as_str().unwrap_or(""), have["twitch"].as_str().unwrap_or(""));
    if !plain(tag) {
        return json!({"action": "error", "note": "The patches release has no plain version number."});
    }
    let versions: Vec<String> = serde_json::from_str(compatible).unwrap_or_default();
    let Some(twitch) = newest(&versions) else {
        return json!({"action": "error", "note": format!("ReVanced patches {tag} don't name a Twitch version to patch.")});
    };
    if have_patches == tag && have_twitch == twitch {
        return json!({"action": "current", "note": format!("Twitch {twitch} is patched with ReVanced patches {tag}.")});
    }
    let fresh = !have_twitch.is_empty() && crate::update::compare(&twitch, have_twitch) == Ordering::Less;
    json!({"action": "patch", "patches": tag, "twitch": twitch, "fresh": fresh})
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway key and signatures made with gpg for these tests only (keys/test/).
    const TEST_KEY: &str = include_str!("../keys/test/key.asc");
    const TEST_DATA: &[u8] = include_bytes!("../keys/test/data.rvp");
    const TEST_SIG: &str = include_str!("../keys/test/data.rvp.asc");
    const OTHER_SIG: &str = include_str!("../keys/test/other.asc");

    #[test]
    fn signatures() {
        assert_eq!(verify_with(TEST_KEY, TEST_DATA, TEST_SIG), Ok(()));
        // Changed data, another key's signature, or no signature at all.
        let mut changed = TEST_DATA.to_vec();
        changed[0] ^= 1;
        assert!(verify_with(TEST_KEY, &changed, TEST_SIG).is_err());
        assert!(verify_with(TEST_KEY, TEST_DATA, OTHER_SIG).is_err());
        assert!(verify_with(TEST_KEY, TEST_DATA, "").is_err());
        assert!(verify_with(TEST_KEY, TEST_DATA, "-----BEGIN PGP SIGNATURE-----\n\nnope\n-----END PGP SIGNATURE-----\n").is_err());
        // The test key isn't ReVanced's.
        assert!(verify_with(REVANCED_KEY, TEST_DATA, TEST_SIG).is_err());
    }

    #[test]
    fn pinned_key_reads() {
        let (key, _) = SignedPublicKey::from_string(REVANCED_KEY).expect("ReVanced's key");
        key.verify_bindings().expect("valid bindings");
    }

    fn release(tag: &str, names: &[&str]) -> String {
        let assets: Vec<Value> = names.iter().map(|n| json!({"browser_download_url": format!("{PATCHES_DOWNLOADS}{tag}/{n}")})).collect();
        json!({"tag_name": tag, "draft": false, "prerelease": false, "assets": assets}).to_string()
    }

    #[test]
    fn patches_releases() {
        let good = release("v5.47.0", &["patches-5.47.0.rvp", "patches-5.47.0.rvp.asc"]);
        assert_eq!(
            patches_release(&good),
            json!({"tag": "5.47.0",
                   "rvp": format!("{PATCHES_DOWNLOADS}v5.47.0/patches-5.47.0.rvp"),
                   "asc": format!("{PATCHES_DOWNLOADS}v5.47.0/patches-5.47.0.rvp.asc")})
        );
        // No signature, pre-releases, odd tags, files from elsewhere.
        assert_eq!(patches_release(&release("v5.47.0", &["patches-5.47.0.rvp"])), json!({}));
        assert_eq!(patches_release(&release("v6.1.1-dev.3", &["patches-6.1.1-dev.3.rvp", "patches-6.1.1-dev.3.rvp.asc"])), json!({}));
        let mut pre: Value = serde_json::from_str(&good).unwrap();
        pre["prerelease"] = json!(true);
        assert_eq!(patches_release(&pre.to_string()), json!({}));
        let elsewhere = json!({"tag_name": "v5.47.0", "assets": [
            {"browser_download_url": "https://example.com/v5.47.0/patches-5.47.0.rvp"},
            {"browser_download_url": "https://example.com/v5.47.0/patches-5.47.0.rvp.asc"}]});
        assert_eq!(patches_release(&elsewhere.to_string()), json!({}));
        assert_eq!(patches_release("not json"), json!({}));
        assert_eq!(patches_release(&release("v../5", &["patches-../5.rvp", "patches-../5.rvp.asc"])), json!({}));
    }

    #[test]
    fn twitch_certs() {
        let twitch = TWITCH_CERTS[0];
        assert_eq!(twitch.len(), 64);
        assert!(twitch.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(genuine_twitch(&json!([twitch.to_uppercase()]).to_string()));
        assert!(genuine_twitch(&json!(["00", twitch]).to_string()));
        assert!(!genuine_twitch(&json!(["0".repeat(64)]).to_string()));
        assert!(!genuine_twitch("[]"));
        assert!(!genuine_twitch("nope"));
    }

    #[test]
    fn plans() {
        let compat = r#"["16.9.1","25.3.0"]"#;
        assert_eq!(plan("{}", "5.47.0", compat), json!({"action": "patch", "patches": "5.47.0", "twitch": "25.3.0", "fresh": false}));
        assert_eq!(plan(r#"{"patches":"5.47.0","twitch":"25.3.0"}"#, "5.47.0", compat)["action"], "current");
        // New patches, same Twitch: patch again.
        assert_eq!(plan(r#"{"patches":"5.46.0","twitch":"25.3.0"}"#, "5.47.0", compat)["twitch"], "25.3.0");
        // Patches that only support an older Twitch than the installed one.
        assert_eq!(plan(r#"{"patches":"5.46.0","twitch":"26.0.0"}"#, "5.47.0", compat)["fresh"], true);
        assert_eq!(plan("{}", "5.47.0", "[]")["action"], "error");
        assert_eq!(plan("{}", "5.47.0", "null")["action"], "error");
        assert_eq!(plan("{}", "v5", compat)["action"], "error");
        assert_eq!(newest(&["25.10.0".into(), "25.9.0".into(), "x".into()]), Some("25.10.0".into()));
    }
}
