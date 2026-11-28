//! Screen time on a child's phone, the parts that aren't Android's: adding up how long each app
//! was in front today, and the words the child's screens show. Java only notices which app is
//! in front (Thurea's helper) and whether the screen is on.

use serde_json::{json, Value};
use thurea_core::family::AppVerdict;
use thurea_core::screen::{Stop, Summary};

/// A missed tick never counts more than 2 minutes.
pub const MAX_TICK_MS: u64 = 120_000;

/// Adds `ms` of `app` in front to today's use. `stored`: `{"day", "ms": {app: ms}}` as last
/// returned (anything else, or another day, starts today afresh).
pub fn add_use(stored: &str, day: &str, app: &str, ms: u64) -> String {
    let mut u: Value = serde_json::from_str(stored).unwrap_or_default();
    if u["day"] != day || !u["ms"].is_object() {
        u = json!({"day": day, "ms": {}});
    }
    let had = u["ms"][app].as_u64().unwrap_or(0);
    u["ms"][app] = json!(had + ms.min(MAX_TICK_MS));
    u.to_string()
}

/// Today's use in whole minutes, `{app: minutes}` (nothing if `stored` is another day's).
pub fn minutes_today(stored: &str, day: &str) -> Value {
    let u: Value = serde_json::from_str(stored).unwrap_or_default();
    let mut out = serde_json::Map::new();
    if u["day"] == day {
        for (app, ms) in u["ms"].as_object().into_iter().flatten() {
            out.insert(app.clone(), json!(ms.as_u64().unwrap_or(0) / 60_000));
        }
    }
    Value::Object(out)
}

/// "1 h 05 min" / "45 min".
pub fn duration(minutes: u64) -> String {
    if minutes >= 60 {
        format!("{} h {:02} min", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} min")
    }
}

/// A line about today's screen time for the child's screens and the status page ("" if none).
pub fn line(s: &Summary) -> String {
    if s.paused {
        return "Paused by a parent".into();
    }
    if s.bedtime {
        return "Bedtime — only always-allowed apps".into();
    }
    let used = u64::from(s.used);
    match (s.allowed, s.left) {
        (None, _) if used > 0 => format!("Today: {}", duration(used)),
        (None, _) => String::new(),
        (Some(_), Some(0)) | (Some(_), None) => format!("Time's up for today ({})", duration(used)),
        (Some(_), Some(left)) => format!("Today: {} · {} left", duration(used), duration(u64::from(left))),
    }
}

/// Why an app is closed, in the child's words.
pub fn reason(v: &AppVerdict) -> String {
    match v {
        AppVerdict::Held => "New app".into(),
        AppVerdict::Blocked { .. } => "Social media".into(),
        AppVerdict::Screen(Stop::Bedtime) => "Bedtime".into(),
        AppVerdict::Screen(Stop::Paused) => "Paused by a parent".into(),
        AppVerdict::Screen(Stop::DailyLimit) => "Time's up for today".into(),
        AppVerdict::Screen(Stop::Limit { what }) => format!("Time's up for {what}"),
        AppVerdict::Allowed => "Closed".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counting() {
        let u = add_use("", "2026-11-28", "com.whatsapp", 30_000);
        let u = add_use(&u, "2026-11-28", "com.whatsapp", 45_000);
        let u = add_use(&u, "2026-11-28", "com.whatsapp", 10 * 60_000); // a missed tick: 2 minutes at most
        let u = add_use(&u, "2026-11-28", "org.example.game", 61_000);
        let m = minutes_today(&u, "2026-11-28");
        assert_eq!(m["com.whatsapp"], 3); // 30 s + 45 s + 120 s
        assert_eq!(m["org.example.game"], 1);
        assert_eq!(minutes_today(&u, "2026-11-29"), json!({}));
        // A new day starts afresh; so does anything unreadable.
        let next = add_use(&u, "2026-11-29", "com.whatsapp", 60_000);
        assert_eq!(minutes_today(&next, "2026-11-29"), json!({"com.whatsapp": 1}));
        assert_eq!(minutes_today(&add_use("{broken", "2026-11-29", "a", 60_000), "2026-11-29"), json!({"a": 1}));
        assert_eq!(minutes_today("", "2026-11-29"), json!({}));
    }

    #[test]
    fn words() {
        assert_eq!(duration(45), "45 min");
        assert_eq!(duration(65), "1 h 05 min");
        assert_eq!(duration(120), "2 h 00 min");
        let s = |used, allowed, left| Summary { used, allowed, left, ..Default::default() };
        assert_eq!(line(&s(0, None, None)), "");
        assert_eq!(line(&s(50, None, None)), "Today: 50 min");
        assert_eq!(line(&s(30, Some(60), Some(30))), "Today: 30 min · 30 min left");
        assert_eq!(line(&s(61, Some(60), Some(0))), "Time's up for today (1 h 01 min)");
        assert_eq!(line(&Summary { paused: true, ..s(5, Some(60), Some(55)) }), "Paused by a parent");
        assert_eq!(line(&Summary { bedtime: true, ..s(5, None, None) }), "Bedtime — only always-allowed apps");
        assert_eq!(reason(&AppVerdict::Held), "New app");
        assert_eq!(reason(&AppVerdict::Blocked { category: "social".into(), label: "Social".into() }), "Social media");
        assert_eq!(reason(&AppVerdict::Screen(Stop::DailyLimit)), "Time's up for today");
        assert_eq!(reason(&AppVerdict::Screen(Stop::Limit { what: "Games".into() })), "Time's up for Games");
    }
}
