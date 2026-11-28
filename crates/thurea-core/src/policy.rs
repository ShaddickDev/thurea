//! The family policy. One document, enforced by every device in its own way, and kept in
//! step across the household by the mesh (the highest `version` wins).

use crate::crypto;
use crate::family::Family;
use crate::matcher::{normalize_host, DomainSet};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

const DEFAULT_BLOCKLIST: &str = include_str!("../../../policy/blocklist.txt");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Policy {
    /// Raised on every change; devices adopt the highest version they see.
    pub version: u64,
    pub family_dns: FamilyDns,
    pub blocklist: Vec<String>,
    pub browser: BrowserRules,
    pub android: AndroidRules,
    /// Parents, children's devices and children's rules. Empty: no family yet.
    ///
    /// Compatibility rule for every field added after 1.0: `#[serde(default,
    /// skip_serializing_if = …unset)]`. A parent's signature covers the policy exactly as written
    /// out, so a field that newer versions write even when unused would invalidate every policy
    /// signed by an older version (see `older_signatures_keep_verifying`).
    #[serde(default, skip_serializing_if = "Family::is_unset")]
    pub family: Family,
    /// The household's self-hosted relay ("host:port"), so devices reach each other from
    /// anywhere. Set by a parent; every device learns it with the policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<String>,
    /// A parent's signature over everything above. Required once the family has a parent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<Signature>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Signature {
    /// The signing parent's public key (hex).
    pub by: String,
    /// Ed25519 signature (hex) over the policy without this field.
    pub sig: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FamilyDns {
    /// DNS-over-TLS / DNS-over-HTTPS host.
    pub host: String,
    /// The filtering resolvers; any other DNS server counts as unfiltered.
    pub servers: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BrowserRules {
    pub safe_search: bool,
    /// 0 off, 1 moderate, 2 strict (Chrome's ForceYouTubeRestrict).
    pub youtube_restrict: u8,
    pub block_private_windows: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AndroidRules {
    /// Browsers hidden on sight (plus anything else that opens any web address).
    pub browsers: Vec<String>,
    /// Never hidden even though they open web addresses.
    pub keep: Vec<String>,
    /// The only apps allowed to install other apps (besides Thurea and the Play Store).
    pub installers: Vec<String>,
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            version: 1,
            family_dns: FamilyDns {
                host: "family.cloudflare-dns.com".into(),
                servers: strs(&["1.1.1.3", "1.0.0.3", "2606:4700:4700::1113", "2606:4700:4700::1003"]),
            },
            blocklist: DEFAULT_BLOCKLIST.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(String::from).collect(),
            browser: BrowserRules { safe_search: true, youtube_restrict: 1, block_private_windows: true },
            android: AndroidRules {
                browsers: strs(&[
                    "com.sec.android.app.sbrowser", "com.sec.android.app.sbrowser.beta", "org.mozilla.firefox",
                    "org.mozilla.firefox_beta", "org.mozilla.fenix", "org.mozilla.focus", "org.mozilla.klar",
                    "com.brave.browser", "com.brave.browser_beta", "com.opera.browser", "com.opera.mini.native",
                    "com.opera.gx", "com.opera.browser.beta", "com.microsoft.emmx", "com.duckduckgo.mobile.android",
                    "com.vivaldi.browser", "com.kiwibrowser.browser", "org.torproject.torbrowser", "org.torproject.android",
                    "net.mullvad.mullvadbrowser", "com.yandex.browser", "com.UCMobile.intl", "com.uc.browser.en",
                    "com.mi.globalbrowser", "mark.via.gp", "com.ecosia.android", "com.chrome.beta", "com.chrome.dev",
                    "com.chrome.canary", "org.chromium.chrome", "org.bromite.bromite", "org.cromite.cromite",
                    "us.spotco.fennec_dos", "io.github.forkmaintainers.iceraven", "com.cloudmosa.puffinFree",
                    "com.aloha.browser", "com.ghostery.android.ghostery", "com.stoutner.privacybrowser.standard",
                    "com.quark.browser", "com.huawei.browser", "com.android.browser", "com.cake.browser",
                    "org.adblockplus.browser",
                ]),
                keep: strs(&[
                    "com.android.chrome", "com.google.android.googlequicksearchbox", "com.google.android.youtube",
                    "tv.twitch.android.app", "ch.protonvpn.android", "com.google.android.webview", "com.android.vending",
                    "com.google.android.gms", "com.looker.droidify", "dev.imranr.obtainium", "dev.imranr.obtainium.fdroid",
                ]),
                installers: strs(&[
                    "com.looker.droidify", "dev.imranr.obtainium", "dev.imranr.obtainium.fdroid", "com.epicgames.portal",
                    "app.morphe.manager", "app.revanced.manager.flutter",
                ]),
            },
            family: Family::default(),
            relay: None,
            signature: None,
        }
    }
}

impl Policy {
    pub fn from_json(s: &str) -> Result<Self, String> {
        let p: Policy = serde_json::from_str(s).map_err(|e| format!("policy: {e}"))?;
        p.validate()?;
        Ok(p)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("policy serializes")
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&s)
    }

    /// Writes atomically (temp file + rename), so a crash never leaves half a policy.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.to_json())?;
        std::fs::rename(tmp, path)
    }

    /// Rejects policies that would switch protection off (no resolvers, garbage rules).
    pub fn validate(&self) -> Result<(), String> {
        if self.family_dns.servers.is_empty() || self.family_dns.host.trim().is_empty() {
            return Err("policy: family DNS is missing".into());
        }
        if let Some(bad) = self.blocklist.iter().find(|r| normalize_host(r).is_none()) {
            return Err(format!("policy: blocklist entry {bad:?} is not a domain"));
        }
        Ok(())
    }

    /// The bytes a parent signs: the whole policy without its signature.
    pub fn signed_bytes(&self) -> Vec<u8> {
        let mut p = self.clone();
        p.signature = None;
        serde_json::to_vec(&p).expect("policy serializes")
    }

    /// Signs as a parent (secret key hex). The signer must be one of the family's parents.
    pub fn sign(&mut self, parent_secret: &str) -> Result<(), String> {
        let by = crypto::parent_public(parent_secret).ok_or("that isn't a parent key")?;
        if !self.family.is_parent(&by) {
            return Err("this parent key isn't one of the family's parents".into());
        }
        self.signature = None;
        let sig = crypto::parent_sign(parent_secret, &self.signed_bytes()).ok_or("signing failed")?;
        self.signature = Some(Signature { by, sig });
        Ok(())
    }

    /// Signed by one of `parents` (and intact)?
    fn signed_by_one_of(&self, family: &Family) -> bool {
        self.signature.as_ref().is_some_and(|s| family.is_parent(&s.by) && crypto::parent_verify(&s.by, &self.signed_bytes(), &s.sig))
    }

    /// May this device, now enforcing `self`, switch to `offered`? (Newness is checked
    /// separately.) Without a family anything valid is accepted, as before. Once there is
    /// a parent, only a policy signed by one of *this* policy's parents is — so a child's
    /// device (or anyone with the household key) can't change the rules, add a parent or
    /// dissolve the family.
    pub fn accepts(&self, offered: &Policy) -> Result<(), String> {
        offered.validate()?;
        if self.family.parents.is_empty() {
            // First family: it must at least be signed by one of its own parents.
            if !offered.family.parents.is_empty() && !offered.signed_by_one_of(&offered.family) {
                return Err("family policy without a valid parent signature".into());
            }
            return Ok(());
        }
        if offered.signed_by_one_of(&self.family) {
            Ok(())
        } else {
            Err("not signed by a parent of this family".into())
        }
    }

    /// The version for a parent's change: the time, or one more than now if that's higher. Two
    /// parents changing things at once on devices that can't reach each other: the later change
    /// wins everywhere (not a random one).
    fn next_version(current: u64) -> u64 {
        (current + 1).max(crate::now())
    }

    /// A parent's change: applies `edit`, raises the version and signs.
    pub fn parent_edit(&mut self, parent_secret: &str, edit: impl FnOnce(&mut Policy)) -> Result<(), String> {
        let mut next = self.clone();
        edit(&mut next);
        next.version = Self::next_version(self.version);
        next.sign(parent_secret)?;
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// Starts a family with this parent (or adds a parent to an existing one, signed by
    /// `signer`, an existing parent).
    pub fn add_parent(&mut self, name: &str, parent_secret: &str, signer: Option<&str>) -> Result<(), String> {
        let key = crypto::parent_public(parent_secret).ok_or("that isn't a parent key")?;
        let signer = match signer {
            Some(s) => s.to_string(),
            None if self.family.parents.is_empty() => parent_secret.to_string(),
            None => return Err("the family already has parents: an existing parent must add you".into()),
        };
        let mut next = self.clone();
        if !next.family.is_parent(&key) {
            next.family.parents.push(crate::family::Parent { name: name.into(), key });
        }
        next.version = Self::next_version(self.version);
        next.sign(&signer)?;
        *self = next;
        Ok(())
    }

    /// Short fingerprint of the exact content, so devices can tell "same version, different rules".
    pub fn digest(&self) -> String {
        let canonical = serde_json::to_vec(self).expect("policy serializes");
        crypto::hex(&crypto::sha256(&canonical)[..8])
    }

    pub fn domains(&self) -> DomainSet {
        DomainSet::new(&self.blocklist)
    }

    pub fn doh_template(&self) -> String {
        doh_template(&self.family_dns.host)
    }

    pub fn ipv4_servers(&self) -> Vec<&str> {
        self.family_dns.servers.iter().map(String::as_str).filter(|s| !s.contains(':')).collect()
    }

    pub fn ipv6_servers(&self) -> Vec<&str> {
        self.family_dns.servers.iter().map(String::as_str).filter(|s| s.contains(':')).collect()
    }

    /// Adds a site; returns false if it was already covered. Raises the version.
    pub fn block(&mut self, site: &str) -> Result<bool, String> {
        let host = normalize_host(site).ok_or_else(|| format!("{site:?} is not a domain"))?;
        if self.domains().is_blocked(&host) {
            return Ok(false);
        }
        self.blocklist.push(host);
        self.version += 1;
        Ok(true)
    }

    /// Removes an exact rule; returns false if there was none. Raises the version.
    pub fn unblock(&mut self, site: &str) -> bool {
        let Some(host) = normalize_host(site) else { return false };
        let before = self.blocklist.len();
        self.blocklist.retain(|r| normalize_host(r).as_deref() != Some(host.as_str()));
        let changed = self.blocklist.len() != before;
        if changed {
            self.version += 1;
        }
        changed
    }

    /// Managed policy for Chrome, Chromium, Edge and Brave (same keys everywhere,
    /// including Android's application restrictions).
    pub fn chromium_policy(&self) -> Value {
        json!({
            "DnsOverHttpsMode": "secure",
            "DnsOverHttpsTemplates": self.doh_template(),
            "SafeSitesFilterBehavior": 1,
            "ForceGoogleSafeSearch": self.browser.safe_search,
            "ForceBingSafeSearch": if self.browser.safe_search { 2 } else { 0 },
            "ForceYouTubeRestrict": self.browser.youtube_restrict,
            "IncognitoModeAvailability": if self.browser.block_private_windows { 1 } else { 0 },
            "InPrivateModeAvailability": if self.browser.block_private_windows { 1 } else { 0 },
            "TorDisabled": true,
            "BrowserGuestModeEnabled": !self.browser.block_private_windows,
            "URLBlocklist": self.blocklist,
        })
    }

    /// Firefox enterprise policy (policies.json).
    pub fn firefox_policy(&self) -> Value {
        let block: Vec<String> = self.blocklist.iter().flat_map(|d| [format!("*://{d}/*"), format!("*://*.{d}/*")]).collect();
        json!({
            "policies": {
                "DNSOverHTTPS": { "Enabled": true, "ProviderURL": self.doh_template(), "Locked": true, "Fallback": false },
                "DisablePrivateBrowsing": self.browser.block_private_windows,
                "WebsiteFilter": { "Block": block },
                "BlockAboutConfig": true,
                "SearchSuggestEnabled": false,
            }
        })
    }

    /// The lines Thurea keeps in a hosts file (both IPv4 and IPv6 sinkholes).
    pub fn hosts_entries(&self) -> String {
        let mut s = String::new();
        for d in &self.blocklist {
            for name in [d.clone(), format!("www.{d}")] {
                s.push_str(&format!("0.0.0.0 {name}\n:: {name}\n"));
            }
        }
        s
    }
}

/// The DNS-over-HTTPS address for a family DNS, from its DNS-over-TLS host name. CleanBrowsing
/// serves DoH on one host with a path per filter, not on its DoT host names.
pub fn doh_template(host: &str) -> String {
    match host.strip_suffix("-dns.cleanbrowsing.org") {
        Some(filter) if !filter.is_empty() => format!("https://doh.cleanbrowsing.org/doh/{filter}/"),
        _ => format!("https://{host}/dns-query"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doh_templates() {
        assert_eq!(doh_template("family.cloudflare-dns.com"), "https://family.cloudflare-dns.com/dns-query");
        assert_eq!(doh_template("family-filter-dns.cleanbrowsing.org"), "https://doh.cleanbrowsing.org/doh/family-filter/");
        assert_eq!(doh_template(crate::family::CHILD_DNS_HOST), "https://doh.cleanbrowsing.org/doh/family-filter/");
    }

    #[test]
    fn default_policy_is_valid_and_matches_android_list() {
        let p = Policy::default();
        p.validate().unwrap();
        assert_eq!(p.blocklist.len(), 1000);
        assert!(p.domains().is_blocked("www.pornhub.com"));
        assert_eq!(p.ipv4_servers(), ["1.1.1.3", "1.0.0.3"]);
    }

    #[test]
    fn json_round_trip_and_digest() {
        let p = Policy::default();
        let q = Policy::from_json(&p.to_json()).unwrap();
        assert_eq!(p, q);
        assert_eq!(p.digest(), q.digest());
        let mut r = q.clone();
        r.blocklist.pop();
        assert_ne!(p.digest(), r.digest());
    }

    #[test]
    fn block_and_unblock_bump_version() {
        let mut p = Policy::default();
        assert_eq!(p.block("https://Example.org/x"), Ok(true));
        assert_eq!(p.version, 2);
        assert_eq!(p.block("www.example.org"), Ok(false)); // already covered
        assert!(p.unblock("example.org"));
        assert_eq!(p.version, 3);
        assert!(!p.unblock("example.org"));
        assert!(p.block("not a domain").is_err());
    }

    #[test]
    fn only_parents_change_a_family() {
        let base = Policy::default();
        let (mum, mum_pub) = crypto::new_parent_key();
        let (stranger, _) = crypto::new_parent_key();

        // A new family is accepted by a device without one, if signed by its own parent.
        let mut fam = base.clone();
        fam.add_parent("Mum", &mum, None).unwrap();
        assert_eq!(fam.family.parents[0].key, mum_pub);
        assert!(base.accepts(&fam).is_ok());
        let mut forged = fam.clone();
        forged.signature = None;
        assert!(base.accepts(&forged).is_err());

        // Mum's edits are accepted.
        let mut next = fam.clone();
        next.parent_edit(&mum, |p| p.family.set_role("kid", "Kid", crate::family::Role::Child, None)).unwrap();
        assert!(fam.accepts(&next).is_ok());

        // Anyone else's are not: unsigned, signed by a stranger, a stranger adding themselves.
        let mut unsigned = next.clone();
        unsigned.family.devices.clear();
        unsigned.version += 1;
        unsigned.signature = None;
        assert!(next.accepts(&unsigned).is_err());
        let mut tampered = next.clone();
        tampered.family.devices.clear();
        tampered.version += 1; // signature no longer matches
        assert!(next.accepts(&tampered).is_err());
        let mut takeover = next.clone();
        takeover.family.parents.push(crate::family::Parent { name: "X".into(), key: crypto::parent_public(&stranger).unwrap() });
        takeover.version += 1;
        takeover.sign(&stranger).unwrap();
        assert!(next.accepts(&takeover).is_err());
        assert!(takeover.add_parent("Y", &stranger, None).is_err());

        // Mum can add Dad; Dad's edits are then accepted.
        let (dad, _) = crypto::new_parent_key();
        let mut two = next.clone();
        two.add_parent("Dad", &dad, Some(&mum)).unwrap();
        assert!(next.accepts(&two).is_ok());
        let mut by_dad = two.clone();
        by_dad.parent_edit(&dad, |p| { p.block("example.org").unwrap(); }).unwrap();
        assert!(two.accepts(&by_dad).is_ok());

        // Signatures survive the JSON round trip.
        let back = Policy::from_json(&by_dad.to_json()).unwrap();
        assert!(two.accepts(&back).is_ok());
    }

    /// A policy signed by a version that didn't know about newer fields must still verify:
    /// unused new fields aren't written out, so the signed bytes come out the same.
    #[test]
    fn older_signatures_keep_verifying() {
        let (mum, _) = crypto::new_parent_key();
        let mut p = Policy::default();
        p.add_parent("Mum", &mum, None).unwrap();
        p.parent_edit(&mum, |x| x.family.set_role("kid", "Kid", crate::family::Role::Child, Some("Sam"))).unwrap();
        let json = p.to_json();
        // What a 1.0-era policy looks like: nothing about screen time, relays or parent requests.
        for k in ["\"screen\"", "\"relay\""] {
            assert!(!json.contains(k), "{k} written out while unused");
        }
        // Round trip through JSON (as it travels) and check the signature again.
        let back = Policy::from_json(&json).unwrap();
        assert!(Policy::default().accepts(&back).is_ok());
        // No family at all: nothing written.
        assert!(!Policy::default().to_json().contains("\"family\""));
    }

    #[test]
    fn rejects_policies_without_dns() {
        let mut p = Policy::default();
        p.family_dns.servers.clear();
        assert!(Policy::from_json(&p.to_json()).is_err());
    }
}
