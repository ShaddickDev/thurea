//! Thurea's Android logic in Rust, called from the app's thin Java layer
//! (`com.thurea.guard.Native`). Java keeps only what needs Android's own APIs (device
//! owner, accessibility, VPN slot); the policy, the protection verdicts and the
//! household mesh — including its network loop — live here, shared with the desktop.
//!
//! Files (in the app's private storage): `policy.json`, `mesh.json` (device id, name,
//! household key). The key arrives only over USB (the DUMP-guarded Remote activity).

use jni::objects::{JClass, JObjectArray, JString};
use jni::sys::{jboolean, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use serde_json::{json, Value};
use thurea_core::mesh::{Event, Mesh};
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
        if let Some(text) = e.alert_text() {
            self.s().events.push(json!({ "text": text, "warning": e.is_warning() }));
        }
    }
    fn on_peers(&mut self, mesh: &Mesh) {
        let mut s = self.s();
        s.peers = mesh
            .peers()
            .map(|p| {
                json!({
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
        let _ = std::fs::remove_dir_all(dir);
    }
}
