//! The family: parents, which devices belong to children, and each child's rules.
//!
//! Part of the family policy, so it travels with it through the mesh. Once a family has a
//! parent, only a parent's signature can change any of it (see [`crate::policy::Policy::accepts`]):
//! a child's device can ask (a [`Request`]), but never decide.
//!
//! Adults' devices keep today's protection, which the family rules never loosen. Children's
//! devices also get: YouTube Restricted Mode (strict), a family DNS that enforces it for every
//! app, social media blocked until a parent approves, new apps held until a parent approves,
//! and no factory reset / safe mode / clock changes.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Family DNS for children: CleanBrowsing's Family filter blocks adult sites and enforces
/// SafeSearch and YouTube Restricted Mode at the DNS level, so it also covers the YouTube app.
pub const CHILD_DNS_HOST: &str = "family-filter-dns.cleanbrowsing.org";
pub const CHILD_DNS_SERVERS: [&str; 4] = ["185.228.168.168", "185.228.169.168", "2a0d:2a00:1::", "2a0d:2a00:2::"];

/// How many answered request ids a child's rules remember (so a request isn't asked twice).
const DECIDED_KEEP: usize = 200;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Family {
    /// Parents' public signing keys. Empty: no family yet (every device is an adult's).
    #[serde(default)]
    pub parents: Vec<Parent>,
    /// Devices by mesh id. Devices not listed are adults' devices.
    #[serde(default)]
    pub devices: BTreeMap<String, Device>,
    /// Each child's rules, by the child's name.
    #[serde(default)]
    pub children: BTreeMap<String, ChildRules>,
    /// Answered requests that weren't a child's (devices asking to become a parent's).
    #[serde(default)]
    pub decided: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Parent {
    pub name: String,
    /// Ed25519 public key, hex.
    pub key: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    Adult,
    Child,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Device {
    pub name: String,
    pub role: Role,
    /// The child this device belongs to (role Child).
    #[serde(default)]
    pub child: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Allowed,
    Blocked,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Grant {
    /// A package name or a domain.
    pub item: String,
    /// Seconds since the epoch.
    pub until: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ChildRules {
    /// Chrome's ForceYouTubeRestrict: 1 moderate, 2 strict.
    pub youtube_restrict: u8,
    /// Catalog category → access ("social" blocked, "messaging" allowed by default).
    pub categories: BTreeMap<String, Access>,
    /// Apps a parent approved for good (package names).
    #[serde(default)]
    pub allowed_apps: Vec<String>,
    /// Sites a parent approved for good (domains).
    #[serde(default)]
    pub allowed_sites: Vec<String>,
    /// Approvals that run out by themselves.
    #[serde(default)]
    pub grants: Vec<Grant>,
    /// New apps stay frozen until a parent approves them.
    pub hold_new_apps: bool,
    /// No developer options / USB debugging on the child's phone.
    pub block_debugging: bool,
    /// Requests a parent has answered (approved or declined), newest last.
    #[serde(default)]
    pub decided: Vec<String>,
}

impl Default for ChildRules {
    fn default() -> Self {
        let mut categories = BTreeMap::new();
        for (c, a) in DEFAULT_ACCESS {
            categories.insert(c.to_string(), *a);
        }
        ChildRules {
            youtube_restrict: 2,
            categories,
            allowed_apps: Vec::new(),
            allowed_sites: Vec::new(),
            grants: Vec::new(),
            hold_new_apps: true,
            block_debugging: true,
            decided: Vec::new(),
        }
    }
}

/// What a category starts as for a new child.
pub const DEFAULT_ACCESS: &[(&str, Access)] = &[("social", Access::Blocked), ("messaging", Access::Allowed)];

/// Apps and sites Thurea knows by category. Packages are Android package names.
pub struct CatalogEntry {
    pub category: &'static str,
    pub label: &'static str,
    pub packages: &'static [&'static str],
    pub domains: &'static [&'static str],
}

pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry { category: "social", label: "TikTok", packages: &["com.zhiliaoapp.musically", "com.ss.android.ugc.trill", "com.zhiliaoapp.musically.go"], domains: &["tiktok.com"] },
    CatalogEntry { category: "social", label: "Instagram", packages: &["com.instagram.android", "com.instagram.lite"], domains: &["instagram.com"] },
    CatalogEntry { category: "social", label: "Threads", packages: &["com.instagram.barcelona"], domains: &["threads.net", "threads.com"] },
    CatalogEntry { category: "social", label: "Snapchat", packages: &["com.snapchat.android"], domains: &["snapchat.com"] },
    CatalogEntry { category: "social", label: "Facebook", packages: &["com.facebook.katana", "com.facebook.lite"], domains: &["facebook.com", "fb.com"] },
    CatalogEntry { category: "social", label: "X (Twitter)", packages: &["com.twitter.android"], domains: &["x.com", "twitter.com"] },
    CatalogEntry { category: "social", label: "Reddit", packages: &["com.reddit.frontpage"], domains: &["reddit.com"] },
    CatalogEntry { category: "social", label: "Discord", packages: &["com.discord"], domains: &["discord.com", "discord.gg"] },
    CatalogEntry { category: "social", label: "Pinterest", packages: &["com.pinterest"], domains: &["pinterest.com"] },
    CatalogEntry { category: "social", label: "Tumblr", packages: &["com.tumblr"], domains: &["tumblr.com"] },
    CatalogEntry { category: "social", label: "BeReal", packages: &["com.bereal.ft"], domains: &["bereal.com"] },
    CatalogEntry { category: "social", label: "Bluesky", packages: &["xyz.blueskyweb.app"], domains: &["bsky.app"] },
    CatalogEntry { category: "social", label: "Mastodon", packages: &["org.joinmastodon.android"], domains: &[] },
    CatalogEntry { category: "social", label: "Twitch", packages: &["tv.twitch.android.app"], domains: &["twitch.tv"] },
    CatalogEntry { category: "messaging", label: "WhatsApp", packages: &["com.whatsapp", "com.whatsapp.w4b"], domains: &["whatsapp.com"] },
    CatalogEntry { category: "messaging", label: "Signal", packages: &["org.thoughtcrime.securesms"], domains: &[] },
    CatalogEntry { category: "messaging", label: "Telegram", packages: &["org.telegram.messenger"], domains: &["telegram.org", "web.telegram.org"] },
    CatalogEntry { category: "messaging", label: "Messenger", packages: &["com.facebook.orca"], domains: &["messenger.com"] },
    CatalogEntry { category: "messaging", label: "Messages", packages: &["com.google.android.apps.messaging", "com.samsung.android.messaging"], domains: &[] },
];

/// The catalog entry for an app, if Thurea knows it.
pub fn catalog_app(package: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.packages.contains(&package))
}

/// The catalog entry for a site (or any subdomain of it), if Thurea knows it.
pub fn catalog_site(host: &str) -> Option<&'static CatalogEntry> {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    CATALOG.iter().find(|e| e.domains.iter().any(|d| h == *d || h.ends_with(&format!(".{d}"))))
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    App,
    Site,
    /// A device asking to become a parent's: `item` is its public key, `label` the parent's name.
    Parent,
}

/// Six digits both devices show while one asks to become a parent's, so the approving parent
/// can tell it's really that device's key (and not someone else's on the network).
pub fn match_code(key_hex: &str) -> String {
    let h = crate::crypto::sha256(key_hex.trim().as_bytes());
    let n = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

fn is_public_key(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A child asking a parent for an app or a site. Sent through the (encrypted) mesh; the
/// answer comes back as a parent-signed policy.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Request {
    pub id: String,
    /// The asking device's mesh id.
    pub device: String,
    pub child: String,
    /// "app" or "site". (Named `what` on the wire: mesh messages already use `kind`.)
    #[serde(rename = "what")]
    pub kind: ItemKind,
    /// Package name or domain.
    pub item: String,
    /// What to show the parent ("Instagram").
    pub label: String,
    pub at: u64,
}

impl Request {
    pub fn new(device: &str, child: &str, kind: ItemKind, item: &str, label: &str, now: u64) -> Self {
        Request { id: crate::crypto::new_device_id(), device: device.into(), child: child.into(), kind, item: item.into(), label: label.into(), at: now }
    }
}

/// Why an app may or may not open on a child's device.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum AppVerdict {
    Allowed,
    /// In a blocked category ("social").
    Blocked { category: String, label: String },
    /// New since the device became a child's, not approved yet.
    Held,
}

impl ChildRules {
    fn granted(&self, item: &str, now: u64) -> bool {
        self.grants.iter().any(|g| g.item == item && g.until > now)
    }

    /// Is this category's content allowed (unknown categories are)?
    pub fn category_allowed(&self, category: &str) -> bool {
        self.categories.get(category).copied().unwrap_or(Access::Allowed) == Access::Allowed
    }

    /// `known`: apps that were on the device when it became a child's (they aren't "new").
    pub fn app_verdict(&self, package: &str, known: bool, now: u64) -> AppVerdict {
        if self.allowed_apps.iter().any(|a| a == package) || self.granted(package, now) {
            return AppVerdict::Allowed;
        }
        if let Some(e) = catalog_app(package) {
            return if self.category_allowed(e.category) {
                AppVerdict::Allowed
            } else {
                AppVerdict::Blocked { category: e.category.into(), label: e.label.into() }
            };
        }
        if self.hold_new_apps && !known {
            AppVerdict::Held
        } else {
            AppVerdict::Allowed
        }
    }

    /// Sites blocked for this child right now (catalog sites of blocked categories, minus
    /// what a parent approved).
    pub fn blocked_sites(&self, now: u64) -> Vec<String> {
        let mut out = Vec::new();
        for e in CATALOG.iter().filter(|e| !self.category_allowed(e.category)) {
            for d in e.domains {
                let ok = self.allowed_sites.iter().any(|a| a == d) || self.granted(d, now);
                if !ok && !out.iter().any(|x| x == d) {
                    out.push(d.to_string());
                }
            }
        }
        out
    }

    /// The next moment something changes by itself (a grant running out).
    pub fn next_change(&self, now: u64) -> Option<u64> {
        self.grants.iter().map(|g| g.until).filter(|&u| u > now).min()
    }

    /// Approves a request: for good (`minutes` None) or for a while.
    pub fn approve(&mut self, r: &Request, minutes: Option<u64>, now: u64) {
        match minutes {
            Some(m) => self.grants.push(Grant { item: r.item.clone(), until: now + m * 60 }),
            None => {
                let list = if r.kind == ItemKind::App { &mut self.allowed_apps } else { &mut self.allowed_sites };
                if !list.contains(&r.item) {
                    list.push(r.item.clone());
                }
            }
        }
        self.decide(&r.id);
        self.prune(now);
    }

    pub fn decline(&mut self, r: &Request, now: u64) {
        self.decide(&r.id);
        self.prune(now);
    }

    fn decide(&mut self, id: &str) {
        if !self.decided.iter().any(|d| d == id) {
            self.decided.push(id.into());
        }
    }

    /// Forgets grants that ran out and old answers.
    pub fn prune(&mut self, now: u64) {
        self.grants.retain(|g| g.until > now);
        let n = self.decided.len();
        if n > DECIDED_KEEP {
            self.decided.drain(..n - DECIDED_KEEP);
        }
    }

    /// Takes an approval back.
    pub fn revoke(&mut self, item: &str) {
        self.allowed_apps.retain(|a| a != item);
        self.allowed_sites.retain(|a| a != item);
        self.grants.retain(|g| g.item != item);
    }
}

impl Family {
    pub fn is_parent(&self, key: &str) -> bool {
        self.parents.iter().any(|p| p.key == key)
    }

    pub fn role(&self, device: &str) -> Role {
        self.devices.get(device).map(|d| d.role).unwrap_or_default()
    }

    /// The child a device belongs to, with their rules (rules default if a parent hasn't set any).
    pub fn child_of(&self, device: &str) -> Option<(String, ChildRules)> {
        let d = self.devices.get(device)?;
        if d.role != Role::Child {
            return None;
        }
        let name = d.child.clone().unwrap_or_else(|| d.name.clone());
        let rules = self.children.get(&name).cloned().unwrap_or_default();
        Some((name, rules))
    }

    /// Has this request already been answered? (A parent request also counts as answered once
    /// its key is a parent's.)
    pub fn decided(&self, r: &Request) -> bool {
        if r.kind == ItemKind::Parent {
            return self.decided.iter().any(|d| d == &r.id) || self.is_parent(&r.item);
        }
        self.children.get(&r.child).is_some_and(|c| c.decided.iter().any(|d| d == &r.id))
    }

    /// Is this a request the family should show a parent at all?
    pub fn acceptable(&self, r: &Request) -> bool {
        match r.kind {
            ItemKind::Parent => is_public_key(&r.item) && !self.parents.is_empty(),
            _ => self.child_of(&r.device).is_some(),
        }
    }

    /// A parent's answer (the caller signs the result). `minutes`: None = for good.
    pub fn answer(&mut self, r: &Request, approve: bool, minutes: Option<u64>, now: u64) {
        if r.kind == ItemKind::Parent {
            if approve && is_public_key(&r.item) && !self.is_parent(&r.item) {
                let name = if r.label.trim().is_empty() { "Parent".to_string() } else { r.label.trim().to_string() };
                self.parents.push(Parent { name, key: r.item.to_ascii_lowercase() });
            }
            if !self.decided.iter().any(|d| d == &r.id) {
                self.decided.push(r.id.clone());
            }
            let n = self.decided.len();
            if n > DECIDED_KEEP {
                self.decided.drain(..n - DECIDED_KEEP);
            }
            return;
        }
        let rules = self.children.entry(r.child.clone()).or_default();
        if approve {
            rules.approve(r, minutes, now);
        } else {
            rules.decline(r, now);
        }
    }

    /// Makes a device a child's (creating the child's rules if needed) or an adult's.
    pub fn set_role(&mut self, device: &str, name: &str, role: Role, child: Option<&str>) {
        if role == Role::Adult {
            self.devices.remove(device);
            return;
        }
        let child = child.unwrap_or(name).to_string();
        self.children.entry(child.clone()).or_default();
        self.devices.insert(device.into(), Device { name: name.into(), role, child: Some(child) });
    }
}

/// What a child's Android phone must do now. Computed here (tested) and carried out by the
/// app as device owner.
#[derive(Serialize, Debug, PartialEq)]
pub struct AndroidPlan {
    pub role: Role,
    pub child: Option<String>,
    /// Apps to suspend, with why.
    pub suspend: Vec<(String, AppVerdict)>,
    /// Apps to un-suspend (allowed now).
    pub allow: Vec<String>,
    /// Extra Chrome URLBlocklist entries.
    pub blocked_sites: Vec<String>,
    pub youtube_restrict: u8,
    /// Private DNS host for this device.
    pub dns_host: String,
    pub block_debugging: bool,
    /// Seconds since the epoch when this plan should be worked out again (a grant ends).
    pub recheck_at: Option<u64>,
}

/// `apps`: the device's launchable, user-installed apps (Thurea leaves out its own and the
/// ones it manages); `known`: the apps that were there when the device became a child's.
pub fn android_plan(policy: &crate::Policy, device: &str, apps: &[String], known: &[String], now: u64) -> AndroidPlan {
    let keep = &policy.android.keep;
    match policy.family.child_of(device) {
        None => AndroidPlan {
            role: Role::Adult,
            child: None,
            suspend: Vec::new(),
            allow: apps.to_vec(),
            blocked_sites: Vec::new(),
            youtube_restrict: policy.browser.youtube_restrict,
            dns_host: policy.family_dns.host.clone(),
            block_debugging: false,
            recheck_at: None,
        },
        Some((name, rules)) => {
            let mut suspend = Vec::new();
            let mut allow = Vec::new();
            for a in apps {
                // Essential apps (Chrome, Play, Google services…) never freeze.
                let v = if keep.contains(a) && catalog_app(a).is_none() {
                    AppVerdict::Allowed
                } else {
                    rules.app_verdict(a, known.contains(a), now)
                };
                if v == AppVerdict::Allowed {
                    allow.push(a.clone());
                } else {
                    suspend.push((a.clone(), v));
                }
            }
            AndroidPlan {
                role: Role::Child,
                child: Some(name),
                suspend,
                allow,
                blocked_sites: rules.blocked_sites(now),
                youtube_restrict: rules.youtube_restrict.max(1),
                dns_host: CHILD_DNS_HOST.into(),
                block_debugging: rules.block_debugging,
                recheck_at: rules.next_change(now),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Policy;

    fn child_policy() -> Policy {
        let mut p = Policy::default();
        p.family.set_role("kidphone", "Sam's phone", Role::Child, Some("Sam"));
        p
    }

    #[test]
    fn adults_are_untouched() {
        let p = child_policy();
        let plan = android_plan(&p, "dadphone", &["com.instagram.android".into()], &[], 100);
        assert_eq!(plan.role, Role::Adult);
        assert!(plan.suspend.is_empty());
        assert_eq!(plan.dns_host, p.family_dns.host);
    }

    #[test]
    fn child_social_blocked_messaging_allowed_new_apps_held() {
        let p = child_policy();
        let apps: Vec<String> = ["com.instagram.android", "com.whatsapp", "com.example.game", "com.example.old", "com.android.chrome"]
            .iter().map(|s| s.to_string()).collect();
        let plan = android_plan(&p, "kidphone", &apps, &["com.example.old".into(), "com.instagram.android".into()], 100);
        assert_eq!(plan.child.as_deref(), Some("Sam"));
        let suspended: Vec<&str> = plan.suspend.iter().map(|(a, _)| a.as_str()).collect();
        assert_eq!(suspended, ["com.instagram.android", "com.example.game"]);
        assert!(matches!(plan.suspend[0].1, AppVerdict::Blocked { ref category, .. } if category == "social"));
        assert_eq!(plan.suspend[1].1, AppVerdict::Held);
        assert!(plan.allow.contains(&"com.whatsapp".to_string()));
        assert!(plan.allow.contains(&"com.android.chrome".to_string()));
        assert!(plan.blocked_sites.contains(&"instagram.com".to_string()));
        assert!(!plan.blocked_sites.contains(&"whatsapp.com".to_string()));
        assert_eq!(plan.youtube_restrict, 2);
        assert_eq!(plan.dns_host, CHILD_DNS_HOST);
        assert!(plan.block_debugging);
    }

    #[test]
    fn approvals_for_good_and_for_a_while() {
        let mut p = child_policy();
        let r = Request::new("kidphone", "Sam", ItemKind::App, "com.instagram.android", "Instagram", 100);
        let sam = p.family.children.get_mut("Sam").unwrap();
        sam.approve(&r, Some(30), 100);
        assert!(p.family.decided(&r));
        let apps = vec!["com.instagram.android".to_string()];
        let plan = android_plan(&p, "kidphone", &apps, &[], 200);
        assert!(plan.suspend.is_empty());
        assert_eq!(plan.recheck_at, Some(100 + 1800));
        // Runs out by itself.
        let plan = android_plan(&p, "kidphone", &apps, &[], 100 + 1800);
        assert_eq!(plan.suspend.len(), 1);
        // For good.
        let site = Request::new("kidphone", "Sam", ItemKind::Site, "instagram.com", "instagram.com", 100);
        let sam = p.family.children.get_mut("Sam").unwrap();
        sam.approve(&site, None, 5000);
        assert!(sam.grants.is_empty(), "expired grants are pruned");
        assert!(!sam.blocked_sites(5000).contains(&"instagram.com".to_string()));
        sam.revoke("instagram.com");
        assert!(sam.blocked_sites(5000).contains(&"instagram.com".to_string()));
    }

    #[test]
    fn catalog_lookups() {
        assert_eq!(catalog_app("com.snapchat.android").unwrap().label, "Snapchat");
        assert_eq!(catalog_site("www.Reddit.com.").unwrap().label, "Reddit");
        assert!(catalog_site("notreddit.com").is_none());
        assert!(catalog_app("com.example").is_none());
    }

    #[test]
    fn a_second_parent_joins_with_a_match_code() {
        let mut p = child_policy();
        let (mum, _) = crate::crypto::new_parent_key();
        p.add_parent("Mum", &mum, None).unwrap();
        let (dad, dad_pub) = crate::crypto::new_parent_key();
        let r = Request::new("laptop", "", ItemKind::Parent, &dad_pub, "Dad", 100);
        assert!(p.family.acceptable(&r));
        assert_eq!(match_code(&dad_pub).len(), 7);
        let mut next = p.clone();
        next.parent_edit(&mum, |x| x.family.answer(&r, true, None, 100)).unwrap();
        assert!(p.accepts(&next).is_ok());
        assert!(next.family.is_parent(&dad_pub) && next.family.decided(&r));
        // Dad can now sign.
        let mut by_dad = next.clone();
        by_dad.parent_edit(&dad, |x| x.family.set_role("kid2", "Tablet", Role::Child, Some("Sam"))).unwrap();
        assert!(next.accepts(&by_dad).is_ok());
        // Junk keys are never shown or added.
        let bad = Request::new("laptop", "", ItemKind::Parent, "nope", "X", 100);
        assert!(!p.family.acceptable(&bad));
        let mut f = p.family.clone();
        f.answer(&bad, true, None, 100);
        assert_eq!(f.parents.len(), 1);
    }

    #[test]
    fn roles_round_trip() {
        let mut p = child_policy();
        assert_eq!(p.family.role("kidphone"), Role::Child);
        p.family.set_role("kidphone", "Sam's phone", Role::Adult, None);
        assert_eq!(p.family.role("kidphone"), Role::Adult);
        assert!(p.family.children.contains_key("Sam"), "the child's rules stay");
    }
}
