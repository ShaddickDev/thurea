//! The household mesh: every Thurea device tells the others, every few seconds, whether it
//! is still protected. Together they
//!
//! * raise an alert on every other device when one drops its protection or goes quiet
//!   (switched off, uninstalled, network cut, Thurea killed);
//! * keep one policy: a block added on any device reaches all of them (highest version wins).
//!
//! Every message is authenticated with HMAC-SHA256 under the household key, carries a
//! strictly increasing sequence number and a timestamp, so it cannot be forged, replayed
//! or delayed past [`Mesh::max_skew`] by anyone without the key.

use crate::crypto;
use crate::policy::Policy;
use crate::status::{Check, State};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PORT: u16 = 47823;
/// A device that sends nothing for this long counts as gone.
pub const SILENT_AFTER: u64 = 120;
/// How far a message's clock may differ from ours.
pub const MAX_SKEW: u64 = 300;
/// Don't send the same peer policy offers/requests more often than this.
const POLICY_COOLDOWN: u64 = 30;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Heartbeat {
    pub name: String,
    pub platform: String,
    pub policy_version: u64,
    pub policy_digest: String,
    pub overall: State,
    /// Only the checks that are not Ok (keeps packets small).
    pub problems: Vec<Check>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    Heartbeat(Heartbeat),
    PolicyRequest { have_version: u64 },
    /// A whole policy in one message (only for small policies / old senders).
    PolicyOffer { policy: Policy },
    /// One piece of a policy. Policies are sent in pieces of [`PART_BYTES`] so every
    /// datagram fits a normal MTU: fragmented UDP is dropped by many NATs, VPNs and Wi-Fi.
    PolicyPart { digest: String, index: u32, total: u32, hex: String },
}

/// Bytes of policy JSON per piece (hex-encoded on the wire: ~1.2 KB per datagram).
pub const PART_BYTES: usize = 500;
const MAX_PARTS: u32 = 1024;

/// Splits a policy into signed-message bodies, in order.
pub fn policy_parts(policy: &Policy) -> Vec<Body> {
    let json = serde_json::to_vec(policy).expect("policy serializes");
    let digest = policy.digest();
    let chunks: Vec<&[u8]> = json.chunks(PART_BYTES).collect();
    let total = chunks.len() as u32;
    chunks
        .iter()
        .enumerate()
        .map(|(i, c)| Body::PolicyPart { digest: digest.clone(), index: i as u32, total, hex: crypto::hex(c) })
        .collect()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Message {
    pub from: String,
    pub seq: u64,
    pub sent_at: u64,
    pub body: Body,
}

#[derive(Serialize, Deserialize)]
struct Wire {
    m: String,
    mac: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MeshError {
    Malformed,
    BadMac,
    Replay,
    Stale,
    FromSelf,
}

/// Signs a message for the wire.
pub fn seal(key: &[u8], msg: &Message) -> Vec<u8> {
    let m = serde_json::to_string(msg).expect("message serializes");
    let mac = crypto::hex(&crypto::mac(key, m.as_bytes()));
    serde_json::to_vec(&Wire { m, mac }).expect("wire serializes")
}

/// Checks the signature and decodes; replay and freshness are checked by [`Mesh::receive`].
pub fn open(key: &[u8], bytes: &[u8]) -> Result<Message, MeshError> {
    let w: Wire = serde_json::from_slice(bytes).map_err(|_| MeshError::Malformed)?;
    let tag = crypto::unhex(&w.mac).ok_or(MeshError::Malformed)?;
    if !crypto::verify(key, w.m.as_bytes(), &tag) {
        return Err(MeshError::BadMac);
    }
    serde_json::from_str(&w.m).map_err(|_| MeshError::Malformed)
}

/// Does `a` supersede `b`? Higher version, or the same version with a larger digest.
pub fn newer(a: &Policy, b: &Policy) -> bool {
    (a.version, a.digest()) > (b.version, b.digest())
}

/// Something the device should act on or show.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    PeerJoined { id: String, name: String, platform: String },
    PeerDegraded { id: String, name: String, state: State, problems: Vec<String> },
    PeerRecovered { id: String, name: String },
    PeerSilent { id: String, name: String, seconds: u64 },
    PeerBack { id: String, name: String },
    /// A peer runs an older policy: offer ours.
    SendPolicy { to: String },
    /// A peer runs a newer policy: ask for it.
    RequestPolicy { from: String },
    /// A newer, valid policy arrived: save and enforce it.
    AdoptPolicy { policy: Policy, from: String },
}

impl Event {
    /// One line for a notification, or None for internal events.
    pub fn alert_text(&self) -> Option<String> {
        Some(match self {
            Event::PeerJoined { name, platform, .. } => format!("{name} ({platform}) joined Thurea"),
            Event::PeerDegraded { name, state, problems, .. } => {
                let what = if *state == State::Degraded { "is not protected" } else { "can't confirm it is protected" };
                if problems.is_empty() {
                    format!("{name} {what} (no checks reported yet)")
                } else {
                    format!("{name} {what}: {}", problems.join("; "))
                }
            }
            Event::PeerRecovered { name, .. } => format!("{name} is protected again"),
            Event::PeerSilent { name, seconds, .. } => format!("{name} has not checked in for {} min — Thurea may be off there", seconds / 60),
            Event::PeerBack { name, .. } => format!("{name} is back online"),
            Event::AdoptPolicy { policy, .. } => format!("Family policy updated to version {}", policy.version),
            Event::SendPolicy { .. } | Event::RequestPolicy { .. } => return None,
        })
    }

    /// True for events a guardian should see as a warning.
    pub fn is_warning(&self) -> bool {
        matches!(self, Event::PeerDegraded { .. } | Event::PeerSilent { .. })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Peer {
    pub id: String,
    pub last_seen: u64,
    pub last_seq: u64,
    pub heartbeat: Option<Heartbeat>,
    pub silent: bool,
    #[serde(skip)]
    last_policy_msg: u64,
}

impl Peer {
    pub fn name(&self) -> String {
        self.heartbeat.as_ref().map(|h| h.name.clone()).unwrap_or_else(|| self.id.clone())
    }
    /// What the household should believe about this device right now.
    pub fn state(&self) -> State {
        match &self.heartbeat {
            _ if self.silent => State::Degraded,
            Some(h) => h.overall,
            None => State::Unknown,
        }
    }
}

pub struct Mesh {
    pub me: String,
    pub silent_after: u64,
    pub max_skew: u64,
    peers: BTreeMap<String, Peer>,
    next_seq: u64,
    /// Policy pieces being collected: (sender, digest) → (pieces, first seen).
    parts: BTreeMap<(String, String), (Vec<Option<Vec<u8>>>, u64)>,
}

impl Mesh {
    pub fn new(me: impl Into<String>) -> Self {
        Mesh { me: me.into(), silent_after: SILENT_AFTER, max_skew: MAX_SKEW, peers: BTreeMap::new(), next_seq: 0, parts: BTreeMap::new() }
    }

    pub fn peers(&self) -> impl Iterator<Item = &Peer> {
        self.peers.values()
    }

    /// Wraps a body in a fresh message. Sequence numbers start from the clock in
    /// milliseconds, so they keep increasing across restarts without any saved state.
    pub fn message(&mut self, body: Body, now_ms: u128) -> Message {
        self.next_seq = (self.next_seq + 1).max(now_ms as u64);
        Message { from: self.me.clone(), seq: self.next_seq, sent_at: (now_ms / 1000) as u64, body }
    }

    /// Takes in an authenticated message. `policy` is what this device enforces now.
    pub fn receive(&mut self, msg: Message, now: u64, policy: &Policy) -> Result<Vec<Event>, MeshError> {
        if msg.from == self.me {
            return Err(MeshError::FromSelf);
        }
        if msg.sent_at.abs_diff(now) > self.max_skew {
            return Err(MeshError::Stale);
        }
        let mut ev = Vec::new();
        let peer = self.peers.entry(msg.from.clone()).or_insert_with(|| Peer {
            id: msg.from.clone(),
            last_seen: 0,
            last_seq: 0,
            heartbeat: None,
            silent: false,
            last_policy_msg: 0,
        });
        if msg.seq <= peer.last_seq {
            return Err(MeshError::Replay);
        }
        peer.last_seq = msg.seq;
        peer.last_seen = now;
        if peer.silent {
            peer.silent = false;
            ev.push(Event::PeerBack { id: peer.id.clone(), name: peer.name() });
        }
        let cooled = now.saturating_sub(peer.last_policy_msg) >= POLICY_COOLDOWN;

        match msg.body {
            Body::Heartbeat(hb) => {
                let before = peer.heartbeat.as_ref().map(|h| h.overall);
                let name = hb.name.clone();
                let id = peer.id.clone();
                match before {
                    None => ev.push(Event::PeerJoined { id: id.clone(), name: name.clone(), platform: hb.platform.clone() }),
                    Some(_) => {}
                }
                let worse = hb.overall != State::Ok
                    && (before.is_none() || before == Some(State::Ok) || before.map_or(false, |b| hb.overall > b));
                if worse {
                    ev.push(Event::PeerDegraded {
                        id: id.clone(),
                        name: name.clone(),
                        state: hb.overall,
                        problems: hb.problems.iter().map(|c| c.detail.clone()).collect(),
                    });
                } else if hb.overall == State::Ok && before.is_some_and(|b| b != State::Ok) {
                    ev.push(Event::PeerRecovered { id: id.clone(), name: name.clone() });
                }
                // Highest version wins; two different policies with the same version (edited
                // on two devices at once) are settled by the larger digest, so all converge.
                let theirs = (hb.policy_version, hb.policy_digest.clone());
                let mine = (policy.version, policy.digest());
                if cooled && theirs != mine {
                    peer.last_policy_msg = now;
                    ev.push(if theirs < mine {
                        Event::SendPolicy { to: id.clone() }
                    } else {
                        Event::RequestPolicy { from: id.clone() }
                    });
                }
                peer.heartbeat = Some(hb);
            }
            Body::PolicyRequest { have_version } => {
                if have_version <= policy.version && cooled {
                    peer.last_policy_msg = now;
                    ev.push(Event::SendPolicy { to: peer.id.clone() });
                }
            }
            Body::PolicyOffer { policy: offered } => {
                if newer(&offered, policy) && offered.validate().is_ok() {
                    ev.push(Event::AdoptPolicy { policy: offered, from: peer.id.clone() });
                }
            }
            Body::PolicyPart { digest, index, total, hex } => {
                let from = peer.id.clone();
                if total == 0 || total > MAX_PARTS || index >= total {
                    return Err(MeshError::Malformed);
                }
                let bytes = crypto::unhex(&hex).ok_or(MeshError::Malformed)?;
                let key = (from.clone(), digest.clone());
                let entry = self.parts.entry(key.clone()).or_insert_with(|| (vec![None; total as usize], now));
                if entry.0.len() != total as usize {
                    return Err(MeshError::Malformed);
                }
                entry.0[index as usize] = Some(bytes);
                if entry.0.iter().all(Option::is_some) {
                    let (pieces, _) = self.parts.remove(&key).unwrap();
                    let json: Vec<u8> = pieces.into_iter().flatten().flatten().collect();
                    // The assembled policy must be exactly the one announced.
                    match serde_json::from_slice::<Policy>(&json) {
                        Ok(p) if p.digest() == digest && newer(&p, policy) && p.validate().is_ok() => {
                            ev.push(Event::AdoptPolicy { policy: p, from });
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(ev)
    }

    /// Call about once a second: notices devices that stopped checking in.
    pub fn tick(&mut self, now: u64) -> Vec<Event> {
        let mut ev = Vec::new();
        // Pieces of a transfer that never completed are dropped after a minute (it is resent).
        self.parts.retain(|_, (_, first)| now.saturating_sub(*first) < 60);
        for p in self.peers.values_mut() {
            let quiet = now.saturating_sub(p.last_seen);
            if !p.silent && quiet >= self.silent_after {
                p.silent = true;
                ev.push(Event::PeerSilent { id: p.id.clone(), name: p.name(), seconds: quiet });
            }
        }
        ev
    }

    /// The household view: every known device and its state.
    pub fn summary(&self) -> (usize, usize) {
        let total = self.peers.len();
        let ok = self.peers.values().filter(|p| p.state() == State::Ok).count();
        (ok, total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hb(name: &str, overall: State, version: u64) -> Body {
        Body::Heartbeat(Heartbeat {
            name: name.into(),
            platform: "linux".into(),
            policy_version: version,
            policy_digest: if version == 1 { Policy::default().digest() } else { String::new() },
            overall,
            problems: if overall == State::Ok { vec![] } else { vec![Check::degraded("dns", "DNS is 8.8.8.8")] },
        })
    }

    fn msg(from: &str, seq: u64, at: u64, body: Body) -> Message {
        Message { from: from.into(), seq, sent_at: at, body }
    }

    #[test]
    fn seal_open_and_tamper() {
        let k = crypto::new_key();
        let m = msg("a", 1, 100, hb("Laptop", State::Ok, 1));
        let bytes = seal(&k, &m);
        assert_eq!(open(&k, &bytes).unwrap(), m);
        assert_eq!(open(&crypto::new_key(), &bytes), Err(MeshError::BadMac));
        let forged = String::from_utf8(bytes).unwrap().replace("Laptop", "Lapt0p");
        assert_eq!(open(&k, forged.as_bytes()), Err(MeshError::BadMac));
        assert_eq!(open(&k, b"junk"), Err(MeshError::Malformed));
    }

    #[test]
    fn join_degrade_recover() {
        let p = Policy::default();
        let mut mesh = Mesh::new("me");
        let e = mesh.receive(msg("pc", 1, 100, hb("PC", State::Ok, 1)), 100, &p).unwrap();
        assert!(matches!(e[..], [Event::PeerJoined { .. }]));
        let e = mesh.receive(msg("pc", 2, 101, hb("PC", State::Degraded, 1)), 101, &p).unwrap();
        assert!(matches!(&e[..], [Event::PeerDegraded { state: State::Degraded, .. }]));
        assert!(e[0].alert_text().unwrap().contains("8.8.8.8"));
        // still degraded: no repeat alert
        assert!(mesh.receive(msg("pc", 3, 102, hb("PC", State::Degraded, 1)), 102, &p).unwrap().is_empty());
        let e = mesh.receive(msg("pc", 4, 103, hb("PC", State::Ok, 1)), 103, &p).unwrap();
        assert!(matches!(e[..], [Event::PeerRecovered { .. }]));
    }

    #[test]
    fn rejects_replay_stale_and_self() {
        let p = Policy::default();
        let mut mesh = Mesh::new("me");
        mesh.receive(msg("pc", 5, 100, hb("PC", State::Ok, 1)), 100, &p).unwrap();
        assert_eq!(mesh.receive(msg("pc", 5, 100, hb("PC", State::Ok, 1)), 100, &p), Err(MeshError::Replay));
        assert_eq!(mesh.receive(msg("pc", 9, 1, hb("PC", State::Ok, 1)), 1000, &p), Err(MeshError::Stale));
        assert_eq!(mesh.receive(msg("me", 9, 100, hb("Me", State::Ok, 1)), 100, &p), Err(MeshError::FromSelf));
    }

    #[test]
    fn silence_is_noticed_once_and_cleared() {
        let p = Policy::default();
        let mut mesh = Mesh::new("me");
        mesh.receive(msg("phone", 1, 100, hb("Phone", State::Ok, 1)), 100, &p).unwrap();
        assert!(mesh.tick(150).is_empty());
        let e = mesh.tick(100 + SILENT_AFTER);
        assert!(matches!(e[..], [Event::PeerSilent { .. }]));
        assert!(e[0].is_warning());
        assert!(mesh.tick(400).is_empty());
        assert_eq!(mesh.summary(), (0, 1));
        let e = mesh.receive(msg("phone", 2, 401, hb("Phone", State::Ok, 1)), 401, &p).unwrap();
        assert!(matches!(e[..], [Event::PeerBack { .. }]));
        assert_eq!(mesh.summary(), (1, 1));
    }

    #[test]
    fn policy_flows_to_older_devices() {
        let mut newer = Policy::default();
        newer.block("example.org").unwrap();
        let old = Policy::default();

        // Old device hears the newer one's heartbeat → asks for the policy.
        let mut old_mesh = Mesh::new("old");
        let e = old_mesh.receive(msg("new", 1, 100, hb("New", State::Ok, newer.version)), 100, &old).unwrap();
        assert!(e.contains(&Event::RequestPolicy { from: "new".into() }));

        // Newer device hears the old heartbeat → offers.
        let mut new_mesh = Mesh::new("new");
        let e = new_mesh.receive(msg("old", 1, 100, hb("Old", State::Ok, old.version)), 100, &newer).unwrap();
        assert!(e.contains(&Event::SendPolicy { to: "old".into() }));
        // ...but not again straight away.
        let e = new_mesh.receive(msg("old", 2, 101, hb("Old", State::Ok, old.version)), 101, &newer).unwrap();
        assert!(!e.contains(&Event::SendPolicy { to: "old".into() }));

        // Old device adopts the offer; an older or invalid offer is ignored.
        let e = old_mesh.receive(msg("new", 2, 102, Body::PolicyOffer { policy: newer.clone() }), 102, &old).unwrap();
        assert_eq!(e, vec![Event::AdoptPolicy { policy: newer.clone(), from: "new".into() }]);
        let e = old_mesh.receive(msg("new", 3, 103, Body::PolicyOffer { policy: old.clone() }), 103, &newer).unwrap();
        assert!(e.is_empty());
        let mut broken = newer.clone();
        broken.version = 99;
        broken.family_dns.servers.clear();
        let e = old_mesh.receive(msg("new", 4, 104, Body::PolicyOffer { policy: broken }), 104, &old).unwrap();
        assert!(e.is_empty());
    }

    #[test]
    fn same_version_conflict_converges() {
        let mut a = Policy::default();
        a.block("a.example").unwrap();
        let mut b = Policy::default();
        b.block("b.example").unwrap();
        assert_eq!(a.version, b.version);
        let (win, lose) = if newer(&a, &b) { (a, b) } else { (b, a) };
        let mut m = Mesh::new("loser");
        let e = m.receive(msg("w", 1, 100, Body::PolicyOffer { policy: win.clone() }), 100, &lose).unwrap();
        assert!(matches!(&e[..], [Event::AdoptPolicy { .. }]));
        let mut m = Mesh::new("winner");
        let e = m.receive(msg("l", 1, 100, Body::PolicyOffer { policy: lose }), 100, &win).unwrap();
        assert!(e.is_empty());
    }

    #[test]
    fn policy_travels_in_small_pieces_in_any_order() {
        let mut newer_p = Policy::default();
        newer_p.block("example.org").unwrap();
        let old = Policy::default();
        let k = crypto::new_key();
        let mut parts = policy_parts(&newer_p);
        assert!(parts.len() > 10);
        let mut sender = Mesh::new("new");
        for b in &parts {
            assert!(seal(&k, &sender.message(b.clone(), 1)).len() <= 1300, "datagram too big for a normal MTU");
        }
        parts.reverse(); // arrival order doesn't matter
        let mut m = Mesh::new("old");
        let n = parts.len();
        let mut adopted = None;
        for (i, b) in parts.into_iter().enumerate() {
            let e = m.receive(msg("new", i as u64 + 1, 100, b), 100, &old).unwrap();
            if i + 1 < n {
                assert!(e.is_empty());
            } else {
                adopted = Some(e);
            }
        }
        assert_eq!(adopted.unwrap(), vec![Event::AdoptPolicy { policy: newer_p.clone(), from: "new".into() }]);
    }

    #[test]
    fn incomplete_or_mismatched_pieces_are_ignored() {
        let mut newer_p = Policy::default();
        newer_p.block("example.org").unwrap();
        let old = Policy::default();
        let mut parts = policy_parts(&newer_p);
        parts.pop(); // one piece lost
        let mut m = Mesh::new("old");
        for (i, b) in parts.into_iter().enumerate() {
            assert!(m.receive(msg("new", i as u64 + 1, 100, b), 100, &old).unwrap().is_empty());
        }
        m.tick(200); // stale buffer dropped
        assert!(m.parts.is_empty());
        // pieces claiming another policy's digest never assemble into an adoption
        let mut forged = policy_parts(&newer_p);
        for b in &mut forged {
            if let Body::PolicyPart { digest, .. } = b {
                *digest = "0000000000000000".into();
            }
        }
        let mut m = Mesh::new("old");
        for (i, b) in forged.into_iter().enumerate() {
            assert!(m.receive(msg("new", i as u64 + 1, 100, b), 100, &old).unwrap().is_empty());
        }
        assert_eq!(
            m.receive(msg("new", 999, 100, Body::PolicyPart { digest: "x".into(), index: 5, total: 2, hex: String::new() }), 100, &old),
            Err(MeshError::Malformed)
        );
    }

    #[test]
    fn sequence_survives_restart() {
        let mut a = Mesh::new("a");
        let s1 = a.message(hb("A", State::Ok, 1), 1_000_000).seq;
        let s2 = a.message(hb("A", State::Ok, 1), 1_000_000).seq;
        assert!(s2 > s1);
        let mut restarted = Mesh::new("a");
        assert!(restarted.message(hb("A", State::Ok, 1), 1_005_000).seq > s2);
    }
}
