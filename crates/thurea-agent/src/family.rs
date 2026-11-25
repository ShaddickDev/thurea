//! `thurea family …` and the control center's actions: a parent's controls on a computer.
//!
//! The parent key belongs to the parent's own account (`~/.config/thurea/parent.key`,
//! `%APPDATA%\Thurea\parent.key`), not to the computer: a parent signs changes as themselves,
//! with no admin rights, and drops them in the service's submit folder. The service (running as
//! root / SYSTEM) takes a change only if it is newer and signed by one of the family's parents,
//! so a child using the same computer can't change anything. Starting a family needs admin
//! once (so a child can't start one first and make themselves the parent).

use crate::config;
use thurea_core::family::{self, Access, ItemKind, Request, Role};
use thurea_core::{crypto, Policy};
use serde_json::{json, Value};
use std::path::PathBuf;

pub const HELP: &str = "\
  thurea family [--json]           Parents, children's devices, their rules, waiting requests
  thurea control-center            The same in a window: requests, children, devices, relay
  thurea family create NAME        Start a family with you as a parent (as admin, once)
  thurea family join NAME          Ask the family's parents to make you a parent too
  thurea family child DEVICE NAME  Make a household device (id from `thurea status`) NAME's
  thurea family adult DEVICE       Make it an adult's device again
  thurea family approve ID [MIN]   Approve a request (for MIN minutes, or for good)
  thurea family decline ID         Decline it
  thurea family allow|block CHILD social|messaging
                                   What a child may use without asking
  thurea family hold|nohold CHILD  Hold a child's new apps until approved (or not)
  thurea family revoke CHILD ITEM  Take back an approval (app package or site)
  thurea family screen CHILD daily|limit|bedtime|pause|lock|extra|always [A] [B]
                                   Screen time, e.g. daily 90 · bedtime 21:00-07:00 · pause 60 ·
                                   lock · extra 30 · limit social 30 · always org.school.app on
  thurea family code CHILD MIN     Offline code for 15/30/60/120 extra minutes today (as admin)
  thurea family account DEVICE USER CHILD|-
                                   Make a user account on a household computer a child's (- : an adult's)
  thurea family check              What Thurea does for children's accounts here (changes nothing)
  thurea family relay HOST:PORT|off
                                   The household's own relay, to reach each other from anywhere
  thurea relay [--port N]          Run a relay (on a server with a public UDP port)";

/// Home directory of the person behind this command, even under sudo / pkexec.
#[cfg(unix)]
fn real_home() -> Option<(PathBuf, Option<String>)> {
    let uid = std::env::var("PKEXEC_UID").ok().or_else(|| std::env::var("SUDO_UID").ok());
    if let Some(uid) = uid.filter(|_| crate::platform::is_admin()) {
        let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
        for l in passwd.lines() {
            let f: Vec<&str> = l.split(':').collect();
            if f.len() > 5 && f[2] == uid {
                return Some((PathBuf::from(f[5]), Some(f[0].to_string())));
            }
        }
    }
    std::env::var_os("HOME").map(|h| (PathBuf::from(h), None))
}

/// Where this person's parent key lives.
pub fn key_path() -> PathBuf {
    #[cfg(windows)]
    {
        return std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default().join("Thurea").join("parent.key");
    }
    #[cfg(unix)]
    {
        if let Some(d) = std::env::var_os("THUREA_PARENT_DIR") {
            return PathBuf::from(d).join("parent.key");
        }
        let (home, _) = real_home().unwrap_or_default();
        home.join(".config").join("thurea").join("parent.key")
    }
}

fn inbox_path() -> PathBuf {
    config::state_dir().join("family-requests.json")
}

/// This person's parent key (secret hex), if they have one.
pub fn parent_key() -> Option<String> {
    std::fs::read_to_string(key_path()).ok().map(|s| s.trim().to_string()).filter(|k| crypto::parent_public(k).is_some())
}

fn my_public() -> Option<String> {
    parent_key().and_then(|k| crypto::parent_public(&k))
}

pub fn is_parent(policy: &Policy) -> bool {
    my_public().is_some_and(|k| policy.family.is_parent(&k))
}

/// The service (root) keeps the inbox; a parent of the family is one with a key anywhere on
/// this computer, so it keeps every open request (they're answered by signature anyway).
pub fn inbox() -> Vec<Request> {
    std::fs::read_to_string(inbox_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_inbox_refs(v: &[&Request]) {
    let _ = std::fs::create_dir_all(config::state_dir());
    let _ = std::fs::write(inbox_path(), serde_json::to_string_pretty(v).unwrap());
}

/// From the service: a request arrived. Kept until a parent answers. Returns true if new.
pub fn received(r: &Request, policy: &Policy) -> bool {
    if policy.family.decided(r) || !policy.family.acceptable(r) {
        return false;
    }
    let v = inbox();
    if v.iter().any(|x| x.id == r.id) {
        return false;
    }
    let mut refs: Vec<&Request> = v.iter().collect();
    refs.push(r);
    save_inbox_refs(&refs);
    true
}

fn usage_path() -> PathBuf {
    config::state_dir().join("family-usage.json")
}

/// From the service: a child's device reported today's screen time.
pub fn save_usage(r: &thurea_core::screen::UsageReport) {
    let mut all: std::collections::BTreeMap<String, thurea_core::screen::UsageReport> =
        std::fs::read_to_string(usage_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    all.insert(format!("{}/{}", r.device, r.child), r.clone());
    let _ = std::fs::write(usage_path(), serde_json::to_string(&all).unwrap());
}

/// Today's local date, "YYYY-MM-DD" (the OS knows the time zone).
pub fn local_day() -> String {
    #[cfg(windows)]
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-Command", "Get-Date -Format yyyy-MM-dd"]).output();
    #[cfg(not(windows))]
    let out = std::process::Command::new("date").arg("+%F").output();
    out.ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|d| d.len() == 10).unwrap_or_default()
}

/// A screen-time change for a child (see `ScreenTime::change`).
pub fn screen(child: &str, what: &str, a: &str, b: &str) -> Result<String, String> {
    let now = thurea_core::now();
    let b = if what == "extra" && b.is_empty() { local_day() } else { b.to_string() };
    thurea_core::screen::ScreenTime::default().change(what, a, &b, now)?;
    let (child, what, a) = (child.to_string(), what.to_string(), a.to_string());
    change(move |p| {
        let _ = p.family.children.entry(child).or_default().screen.change(&what, &a, &b, now);
    })
}

/// The policy in force here: the service's readable copy, or (as admin) the config.
pub fn current_policy() -> Policy {
    Policy::load(&config::public_policy_path()).ok().filter(|p| !config::policy_path().exists() || Policy::load(&config::policy_path()).map_or(true, |c| !thurea_core::mesh::newer(&c, p))).unwrap_or_else(config::load_policy)
}

/// Hands a signed policy to the service (and, as admin, saves it straight away).
fn submit(p: &Policy) -> Result<(), String> {
    if crate::platform::is_admin() && config::config_dir().exists() {
        p.save(&config::policy_path()).map_err(|e| e.to_string())?;
    }
    write_submission(&format!("{}.policy.json", crypto::new_device_id()), &p.to_json())
}

fn write_submission(name: &str, body: &str) -> Result<(), String> {
    let dir = config::submit_dir();
    if !dir.exists() {
        return Err("Thurea's service isn't set up on this computer (sudo thurea init / join first)".into());
    }
    let tmp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&tmp, body).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::rename(&tmp, dir.join(name)).map_err(|e| e.to_string())
}

/// A parent's change, signed and handed over.
pub fn change(edit: impl FnOnce(&mut Policy)) -> Result<String, String> {
    let key = parent_key().ok_or("you aren't a parent of this family on this computer")?;
    let mut p = current_policy();
    if !p.family.is_parent(&crypto::parent_public(&key).unwrap()) {
        return Err("your key isn't one of the family's parents yet (thurea family join NAME)".into());
    }
    p.parent_edit(&key, edit)?;
    submit(&p)?;
    Ok(format!("Done — the household's devices follow within a minute (family policy v{}).", p.version))
}

fn write_key(key: &str) -> Result<(), String> {
    let path = key_path();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, key).map_err(|e| format!("{}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        // Created under sudo / pkexec: give it to the parent, not root.
        if let Some((_, Some(user))) = real_home() {
            let _ = std::process::Command::new("chown").arg(format!("{user}:")).arg(&path).arg(path.parent().unwrap()).status();
        }
    }
    Ok(())
}

/// Starts a family (admin, once). The parent key goes to the person who ran it.
pub fn create(name: &str) -> Result<String, String> {
    if !crate::platform::is_admin() && std::env::var_os("THUREA_CONFIG_DIR").is_none() {
        return Err("starting a family needs admin once (sudo thurea family create NAME)".into());
    }
    let mut p = config::load_policy();
    if !p.family.parents.is_empty() {
        return Err("there's already a family: an existing parent must approve you (thurea family join NAME)".into());
    }
    let key = parent_key().unwrap_or_else(|| crypto::new_parent_key().0);
    write_key(&key)?;
    p.add_parent(name.trim(), &key, None)?;
    p.save(&config::policy_path()).map_err(|e| e.to_string())?;
    let _ = write_submission(&format!("{}.policy.json", crypto::new_device_id()), &p.to_json());
    Ok(format!("Family started: {} is a parent (key in {}).", name.trim(), key_path().display()))
}

/// Asks the family's parents to add this person as a parent; returns the match code message.
pub fn join(name: &str) -> Result<String, String> {
    let p = current_policy();
    if p.family.parents.is_empty() {
        return Err("there's no family yet (an admin starts one: sudo thurea family create NAME)".into());
    }
    if is_parent(&p) {
        return Ok("You're already a parent of this family.".into());
    }
    let key = match parent_key() {
        Some(k) => k,
        None => {
            let k = crypto::new_parent_key().0;
            write_key(&k)?;
            k
        }
    };
    let public = crypto::parent_public(&key).unwrap();
    let state: Value = std::fs::read_to_string(config::state_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let device = state["id"].as_str().ok_or("Thurea's service isn't running on this computer")?;
    let r = Request::new(device, "", ItemKind::Parent, &public, name.trim(), thurea_core::now());
    write_submission(&format!("{}.ask.json", r.id), &serde_json::to_string(&r).unwrap())?;
    Ok(format!("Asked. On a parent's device, approve \"{} wants to be a parent\" — only if it shows the code {}.", name.trim(), family::match_code(&public)))
}

pub fn answer(id: &str, approve: bool, minutes: Option<u64>) -> Result<String, String> {
    let r = inbox().into_iter().find(|r| r.id == id).ok_or("that request is gone (already answered?)")?;
    let now = thurea_core::now();
    change(|p| p.family.answer(&r, approve, minutes, now))
}

/// Everything the control center shows.
pub fn json() -> Value {
    let p = current_policy();
    let f = &p.family;
    let state: Value = std::fs::read_to_string(config::state_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let inbox: Vec<Value> = inbox()
        .into_iter()
        .filter(|r| !f.decided(r))
        .map(|r| {
            let mut v = serde_json::to_value(&r).unwrap();
            if r.kind == ItemKind::Parent {
                v["code"] = json!(family::match_code(&r.item));
            }
            v
        })
        .collect();
    let now = thurea_core::now();
    let usage: std::collections::BTreeMap<String, thurea_core::screen::UsageReport> =
        std::fs::read_to_string(usage_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let today = local_day();
    let children: serde_json::Map<String, Value> = f
        .children
        .iter()
        .map(|(n, r)| {
            let reports: Vec<&thurea_core::screen::UsageReport> = usage.values().filter(|u| &u.child == n && u.day == today).collect();
            let mut top: Vec<(String, u32)> = Vec::new();
            for u in &reports {
                for (_, label, m) in &u.top {
                    match top.iter_mut().find(|(l, _)| l == label) {
                        Some(t) => t.1 += m,
                        None => top.push((label.clone(), *m)),
                    }
                }
            }
            top.sort_by(|a, b| b.1.cmp(&a.1));
            top.truncate(4);
            (n.clone(), json!({
                "screen": r.screen,
                "today": { "used": reports.iter().map(|u| u.used).sum::<u32>(), "allowed": reports.iter().filter_map(|u| u.allowed).max(), "top": top },
                "social": r.category_allowed("social"),
                "messaging": r.category_allowed("messaging"),
                "hold_new_apps": r.hold_new_apps,
                "block_debugging": r.block_debugging,
                "approved": r.allowed_apps.iter().chain(&r.allowed_sites).collect::<Vec<_>>(),
                "grants": r.grants.iter().filter(|g| g.until > now).map(|g| json!({ "item": g.item, "minutes_left": (g.until - now).div_ceil(60) })).collect::<Vec<_>>(),
            }))
        })
        .collect();
    let me = my_public();
    json!({
        "service": config::submit_dir().exists() && state.is_object(),
        "this_device": state["id"],
        "computer": state["name"],
        "has_family": !f.parents.is_empty(),
        "parent": me.as_ref().is_some_and(|k| f.is_parent(k)),
        "my_code": me.as_ref().map(|k| family::match_code(k)),
        "parents": f.parents.iter().map(|x| x.name.clone()).collect::<Vec<_>>(),
        "devices": f.devices,
        "peers": state["peers"],
        "household": state["household"],
        "children": children,
        "inbox": inbox,
        "relay": p.relay,
        "version": p.version,
        "my_accounts": crate::platform::accounts(),
        "my_platform": crate::platform::PLATFORM,
    })
}

pub fn run(args: &[String]) -> Result<String, String> {
    let sub = args.get(1).map(String::as_str).filter(|s| !s.starts_with("--")).unwrap_or("status");
    let a = |i: usize, what: &str| args.get(i).cloned().ok_or(format!("usage: thurea family {sub} {what}"));
    match sub {
        "status" if args.iter().any(|x| x == "--json") => Ok(json().to_string()),
        "status" => Ok(status()),
        "create" => create(&a(2, "NAME")?),
        "join" => join(&a(2, "NAME")?),
        "child" => {
            let (dev, child) = (a(2, "DEVICE NAME")?, a(3, "DEVICE NAME")?);
            let name = device_name(&dev);
            change(|p| p.family.set_role(&dev, &name, Role::Child, Some(&child)))
        }
        "adult" => {
            let dev = a(2, "DEVICE")?;
            change(|p| p.family.set_role(&dev, "", Role::Adult, None))
        }
        "approve" | "decline" => {
            let id = a(2, "ID")?;
            let minutes: Option<u64> = args.get(3).map(|m| m.parse().map_err(|_| "minutes must be a number")).transpose()?;
            answer(&id, sub == "approve", minutes)
        }
        "allow" | "block" => {
            let (child, cat) = (a(2, "CHILD social|messaging")?, a(3, "CHILD social|messaging")?);
            category(&child, &cat, sub == "allow")
        }
        "hold" | "nohold" => {
            let child = a(2, "CHILD")?;
            change(|p| p.family.children.entry(child).or_default().hold_new_apps = sub == "hold")
        }
        "revoke" => {
            let (child, item) = (a(2, "CHILD ITEM")?, a(3, "CHILD ITEM")?);
            change(|p| {
                if let Some(r) = p.family.children.get_mut(&child) {
                    r.revoke(&item)
                }
            })
        }
        "relay" => relay(&a(2, "HOST:PORT|off")?),
        "account" => {
            let (dev, user, child) = (a(2, "DEVICE USER CHILD|-")?, a(3, "DEVICE USER CHILD|-")?, a(4, "DEVICE USER CHILD|-")?);
            account(&dev, &user, &child)
        }
        "check" => {
            let state: Value = std::fs::read_to_string(config::state_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            let id = state["id"].as_str().map(String::from).or_else(|| config::Config::load().ok().map(|c| c.id)).ok_or("this computer isn't in a household")?;
            let p = current_policy();
            let mapped = thurea_core::desktop::child_accounts(&p, &id);
            if mapped.is_empty() {
                return Ok("No children's accounts on this computer.".into());
            }
            let (reports, done, problems) = crate::accounts::tick(&id, &p, false);
            let mut out = String::new();
            for (u, c) in mapped {
                out += &format!("{u}: {c}'s account\n");
            }
            for r in reports {
                out += &format!("  {} today: {} min{}\n", r.child, r.used, r.allowed.map_or(String::new(), |a| format!(" of {a}")));
            }
            for d in done {
                out += &format!("  would {d}\n");
            }
            for p in problems {
                out += &format!("  ! {p}\n");
            }
            Ok(out)
        }
        "screen" => {
            let (child, what) = (a(2, "CHILD WHAT [A] [B]")?, a(3, "CHILD WHAT [A] [B]")?);
            screen(&child, &what, args.get(4).map(String::as_str).unwrap_or(""), args.get(5).map(String::as_str).unwrap_or(""))
        }
        "code" => {
            let (child, mins) = (a(2, "CHILD MINUTES")?, a(3, "CHILD MINUTES")?);
            let m: u32 = mins.parse().map_err(|_| "minutes must be 15, 30, 60 or 120")?;
            if !thurea_core::screen::CODE_MINUTES.contains(&m) {
                return Err("codes are for 15, 30, 60 or 120 minutes".into());
            }
            if !is_parent(&current_policy()) {
                return Err("only a parent makes codes".into());
            }
            // The household key is readable by admins only.
            let key = config::Config::load()?.key_bytes()?;
            let day = local_day();
            Ok(format!("{}  ({m} extra minutes for {child}, today {day} only; type it in Thurea → Ask a parent on {child}'s phone)",
                thurea_core::screen::offline_code(&key, &child, &day, m)))
        }
        _ => Err(format!("unknown: thurea family {sub}\n\n{HELP}")),
    }
}

/// The browser extension's helper (native messaging, runs as the signed-in user): answers one
/// message with this account's rules — {"child": name or null, "blocked_sites", "youtube_restrict"}.
pub fn host() -> Result<String, String> {
    use std::io::{Read, Write};
    let mut len = [0u8; 4];
    let mut stdin = std::io::stdin();
    let mut msg = Value::Null;
    if stdin.read_exact(&mut len).is_ok() {
        let n = u32::from_le_bytes(len).min(64 * 1024) as usize;
        let mut raw = vec![0u8; n];
        if stdin.read_exact(&mut raw).is_ok() {
            msg = serde_json::from_slice(&raw).unwrap_or(Value::Null);
        }
    }
    // {"type":"rules"} (or nothing): the family's rules for this account. Anything else is the
    // extension's Thurea tab asking what the tray apps show, or for one of their actions.
    let reply = match msg["type"].as_str() {
        None | Some("rules") => host_reply(&current_user()),
        Some(_) => crate::browser::reply(&msg),
    }
    .to_string();
    let mut out = std::io::stdout();
    let _ = out.write_all(&(reply.len() as u32).to_le_bytes());
    let _ = out.write_all(reply.as_bytes());
    let _ = out.flush();
    Ok(String::new())
}

fn current_user() -> String {
    let u = std::env::var(if cfg!(windows) { "USERNAME" } else { "USER" }).unwrap_or_default();
    if !u.is_empty() {
        return u;
    }
    std::process::Command::new("whoami").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().rsplit('\\').next().unwrap_or("").to_string()).unwrap_or_default()
}

pub fn host_reply(user: &str) -> Value {
    let state: Value = std::fs::read_to_string(config::state_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let Some(device) = state["id"].as_str() else { return json!({ "child": null }) };
    let plan = thurea_core::desktop::desktop_plan(&current_policy(), device, user, &[], &thurea_core::screen::Used::new(), None, 0);
    match plan.child {
        None => json!({ "child": null }),
        Some(c) => json!({ "child": c, "blocked_sites": plan.blocked_sites, "youtube_restrict": plan.youtube_restrict }),
    }
}

/// Marks a user account on a household computer as a child's ("-" / "": an adult's).
pub fn account(device: &str, user: &str, child: &str) -> Result<String, String> {
    let name = device_name(device);
    let (device, user) = (device.to_string(), user.to_string());
    let child = Some(child.trim().to_string()).filter(|c| !c.is_empty() && c != "-");
    change(move |p| p.family.set_account(&device, &name, &user, child.as_deref()))
}

pub fn category(child: &str, cat: &str, allow: bool) -> Result<String, String> {
    if !matches!(cat, "social" | "messaging") {
        return Err("category must be social or messaging".into());
    }
    let access = if allow { Access::Allowed } else { Access::Blocked };
    let (child, cat) = (child.to_string(), cat.to_string());
    change(move |p| {
        p.family.children.entry(child).or_default().categories.insert(cat, access);
    })
}

pub fn relay(addr: &str) -> Result<String, String> {
    let a = addr.trim();
    let value = if a.is_empty() || a == "off" { None } else { Some(a.to_string()) };
    change(move |p| p.relay = value)
}

/// A device's name from the service's household view, if known.
pub fn device_name(id: &str) -> String {
    let v: Value = std::fs::read_to_string(config::state_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if v["id"] == id {
        if let Some(n) = v["name"].as_str() {
            return n.to_string(); // this computer
        }
    }
    v["peers"].as_array().and_then(|a| a.iter().find(|p| p["id"] == id).and_then(|p| p["name"].as_str().map(String::from))).unwrap_or_else(|| id.to_string())
}

fn status() -> String {
    let j = json();
    if j["has_family"] != true {
        return format!("No family yet. Start one (as admin, once): sudo thurea family create NAME\n\n{HELP}");
    }
    let mut out = format!(
        "Parents: {}{}\n",
        j["parents"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
        if j["parent"] == true { " (you're one of them)" } else { "" }
    );
    if let Some(r) = j["relay"].as_str() {
        out += &format!("Relay: {r}\n");
    }
    let devices = j["devices"].as_object().cloned().unwrap_or_default();
    for (name, c) in j["children"].as_object().unwrap() {
        let yes = |k: &str| if c[k] == true { "allowed" } else { "blocked" };
        out += &format!("\n{name}: social {}, messaging {}; new apps {}\n", yes("social"), yes("messaging"),
            if c["hold_new_apps"] == true { "held until approved" } else { "allowed" });
        for (id, d) in devices.iter().filter(|(_, d)| d["child"] == name.as_str()) {
            out += &format!("  device {} ({id})\n", d["name"].as_str().unwrap_or(id));
        }
        for a in c["approved"].as_array().unwrap() {
            out += &format!("  approved: {}\n", a.as_str().unwrap_or(""));
        }
        for g in c["grants"].as_array().unwrap() {
            out += &format!("  approved for {} more min: {}\n", g["minutes_left"], g["item"].as_str().unwrap_or(""));
        }
    }
    let inbox = j["inbox"].as_array().unwrap();
    if !inbox.is_empty() {
        out += "\nWaiting for you:\n";
        for r in inbox {
            if r["what"] == "parent" {
                out += &format!("  {}  {} wants to be a parent (code {})\n", r["id"].as_str().unwrap(), r["label"].as_str().unwrap_or(""), r["code"].as_str().unwrap_or(""));
            } else {
                out += &format!("  {}  {} asks for {} ({})\n", r["id"].as_str().unwrap(), r["child"].as_str().unwrap_or(""), r["label"].as_str().unwrap_or(""), r["item"].as_str().unwrap_or(""));
            }
        }
        out += "Answer with: thurea family approve ID [MINUTES] | thurea family decline ID\n";
    }
    out
}
