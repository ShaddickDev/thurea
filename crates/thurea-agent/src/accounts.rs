//! Children's accounts on this computer, once a minute (from the service): count each child
//! account's active session time, close blocked programs, lock the session at bedtime, while
//! paused, when the day's time is used up and on "Lock now", and report the day to the parents.
//!
//! The decisions come from `thurea_core::desktop`; this only reads the computer's state and
//! acts on it. `thurea family check` shows what it would do without doing anything.

use crate::{config, platform};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thurea_core::desktop::{self, SESSION};
use thurea_core::screen::{Clock, UsageReport, Used};
use thurea_core::Policy;

#[derive(Serialize, Deserialize, Default)]
struct State {
    day: String,
    /// user → minutes today (session and programs).
    used: BTreeMap<String, Used>,
    /// child → last "Lock now" count acted on.
    lock_seen: BTreeMap<String, u64>,
}

fn state_path() -> std::path::PathBuf {
    config::state_dir().join("child-accounts.json")
}

/// One minute's work. `act`: false = only say what would happen (and change nothing).
/// Returns (usage reports for the parents, what was done / would be done, problems).
pub fn tick(device: &str, policy: &Policy, act: bool) -> (Vec<UsageReport>, Vec<String>, Vec<String>) {
    let (mut reports, mut done, mut problems) = (Vec::new(), Vec::new(), Vec::new());
    let mapped = desktop::child_accounts(policy, device);
    if mapped.is_empty() {
        return (reports, done, problems);
    }
    let mut st: State = std::fs::read_to_string(state_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let clock = platform::local_clock().map(|(day, minute)| Clock { now: thurea_core::now(), day, minute });
    if let Some(c) = &clock {
        if st.day != c.day {
            st.day = c.day.clone();
            st.used.clear();
        }
    }
    let sessions = platform::sessions();
    for (user, child) in mapped {
        if platform::is_admin_account(&user) {
            problems.push(format!("{user} ({child}'s account) is an administrator: Thurea can't hold it — make it a standard account"));
        }
        let mine: Vec<&desktop::Session> = sessions.iter().filter(|s| s.user.eq_ignore_ascii_case(&user)).collect();
        let active = mine.iter().any(|s| s.active);
        let running = platform::running(&user);
        let programs: Vec<String> = running.iter().map(|(_, p)| p.clone()).collect();
        let used = st.used.entry(user.clone()).or_default();
        if active && act {
            *used.entry(SESSION.into()).or_default() += 1;
            let mut seen = std::collections::BTreeSet::new();
            for p in &programs {
                if desktop::catalog_program(p).is_some() && seen.insert(p.clone()) {
                    *used.entry(p.rsplit(['/', '\\']).next().unwrap_or(p).to_ascii_lowercase()).or_default() += 1;
                }
            }
        }
        let plan = desktop::desktop_plan(policy, device, &user, &programs, used, clock.as_ref(), 0);
        for (p, why) in &plan.close {
            done.push(format!("close {p} for {user} ({})", serde_json::to_string(why).unwrap_or_default()));
            if act {
                platform::close(&user, p);
            }
        }
        // Lock: bedtime / paused / time's up (while the session is in use), or "Lock now".
        let seen = st.lock_seen.get(&child).copied();
        let lock_now = seen.is_some_and(|s| plan.lock_seq > s);
        if (plan.lock.is_some() && active) || lock_now {
            for s in mine.iter().filter(|s| s.active || lock_now) {
                done.push(format!("lock {user}'s session {} ({})", s.id, plan.lock.as_ref().map_or("lock now".into(), |w| serde_json::to_string(w).unwrap_or_default())));
                if act {
                    platform::lock_session(&s.id);
                }
            }
        }
        if act {
            st.lock_seen.insert(child.clone(), plan.lock_seq);
        }
        if let (Some(c), Some(sum)) = (&clock, &plan.screen) {
            let mut top: Vec<(String, String, u32)> = used.iter().filter(|(k, _)| k.as_str() != SESSION).map(|(k, m)| {
                let label = desktop::catalog_program(k).map_or(k.clone(), |e| e.label.to_string());
                (k.clone(), label, *m)
            }).collect();
            top.sort_by(|a, b| b.2.cmp(&a.2));
            top.truncate(6);
            reports.push(UsageReport { device: device.into(), child: child.clone(), day: c.day.clone(), used: sum.used, allowed: sum.allowed, top, at: c.now });
        }
    }
    if act {
        let _ = std::fs::create_dir_all(config::state_dir());
        let _ = std::fs::write(state_path(), serde_json::to_string(&st).unwrap());
    }
    (reports, done, problems)
}
