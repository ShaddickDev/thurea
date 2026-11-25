//! Thurea's shared brain, used unchanged by the Android app (through JNI) and by the
//! Linux/Windows agent.
//!
//! * [`policy`]  — the one family policy every device enforces (DNS, blocklist, app rules),
//!                 signed by a parent once there is a family.
//! * [`family`]  — parents, children's devices, children's rules, the app/site catalog.
//! * [`matcher`] — "is this site blocked?", with subdomains.
//! * [`status`]  — protection checks. Missing evidence is `Unknown`, never `Ok`.
//! * [`mesh`]    — authenticated heartbeats between the household's devices, policy sync,
//!                 and alerts when a device drops its protection or goes quiet.
//! * [`net`]     — the UDP loop that carries the mesh (std only, runs on every platform).
//! * [`qr`]      — QR codes for one-scan phone setup.
//! * [`bend`]    — bulk audits on the Bend/HVM parallel runtime, with an exact Rust re-check.
//! * [`verses`]  — verse of the day (World English Bible), the same on every device.

pub mod bend;
pub mod crypto;
pub mod family;
pub mod matcher;
pub mod mesh;
pub mod net;
pub mod policy;
pub mod qr;
pub mod status;
pub mod verses;

pub use matcher::DomainSet;
pub use mesh::{Event, Mesh};
pub use policy::Policy;
pub use status::{Check, Report, State};

/// Seconds since the Unix epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
