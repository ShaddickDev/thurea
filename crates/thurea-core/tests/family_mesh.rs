//! Two real mesh nodes on this computer: a child's device asks, a parent's device hears it;
//! the parent's signed answer reaches the child.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thurea_core::family::{ItemKind, Request, Role};
use thurea_core::mesh::{Body, Event, Mesh};
use thurea_core::net::{self, Host, NodeConfig};
use thurea_core::{crypto, Policy, Report};

#[derive(Clone)]
struct Node {
    policy: Arc<Mutex<Policy>>,
    asks: Arc<Mutex<Vec<Request>>>,
    heard: Arc<Mutex<Vec<Request>>>,
}

impl Host for Node {
    fn local_report(&mut self) -> Report { Report::default() }
    fn policy(&self) -> Policy { self.policy.lock().unwrap().clone() }
    fn adopt_policy(&mut self, p: Policy) { *self.policy.lock().unwrap() = p; }
    fn on_event(&mut self, e: &Event) {
        if let Event::Request { request } = e { self.heard.lock().unwrap().push(request.clone()); }
    }
    fn on_peers(&mut self, _: &Mesh) {}
    fn outgoing(&mut self) -> Vec<Body> {
        let p = self.policy.lock().unwrap().clone();
        self.asks.lock().unwrap().iter().filter(|r| !p.family.decided(r)).cloned().map(Body::Request).collect()
    }
}

fn node(policy: &Policy) -> Node {
    Node { policy: Arc::new(Mutex::new(policy.clone())), asks: Default::default(), heard: Default::default() }
}

#[test]
fn child_asks_parent_answers_over_udp() {
    let key = crypto::new_key().to_vec();
    let (mum, _) = crypto::new_parent_key();
    let mut fam = Policy::default();
    fam.add_parent("Mum", &mum, None).unwrap();
    fam.parent_edit(&mum, |p| p.family.set_role("kid", "Kid phone", Role::Child, Some("Sam"))).unwrap();

    let parent = node(&fam);
    let child = node(&Policy::default()); // learns the family over the mesh
    let stop = Arc::new(AtomicBool::new(false));
    let (pp, cp) = (47931u16, 47932u16);
    for (id, port, peer, host) in [("mum", pp, cp, parent.clone()), ("kid", cp, pp, child.clone())] {
        let mut cfg = NodeConfig::new(id.into(), id.into(), "test", key.clone());
        cfg.port = port;
        cfg.bind = [127, 0, 0, 1].into();
        cfg.broadcast = false;
        cfg.peers = vec![format!("127.0.0.1:{peer}")];
        cfg.heartbeat_every = Duration::from_millis(300);
        let stop = stop.clone();
        let mut h = host;
        std::thread::spawn(move || net::run(&cfg, &mut h, &stop).unwrap());
    }
    let wait = |what: &str, f: &dyn Fn() -> bool| {
        let t = Instant::now();
        while !f() {
            assert!(t.elapsed() < Duration::from_secs(60), "timed out: {what}");
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    wait("child gets the family", &|| child.policy.lock().unwrap().family.child_of("kid").is_some());
    let r = Request::new("kid", "Sam", ItemKind::App, "com.snapchat.android", "Snapchat", thurea_core::now());
    child.asks.lock().unwrap().push(r.clone());
    wait("parent hears the request", &|| parent.heard.lock().unwrap().iter().any(|x| x.id == r.id));
    {
        let mut p = parent.policy.lock().unwrap();
        let now = thurea_core::now();
        p.parent_edit(&mum, |p| p.family.children.get_mut("Sam").unwrap().approve(&r, Some(30), now)).unwrap();
    }
    wait("child gets the approval", &|| child.policy.lock().unwrap().family.decided(&r));
    stop.store(true, Ordering::Relaxed);
}
