//! `thurea run`: re-applies the policy every minute and keeps this computer in the
//! household mesh. Writes `state.json` so `thurea status` works without admin rights.

use crate::config::{self, Config};
use crate::platform;
use serde::Serialize;
use thurea_core::family::Request;
use thurea_core::mesh::{Body, Event, Mesh};
use thurea_core::net::{self, Host, NodeConfig};
use thurea_core::{Policy, Report, State};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant, SystemTime};

const REAPPLY_EVERY: Duration = Duration::from_secs(60);

#[derive(Serialize, Clone)]
struct PeerState {
    id: String,
    name: String,
    platform: String,
    state: State,
    last_seen: u64,
    policy_version: u64,
    problems: Vec<String>,
}

#[derive(Serialize)]
struct StateFile<'a> {
    updated: u64,
    id: &'a str,
    name: &'a str,
    platform: &'a str,
    policy_version: u64,
    report: &'a Report,
    household: String,
    peers: Vec<PeerState>,
    recent: &'a [(u64, String)],
}

struct Desktop {
    cfg: Config,
    policy: Policy,
    policy_mtime: Option<SystemTime>,
    report: Report,
    applied_at: Option<Instant>,
    recent: Vec<(u64, String)>,
    peers: Vec<PeerState>,
    household: String,
    /// Only the household mesh: the package's own scripts enforce (Linux / Windows packages).
    mesh_only: bool,
    /// This computer asking to become a parent's (from `thurea family join`).
    asks: Vec<Request>,
}

impl Desktop {
    /// Takes what was dropped in the submit folder: a newer parent-signed policy, or a
    /// "make this computer a parent's" request. Returns true if the policy changed.
    fn take_submissions(&mut self) -> bool {
        let Ok(dir) = std::fs::read_dir(config::submit_dir()) else { return false };
        let mut changed = false;
        for f in dir.flatten() {
            let path = f.path();
            let name = f.file_name().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let _ = std::fs::remove_file(&path);
            if text.len() > 4 << 20 {
                continue;
            }
            if name.ends_with(".policy.json") {
                if let Ok(p) = Policy::from_json(&text) {
                    if thurea_core::mesh::newer(&p, &self.policy) && self.policy.accepts(&p).is_ok() {
                        self.adopt_policy(p);
                        changed = true;
                    } else {
                        eprintln!("thurea: ignored {name}: not newer, or not signed by a parent");
                    }
                }
            } else if name.ends_with(".ask.json") {
                if let Ok(r) = serde_json::from_str::<Request>(&text) {
                    if r.device == self.cfg.id && !self.asks.iter().any(|a| a.item == r.item) {
                        self.asks.push(r);
                        let _ = std::fs::write(config::state_dir().join("asks.json"), serde_json::to_string(&self.asks).unwrap());
                    }
                }
            }
        }
        changed
    }

    /// The report for the household: enforce (agent mode) or read the package's checks.
    fn mesh_only_report() -> Report {
        let v = platform::desktop_status();
        let mut r = Report::default();
        match v["state"].as_str() {
            Some("ok") => r.push(thurea_core::Check::ok("desktop", "This computer is protected")),
            Some("unknown") => r.push(thurea_core::Check::unknown("desktop", v["line"].as_str().unwrap_or("Not reporting"))),
            _ => {
                let failing: Vec<String> = v["checks"].as_object().map(|m| m.iter().filter(|(_, ok)| ok.as_bool() == Some(false)).map(|(k, _)| k.clone()).collect()).unwrap_or_default();
                let line = v["line"].as_str().unwrap_or("Not fully protected").to_string();
                r.push(thurea_core::Check::degraded("desktop", &if failing.is_empty() { line } else { format!("{line} ({})", failing.join(", ")) }));
            }
        }
        r
    }
    /// Picks up `thurea block`/`unblock` made while running.
    fn reload_policy_if_changed(&mut self) {
        let m = std::fs::metadata(config::policy_path()).and_then(|m| m.modified()).ok();
        if m != self.policy_mtime {
            self.policy_mtime = m;
            self.policy = config::load_policy();
            self.applied_at = None;
        }
    }

    fn write_state(&self) {
        let s = StateFile {
            updated: thurea_core::now(),
            id: &self.cfg.id,
            name: &self.cfg.name,
            platform: platform::PLATFORM,
            policy_version: self.policy.version,
            report: &self.report,
            household: self.household.clone(),
            peers: self.peers.clone(),
            recent: &self.recent,
        };
        let _ = std::fs::create_dir_all(config::state_dir());
        let _ = platform::write_if_changed(&config::state_path(), serde_json::to_string_pretty(&s).unwrap().as_bytes());
        let _ = platform::write_if_changed(&config::public_policy_path(), self.policy.to_json().as_bytes());
        // Answered requests leave the parents' inbox.
        let inbox = crate::family::inbox();
        let open: Vec<&Request> = inbox.iter().filter(|r| !self.policy.family.decided(r)).collect();
        if open.len() != inbox.len() {
            crate::family::save_inbox_refs(&open);
        }
    }
}

impl Host for Desktop {
    fn local_report(&mut self) -> Report {
        self.reload_policy_if_changed();
        if self.applied_at.map_or(true, |t| t.elapsed() >= REAPPLY_EVERY) {
            let before = self.report.overall();
            self.report = if self.mesh_only { Self::mesh_only_report() } else { platform::apply(&self.policy) };
            self.applied_at = Some(Instant::now());
            let now = self.report.overall();
            if now != State::Ok && before == State::Ok {
                let what: Vec<String> = self.report.problems().map(|c| c.detail.clone()).collect();
                platform::notify(&format!("This computer is not fully protected: {}", what.join("; ")), true);
            }
            self.write_state();
        }
        self.report.clone()
    }

    fn policy(&self) -> Policy {
        self.policy.clone()
    }

    fn adopt_policy(&mut self, p: Policy) {
        if !thurea_core::mesh::newer(&p, &self.policy) {
            return;
        }
        if let Err(e) = p.save(&config::policy_path()) {
            eprintln!("thurea: can't save the new policy: {e}");
        }
        self.policy_mtime = std::fs::metadata(config::policy_path()).and_then(|m| m.modified()).ok();
        self.policy = p;
        self.applied_at = None; // enforce right away
    }

    fn on_event(&mut self, e: &Event) {
        if let Event::Usage { report } = e {
            crate::family::save_usage(report);
            return;
        }
        if let Event::Request { request } = e {
            // A child asks: only a parent's computer keeps it (and says so).
            if crate::family::received(request, &self.policy) {
                platform::notify(&format!("{} — answer with: thurea family", e.alert_text().unwrap_or_default()), false);
            }
            return;
        }
        if let Some(t) = e.alert_text() {
            if e.is_warning() || matches!(e, Event::PeerJoined { .. } | Event::AdoptPolicy { .. }) {
                platform::notify(&t, e.is_warning());
            } else {
                eprintln!("thurea: {t}");
            }
            self.recent.push((thurea_core::now(), t));
            let n = self.recent.len();
            if n > 50 {
                self.recent.drain(..n - 50);
            }
        }
    }

    fn outgoing(&mut self) -> Vec<Body> {
        let f = &self.policy.family;
        self.asks.retain(|r| !f.decided(r));
        self.asks.iter().cloned().map(Body::Request).collect()
    }

    fn tick(&mut self) -> bool {
        self.take_submissions()
    }

    fn on_peers(&mut self, mesh: &Mesh) {
        self.peers = mesh
            .peers()
            .map(|p| PeerState {
                id: p.id.clone(),
                name: p.name(),
                platform: p.heartbeat.as_ref().map(|h| h.platform.clone()).unwrap_or_default(),
                state: p.state(),
                last_seen: p.last_seen,
                policy_version: p.heartbeat.as_ref().map(|h| h.policy_version).unwrap_or(0),
                problems: p.heartbeat.as_ref().map(|h| h.problems.iter().map(|c| c.detail.clone()).collect()).unwrap_or_default(),
            })
            .collect();
        self.household = net::household_line(mesh, self.report.overall());
        self.write_state();
    }
}

/// The service. `mesh_only`: don't enforce (the package's scripts do), just keep this computer
/// in the household and take parents' changes.
pub fn run(mesh_only: bool) -> Result<(), String> {
    let cfg = Config::load()?;
    crate::platform::open_submit_dir(&config::submit_dir())?;
    let mut node = NodeConfig::new(cfg.id.clone(), cfg.name.clone(), platform::PLATFORM, cfg.key_bytes()?);
    node.port = cfg.port;
    node.peers = cfg.peers.clone();
    node.broadcast = cfg.lan_broadcast;
    node.relay = cfg.relay.clone();
    let mut host = Desktop {
        cfg,
        policy: Policy::default(),
        policy_mtime: None,
        report: Report::default(),
        applied_at: None,
        recent: Vec::new(),
        peers: Vec::new(),
        household: "1 of 1 devices".into(),
        mesh_only,
        asks: std::fs::read_to_string(config::state_dir().join("asks.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
    };
    host.reload_policy_if_changed();
    eprintln!("thurea: running as {} (policy v{}), mesh on UDP {}", host.cfg.name, host.policy.version, node.port);
    let stop = AtomicBool::new(false);
    // Keep trying if the port is briefly taken (e.g. an old copy still shutting down).
    loop {
        match net::run(&node, &mut host, &stop) {
            Ok(()) => return Ok(()),
            Err(e) => {
                eprintln!("thurea: mesh socket: {e}; retrying in 10 s");
                host.local_report(); // keep enforcing even without the mesh
                std::thread::sleep(Duration::from_secs(10));
            }
        }
    }
}

/// `thurea watch`: joins the mesh as an observer — enforces nothing, needs no admin
/// rights, prints what the household's devices report. For a guardian's own computer.
struct Observer {
    policy: Policy,
}

impl Host for Observer {
    fn local_report(&mut self) -> Report {
        let mut r = Report::default();
        r.push(thurea_core::Check::unknown("observer", "Observer only (enforces nothing)"));
        r
    }
    fn policy(&self) -> Policy {
        self.policy.clone()
    }
    fn adopt_policy(&mut self, p: Policy) {
        if thurea_core::mesh::newer(&p, &self.policy) {
            println!("policy: now version {}", p.version);
            if config::policy_path().parent().is_some_and(|d| d.exists()) {
                let _ = p.save(&config::policy_path());
            }
            self.policy = p;
        }
    }
    fn on_event(&mut self, e: &Event) {
        match e.alert_text() {
            Some(t) => println!("{}{t}", if e.is_warning() { "WARNING: " } else { "" }),
            None => println!("mesh: {}", serde_json::to_string(e).unwrap_or_default()),
        }
    }
    fn on_peers(&mut self, mesh: &Mesh) {
        for p in mesh.peers() {
            let probs: Vec<String> = p.heartbeat.iter().flat_map(|h| h.problems.iter().map(|c| c.detail.clone())).collect();
            let (plat, ver) = p.heartbeat.as_ref().map(|h| (h.platform.as_str(), h.policy_version)).unwrap_or(("?", 0));
            println!("  {} {} [{plat}, policy v{ver}] {}", p.state().mark(), p.name(), probs.join("; "));
        }
    }
}

pub fn watch() -> Result<(), String> {
    let cfg = Config::load()?;
    let mut node = NodeConfig::new(cfg.id.clone(), format!("{} (observer)", cfg.name), platform::PLATFORM, cfg.key_bytes()?);
    node.port = cfg.port;
    node.peers = cfg.peers.clone();
    node.broadcast = cfg.lan_broadcast;
    node.relay = cfg.relay.clone();
    let mut host = Observer { policy: config::load_policy() };
    println!("Watching the household on UDP {} (policy v{}). Ctrl+C to stop.", node.port, host.policy.version);
    net::run(&node, &mut host, &AtomicBool::new(false)).map_err(|e| e.to_string())
}
