//! The mesh over UDP. Heartbeats go to the LAN broadcast address, to any peers listed by
//! address (for devices on other networks, e.g. over Tailscale or WireGuard), and through the
//! household's self-hosted relay if the family policy names one ([`crate::relay`]).
//! Uses only `std::net`, so the same loop runs on Android, Linux and Windows.

use crate::mesh::{self, Body, Event, Heartbeat, Mesh};
use crate::policy::Policy;
use crate::relay;
use crate::status::{Report, State};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct NodeConfig {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub key: Vec<u8>,
    pub port: u16,
    /// Extra peers by address ("100.64.0.7", "laptop.tailnet:47823").
    pub peers: Vec<String>,
    pub heartbeat_every: Duration,
    /// Send to the LAN broadcast address too.
    pub broadcast: bool,
    /// Address to bind (0.0.0.0 normally; 127.0.0.1 keeps a test node off every real network).
    pub bind: std::net::IpAddr,
    /// Seconds without a message before a peer counts as gone.
    pub silent_after: u64,
    /// Relay to use instead of the policy's ("host:port").
    pub relay: Option<String>,
}

impl NodeConfig {
    pub fn new(id: String, name: String, platform: &str, key: Vec<u8>) -> Self {
        NodeConfig {
            id,
            name,
            platform: platform.into(),
            key,
            port: mesh::PORT,
            peers: Vec::new(),
            heartbeat_every: Duration::from_secs(15),
            broadcast: true,
            bind: std::net::IpAddr::from([0, 0, 0, 0]),
            silent_after: mesh::SILENT_AFTER,
            relay: None,
        }
    }
}

/// What the platform provides to the mesh.
pub trait Host {
    /// Current protection checks of this device.
    fn local_report(&mut self) -> Report;
    fn policy(&self) -> Policy;
    /// A newer family policy arrived: save it and enforce it.
    fn adopt_policy(&mut self, policy: Policy);
    /// Show an alert or log an event.
    fn on_event(&mut self, event: &Event);
    /// The household view changed (for status screens).
    fn on_peers(&mut self, _mesh: &Mesh) {}
    /// Extra messages to send with each heartbeat (a child's open requests, resent until
    /// a parent answers).
    fn outgoing(&mut self) -> Vec<Body> {
        Vec::new()
    }
    /// About once a second. Returns true to announce right away (the policy just changed here).
    fn tick(&mut self) -> bool {
        false
    }
}

/// Debug log of the mesh (THUREA_DEBUG=1 → stderr; THUREA_DEBUG_FILE=path → appended there).
pub fn debug(line: &str) {
    if std::env::var_os("THUREA_DEBUG").is_some() {
        eprintln!("{line}");
    }
    if let Some(p) = std::env::var_os("THUREA_DEBUG_FILE") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{} {line}", crate::now());
        }
    }
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

pub fn heartbeat(cfg: &NodeConfig, report: &Report, policy: &Policy) -> Body {
    Body::Heartbeat(Heartbeat {
        name: cfg.name.clone(),
        platform: cfg.platform.clone(),
        policy_version: policy.version,
        policy_digest: policy.digest(),
        overall: report.overall(),
        // Kept small: a heartbeat must fit one datagram (relays and many networks drop bigger).
        problems: report
            .problems()
            .take(4)
            .map(|c| {
                let mut c = c.clone();
                if c.detail.chars().count() > 80 {
                    c.detail = c.detail.chars().take(79).collect::<String>() + "…";
                }
                c
            })
            .collect(),
    })
}

fn resolve(peers: &[String], port: u16) -> Vec<SocketAddr> {
    peers
        .iter()
        .flat_map(|p| {
            if let Ok(ip) = p.parse::<std::net::IpAddr>() {
                return vec![SocketAddr::new(ip, port)];
            }
            let with_port = if p.parse::<SocketAddr>().is_ok() || p.rsplit_once(':').is_some_and(|(h, _)| !h.contains(':')) {
                p.clone()
            } else {
                format!("{p}:{port}")
            };
            with_port.to_socket_addrs().map(|a| a.collect::<Vec<_>>()).unwrap_or_default()
        })
        .collect()
}

/// Runs until `stop` is set. Errors only if the socket can't be opened.
pub fn run(cfg: &NodeConfig, host: &mut dyn Host, stop: &AtomicBool) -> io::Result<()> {
    let sock = UdpSocket::bind((cfg.bind, cfg.port))?;
    sock.set_broadcast(cfg.broadcast)?;
    sock.set_read_timeout(Some(Duration::from_millis(500)))?;
    let mut mesh = Mesh::new(cfg.id.clone());
    mesh.silent_after = cfg.silent_after;
    let mut buf = vec![0u8; 65536];
    let mut last_hb: Option<Instant> = None;
    let mut last_tick = Instant::now();
    let mut addrs = resolve(&cfg.peers, cfg.port);
    let mut addrs_at = Instant::now();
    // Where each peer was last heard from, for direct replies.
    let mut seen_at: HashMap<String, SocketAddr> = HashMap::new();

    // The relay (if any): packets to it are wrapped with the household's room.
    let room = relay::room(&cfg.key);
    let relay_name = |p: &Policy| cfg.relay.clone().or_else(|| p.relay.clone()).filter(|r| !r.trim().is_empty());
    let mut relay_for = relay_name(&host.policy());
    let mut relay_addr = relay_for.as_ref().and_then(|r| resolve(&[r.clone()], relay::PORT).into_iter().next());

    let send = |mesh: &mut Mesh, body: Body, to: &[SocketAddr], via: Option<SocketAddr>| {
        let pkt = mesh::seal(&cfg.key, &mesh.message(body, now_ms()));
        for a in to {
            let bytes = if Some(*a) == via { relay::wrap(&room, &pkt) } else { pkt.clone() };
            if let Err(e) = sock.send_to(&bytes, a) {
                self::debug(&format!("mesh: send to {a} failed: {e}"));
            }
        }
    };

    while !stop.load(Ordering::Relaxed) {
        let wanted = relay_name(&host.policy());
        if addrs_at.elapsed() > Duration::from_secs(300) || wanted != relay_for {
            addrs = resolve(&cfg.peers, cfg.port); // names may move (DHCP, VPN)
            relay_addr = wanted.as_ref().and_then(|r| resolve(&[r.clone()], relay::PORT).into_iter().next());
            relay_for = wanted;
            addrs_at = Instant::now();
        }
        if last_hb.map_or(true, |t| t.elapsed() >= cfg.heartbeat_every) {
            let report = host.local_report();
            let policy = host.policy();
            let mut to = addrs.clone();
            // Also where each peer was last heard from: a device behind NAT (a phone on
            // another network) is only reachable at the address its own packets came from.
            for a in seen_at.values() {
                if !to.contains(a) {
                    to.push(*a);
                }
            }
            if cfg.broadcast {
                to.push(SocketAddr::from(([255, 255, 255, 255], cfg.port)));
            }
            if let Some(r) = relay_addr {
                if !to.contains(&r) {
                    to.push(r);
                }
            }
            send(&mut mesh, heartbeat(cfg, &report, &policy), &to, relay_addr);
            for b in host.outgoing() {
                send(&mut mesh, b, &to, relay_addr);
            }
            last_hb = Some(Instant::now());
        }

        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                // Through the relay: only our household's room.
                let packet = match relay::unwrap(&buf[..n]) {
                    Some((r, inner)) if r == room => inner,
                    Some(_) => continue,
                    None => &buf[..n],
                };
                let debug = std::env::var_os("THUREA_DEBUG").is_some() || std::env::var_os("THUREA_DEBUG_FILE").is_some();
                let msg = match mesh::open(&cfg.key, packet) {
                    Ok(m) => m,
                    Err(e) => {
                        if debug {
                            self::debug(&format!("mesh: dropped {} bytes from {from}: {e:?}", packet.len()));
                        }
                        continue;
                    }
                };
                let sender = msg.from.clone();
                let kind = if debug { serde_json::to_value(&msg.body).ok().and_then(|v| v["kind"].as_str().map(String::from)).unwrap_or_default() } else { String::new() };
                let policy = host.policy();
                let events = match mesh.receive(msg, crate::now(), &policy) {
                    Ok(e) => e,
                    Err(e) => {
                        if debug {
                            self::debug(&format!("mesh: rejected {kind} from {sender} ({from}): {e:?}"));
                        }
                        continue;
                    }
                };
                if debug {
                    self::debug(&format!("mesh: {kind} from {sender} ({from}), {} bytes", packet.len()));
                    for e in &events {
                        self::debug(&format!("mesh:   -> {}", serde_json::to_string(e).unwrap_or_default().chars().take(200).collect::<String>()));
                    }
                }
                seen_at.insert(sender, from);
                // Policy messages go to where the peer was heard from and to every known
                // route (NAT may hide the first); receivers ignore offers that aren't newer.
                let routes = |peer: &str| {
                    let mut to: Vec<SocketAddr> = seen_at.get(peer).copied().into_iter().collect();
                    for a in &addrs {
                        if !to.contains(a) {
                            to.push(*a);
                        }
                    }
                    if cfg.broadcast {
                        to.push(SocketAddr::from(([255, 255, 255, 255], cfg.port)));
                    }
                    if let Some(r) = relay_addr {
                        if !to.contains(&r) {
                            to.push(r);
                        }
                    }
                    to
                };
                for e in &events {
                    match e {
                        Event::SendPolicy { to } => {
                            let to = routes(to);
                            for part in mesh::policy_parts(&policy) {
                                send(&mut mesh, part, &to, relay_addr);
                                // Paced: a burst of datagrams overflows small NAT/Wi-Fi buffers.
                                std::thread::sleep(Duration::from_millis(3));
                            }
                        }
                        Event::RequestPolicy { from } => {
                            send(&mut mesh, Body::PolicyRequest { have_version: policy.version }, &routes(from), relay_addr);
                        }
                        Event::AdoptPolicy { policy, .. } => {
                            host.adopt_policy(policy.clone());
                            last_hb = None; // announce the new version right away
                        }
                        _ => {}
                    }
                    host.on_event(e);
                }
                host.on_peers(&mesh); // keep "last seen" and versions current
            }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }

        if last_tick.elapsed() >= Duration::from_secs(1) {
            last_tick = Instant::now();
            if host.tick() {
                // A parent changed the rules here: push them to every device now, instead of
                // waiting for each to notice the new version in a heartbeat.
                last_hb = None;
                let policy = host.policy();
                let mut to = addrs.clone();
                for a in seen_at.values() {
                    if !to.contains(a) {
                        to.push(*a);
                    }
                }
                if cfg.broadcast {
                    to.push(SocketAddr::from(([255, 255, 255, 255], cfg.port)));
                }
                if let Some(r) = relay_addr {
                    if !to.contains(&r) {
                        to.push(r);
                    }
                }
                for part in mesh::policy_parts(&policy) {
                    send(&mut mesh, part, &to, relay_addr);
                    std::thread::sleep(Duration::from_millis(3));
                }
            }
            let events = mesh.tick(crate::now());
            for e in &events {
                host.on_event(e);
            }
            if !events.is_empty() {
                host.on_peers(&mesh);
            }
        }
    }
    Ok(())
}

/// The household line for status screens: "3 of 3 devices protected".
pub fn household_line(mesh: &Mesh, me: State) -> String {
    let (ok, total) = mesh.summary();
    let ok = ok + usize::from(me == State::Ok);
    format!("{ok} of {} devices protected", total + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::Check;
    use std::sync::{Arc, Mutex};

    struct TestHost {
        report: Report,
        policy: Policy,
        events: Arc<Mutex<Vec<Event>>>,
    }

    impl Host for TestHost {
        fn local_report(&mut self) -> Report {
            self.report.clone()
        }
        fn policy(&self) -> Policy {
            self.policy.clone()
        }
        fn adopt_policy(&mut self, p: Policy) {
            self.policy = p;
        }
        fn on_event(&mut self, e: &Event) {
            self.events.lock().unwrap().push(e.clone());
        }
    }

    /// Two real nodes on localhost: the degraded one is reported, and the newer policy
    /// travels to the older node.
    #[test]
    fn heartbeats_fit_one_datagram() {
        let mut r = Report::default();
        for i in 0..40 {
            r.push(crate::Check::degraded(&format!("c{i}"), "x".repeat(500)));
        }
        let cfg = NodeConfig::new("a".repeat(16), "A very long device name".into(), "android", crate::crypto::new_key().to_vec());
        let mut m = Mesh::new("a");
        let pkt = mesh::seal(&cfg.key, &m.message(heartbeat(&cfg, &r, &Policy::default()), 1));
        assert!(pkt.len() < 1400, "{}", pkt.len());
    }

    #[test]
    fn two_nodes_talk_over_udp() {
        let key = crate::crypto::new_key().to_vec();
        let (pa, pb) = (47911, 47912);
        let mut newer = Policy::default();
        newer.block("example.org").unwrap();

        let mut ca = NodeConfig::new("a".into(), "Laptop".into(), "linux", key.clone());
        ca.port = pa;
        ca.peers = vec![format!("127.0.0.1:{pb}")];
        ca.broadcast = false;
        ca.bind = std::net::IpAddr::from([127, 0, 0, 1]);
        ca.heartbeat_every = Duration::from_millis(200);
        let mut cb = NodeConfig::new("b".into(), "Phone".into(), "android", key);
        cb.port = pb;
        cb.peers = vec![format!("127.0.0.1:{pa}")];
        cb.broadcast = false;
        cb.bind = std::net::IpAddr::from([127, 0, 0, 1]);
        cb.heartbeat_every = Duration::from_millis(200);

        let ea = Arc::new(Mutex::new(Vec::new()));
        let eb = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let mut degraded = Report::default();
        degraded.push(Check::degraded("dns", "DNS is 8.8.8.8"));

        let (s1, e1, n) = (stop.clone(), ea.clone(), newer.clone());
        let ta = std::thread::spawn(move || {
            let mut h = TestHost { report: degraded, policy: n, events: e1 };
            run(&ca, &mut h, &s1).unwrap();
        });
        let (s2, e2) = (stop.clone(), eb.clone());
        let tb = std::thread::spawn(move || {
            let mut ok = Report::default();
            ok.push(Check::ok("dns", "family DNS"));
            let mut h = TestHost { report: ok, policy: Policy::default(), events: e2 };
            run(&cb, &mut h, &s2).unwrap();
            h.policy
        });

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            // Wait for both: the policy and the warning can arrive in either order.
            let got = {
                let eb = eb.lock().unwrap();
                eb.iter().any(|e| matches!(e, Event::AdoptPolicy { .. }))
                    && eb.iter().any(|e| matches!(e, Event::PeerDegraded { name, .. } if name == "Laptop"))
            };
            if got {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        stop.store(true, Ordering::Relaxed);
        ta.join().unwrap();
        let b_policy = tb.join().unwrap();

        let eb = eb.lock().unwrap();
        assert!(eb.iter().any(|e| matches!(e, Event::PeerDegraded { name, .. } if name == "Laptop")), "{eb:?}");
        assert_eq!(b_policy.version, newer.version);
        assert!(b_policy.domains().is_blocked("example.org"));
        assert!(ea.lock().unwrap().iter().any(|e| matches!(e, Event::PeerJoined { name, .. } if name == "Phone")));
    }
}
