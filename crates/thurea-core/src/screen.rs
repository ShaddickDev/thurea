//! Screen time for children's devices: daily limits (in total, per category, per app),
//! bedtime, pause, lock now, extra time and offline codes.
//!
//! The rules travel in the parent-signed family policy ([`ScreenTime`] in each child's rules);
//! every child's device counts its own use and works out here, from the rules, its use today and
//! the local clock, which apps must close. Pure functions, so every platform decides the same way.

use crate::crypto;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// Minutes after local midnight (21:00 = 1260). `end` may be earlier than `start`
    /// (over midnight).
    pub start: u16,
    pub end: u16,
}

impl Window {
    pub fn contains(&self, minute: u16) -> bool {
        if self.start <= self.end {
            minute >= self.start && minute < self.end
        } else {
            minute >= self.start || minute < self.end
        }
    }
    /// Minutes until the window next starts or ends (whichever comes first).
    pub fn minutes_to_change(&self, minute: u16) -> u16 {
        let d = |to: u16| (to + 1440 - minute) % 1440;
        let a = d(self.start);
        let b = d(self.end);
        [a, b].into_iter().filter(|x| *x > 0).min().unwrap_or(1440)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Bonus {
    /// Local date, "YYYY-MM-DD".
    pub day: String,
    pub minutes: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ScreenTime {
    /// Total minutes a day (apps that are always allowed don't count). None: no limit.
    #[serde(default)]
    pub daily_minutes: Option<u32>,
    /// Minutes a day for a category ("social", "messaging") or an app (package name).
    #[serde(default)]
    pub limits: BTreeMap<String, u32>,
    /// Nothing but always-allowed apps between these times.
    #[serde(default)]
    pub bedtime: Option<Window>,
    /// Apps that are never stopped by limits, bedtime or pause (e.g. a school app).
    #[serde(default)]
    pub always_allowed: Vec<String>,
    /// Extra minutes for one day.
    #[serde(default)]
    pub bonus: Vec<Bonus>,
    /// Paused (as at bedtime) until this moment (seconds since the epoch).
    #[serde(default)]
    pub paused_until: Option<u64>,
    /// Raised by a parent's "Lock now": the device locks its screen once per raise.
    #[serde(default)]
    pub lock_seq: u64,
}

/// The device's local clock, as the screen-time rules need it.
#[derive(Clone, Debug)]
pub struct Clock {
    pub now: u64,
    /// "YYYY-MM-DD", local.
    pub day: String,
    /// Minutes after local midnight.
    pub minute: u16,
}

/// What this child used today, in minutes: per app (package, or program on a computer).
pub type Used = BTreeMap<String, u32>;

/// Why screen time closes an app.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "why", rename_all = "snake_case")]
pub enum Stop {
    Bedtime,
    Paused,
    /// The daily total is used up.
    DailyLimit,
    /// A category's or an app's own limit is used up.
    Limit { what: String },
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Summary {
    /// Minutes used today (counted apps only).
    pub used: u32,
    /// Today's total allowance including extra time, if there is a limit.
    pub allowed: Option<u32>,
    pub left: Option<u32>,
    pub bedtime: bool,
    pub paused: bool,
    /// When to look again, in minutes (a window starts/ends, the limit runs out). None: no need.
    pub recheck_in: Option<u32>,
}

impl ScreenTime {
    /// Nothing set (then it isn't written out: policies signed before screen time existed keep
    /// verifying).
    pub fn is_unset(&self) -> bool {
        *self == ScreenTime::default()
    }

    pub fn is_empty(&self) -> bool {
        self.daily_minutes.is_none() && self.limits.is_empty() && self.bedtime.is_none() && self.paused_until.is_none()
    }

    fn bonus_today(&self, day: &str, local_bonus: u32) -> u32 {
        self.bonus.iter().filter(|b| b.day == day).map(|b| b.minutes).sum::<u32>() + local_bonus
    }

    /// Minutes that count today: everything except always-allowed apps.
    pub fn counted(&self, used: &Used) -> u32 {
        used.iter().filter(|(a, _)| !self.always_allowed.contains(a)).map(|(_, m)| *m).sum()
    }

    /// Works out which of `apps` must close now, and a summary for screens. `category` tells
    /// an app's catalog category; `local_bonus`: minutes from offline codes used on this device.
    pub fn verdict(&self, apps: &[String], used: &Used, clock: &Clock, local_bonus: u32, category: &dyn Fn(&str) -> Option<&'static str>) -> (Vec<(String, Stop)>, Summary) {
        let mut s = Summary { used: self.counted(used), ..Default::default() };
        let mut stops = Vec::new();
        let mut recheck: Vec<u32> = Vec::new();
        s.paused = self.paused_until.is_some_and(|t| t > clock.now);
        if let Some(t) = self.paused_until.filter(|t| *t > clock.now) {
            recheck.push(((t - clock.now) / 60 + 1) as u32);
        }
        if let Some(w) = self.bedtime {
            s.bedtime = w.contains(clock.minute);
            recheck.push(w.minutes_to_change(clock.minute) as u32);
        }
        if let Some(d) = self.daily_minutes {
            let allowed = d + self.bonus_today(&clock.day, local_bonus);
            s.allowed = Some(allowed);
            s.left = Some(allowed.saturating_sub(s.used));
            if s.used < allowed {
                recheck.push(allowed - s.used);
            }
        }
        // Minutes to local midnight: a new day's allowance (only matters with limits).
        if self.daily_minutes.is_some() || !self.limits.is_empty() {
            recheck.push((1440 - clock.minute as u32).max(1));
        }
        for a in apps {
            if self.always_allowed.contains(a) {
                continue;
            }
            let stop = if s.paused {
                Some(Stop::Paused)
            } else if s.bedtime {
                Some(Stop::Bedtime)
            } else if s.left == Some(0) {
                Some(Stop::DailyLimit)
            } else {
                let cat = category(a);
                let mut hit = None;
                for key in [Some(a.as_str()), cat].into_iter().flatten() {
                    if let Some(limit) = self.limits.get(key) {
                        let spent: u32 = if key == a { used.get(a).copied().unwrap_or(0) } else { used.iter().filter(|(p, _)| category(p) == Some(key)).map(|(_, m)| *m).sum() };
                        if spent >= *limit {
                            hit = Some(Stop::Limit { what: key.to_string() });
                            break;
                        }
                        recheck.push(limit - spent);
                    }
                }
                hit
            };
            if let Some(st) = stop {
                stops.push((a.clone(), st));
            }
        }
        s.recheck_in = recheck.into_iter().filter(|m| *m > 0).min();
        (stops, s)
    }
}

/// "HH:MM-HH:MM" → a window.
pub fn parse_window(x: &str) -> Option<Window> {
    let (st, en) = x.split_once('-')?;
    let m = |t: &str| -> Option<u16> {
        let (h, mi) = t.trim().split_once(':')?;
        let v = h.trim().parse::<u16>().ok()? * 60 + mi.trim().parse::<u16>().ok()?;
        (v < 1440).then_some(v)
    };
    Some(Window { start: m(st)?, end: m(en)? })
}

impl ScreenTime {
    /// A parent's screen-time change, the same from every device. `what`: daily (minutes, 0 =
    /// off), limit (key, minutes, 0 = off), bedtime ("HH:MM-HH:MM" or "off"), pause (minutes,
    /// 0 = resume), lock, extra (minutes, `b` = local day "YYYY-MM-DD"), always (app, "on"/"off").
    pub fn change(&mut self, what: &str, a: &str, b: &str, now: u64) -> Result<(), String> {
        let num = |x: &str| x.trim().parse::<u32>().ok();
        match what {
            "daily" => self.daily_minutes = num(a).filter(|m| *m > 0),
            "limit" => match num(b).filter(|m| *m > 0) {
                Some(m) => {
                    self.limits.insert(a.to_string(), m);
                }
                None => {
                    self.limits.remove(a);
                }
            },
            "bedtime" if a == "off" => self.bedtime = None,
            "bedtime" => self.bedtime = Some(parse_window(a).ok_or("bedtime looks like 21:00-07:00")?),
            "pause" => match num(a).filter(|m| *m > 0) {
                Some(m) => {
                    self.paused_until = Some(now + m as u64 * 60);
                    self.lock_seq += 1;
                }
                None => self.paused_until = None,
            },
            "lock" => self.lock_seq += 1,
            "extra" => {
                let m = num(a).filter(|m| *m > 0 && *m <= 600).ok_or("extra time is 1–600 minutes")?;
                self.bonus.retain(|x| x.day.as_str() >= b); // forget old days
                self.bonus.push(Bonus { day: b.to_string(), minutes: m });
            }
            "always" => {
                self.always_allowed.retain(|x| x != a);
                if b == "on" {
                    self.always_allowed.push(a.to_string());
                }
            }
            _ => return Err(format!("unknown screen-time change {what:?}")),
        }
        Ok(())
    }
}

// ── Offline codes: extra time without a network ──

/// Minutes a code can give.
pub const CODE_MINUTES: [u32; 4] = [15, 30, 60, 120];

fn code_secret(household_key: &[u8]) -> [u8; 32] {
    crypto::mac(household_key, b"thurea offline code v1")
}

/// Six digits a parent's device shows: `minutes` extra for `child` on `day`.
pub fn offline_code(household_key: &[u8], child: &str, day: &str, minutes: u32) -> String {
    let m = crypto::mac(&code_secret(household_key), format!("{child}\n{day}\n{minutes}").as_bytes());
    let n = u32::from_be_bytes([m[0], m[1], m[2], m[3]]) % 1_000_000;
    format!("{:03} {:03}", n / 1000, n % 1000)
}

/// On the child's device: the minutes a typed code gives today, or None.
pub fn check_code(household_key: &[u8], child: &str, day: &str, typed: &str) -> Option<u32> {
    let digits: String = typed.chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 6 {
        return None;
    }
    CODE_MINUTES.into_iter().find(|m| offline_code(household_key, child, day, *m).replace(' ', "") == digits)
}

/// A child device's use today, sent to the parents (encrypted, like everything in the mesh).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UsageReport {
    pub device: String,
    pub child: String,
    pub day: String,
    /// Counted minutes today.
    pub used: u32,
    pub allowed: Option<u32>,
    /// Most used apps today: (app, label, minutes), longest first.
    pub top: Vec<(String, String, u32)>,
    pub at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(minute: u16) -> Clock {
        Clock { now: 1_000_000, day: "2026-11-25".into(), minute }
    }
    fn cat(a: &str) -> Option<&'static str> {
        crate::family::catalog_app(a).map(|e| e.category)
    }
    fn apps() -> Vec<String> {
        ["com.instagram.android", "com.whatsapp", "com.example.game", "org.school.app"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_rules_no_stops() {
        let (stops, s) = ScreenTime::default().verdict(&apps(), &Used::new(), &clock(600), 0, &cat);
        assert!(stops.is_empty());
        assert_eq!(s.left, None);
    }

    #[test]
    fn bedtime_over_midnight_spares_always_allowed() {
        let st = ScreenTime { bedtime: Some(Window { start: 21 * 60, end: 7 * 60 }), always_allowed: vec!["org.school.app".into()], ..Default::default() };
        let (stops, s) = st.verdict(&apps(), &Used::new(), &clock(22 * 60), 0, &cat);
        assert!(s.bedtime);
        assert_eq!(stops.len(), 3);
        assert!(stops.iter().all(|(_, w)| *w == Stop::Bedtime));
        assert_eq!(s.recheck_in, Some(9 * 60)); // 07:00
        let (stops, s) = st.verdict(&apps(), &Used::new(), &clock(12 * 60), 0, &cat);
        assert!(!s.bedtime && stops.is_empty());
        assert_eq!(s.recheck_in, Some(9 * 60)); // 21:00
        assert!(st.verdict(&apps(), &Used::new(), &clock(6 * 60 + 59), 0, &cat).1.bedtime);
        assert!(!st.verdict(&apps(), &Used::new(), &clock(7 * 60), 0, &cat).1.bedtime);
    }

    #[test]
    fn daily_limit_with_extra_time_and_codes() {
        let mut st = ScreenTime { daily_minutes: Some(60), always_allowed: vec!["org.school.app".into()], ..Default::default() };
        let mut used = Used::new();
        used.insert("com.example.game".into(), 50);
        used.insert("org.school.app".into(), 300); // doesn't count
        let (stops, s) = st.verdict(&apps(), &used, &clock(600), 0, &cat);
        assert_eq!((s.used, s.left, stops.len()), (50, Some(10), 0));
        assert_eq!(s.recheck_in, Some(10));
        used.insert("com.whatsapp".into(), 10);
        let (stops, s) = st.verdict(&apps(), &used, &clock(600), 0, &cat);
        assert_eq!(s.left, Some(0));
        assert_eq!(stops.len(), 3);
        // A parent adds 15 minutes today; an offline code adds 30 more.
        st.bonus.push(Bonus { day: "2026-11-25".into(), minutes: 15 });
        st.bonus.push(Bonus { day: "2026-11-24".into(), minutes: 500 }); // yesterday's: no effect
        let (_, s) = st.verdict(&apps(), &used, &clock(600), 30, &cat);
        assert_eq!(s.left, Some(45));
    }

    #[test]
    fn category_and_app_limits() {
        let mut st = ScreenTime::default();
        st.limits.insert("social".into(), 30);
        st.limits.insert("com.example.game".into(), 20);
        let mut used = Used::new();
        used.insert("com.instagram.android".into(), 31);
        used.insert("com.example.game".into(), 5);
        let (stops, s) = st.verdict(&apps(), &used, &clock(600), 0, &cat);
        assert_eq!(stops, vec![("com.instagram.android".into(), Stop::Limit { what: "social".into() })]);
        assert_eq!(s.recheck_in, Some(15));
    }

    #[test]
    fn paused_until() {
        let st = ScreenTime { paused_until: Some(1_000_000 + 600), ..Default::default() };
        let (stops, s) = st.verdict(&apps(), &Used::new(), &clock(600), 0, &cat);
        assert!(s.paused);
        assert_eq!(stops.len(), 4);
        assert_eq!(s.recheck_in, Some(11));
        let later = Clock { now: 1_000_000 + 601, ..clock(610) };
        assert!(st.verdict(&apps(), &Used::new(), &later, 0, &cat).0.is_empty());
    }

    #[test]
    fn parent_changes() {
        let mut st = ScreenTime::default();
        st.change("daily", "90", "", 0).unwrap();
        st.change("limit", "social", "30", 0).unwrap();
        st.change("bedtime", "21:30-06:45", "", 0).unwrap();
        assert_eq!(st.bedtime, Some(Window { start: 21 * 60 + 30, end: 6 * 60 + 45 }));
        assert!(st.change("bedtime", "late", "", 0).is_err());
        st.change("pause", "60", "", 1000).unwrap();
        assert_eq!((st.paused_until, st.lock_seq), (Some(1000 + 3600), 1));
        st.change("extra", "30", "2026-11-25", 0).unwrap();
        st.change("extra", "15", "2026-11-26", 0).unwrap();
        assert_eq!(st.bonus.len(), 1, "yesterday's extra time is forgotten");
        st.change("always", "org.school.app", "on", 0).unwrap();
        st.change("daily", "0", "", 0).unwrap();
        st.change("limit", "social", "0", 0).unwrap();
        st.change("pause", "0", "", 0).unwrap();
        st.change("bedtime", "off", "", 0).unwrap();
        assert!(st.daily_minutes.is_none() && st.limits.is_empty() && st.paused_until.is_none() && st.bedtime.is_none());
        assert!(st.change("nonsense", "", "", 0).is_err());
    }

    #[test]
    fn offline_codes() {
        let k = crypto::new_key();
        let c = offline_code(&k, "Sam", "2026-11-25", 30);
        assert_eq!(c.len(), 7);
        assert_eq!(check_code(&k, "Sam", "2026-11-25", &c), Some(30));
        assert_eq!(check_code(&k, "Sam", "2026-11-25", &c.replace(' ', "")), Some(30));
        assert_eq!(check_code(&k, "Sam", "2026-11-26", &c), None, "only that day");
        assert_eq!(check_code(&k, "Max", "2026-11-25", &c), None, "only that child");
        assert_eq!(check_code(&crypto::new_key(), "Sam", "2026-11-25", &c), None);
        assert_eq!(check_code(&k, "Sam", "2026-11-25", "12"), None);
    }
}
