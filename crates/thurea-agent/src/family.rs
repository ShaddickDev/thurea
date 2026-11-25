//! `thurea family …`: a parent's controls on a computer (the same family as Thurea on
//! Android). The parent key lives next to the household key (root / Administrators only);
//! children's requests arrive through the running service (`thurea run`) and wait in
//! `family-requests.json` in the state directory.

use crate::config;
use thurea_core::family::{Access, Request, Role};
use thurea_core::{crypto, Policy};
use std::path::PathBuf;

pub const HELP: &str = "\
  thurea family                    Parents, children's devices, their rules, waiting requests
  thurea family create NAME        Start a family with this computer as a parent's
  thurea family child DEVICE NAME  Make a household device (id from `thurea status`) NAME's
  thurea family adult DEVICE       Make it an adult's device again
  thurea family approve ID [MIN]   Approve a child's request (for MIN minutes, or for good)
  thurea family decline ID         Decline it
  thurea family allow|block CHILD social|messaging
                                   What a child may use without asking
  thurea family revoke CHILD ITEM  Take back an approval (app package or site)";

fn key_path() -> PathBuf {
    config::config_dir().join("parent.key")
}

fn inbox_path() -> PathBuf {
    config::state_dir().join("family-requests.json")
}

/// This computer's parent key (secret hex), if it is a parent's.
pub fn parent_key() -> Option<String> {
    std::fs::read_to_string(key_path()).ok().map(|s| s.trim().to_string()).filter(|k| crypto::parent_public(k).is_some())
}

pub fn is_parent(policy: &Policy) -> bool {
    parent_key().and_then(|k| crypto::parent_public(&k)).is_some_and(|k| policy.family.is_parent(&k))
}

pub fn inbox() -> Vec<Request> {
    std::fs::read_to_string(inbox_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_inbox(v: &[Request]) {
    let _ = std::fs::create_dir_all(config::state_dir());
    let _ = std::fs::write(inbox_path(), serde_json::to_string_pretty(v).unwrap());
}

/// From the service: a child asked. Kept until a parent answers. Returns true if new.
pub fn received(r: &Request, policy: &Policy) -> bool {
    if policy.family.decided(r) || !is_parent(policy) {
        return false;
    }
    let mut v = inbox();
    if v.iter().any(|x| x.id == r.id) {
        return false;
    }
    v.push(r.clone());
    save_inbox(&v);
    true
}

/// Saves a policy change: signed with this computer's parent key once there is a family.
pub fn save_signed(before: &Policy, mut after: Policy) -> Result<Policy, String> {
    if !before.family.parents.is_empty() {
        let key = parent_key().ok_or("this family's rules can only be changed by a parent (no parent key on this computer)")?;
        after.version = before.version + 1;
        after.sign(&key)?;
        before.accepts(&after)?;
    }
    after.save(&config::policy_path()).map_err(|e| e.to_string())?;
    Ok(after)
}

fn change(edit: impl FnOnce(&mut Policy)) -> Result<Policy, String> {
    let before = config::load_policy();
    let key = parent_key().ok_or("this computer isn't a parent's (thurea family create NAME)")?;
    let mut p = before.clone();
    p.parent_edit(&key, edit)?;
    p.save(&config::policy_path()).map_err(|e| e.to_string())?;
    Ok(p)
}

fn done(p: &Policy) -> String {
    format!("Done (family policy version {}). The household's devices follow within a minute.", p.version)
}

pub fn run(args: &[String]) -> Result<String, String> {
    let sub = args.get(1).map(String::as_str).unwrap_or("status");
    let a = |i: usize, what: &str| args.get(i).cloned().ok_or(format!("usage: thurea family {sub} {what}"));
    match sub {
        "status" => Ok(status()),
        "create" => {
            let name = a(2, "NAME")?;
            let mut p = config::load_policy();
            if !p.family.parents.is_empty() {
                return Err("there's already a family: an existing parent must add you".into());
            }
            let key = parent_key().unwrap_or_else(|| crypto::new_parent_key().0);
            write_secret(&key)?;
            p.add_parent(&name, &key, None)?;
            p.save(&config::policy_path()).map_err(|e| e.to_string())?;
            Ok(format!("Family started: {name} is a parent. {}", done(&p)))
        }
        "child" => {
            let (dev, child) = (a(2, "DEVICE NAME")?, a(3, "DEVICE NAME")?);
            let name = device_name(&dev);
            change(|p| p.family.set_role(&dev, &name, Role::Child, Some(&child))).map(|p| done(&p))
        }
        "adult" => {
            let dev = a(2, "DEVICE")?;
            change(|p| p.family.set_role(&dev, "", Role::Adult, None)).map(|p| done(&p))
        }
        "approve" | "decline" => {
            let id = a(2, "ID")?;
            let r = inbox().into_iter().find(|r| r.id == id).ok_or("no such request (thurea family)")?;
            let minutes: Option<u64> = args.get(3).map(|m| m.parse().map_err(|_| "minutes must be a number")).transpose()?;
            let now = thurea_core::now();
            let p = change(|p| {
                let rules = p.family.children.entry(r.child.clone()).or_default();
                if sub == "approve" { rules.approve(&r, minutes, now) } else { rules.decline(&r, now) }
            })?;
            save_inbox(&inbox().into_iter().filter(|x| x.id != id).collect::<Vec<_>>());
            Ok(done(&p))
        }
        "allow" | "block" => {
            let (child, cat) = (a(2, "CHILD social|messaging")?, a(3, "CHILD social|messaging")?);
            if !matches!(cat.as_str(), "social" | "messaging") {
                return Err("category must be social or messaging".into());
            }
            let access = if sub == "allow" { Access::Allowed } else { Access::Blocked };
            change(|p| { p.family.children.entry(child).or_default().categories.insert(cat, access); }).map(|p| done(&p))
        }
        "revoke" => {
            let (child, item) = (a(2, "CHILD ITEM")?, a(3, "CHILD ITEM")?);
            change(|p| { if let Some(r) = p.family.children.get_mut(&child) { r.revoke(&item) } }).map(|p| done(&p))
        }
        _ => Err(format!("unknown: thurea family {sub}\n\n{HELP}")),
    }
}

fn write_secret(key: &str) -> Result<(), String> {
    let path = key_path();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, key).map_err(|e| format!("{}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// A device's name from the service's household view, if known.
fn device_name(id: &str) -> String {
    std::fs::read_to_string(config::state_path())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["peers"].as_array().and_then(|a| a.iter().find(|p| p["id"] == id).and_then(|p| p["name"].as_str().map(String::from))))
        .unwrap_or_else(|| id.to_string())
}

fn status() -> String {
    let p = config::load_policy();
    let f = &p.family;
    if f.parents.is_empty() {
        return format!("No family yet. A parent starts one with: thurea family create NAME\n\n{HELP}");
    }
    let mut out = format!(
        "Parents: {}{}\n",
        f.parents.iter().map(|x| x.name.clone()).collect::<Vec<_>>().join(", "),
        if is_parent(&p) { " (this computer is a parent's)" } else { "" }
    );
    for (name, r) in &f.children {
        let cats: Vec<String> = r.categories.iter().map(|(c, a)| format!("{c} {}", if *a == Access::Allowed { "allowed" } else { "blocked" })).collect();
        out += &format!("\n{name}: {}; new apps {}\n", cats.join(", "), if r.hold_new_apps { "held until approved" } else { "allowed" });
        for (id, d) in f.devices.iter().filter(|(_, d)| d.child.as_deref() == Some(name)) {
            out += &format!("  device {} ({id})\n", d.name);
        }
        for a in r.allowed_apps.iter().chain(&r.allowed_sites) {
            out += &format!("  approved: {a}\n");
        }
        let now = thurea_core::now();
        for g in r.grants.iter().filter(|g| g.until > now) {
            out += &format!("  approved for {} more min: {}\n", (g.until - now).div_ceil(60), g.item);
        }
    }
    let waiting: Vec<Request> = inbox().into_iter().filter(|r| !f.decided(r)).collect();
    if !waiting.is_empty() {
        out += "\nWaiting for you:\n";
        for r in &waiting {
            out += &format!("  {}  {} asks for {} ({})\n", r.id, r.child, r.label, r.item);
        }
        out += "Answer with: thurea family approve ID [MINUTES] | thurea family decline ID\n";
    }
    out
}
