//! Thurea's Android logic in Rust, called from the app's thin Java layer
//! (`com.thurea.guard.Native`). Java keeps only what needs Android's own APIs (device
//! owner, accessibility, VPN slot); the policy, the protection verdicts and the
//! household mesh — including its network loop — live here, shared with the desktop.
//!
//! Files (in the app's private storage): `policy.json`, `mesh.json` (device id, name,
//! household key; arrives only over USB or the setup QR), `parent.key` (this phone's parent
//! signing key, if it is a parent's), `inbox.json` (children's requests waiting for a
//! parent), `asks.json` (this child's own open requests).

use jni::objects::{JClass, JObjectArray, JString};
use jni::sys::{jboolean, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use serde_json::{json, Value};
use thurea_core::family::{self, ItemKind, Request, Role};
use thurea_core::mesh::{Body, Event, Mesh};
use thurea_core::net::{self, Host, NodeConfig};
use thurea_core::status::dns_verdict;
use thurea_core::{crypto, Check, Policy, Report, State};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub const PLATFORM: &str = "android";

#[derive(Default)]
struct Shared {
    dir: PathBuf,
    policy: Policy,
    report: Report,
    /// Events for Java to show, oldest first.
    events: Vec<Value>,
    peers: Vec<Value>,
    household: String,
    running: bool,
    /// Tells the running mesh thread to stop (re-pairing, new peers).
    stop: Option<Arc<AtomicBool>>,
}

fn shared() -> &'static Arc<Mutex<Shared>> {
    static S: OnceLock<Arc<Mutex<Shared>>> = OnceLock::new();
    S.get_or_init(|| Arc::new(Mutex::new(Shared::default())))
}

fn lock() -> std::sync::MutexGuard<'static, Shared> {
    shared().lock().unwrap_or_else(|e| e.into_inner())
}

// ── Plain Rust API (tested on the desktop; the JNI functions below only convert) ──

pub fn init(dir: PathBuf) -> String {
    let mut s = lock();
    let path = dir.join("policy.json");
    s.policy = Policy::load(&path).unwrap_or_else(|_| {
        let p = Policy::default();
        let _ = p.save(&path);
        p
    });
    s.dir = dir;
    if s.household.is_empty() {
        s.household = "Not joined to a household".into();
    }
    s.policy.to_json()
}

pub fn policy_json() -> String {
    lock().policy.to_json()
}

pub fn is_blocked(site: &str) -> bool {
    lock().policy.domains().is_blocked(site)
}

/// "ok", "unknown" (no DNS servers reported yet), or "bad:<server>".
pub fn vpn_verdict(dns: &[String]) -> String {
    let fam = lock().policy.family_dns.servers.clone();
    match dns_verdict(dns, &fam) {
        (State::Ok, _) => "ok".into(),
        (State::Unknown, _) => "unknown".into(),
        (State::Degraded, bad) => format!("bad:{}", bad.unwrap_or_default()),
    }
}

/// Java's status lines become this device's report, by the app's own rule
/// (`MainActivity.bad`): "!" lines and "Thurea is not…" are problems, "?" lines are
/// unconfirmed, everything else ("✓ …", "📌 YouTube pinned …") is fine.
pub fn set_status(lines: &str) -> String {
    let mut r = Report::default();
    for (i, l) in lines.lines().map(str::trim).filter(|l| !l.is_empty()).enumerate() {
        let id = format!("android{i}");
        let bad = l.starts_with('!') || l.contains(" ! ") || l.contains("— !") || l.starts_with("Thurea is not") || l.starts_with("Thurea is not");
        let state = if bad {
            State::Degraded
        } else if l.starts_with('?') {
            State::Unknown
        } else {
            State::Ok
        };
        let text = l.trim_start_matches(['✓', '!', '?']).trim();
        r.push(Check::new(&id, state, text));
    }
    let overall = r.overall();
    lock().report = r;
    serde_json::to_string(&overall).unwrap()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MeshFile {
    id: String,
    name: String,
    key: String,
    #[serde(default)]
    peers: Vec<String>,
}

/// Stops the mesh thread so the next `mesh_start` uses the new settings.
fn restart_mesh() {
    if let Some(stop) = lock().stop.take() {
        stop.store(true, Ordering::Relaxed);
    }
}

/// Adds peers reached by address ("100.64.0.7", "laptop:47823"), comma separated.
pub fn mesh_add_peers(list: &str) -> bool {
    let dir = lock().dir.clone();
    let Some(mut f) = std::fs::read_to_string(dir.join("mesh.json")).ok().and_then(|s| serde_json::from_str::<MeshFile>(&s).ok()) else {
        return false;
    };
    for p in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if !f.peers.iter().any(|x| x == p) {
            f.peers.push(p.to_string());
        }
    }
    let ok = std::fs::write(dir.join("mesh.json"), serde_json::to_string_pretty(&f).unwrap()).is_ok();
    restart_mesh();
    ok
}

pub fn mesh_join(key: &str, name: &str) -> bool {
    let Some(k) = crypto::parse_key(key) else { return false };
    let dir = lock().dir.clone();
    let old: Option<MeshFile> = std::fs::read_to_string(dir.join("mesh.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
    let f = MeshFile {
        id: old.as_ref().map(|o| o.id.clone()).unwrap_or_else(crypto::new_device_id),
        name: if name.trim().is_empty() { "Phone".into() } else { name.trim().into() },
        key: crypto::hex(&k),
        peers: old.map(|o| o.peers).unwrap_or_default(),
    };
    let ok = std::fs::write(dir.join("mesh.json"), serde_json::to_string_pretty(&f).unwrap()).is_ok();
    restart_mesh();
    ok
}

struct AndroidHost(Arc<Mutex<Shared>>);

impl AndroidHost {
    fn s(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Host for AndroidHost {
    fn local_report(&mut self) -> Report {
        self.s().report.clone()
    }
    fn policy(&self) -> Policy {
        self.s().policy.clone()
    }
    fn adopt_policy(&mut self, p: Policy) {
        let mut s = self.s();
        if !thurea_core::mesh::newer(&p, &s.policy) {
            return;
        }
        let _ = p.save(&s.dir.join("policy.json"));
        s.policy = p;
        s.events.push(json!({ "policy_updated": true }));
    }
    fn on_event(&mut self, e: &Event) {
        if let Event::Request { request } = e {
            let mut s = self.s();
            if s.policy.family.decided(request) || !am_parent(&s.dir, &s.policy) {
                return;
            }
            let mut inbox = read_list::<Request>(&s.dir, "inbox.json");
            if !inbox.iter().any(|r| r.id == request.id) {
                inbox.push(request.clone());
                write_list(&s.dir, "inbox.json", &inbox);
                s.events.push(json!({ "text": e.alert_text().unwrap_or_default(), "request": true }));
            }
            return;
        }
        if let Some(text) = e.alert_text() {
            self.s().events.push(json!({ "text": text, "warning": e.is_warning() }));
        }
    }
    fn outgoing(&mut self) -> Vec<Body> {
        let s = self.s();
        open_asks(&s.dir, &s.policy).into_iter().map(Body::Request).collect()
    }
    fn on_peers(&mut self, mesh: &Mesh) {
        let mut s = self.s();
        s.peers = mesh
            .peers()
            .map(|p| {
                json!({
                    "id": p.id,
                    "name": p.name(),
                    "platform": p.heartbeat.as_ref().map(|h| h.platform.clone()).unwrap_or_default(),
                    "state": p.state(),
                    "last_seen": p.last_seen,
                })
            })
            .collect();
        s.household = net::household_line(mesh, s.report.overall());
    }
}

/// Starts the mesh thread once a household key is present. Returns whether it runs.
pub fn mesh_start() -> bool {
    // A thread told to stop exits within a second (its socket wakes every 500 ms).
    for _ in 0..40 {
        let s = lock();
        if !s.running || s.stop.is_some() {
            break;
        }
        drop(s);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let (dir, running) = {
        let s = lock();
        (s.dir.clone(), s.running)
    };
    if running {
        return true;
    }
    let Some(f) = std::fs::read_to_string(dir.join("mesh.json")).ok().and_then(|s| serde_json::from_str::<MeshFile>(&s).ok()) else {
        return false;
    };
    let Some(key) = crypto::parse_key(&f.key) else { return false };
    let mut cfg = NodeConfig::new(f.id, f.name, PLATFORM, key);
    cfg.peers = f.peers;
    let stop = Arc::new(AtomicBool::new(false));
    {
        let mut s = lock();
        s.running = true;
        s.stop = Some(stop.clone());
    }
    let state = shared().clone();
    std::thread::Builder::new()
        .name("thurea-mesh".into())
        .spawn(move || {
            let mut host = AndroidHost(state.clone());
            while !stop.load(Ordering::Relaxed) {
                if net::run(&cfg, &mut host, &stop).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
            state.lock().unwrap_or_else(|e| e.into_inner()).running = false;
        })
        .is_ok()
}

/// Pending events as a JSON array; clears them.
pub fn poll_events() -> String {
    let ev = std::mem::take(&mut lock().events);
    Value::Array(ev).to_string()
}

/// {"line": "3 of 3 devices protected", "peers": [...]}
pub fn household() -> String {
    let s = lock();
    json!({ "line": s.household, "peers": s.peers, "joined": s.dir.join("mesh.json").exists() }).to_string()
}

// ── Family: parents decide, children ask ──

fn read_list<T: serde::de::DeserializeOwned>(dir: &std::path::Path, file: &str) -> Vec<T> {
    std::fs::read_to_string(dir.join(file)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn write_list<T: serde::Serialize>(dir: &std::path::Path, file: &str, v: &[T]) {
    let tmp = dir.join(format!("{file}.tmp"));
    if std::fs::write(&tmp, serde_json::to_string(v).unwrap()).is_ok() {
        let _ = std::fs::rename(tmp, dir.join(file));
    }
}

fn mesh_file(dir: &std::path::Path) -> Option<MeshFile> {
    std::fs::read_to_string(dir.join("mesh.json")).ok().and_then(|s| serde_json::from_str(&s).ok())
}

fn parent_secret(dir: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(dir.join("parent.key")).ok().map(|s| s.trim().to_string()).filter(|s| crypto::parent_public(s).is_some())
}

/// Is this phone one of the family's parents?
fn am_parent(dir: &std::path::Path, policy: &Policy) -> bool {
    parent_secret(dir).and_then(|k| crypto::parent_public(&k)).is_some_and(|k| policy.family.is_parent(&k))
}

/// This child's requests that no parent has answered yet.
fn open_asks(dir: &std::path::Path, policy: &Policy) -> Vec<Request> {
    let now = thurea_core::now();
    read_list::<Request>(dir, "asks.json")
        .into_iter()
        .filter(|r| !policy.family.decided(r) && now.saturating_sub(r.at) < 7 * 86400)
        .collect()
}

/// Everything the Family / child screens show, as JSON.
pub fn family_json() -> String {
    let s = lock();
    let me = mesh_file(&s.dir).map(|f| f.id).unwrap_or_default();
    let fam = &s.policy.family;
    let child = fam.child_of(&me);
    let parent = am_parent(&s.dir, &s.policy);
    // Drop answered requests from the inbox.
    let inbox: Vec<Request> = read_list::<Request>(&s.dir, "inbox.json").into_iter().filter(|r| !fam.decided(r)).collect();
    write_list(&s.dir, "inbox.json", &inbox);
    let children: serde_json::Map<String, Value> = fam
        .children
        .iter()
        .map(|(name, r)| {
            let cats: serde_json::Map<String, Value> = r.categories.iter().map(|(c, a)| (c.clone(), json!(*a == family::Access::Allowed))).collect();
            (name.clone(), json!({ "categories": cats, "allowed_apps": r.allowed_apps, "allowed_sites": r.allowed_sites, "grants": r.grants,
                                    "hold_new_apps": r.hold_new_apps, "block_debugging": r.block_debugging }))
        })
        .collect();
    json!({
        "joined": !me.is_empty(),
        "device": me,
        "has_family": !fam.parents.is_empty(),
        "parent": parent,
        "parents": fam.parents.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        "role": if child.is_some() { "child" } else { "adult" },
        "child": child.as_ref().map(|(n, _)| n.clone()),
        "devices": fam.devices,
        "children": children,
        "inbox": inbox,
        "asks": open_asks(&s.dir, &s.policy),
    })
    .to_string()
}

/// What this phone must enforce now (see `thurea_core::family::android_plan`).
pub fn android_plan(apps_json: &str, known_json: &str) -> String {
    let s = lock();
    let me = mesh_file(&s.dir).map(|f| f.id).unwrap_or_default();
    let apps: Vec<String> = serde_json::from_str(apps_json).unwrap_or_default();
    let known: Vec<String> = serde_json::from_str(known_json).unwrap_or_default();
    let plan = family::android_plan(&s.policy, &me, &apps, &known, thurea_core::now());
    serde_json::to_string(&plan).unwrap()
}

/// A child asks for an app ("app") or a site ("site"). Sent with every heartbeat until answered.
pub fn ask(kind: &str, item: &str, label: &str) -> String {
    let s = lock();
    let me = mesh_file(&s.dir).map(|f| f.id).unwrap_or_default();
    let Some((child, _)) = s.policy.family.child_of(&me) else { return "! This isn't a child's device".into() };
    let kind = if kind == "site" { ItemKind::Site } else { ItemKind::App };
    let item = if kind == ItemKind::Site {
        match thurea_core::matcher::normalize_host(item) {
            Some(h) => h,
            None => return "! That isn't a website address".into(),
        }
    } else {
        item.trim().to_string()
    };
    let mut asks = open_asks(&s.dir, &s.policy);
    if asks.iter().any(|r| r.item == item) {
        return "✓ Already asked — waiting for a parent".into();
    }
    asks.push(Request::new(&me, &child, kind, &item, if label.trim().is_empty() { &item } else { label.trim() }, thurea_core::now()));
    write_list(&s.dir, "asks.json", &asks);
    drop(s);
    restart_mesh(); // send now, not at the next heartbeat
    "✓ Asked — a parent will see it".into()
}

/// A parent's change: signs with this phone's parent key, saves, and lets the app re-apply.
fn parent_change(edit: impl FnOnce(&mut Policy)) -> String {
    let mut s = lock();
    let Some(secret) = parent_secret(&s.dir) else { return "! This phone isn't a parent's".into() };
    let mut p = s.policy.clone();
    if let Err(e) = p.parent_edit(&secret, edit) {
        return format!("! {e}");
    }
    if let Err(e) = p.save(&s.dir.join("policy.json")) {
        return format!("! Couldn't save: {e}");
    }
    s.policy = p;
    s.events.push(json!({ "policy_updated": true }));
    "✓ Done — your family's devices follow within seconds".into()
}

/// Starts a family with this phone as a parent's (needs the household mesh).
pub fn family_create(name: &str) -> String {
    let mut s = lock();
    if mesh_file(&s.dir).is_none() {
        return "! Join the household first (pair this phone with a computer)".into();
    }
    if !s.policy.family.parents.is_empty() {
        return "! There's already a family: a parent must add you".into();
    }
    let secret = parent_secret(&s.dir).unwrap_or_else(|| {
        let (sk, _) = crypto::new_parent_key();
        let _ = std::fs::write(s.dir.join("parent.key"), &sk);
        sk
    });
    let mut p = s.policy.clone();
    let name = if name.trim().is_empty() { "Parent" } else { name.trim() };
    if let Err(e) = p.add_parent(name, &secret, None) {
        return format!("! {e}");
    }
    let _ = p.save(&s.dir.join("policy.json"));
    s.policy = p;
    s.events.push(json!({ "policy_updated": true }));
    format!("✓ Family started — {name} is a parent")
}

pub fn family_set_role(device: &str, name: &str, role: &str, child: &str) -> String {
    let role = if role == "child" { Role::Child } else { Role::Adult };
    let child = Some(child.trim()).filter(|c| !c.is_empty());
    parent_change(|p| p.family.set_role(device, name, role, child))
}

/// Answers a request: approve for `minutes` (0 = for good) or decline.
pub fn family_answer(id: &str, approve: bool, minutes: i64) -> String {
    let dir = lock().dir.clone();
    let Some(r) = read_list::<Request>(&dir, "inbox.json").into_iter().find(|r| r.id == id) else { return "! That request is gone".into() };
    let now = thurea_core::now();
    let msg = parent_change(|p| {
        let rules = p.family.children.entry(r.child.clone()).or_default();
        if approve {
            rules.approve(&r, (minutes > 0).then_some(minutes as u64), now);
        } else {
            rules.decline(&r, now);
        }
    });
    if msg.starts_with('✓') {
        let left: Vec<Request> = read_list::<Request>(&dir, "inbox.json").into_iter().filter(|x| x.id != id).collect();
        write_list(&dir, "inbox.json", &left);
    }
    msg
}

pub fn family_category(child: &str, category: &str, allowed: bool) -> String {
    let a = if allowed { family::Access::Allowed } else { family::Access::Blocked };
    parent_change(|p| {
        p.family.children.entry(child.into()).or_default().categories.insert(category.into(), a);
    })
}

/// A child's switch: "hold_new_apps" or "block_debugging".
pub fn family_flag(child: &str, flag: &str, on: bool) -> String {
    if !matches!(flag, "hold_new_apps" | "block_debugging") {
        return "! Unknown setting".into();
    }
    parent_change(|p| {
        let r = p.family.children.entry(child.into()).or_default();
        if flag == "hold_new_apps" {
            r.hold_new_apps = on;
        } else {
            r.block_debugging = on;
        }
    })
}

pub fn family_revoke(child: &str, item: &str) -> String {
    parent_change(|p| {
        if let Some(r) = p.family.children.get_mut(child) {
            r.revoke(item);
        }
    })
}

// ── JNI ──

fn jstr(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(Into::into).unwrap_or_default()
}

fn out(env: &mut JNIEnv, s: String) -> jstring {
    env.new_string(s).map(|j| j.into_raw()).unwrap_or(std::ptr::null_mut())
}

fn jbool(b: bool) -> jboolean {
    if b { JNI_TRUE } else { JNI_FALSE }
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_init<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, dir: JString<'l>) -> jstring {
    let d = jstr(&mut env, &dir);
    let r = init(PathBuf::from(d));
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_policyJson<'l>(mut env: JNIEnv<'l>, _: JClass<'l>) -> jstring {
    let r = policy_json();
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_isBlocked<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, site: JString<'l>) -> jboolean {
    let s = jstr(&mut env, &site);
    jbool(is_blocked(&s))
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_vpnVerdict<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, dns: JObjectArray<'l>) -> jstring {
    let n = env.get_array_length(&dns).unwrap_or(0);
    let mut v = Vec::new();
    for i in 0..n {
        if let Ok(o) = env.get_object_array_element(&dns, i) {
            let s = JString::from(o);
            v.push(jstr(&mut env, &s));
        }
    }
    let r = vpn_verdict(&v);
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_setStatus<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, lines: JString<'l>) -> jstring {
    let l = jstr(&mut env, &lines);
    let r = set_status(&l);
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_meshJoin<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, key: JString<'l>, name: JString<'l>) -> jboolean {
    let (k, n) = (jstr(&mut env, &key), jstr(&mut env, &name));
    jbool(mesh_join(&k, &n))
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_meshAddPeers<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, list: JString<'l>) -> jboolean {
    let l = jstr(&mut env, &list);
    jbool(mesh_add_peers(&l))
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_meshStart<'l>(_: JNIEnv<'l>, _: JClass<'l>) -> jboolean {
    jbool(mesh_start())
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_pollEvents<'l>(mut env: JNIEnv<'l>, _: JClass<'l>) -> jstring {
    let r = poll_events();
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_household<'l>(mut env: JNIEnv<'l>, _: JClass<'l>) -> jstring {
    let r = household();
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_familyJson<'l>(mut env: JNIEnv<'l>, _: JClass<'l>) -> jstring {
    let r = family_json();
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_androidPlan<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, apps: JString<'l>, known: JString<'l>) -> jstring {
    let (a, k) = (jstr(&mut env, &apps), jstr(&mut env, &known));
    let r = android_plan(&a, &k);
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_ask<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, kind: JString<'l>, item: JString<'l>, label: JString<'l>) -> jstring {
    let (k, i, l) = (jstr(&mut env, &kind), jstr(&mut env, &item), jstr(&mut env, &label));
    let r = ask(&k, &i, &l);
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_familyCreate<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, name: JString<'l>) -> jstring {
    let n = jstr(&mut env, &name);
    let r = family_create(&n);
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_familySetRole<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, device: JString<'l>, name: JString<'l>, role: JString<'l>, child: JString<'l>) -> jstring {
    let (d, n, ro, c) = (jstr(&mut env, &device), jstr(&mut env, &name), jstr(&mut env, &role), jstr(&mut env, &child));
    let r = family_set_role(&d, &n, &ro, &c);
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_familyAnswer<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, id: JString<'l>, approve: jboolean, minutes: jni::sys::jint) -> jstring {
    let i = jstr(&mut env, &id);
    let r = family_answer(&i, approve != 0, minutes as i64);
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_familyCategory<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, child: JString<'l>, category: JString<'l>, allowed: jboolean) -> jstring {
    let (c, cat) = (jstr(&mut env, &child), jstr(&mut env, &category));
    let r = family_category(&c, &cat, allowed != 0);
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_familyFlag<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, child: JString<'l>, flag: JString<'l>, on: jboolean) -> jstring {
    let (c, f) = (jstr(&mut env, &child), jstr(&mut env, &flag));
    let r = family_flag(&c, &f, on != 0);
    out(&mut env, r)
}

#[no_mangle]
pub extern "system" fn Java_com_thurea_guard_Native_familyRevoke<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, child: JString<'l>, item: JString<'l>) -> jstring {
    let (c, i) = (jstr(&mut env, &child), jstr(&mut env, &item));
    let r = family_revoke(&c, &i);
    out(&mut env, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_flow() {
        let dir = std::env::temp_dir().join(format!("thurea-android-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = init(dir.clone());
        assert!(Policy::from_json(&p).is_ok());
        assert!(is_blocked("m.pornhub.com"));
        assert_eq!(vpn_verdict(&[]), "unknown");
        assert_eq!(vpn_verdict(&["1.1.1.3".into()]), "ok");
        assert_eq!(vpn_verdict(&["8.8.8.8".into()]), "bad:8.8.8.8");
        assert_eq!(set_status("✓ Family DNS locked on\n✓ Chrome locked\n"), "\"ok\"");
        assert_eq!(set_status("✓ Family DNS locked on\n? VPN DNS not reported yet\n"), "\"unknown\"");
        assert_eq!(set_status("Thurea is not the device owner yet — nothing is enforced."), "\"degraded\"");
        assert_eq!(set_status("✓ Family DNS locked on\n📌 YouTube pinned at 21.30\n"), "\"ok\"");
        assert_eq!(set_status("📌 Twitch — ! needs repair\n"), "\"degraded\"");
        assert!(!mesh_join("nope", "Phone"));
        assert!(!mesh_start());
        assert!(mesh_join(&crypto::hex(&crypto::new_key()), "Test phone"));
        assert!(household().contains("\"joined\":true"));
        assert!(mesh_add_peers("100.64.0.7, laptop:47823"));
        assert!(std::fs::read_to_string(dir.join("mesh.json")).unwrap().contains("100.64.0.7"));
        assert_eq!(poll_events(), "[]");

        // Family: this phone starts one, then (as the test's stand-in) is made a child's device.
        let me = mesh_file(&dir).unwrap().id;
        let f: Value = serde_json::from_str(&family_json()).unwrap();
        assert_eq!(f["has_family"], false);
        assert!(ask("app", "com.snapchat.android", "Snapchat").starts_with('!'), "adults don't ask");
        assert!(family_create("Mum").starts_with('✓'));
        assert!(family_create("Mum").starts_with('!'));
        assert!(poll_events().contains("policy_updated"));
        assert!(family_set_role(&me, "Test phone", "child", "Sam").starts_with('✓'));
        let f: Value = serde_json::from_str(&family_json()).unwrap();
        assert_eq!(f["parent"], true);
        assert_eq!(f["role"], "child");
        assert_eq!(f["child"], "Sam");
        let plan: Value = serde_json::from_str(&android_plan(r#"["com.snapchat.android","com.whatsapp"]"#, "[]")).unwrap();
        assert_eq!(plan["role"], "child");
        assert_eq!(plan["suspend"][0][0], "com.snapchat.android");
        assert_eq!(plan["dns_host"], thurea_core::family::CHILD_DNS_HOST);

        // The child asks; the request goes out with heartbeats; a parent's inbox gets it.
        assert!(ask("app", "com.snapchat.android", "Snapchat").starts_with('✓'));
        assert!(ask("app", "com.snapchat.android", "Snapchat").contains("Already"));
        assert!(ask("site", "not a site", "").starts_with('!'));
        let mut host = AndroidHost(shared().clone());
        let out = host.outgoing();
        assert_eq!(out.len(), 1);
        let Body::Request(r) = &out[0] else { panic!() };
        host.on_event(&Event::Request { request: r.clone() });
        let f: Value = serde_json::from_str(&family_json()).unwrap();
        assert_eq!(f["inbox"].as_array().unwrap().len(), 1);
        // Approved for 30 minutes: no longer suspended, no longer asked, inbox empty.
        assert!(family_answer(&r.id, true, 30).starts_with('✓'));
        let plan: Value = serde_json::from_str(&android_plan(r#"["com.snapchat.android"]"#, "[]")).unwrap();
        assert!(plan["suspend"].as_array().unwrap().is_empty());
        assert!(plan["recheck_at"].as_u64().is_some());
        assert!(host.outgoing().is_empty());
        let f: Value = serde_json::from_str(&family_json()).unwrap();
        assert!(f["inbox"].as_array().unwrap().is_empty());
        // Messaging switched off for Sam.
        assert!(family_category("Sam", "messaging", false).starts_with('✓'));
        let plan: Value = serde_json::from_str(&android_plan(r#"["com.whatsapp"]"#, "[]")).unwrap();
        assert_eq!(plan["suspend"][0][0], "com.whatsapp");
        assert_eq!(plan["block_debugging"], true);
        assert!(family_flag("Sam", "block_debugging", false).starts_with('✓'));
        assert!(family_flag("Sam", "nope", false).starts_with('!'));
        let plan: Value = serde_json::from_str(&android_plan("[]", "[]")).unwrap();
        assert_eq!(plan["block_debugging"], false);
        // The saved policy is parent-signed and a fresh device accepts it.
        let saved = Policy::load(&dir.join("policy.json")).unwrap();
        assert!(Policy::default().accepts(&saved).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }
}
