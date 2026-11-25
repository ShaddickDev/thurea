//! Where Thurea keeps things, and the device's mesh identity.
//!
//! The config (with the household key) is readable by root/Administrators only; the state
//! file the daemon writes for `thurea status` is readable by everyone.

use serde::{Deserialize, Serialize};
use thurea_core::{crypto, Policy};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub id: String,
    pub name: String,
    /// Household key, hex. Every device in the household has the same one.
    pub key: String,
    /// Peers outside this network, by address.
    #[serde(default)]
    pub peers: Vec<String>,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Find household devices on this network by broadcast. Off: only `peers` are contacted.
    #[serde(default = "yes")]
    pub lan_broadcast: bool,
}

fn yes() -> bool {
    true
}

fn default_port() -> u16 {
    thurea_core::mesh::PORT
}

#[cfg(unix)]
pub fn config_dir() -> PathBuf {
    std::env::var_os("THUREA_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| "/etc/thurea".into())
}
#[cfg(unix)]
pub fn state_dir() -> PathBuf {
    std::env::var_os("THUREA_STATE_DIR").map(PathBuf::from).unwrap_or_else(|| "/var/lib/thurea".into())
}

#[cfg(windows)]
fn program_data() -> PathBuf {
    std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| r"C:\ProgramData".into())
}
#[cfg(windows)]
pub fn config_dir() -> PathBuf {
    std::env::var_os("THUREA_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| program_data().join("Thurea").join("private"))
}
#[cfg(windows)]
pub fn state_dir() -> PathBuf {
    std::env::var_os("THUREA_STATE_DIR").map(PathBuf::from).unwrap_or_else(|| program_data().join("Thurea"))
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}
pub fn policy_path() -> PathBuf {
    config_dir().join("policy.json")
}
pub fn state_path() -> PathBuf {
    state_dir().join("state.json")
}

impl Config {
    pub fn new(name: String, key: &[u8]) -> Self {
        Config { id: crypto::new_device_id(), name, key: crypto::hex(key), peers: Vec::new(), port: default_port(), lan_broadcast: true }
    }

    pub fn load() -> Result<Self, String> {
        let p = config_path();
        let s = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e} (run `thurea init` or `thurea join` first)", p.display()))?;
        serde_json::from_str(&s).map_err(|e| format!("{}: {e}", p.display()))
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        crate::platform::lock_down_private_dir(&dir)?;
        let tmp = config_path().with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap()).map_err(|e| e.to_string())?;
        crate::platform::lock_down_private_file(&tmp)?;
        std::fs::rename(tmp, config_path()).map_err(|e| e.to_string())
    }

    pub fn key_bytes(&self) -> Result<Vec<u8>, String> {
        crypto::parse_key(&self.key).ok_or_else(|| "config: household key is damaged".into())
    }

    /// The pairing code, grouped for reading aloud: "a1b2c3d4-...".
    pub fn pairing_code(&self) -> String {
        self.key.as_bytes().chunks(8).map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join("-")
    }
}

/// The policy on disk, or the built-in default if there is none yet.
pub fn load_policy() -> Policy {
    match Policy::load(&policy_path()) {
        Ok(p) => p,
        Err(e) => {
            if policy_path().exists() {
                eprintln!("thurea: {e}; using the built-in policy");
            }
            Policy::default()
        }
    }
}

pub fn hostname() -> String {
    #[cfg(windows)]
    if let Some(n) = std::env::var_os("COMPUTERNAME") {
        return n.to_string_lossy().into_owned();
    }
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "computer".into())
}
