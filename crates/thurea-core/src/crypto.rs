//! Household key, HMAC-SHA256, parent signatures (Ed25519), mesh encryption
//! (XChaCha20-Poly1305) and hex helpers.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub const KEY_LEN: usize = 32;

/// A fresh random household key.
pub fn new_key() -> [u8; KEY_LEN] {
    let mut k = [0u8; KEY_LEN];
    getrandom::getrandom(&mut k).expect("no system randomness");
    k
}

/// A random device id (16 hex characters).
pub fn new_device_id() -> String {
    let mut b = [0u8; 8];
    getrandom::getrandom(&mut b).expect("no system randomness");
    hex(&b)
}

pub fn mac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut m = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    m.update(data);
    m.finalize().into_bytes().into()
}

/// Constant-time check of a MAC.
pub fn verify(key: &[u8], data: &[u8], tag: &[u8]) -> bool {
    let mut m = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    m.update(data);
    m.verify_slice(tag).is_ok()
}

// ── Parent keys: only a parent's signature can change a family's rules ──

/// A new parent signing key: (secret hex, public hex).
pub fn new_parent_key() -> (String, String) {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).expect("no system randomness");
    let sk = SigningKey::from_bytes(&seed);
    (hex(&seed), hex(sk.verifying_key().as_bytes()))
}

/// The public half of a parent's secret key (hex), or None if it isn't one.
pub fn parent_public(secret_hex: &str) -> Option<String> {
    let seed: [u8; 32] = unhex(secret_hex)?.try_into().ok()?;
    Some(hex(SigningKey::from_bytes(&seed).verifying_key().as_bytes()))
}

/// Signs with a parent's secret key (hex); the signature is hex.
pub fn parent_sign(secret_hex: &str, data: &[u8]) -> Option<String> {
    let seed: [u8; 32] = unhex(secret_hex)?.try_into().ok()?;
    Some(hex(&SigningKey::from_bytes(&seed).sign(data).to_bytes()))
}

/// Checks a parent's signature (all hex). Strict: no malleable signatures.
pub fn parent_verify(public_hex: &str, data: &[u8], sig_hex: &str) -> bool {
    let Some(pk) = unhex(public_hex).and_then(|b| <[u8; 32]>::try_from(b).ok()) else { return false };
    let Some(sig) = unhex(sig_hex).and_then(|b| <[u8; 64]>::try_from(b).ok()) else { return false };
    let Ok(vk) = VerifyingKey::from_bytes(&pk) else { return false };
    vk.verify_strict(data, &ed25519_dalek::Signature::from_bytes(&sig)).is_ok()
}

// ── Mesh encryption: what devices tell each other (usage, requests) stays in the family ──

/// The encryption key for mesh traffic, derived from the household key.
fn mesh_key(household: &[u8]) -> [u8; 32] {
    mac(household, b"thurea mesh encryption v2")
}

/// Encrypts (and authenticates) with the household key: 24-byte nonce || ciphertext.
pub fn encrypt(household: &[u8], plain: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(&mesh_key(household).into());
    let mut nonce = [0u8; 24];
    getrandom::getrandom(&mut nonce).expect("no system randomness");
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(XNonce::from_slice(&nonce), plain).expect("encryption can't fail"));
    out
}

/// Decrypts what [`encrypt`] made; None if the key is wrong or anything was changed.
pub fn decrypt(household: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < 24 + 16 {
        return None;
    }
    let cipher = XChaCha20Poly1305::new(&mesh_key(household).into());
    cipher.decrypt(XNonce::from_slice(&sealed[..24]), &sealed[24..]).ok()
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn hex(b: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(D[(x >> 4) as usize] as char);
        s.push(D[(x & 15) as usize] as char);
    }
    s
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Parses a pairing code into a household key.
pub fn parse_key(s: &str) -> Option<Vec<u8>> {
    let k = unhex(&s.replace(['-', ' '], ""))?;
    (k.len() == KEY_LEN).then_some(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let k = new_key();
        assert_eq!(unhex(&hex(&k)).unwrap(), k);
        assert_eq!(parse_key(&hex(&k)).unwrap(), k);
        assert!(parse_key("abcd").is_none());
        assert!(unhex("zz").is_none());
    }

    #[test]
    fn parent_signatures() {
        let (sk, pk) = new_parent_key();
        assert_eq!(parent_public(&sk).unwrap(), pk);
        let sig = parent_sign(&sk, b"rules").unwrap();
        assert!(parent_verify(&pk, b"rules", &sig));
        assert!(!parent_verify(&pk, b"rulez", &sig));
        let (_, other) = new_parent_key();
        assert!(!parent_verify(&other, b"rules", &sig));
        assert!(!parent_verify("zz", b"rules", &sig));
    }

    #[test]
    fn encryption_round_trip_and_tamper() {
        let k = new_key();
        let c = encrypt(&k, b"usage report");
        assert!(!c.windows(5).any(|w| w == b"usage"));
        assert_eq!(decrypt(&k, &c).unwrap(), b"usage report");
        assert!(decrypt(&new_key(), &c).is_none());
        let mut t = c.clone();
        let n = t.len() - 1;
        t[n] ^= 1;
        assert!(decrypt(&k, &t).is_none());
        assert!(decrypt(&k, b"short").is_none());
    }

    #[test]
    fn mac_verifies_and_rejects() {
        let k = new_key();
        let t = mac(&k, b"hello");
        assert!(verify(&k, b"hello", &t));
        assert!(!verify(&k, b"hellp", &t));
        assert!(!verify(&new_key(), b"hello", &t));
    }
}

/// URL-safe base64 without padding (Android's provisioning checksums use it).
pub fn base64url(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..=c.len() {
            out.push(A[(n >> (18 - 6 * i)) as usize & 63] as char);
        }
    }
    out
}

/// Decodes [`base64url`] (no padding); None on anything else.
pub fn unbase64url(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        } as u32)
    };
    let b = s.as_bytes();
    if b.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(b.len() * 3 / 4);
    for chunk in b.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod b64 {
    #[test]
    fn base64url_matches_rfc4648() {
        for (i, o) in [("", ""), ("f", "Zg"), ("fo", "Zm8"), ("foo", "Zm9v"), ("foob", "Zm9vYg"), ("fooba", "Zm9vYmE"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(super::base64url(i.as_bytes()), o);
        }
        assert_eq!(super::base64url(&[0xfb, 0xff]), "-_8");
        for n in 0..40usize {
            let d: Vec<u8> = (0..n as u8).map(|x| x.wrapping_mul(37)).collect();
            assert_eq!(super::unbase64url(&super::base64url(&d)).unwrap(), d);
        }
        assert!(super::unbase64url("a").is_none());
        assert!(super::unbase64url("ab!c").is_none());
    }
}
