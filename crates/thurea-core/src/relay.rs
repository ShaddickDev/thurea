//! The self-hosted relay: lets the household's devices reach each other from anywhere (a
//! child's phone on mobile data, a parent at work) without opening ports at home.
//!
//! It is a plain UDP forwarder. Every device sends its mesh packets — already encrypted with
//! the household key — to the relay, tagged with the household's *room*, a number derived from
//! the household key (it identifies the household without revealing the key). The relay
//! passes each packet to the other devices it has recently heard from in the same room, at the
//! address their packets came from (which keeps their NAT open: devices send every 15 s).
//!
//! The relay never sees anything but ciphertext, room numbers and IP addresses; it can't read,
//! forge or alter messages (the mesh rejects anything that doesn't decrypt). It keeps nothing on
//! disk. Run it with `thurea relay` on any computer with a public UDP port.

use crate::crypto;
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const PORT: u16 = 47830;
const MAGIC: &[u8; 4] = b"TRL1";
/// A device not heard from for this long is dropped from its room.
const MEMBER_TTL: Duration = Duration::from_secs(120);
const MAX_MEMBERS: usize = 32;
const MAX_ROOMS: usize = 10_000;
const MAX_PACKET: usize = 2048;
/// Packets per second one address may send (a burst of policy pieces fits).
const RATE: f64 = 200.0;

pub type Room = [u8; 16];

/// The household's room on the relay.
pub fn room(household_key: &[u8]) -> Room {
    crypto::mac(household_key, b"thurea relay room v1")[..16].try_into().unwrap()
}

pub fn wrap(room: &Room, packet: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(4 + 16 + packet.len());
    v.extend_from_slice(MAGIC);
    v.extend_from_slice(room);
    v.extend_from_slice(packet);
    v
}

/// (room, inner packet), or None if it isn't a relay packet.
pub fn unwrap(bytes: &[u8]) -> Option<(Room, &[u8])> {
    if bytes.len() < 20 || &bytes[..4] != MAGIC {
        return None;
    }
    Some((bytes[4..20].try_into().unwrap(), &bytes[20..]))
}

struct Bucket {
    tokens: f64,
    at: Instant,
}

/// The relay's memory: who is in which room, and how fast each address is sending.
#[derive(Default)]
pub struct Relay {
    rooms: HashMap<Room, Vec<(SocketAddr, Instant)>>,
    rate: HashMap<SocketAddr, Bucket>,
}

impl Relay {
    /// Takes a packet from `from`; returns where to forward it (unchanged).
    pub fn handle(&mut self, from: SocketAddr, bytes: &[u8], now: Instant) -> Vec<SocketAddr> {
        if bytes.len() > MAX_PACKET || bytes.len() <= 20 {
            return Vec::new();
        }
        let Some((room, _)) = unwrap(bytes) else { return Vec::new() };
        // Rate limit per sender.
        let b = self.rate.entry(from).or_insert(Bucket { tokens: RATE, at: now });
        b.tokens = (b.tokens + now.duration_since(b.at).as_secs_f64() * RATE).min(RATE);
        b.at = now;
        if b.tokens < 1.0 {
            return Vec::new();
        }
        b.tokens -= 1.0;
        if !self.rooms.contains_key(&room) && self.rooms.len() >= MAX_ROOMS {
            self.prune(now);
            if self.rooms.len() >= MAX_ROOMS {
                return Vec::new();
            }
        }
        let members = self.rooms.entry(room).or_default();
        members.retain(|(_, seen)| now.duration_since(*seen) < MEMBER_TTL);
        if let Some(m) = members.iter_mut().find(|(a, _)| *a == from) {
            m.1 = now;
        } else if members.len() < MAX_MEMBERS {
            members.push((from, now));
        } else {
            return Vec::new();
        }
        members.iter().map(|(a, _)| *a).filter(|a| *a != from).collect()
    }

    /// Forgets quiet devices, empty rooms and old rate counters.
    pub fn prune(&mut self, now: Instant) {
        for m in self.rooms.values_mut() {
            m.retain(|(_, seen)| now.duration_since(*seen) < MEMBER_TTL);
        }
        self.rooms.retain(|_, m| !m.is_empty());
        self.rate.retain(|_, b| now.duration_since(b.at) < MEMBER_TTL);
    }

    pub fn stats(&self) -> (usize, usize) {
        (self.rooms.len(), self.rooms.values().map(Vec::len).sum())
    }
}

/// Runs the relay until `stop` is set.
pub fn serve(bind: SocketAddr, stop: &AtomicBool, log: &mut dyn FnMut(&str)) -> io::Result<()> {
    let sock = UdpSocket::bind(bind)?;
    sock.set_read_timeout(Some(Duration::from_millis(500)))?;
    log(&format!("Thurea relay on UDP {bind} (forwards encrypted household packets; stores nothing)"));
    let mut relay = Relay::default();
    let mut buf = vec![0u8; MAX_PACKET + 1];
    let mut last_prune = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                for to in relay.handle(from, &buf[..n], Instant::now()) {
                    let _ = sock.send_to(&buf[..n], to);
                }
            }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
        if last_prune.elapsed() > Duration::from_secs(60) {
            relay.prune(Instant::now());
            last_prune = Instant::now();
            let (rooms, devices) = relay.stats();
            log(&format!("{rooms} households, {devices} devices connected"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(p: u16) -> SocketAddr {
        SocketAddr::from(([203, 0, 113, 1], p))
    }

    #[test]
    fn forwards_within_a_room_only() {
        let (a, b) = (room(b"household one"), room(b"household two"));
        assert_ne!(a, b);
        let mut r = Relay::default();
        let t = Instant::now();
        assert!(r.handle(addr(1), &wrap(&a, b"hello"), t).is_empty(), "first one in: nobody to send to");
        assert_eq!(r.handle(addr(2), &wrap(&a, b"hi"), t), vec![addr(1)]);
        assert!(r.handle(addr(3), &wrap(&b, b"other"), t).is_empty(), "another household hears nothing");
        assert_eq!(r.handle(addr(1), &wrap(&a, b"again"), t), vec![addr(2)]);
        // Not relay packets: dropped.
        assert!(r.handle(addr(1), b"junk", t).is_empty());
        assert!(r.handle(addr(1), &vec![0u8; 5000], t).is_empty());
    }

    #[test]
    fn quiet_devices_leave() {
        let a = room(b"k");
        let mut r = Relay::default();
        let t = Instant::now();
        r.handle(addr(1), &wrap(&a, b"x"), t);
        let later = t + MEMBER_TTL + Duration::from_secs(1);
        assert!(r.handle(addr(2), &wrap(&a, b"y"), later).is_empty());
        r.prune(later + MEMBER_TTL + Duration::from_secs(1));
        assert_eq!(r.stats(), (0, 0));
    }

    #[test]
    fn rate_limited() {
        let a = room(b"k");
        let mut r = Relay::default();
        let t = Instant::now();
        r.handle(addr(2), &wrap(&a, b"x"), t);
        let sent = (0..1000).filter(|_| !r.handle(addr(1), &wrap(&a, b"x"), t).is_empty()).count();
        assert!(sent <= RATE as usize + 1, "{sent}");
    }

    #[test]
    fn wrap_unwrap() {
        let a = room(b"k");
        let w = wrap(&a, b"payload");
        assert_eq!(unwrap(&w), Some((a, &b"payload"[..])));
        assert!(unwrap(b"TRL1short").is_none());
    }
}
