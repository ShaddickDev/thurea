//! Patching Twitch on the phone: which ReVanced patches release to use, whether its signature is
//! ReVanced's own, which Twitch version to patch, and whether a Twitch APK is signed by Twitch.
//!
//! Java does the downloads, runs the patcher and installs the result; every decision about what
//! to trust is made here, so it is the same code the tests run.
//!
//! Trust:
//! - The patches come from ReVanced's own API (`api.revanced.app/v5/patches`: GitHub has blocked
//!   their repository) with a detached OpenPGP signature. It must be made by ReVanced's key,
//!   whose primary key is pinned by fingerprint in [`REVANCED_PRIMARY`]. ReVanced adds signing
//!   subkeys over time, so Java also fetches the current key (`/v5/patches/keys`): it counts only
//!   if its primary key is the pinned one, and only subkeys that primary key has bound are used.
//!   A copy of the key is built in ([`REVANCED_KEY`]) for when that fetch fails.
//! - The Twitch APK can come from anywhere (it's fetched from APKMirror): it is used only if its
//!   signing certificate is Twitch's own, pinned in [`TWITCH_CERTS`]. APK signatures cover the
//!   whole file, so a mirror can't change a byte of it without that check failing.

use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use pgp::types::KeyDetails;
use serde_json::{json, Value};
use std::cmp::Ordering;

/// Twitch's package. ReVanced's Twitch keeps it (no rename), so the patched app replaces the
/// official one.
pub const PACKAGE: &str = "tv.twitch.android.app";

/// ReVanced's API: where the latest patches release, its signature and ReVanced's key are read.
pub const API: &str = "https://api.revanced.app/v5/";

/// Fingerprint of ReVanced's primary key ("ReVanced <contact@revanced.app>", ed25519, 2023).
/// Read from api.revanced.app and confirmed against keyserver.ubuntu.com.
pub const REVANCED_PRIMARY: &str = "A7835DFCACA14BDA3CD6EDD3633C6920FBE6A2FF";

/// ReVanced's public key as api.revanced.app served it when this Thurea was built.
pub const REVANCED_KEY: &str = include_str!("../keys/revanced-patches.asc");

/// SHA-256 of Twitch's APK signing certificate (self-signed, O=Twitch, valid 2015-2040; the
/// same on every release checked, 24.8.0 to 31.0.1).
pub const TWITCH_CERTS: &[&str] = &["1338f9b049893cc70b78432a177582f90bd4bc6296ea4ed35bcc7df59687ac53"];

/// A plain version number ("25.3.0"): it goes into download addresses and file names.
fn plain(v: &str) -> bool {
    !v.is_empty() && v.len() <= 32 && v.chars().all(|c| c.is_ascii_digit() || c == '.') && !v.starts_with('.') && !v.contains("..")
}

/// `{"tag", "rvp", "asc"}` from ReVanced's `/v5/patches` answer: the version without its "v",
/// and where the patches and their signature download from (only from ReVanced's API).
/// `{}` for anything else.
pub fn patches_release(body: &str) -> Value {
    let Ok(r) = serde_json::from_str::<Value>(body) else { return json!({}) };
    let Some(tag) = r["version"].as_str() else { return json!({}) };
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    let (Some(rvp), Some(asc)) = (r["download_url"].as_str(), r["signature_download_url"].as_str()) else { return json!({}) };
    let ours = |u: &str| u.starts_with(API) && !u[API.len()..].contains(['"', '\\', '?', '#', '@']) && !u.contains("..");
    if !plain(tag) || !ours(rvp) || !ours(asc) {
        return json!({});
    }
    json!({"tag": tag, "rvp": rvp, "asc": asc})
}

/// ReVanced's key from a `/v5/patches/keys` answer (`{"patches_public_key": "<armored>"}`), if its
/// primary key is the pinned one and it is otherwise valid; else the built-in copy.
pub fn current_key(keys_body: &str) -> SignedPublicKey {
    let fetched = serde_json::from_str::<Value>(keys_body)
        .ok()
        .and_then(|v| v["patches_public_key"].as_str().map(str::to_string))
        .and_then(|k| pinned_key(&k, REVANCED_PRIMARY).ok());
    fetched.unwrap_or_else(|| pinned_key(REVANCED_KEY, REVANCED_PRIMARY).expect("built-in ReVanced key"))
}

/// `key` (armored) if its primary key has fingerprint `primary` and its bindings are valid.
pub fn pinned_key(key: &str, primary: &str) -> Result<SignedPublicKey, String> {
    let (key, _) = SignedPublicKey::from_string(key).map_err(|e| format!("the key can't be read: {e}"))?;
    if !format!("{:X}", key.primary_key.fingerprint()).eq_ignore_ascii_case(primary) {
        return Err("the key isn't the pinned key".into());
    }
    key.verify_bindings().map_err(|e| format!("the key isn't valid: {e}"))?;
    Ok(key)
}

/// Is `sig` (a detached signature, binary or armored) a valid signature over `data` by `key` or
/// one of the subkeys it has bound? Err says why not.
pub fn verify_with(key: &SignedPublicKey, data: &[u8], sig: &[u8]) -> Result<(), String> {
    let (sig, _) = DetachedSignature::from_reader_single(sig).map_err(|_| "the signature file isn't an OpenPGP signature".to_string())?;
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

/// Are the patches at `rvp` signed by ReVanced? `sig`: the downloaded signature file;
/// `keys_body`: the `/v5/patches/keys` answer ("" if it couldn't be fetched).
pub fn verify_patches(rvp: &str, sig: &str, keys_body: &str) -> Result<(), String> {
    let data = std::fs::read(rvp).map_err(|e| format!("can't read the patches: {e}"))?;
    let sig = std::fs::read(sig).map_err(|e| format!("can't read the signature: {e}"))?;
    verify_with(&current_key(keys_body), &data, &sig)
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
    const TEST_SIG: &[u8] = include_bytes!("../keys/test/data.rvp.asc");
    const TEST_SIG_BINARY: &[u8] = include_bytes!("../keys/test/data.rvp.sig");
    const OTHER_SIG: &[u8] = include_bytes!("../keys/test/other.asc");

    fn test_key() -> SignedPublicKey {
        let (k, _) = SignedPublicKey::from_string(TEST_KEY).unwrap();
        pinned_key(TEST_KEY, &format!("{:X}", k.primary_key.fingerprint())).unwrap()
    }

    #[test]
    fn signatures() {
        let key = test_key();
        assert_eq!(verify_with(&key, TEST_DATA, TEST_SIG), Ok(()));
        // ReVanced's are binary, despite the .asc name.
        assert_eq!(verify_with(&key, TEST_DATA, TEST_SIG_BINARY), Ok(()));
        // Changed data, another key's signature, or no signature at all.
        let mut changed = TEST_DATA.to_vec();
        changed[0] ^= 1;
        assert!(verify_with(&key, &changed, TEST_SIG).is_err());
        assert!(verify_with(&key, &changed, TEST_SIG_BINARY).is_err());
        assert!(verify_with(&key, TEST_DATA, OTHER_SIG).is_err());
        assert!(verify_with(&key, TEST_DATA, b"").is_err());
        assert!(verify_with(&key, TEST_DATA, b"-----BEGIN PGP SIGNATURE-----\n\nnope\n-----END PGP SIGNATURE-----\n").is_err());
        // Signed by the test key, not ReVanced's.
        assert!(verify_with(&current_key(""), TEST_DATA, TEST_SIG).is_err());
    }

    #[test]
    fn revanced_key() {
        let key = pinned_key(REVANCED_KEY, REVANCED_PRIMARY).expect("the built-in key is ReVanced's");
        // The subkey that signed patches 6.2.1 (June 2026).
        assert!(key.public_subkeys.iter().any(|s| format!("{:X}", s.key.fingerprint()) == "61C079D8CD06B2B14B4A462F1EDEE52215B296C4"));
        // A key from the API counts only with the pinned primary key; otherwise the built-in one is used.
        let other = json!({"patches_public_key": TEST_KEY}).to_string();
        assert_eq!(current_key(&other).primary_key.fingerprint(), key.primary_key.fingerprint());
        assert_eq!(current_key("not json").primary_key.fingerprint(), key.primary_key.fingerprint());
        let same = json!({"patches_public_key": REVANCED_KEY}).to_string();
        assert_eq!(current_key(&same).public_subkeys.len(), key.public_subkeys.len());
        assert!(pinned_key(TEST_KEY, REVANCED_PRIMARY).is_err());
    }

    #[test]
    fn real_revanced_signature() {
        // ReVanced's signature on patches 6.2.1 (binary, from api.revanced.app/v5/patches.rvp.asc).
        let sig: &[u8] = include_bytes!("../keys/test/revanced-6.2.1.rvp.asc");
        let (d, _) = DetachedSignature::from_reader_single(sig).expect("a signature");
        let issuer = d.signature.issuer_fingerprint().into_iter().map(|f| format!("{f:X}")).collect::<Vec<_>>();
        assert_eq!(issuer, ["61C079D8CD06B2B14B4A462F1EDEE52215B296C4"]);
        // Not a signature over anything else.
        assert!(verify_with(&current_key(""), TEST_DATA, sig).is_err());
    }

    #[test]
    fn patches_releases() {
        let good = json!({"version": "v6.2.1", "created_at": "2026-06-02T18:30:00",
            "download_url": "https://api.revanced.app/v5/patches.rvp",
            "signature_download_url": "https://api.revanced.app/v5/patches.rvp.asc"});
        assert_eq!(
            patches_release(&good.to_string()),
            json!({"tag": "6.2.1", "rvp": "https://api.revanced.app/v5/patches.rvp", "asc": "https://api.revanced.app/v5/patches.rvp.asc"})
        );
        // Files from elsewhere, odd versions, missing parts.
        let mut bad = good.clone();
        bad["download_url"] = json!("https://example.com/v5/patches.rvp");
        assert_eq!(patches_release(&bad.to_string()), json!({}));
        let mut bad = good.clone();
        bad["signature_download_url"] = json!("https://api.revanced.app.example.com/x.asc");
        assert_eq!(patches_release(&bad.to_string()), json!({}));
        let mut bad = good.clone();
        bad["download_url"] = json!("https://api.revanced.app/v5/../x.rvp");
        assert_eq!(patches_release(&bad.to_string()), json!({}));
        let mut bad = good.clone();
        bad["version"] = json!("v6.2.1-dev.3");
        assert_eq!(patches_release(&bad.to_string()), json!({}));
        let mut bad = good.clone();
        bad.as_object_mut().unwrap().remove("signature_download_url");
        assert_eq!(patches_release(&bad.to_string()), json!({}));
        assert_eq!(patches_release("not json"), json!({}));
    }

    #[test]
    fn twitch_certs() {
        let twitch = TWITCH_CERTS[0];
        assert!(genuine_twitch(&json!([twitch.to_uppercase()]).to_string()));
        assert!(genuine_twitch(&json!(["00", twitch]).to_string()));
        assert!(!genuine_twitch(&json!(["0".repeat(64)]).to_string()));
        assert!(!genuine_twitch("[]"));
        assert!(!genuine_twitch("nope"));
    }

    #[test]
    fn plans() {
        let compat = r#"["16.9.1","25.3.0"]"#;
        assert_eq!(plan("{}", "6.2.1", compat), json!({"action": "patch", "patches": "6.2.1", "twitch": "25.3.0", "fresh": false}));
        assert_eq!(plan(r#"{"patches":"6.2.1","twitch":"25.3.0"}"#, "6.2.1", compat)["action"], "current");
        // New patches, same Twitch: patch again.
        assert_eq!(plan(r#"{"patches":"6.2.0","twitch":"25.3.0"}"#, "6.2.1", compat)["twitch"], "25.3.0");
        // Patches that only support an older Twitch than the installed one.
        assert_eq!(plan(r#"{"patches":"6.2.0","twitch":"31.0.1"}"#, "6.2.1", compat)["fresh"], true);
        assert_eq!(plan("{}", "6.2.1", "[]")["action"], "error");
        assert_eq!(plan("{}", "6.2.1", "null")["action"], "error");
        assert_eq!(plan("{}", "v6", compat)["action"], "error");
        assert_eq!(newest(&["25.10.0".into(), "25.9.0".into(), "x".into()]), Some("25.10.0".into()));
    }
}
