//! Children's accounts on shared computers (Windows, Linux).
//!
//! A household computer stays an adult's device; a parent marks some of its user accounts as a
//! child's ([`crate::family::Device::accounts`]). For those accounts the computer's Thurea
//! service closes blocked programs and locks the session at bedtime, while paused, when the
//! day's time is used up, and on "Lock now"; the browser extension (asking `thurea family-host`,
//! which runs as the user) blocks the child's blocked sites and forces YouTube Restricted Mode.
//!
//! Everything here decides; the agent carries it out (and can show it without doing it: dry run).

use crate::family::{AppVerdict, CATALOG};
use crate::screen::{Clock, ScreenTime, Stop, Summary, Used};
use serde::Serialize;

/// Desktop programs of the catalog's apps (executable names, compared without case or ".exe").
pub const PROGRAMS: &[(&str, &[&str])] = &[
    ("Discord", &["discord", "discordcanary", "discordptb", "vesktop"]),
    ("Telegram", &["telegram-desktop", "telegram", "ayugram"]),
    ("Signal", &["signal-desktop", "signal"]),
    ("WhatsApp", &["whatsapp", "whatsapp-desktop", "whatsapp-for-linux"]),
    ("Messenger", &["messenger"]),
    ("TikTok", &["tiktok"]),
    ("Instagram", &["instagram"]),
    ("Snapchat", &["snapchat"]),
    ("Twitch", &["twitch"]),
];

/// Anti-cheat and game-protection components Thurea never closes, whatever a rule says. Closing
/// the game itself is fine (a parent can limit or block it); killing its anti-cheat out from under
/// it is not: that gets accounts flagged and can leave the anti-cheat's driver in a bad state.
/// Riot Vanguard, Easy Anti-Cheat, BattlEye, FACEIT, ESEA, nProtect GameGuard, XIGNCODE, Ricochet.
pub const NEVER_CLOSE: &[&str] = &[
    "vgc", "vgtray", "vgk", "easyanticheat", "easyanticheat_eos", "easyanticheat_eos_setup", "eac_launcher",
    "beservice", "beservice_x64", "battleye", "faceit", "faceitservice", "faceitclient", "esea", "eseaclient",
    "gamemon", "gamemon64", "gameguard", "xigncode", "xxd-0.xem", "randgrid",
];

/// True for a program Thurea must leave running (see [`NEVER_CLOSE`]).
pub fn never_close(program: &str) -> bool {
    NEVER_CLOSE.contains(&normalize(program).as_str())
}

/// The key the session's own time is counted under (in [`Used`]).
pub const SESSION: &str = "session";

fn normalize(program: &str) -> String {
    let p = program.rsplit(['/', '\\']).next().unwrap_or(program).to_ascii_lowercase();
    p.strip_suffix(".exe").unwrap_or(&p).to_string()
}

/// The catalog entry for a desktop program, if Thurea knows it.
pub fn catalog_program(program: &str) -> Option<&'static crate::family::CatalogEntry> {
    let p = normalize(program);
    let label = PROGRAMS.iter().find(|(_, names)| names.contains(&p.as_str()))?.0;
    CATALOG.iter().find(|e| e.label == label)
}

#[derive(Serialize, Debug, PartialEq)]
pub struct DesktopPlan {
    /// The child this account belongs to (None: an adult's account, nothing to do).
    pub child: Option<String>,
    pub blocked_sites: Vec<String>,
    pub youtube_restrict: u8,
    /// Running programs to close, with why.
    pub close: Vec<(String, AppVerdict)>,
    /// Lock the session (bedtime, paused, the day's time used up).
    pub lock: Option<Stop>,
    pub screen: Option<Summary>,
    pub lock_seq: u64,
}

impl DesktopPlan {
    fn adult() -> Self {
        DesktopPlan { child: None, blocked_sites: Vec::new(), youtube_restrict: 0, close: Vec::new(), lock: None, screen: None, lock_seq: 0 }
    }
}

/// What a child's account on this computer must have now. `used`: today's minutes — the
/// session's own time under [`SESSION`], each program's under its name; `running`: programs the
/// account is running now. Without a clock only sites and programs are decided.
pub fn desktop_plan(policy: &crate::Policy, device: &str, user: &str, running: &[String], used: &Used, clock: Option<&Clock>, bonus: u32) -> DesktopPlan {
    let Some((child, rules)) = policy.family.child_of_account(device, user) else { return DesktopPlan::adult() };
    let mut close = Vec::new();
    for p in running {
        let Some(e) = catalog_program(p) else { continue };
        let approved = rules.allowed_apps.iter().any(|a| normalize(a) == normalize(p) || e.packages.contains(&a.as_str()));
        if !rules.category_allowed(e.category) && !approved {
            close.push((p.clone(), AppVerdict::Blocked { category: e.category.into(), label: e.label.into() }));
        }
    }
    let mut lock = None;
    let mut screen = None;
    if let Some(clock) = clock {
        // The day's total is the session's time; bedtime and pause lock the session.
        let session: Used = [(SESSION.to_string(), used.get(SESSION).copied().unwrap_or(0))].into();
        let (stops, summary) = rules.screen.verdict(&[SESSION.to_string()], &session, clock, bonus, &|_| None);
        lock = stops.into_iter().next().map(|(_, s)| s);
        // Category / program limits on what's running.
        let only_limits = ScreenTime { limits: rules.screen.limits.clone(), always_allowed: rules.screen.always_allowed.clone(), ..Default::default() };
        let programs: Used = used.iter().filter(|(k, _)| k.as_str() != SESSION).map(|(k, v)| (normalize(k), *v)).collect();
        let running_n: Vec<String> = running.iter().map(|p| normalize(p)).collect();
        let cat = |p: &str| catalog_program(p).map(|e| e.category);
        let (limited, _) = only_limits.verdict(&running_n, &programs, clock, 0, &cat);
        for (p, why) in limited {
            if let Some(orig) = running.iter().find(|r| normalize(r) == p) {
                if !close.iter().any(|(c, _)| c == orig) {
                    close.push((orig.clone(), AppVerdict::Screen(why)));
                }
            }
        }
        screen = Some(summary);
    }
    close.retain(|(p, _)| !never_close(p));
    DesktopPlan {
        child: Some(child),
        blocked_sites: rules.blocked_sites(clock.map_or_else(crate::now, |c| c.now)),
        youtube_restrict: rules.youtube_restrict.max(1),
        close,
        lock,
        screen,
        lock_seq: rules.screen.lock_seq,
    }
}

// ── Reading the computer's state (the agent runs the commands; parsing is here, tested) ──

/// A login session as the OS reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: String,
    pub user: String,
    /// In use now: active, not locked, not idle.
    pub active: bool,
}

/// `loginctl list-sessions --no-legend` lines: "SESSION UID USER SEAT TTY …".
pub fn parse_loginctl_sessions(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() >= 3).then(|| (f[0].to_string(), f[2].to_string()))
        })
        .collect()
}

/// `loginctl show-session ID -p Active -p LockedHint -p IdleHint -p Class`: in use by a person now?
pub fn parse_loginctl_show(out: &str) -> bool {
    let get = |k: &str| out.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).unwrap_or("").trim().to_string();
    get("Active") == "yes" && get("LockedHint") != "yes" && get("IdleHint") != "yes" && get("Class") != "greeter"
}

/// Windows `query user` output: USERNAME SESSIONNAME ID STATE IDLE TIME LOGON TIME.
pub fn parse_query_user(out: &str) -> Vec<Session> {
    out.lines()
        .skip(1)
        .filter_map(|l| {
            let l = l.trim_start_matches('>');
            let f: Vec<&str> = l.split_whitespace().collect();
            // With a session name (console / rdp-tcp#0) or without (disconnected).
            let (user, id, state, idle) = if f.len() >= 6 && f[2].chars().all(|c| c.is_ascii_digit()) {
                (f[0], f[2], f[3], f[4])
            } else if f.len() >= 5 && f[1].chars().all(|c| c.is_ascii_digit()) {
                (f[0], f[1], f[2], f[3])
            } else {
                return None;
            };
            let idle_ok = idle == "." || idle == "none" || idle.parse::<u32>().is_ok_and(|m| m < 2);
            Some(Session { id: id.into(), user: user.to_ascii_lowercase(), active: state.eq_ignore_ascii_case("Active") && idle_ok })
        })
        .collect()
}

/// Windows `tasklist /FO CSV /NH` lines: "Image Name","PID",… → image names.
pub fn parse_tasklist(out: &str) -> Vec<String> {
    out.lines().filter_map(|l| l.trim().strip_prefix('"').and_then(|r| r.split('"').next()).map(String::from)).filter(|s| !s.is_empty()).collect()
}

/// Human accounts from /etc/passwd (uid 1000–59999 with a login shell).
pub fn parse_passwd(passwd: &str) -> Vec<String> {
    passwd
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            let uid: u32 = f.get(2)?.parse().ok()?;
            let shell = f.get(6).copied().unwrap_or("");
            ((1000..60000).contains(&uid) && !shell.ends_with("nologin") && !shell.ends_with("false")).then(|| f[0].to_string())
        })
        .collect()
}

/// Is this account role a child's anywhere on this device? (for status lines)
pub fn child_accounts(policy: &crate::Policy, device: &str) -> Vec<(String, String)> {
    policy.family.devices.get(device).map(|d| d.accounts.iter().map(|(u, c)| (u.clone(), c.clone())).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::family::Role;
    use crate::Policy;

    fn fam() -> Policy {
        let mut p = Policy::default();
        p.family.set_account("pc", "Family PC", "sam", Some("Sam"));
        p
    }
    fn clock(minute: u16) -> Clock {
        Clock { now: 1_000_000, day: "2026-11-25".into(), minute }
    }

    #[test]
    fn anti_cheat_is_never_closed() {
        let mut p = fam();
        let rules = p.family.children.entry("Sam".into()).or_default();
        rules.screen.limits.insert("vgc".into(), 0);
        rules.screen.limits.insert("valorant".into(), 0);
        rules.screen.limits.insert("easyanticheat_eos".into(), 0);
        let mut used = Used::new();
        used.insert("vgc".into(), 30);
        used.insert("valorant".into(), 30);
        let running: Vec<String> = ["C:\\Program Files\\Riot Vanguard\\vgc.exe", "VALORANT.exe", "EasyAntiCheat_EOS.exe"].iter().map(|s| s.to_string()).collect();
        let plan = desktop_plan(&p, "pc", "sam", &running, &used, Some(&clock(600)), 0);
        let closed: Vec<&str> = plan.close.iter().map(|(p, _)| p.as_str()).collect();
        assert!(!closed.iter().any(|p| never_close(p)), "closed an anti-cheat component: {closed:?}");
        assert!(never_close("BEService_x64.exe") && !never_close("Discord.exe"));
    }

    #[test]
    fn adults_accounts_untouched() {
        let p = fam();
        let plan = desktop_plan(&p, "pc", "mum", &["Discord.exe".into()], &Used::new(), Some(&clock(600)), 0);
        assert!(plan.child.is_none() && plan.close.is_empty() && plan.lock.is_none());
        assert_eq!(p.family.role("pc"), Role::Adult, "the computer stays an adult's device");
    }

    #[test]
    fn child_account_programs_sites_and_session() {
        let mut p = fam();
        let plan = desktop_plan(&p, "pc", "sam", &["Discord.exe".into(), "/usr/bin/telegram-desktop".into(), "firefox".into()], &Used::new(), None, 0);
        assert_eq!(plan.child.as_deref(), Some("Sam"));
        let closed: Vec<&str> = plan.close.iter().map(|(a, _)| a.as_str()).collect();
        assert_eq!(closed, ["Discord.exe"], "social closed, messaging allowed");
        assert!(plan.blocked_sites.contains(&"discord.com".to_string()));
        assert_eq!(plan.youtube_restrict, 2);
        // Screen time: 90 minutes a day, 95 used: lock.
        p.family.children.get_mut("Sam").unwrap().screen.daily_minutes = Some(90);
        let used: Used = [(SESSION.to_string(), 95)].into();
        let plan = desktop_plan(&p, "pc", "SAM", &[], &used, Some(&clock(600)), 0);
        assert_eq!(plan.lock, Some(Stop::DailyLimit));
        assert_eq!(plan.screen.unwrap().used, 95);
        // Bedtime.
        let sam = p.family.children.get_mut("Sam").unwrap();
        sam.screen.daily_minutes = None;
        sam.screen.bedtime = crate::screen::parse_window("21:00-07:00");
        assert_eq!(desktop_plan(&p, "pc", "sam", &[], &Used::new(), Some(&clock(22 * 60)), 0).lock, Some(Stop::Bedtime));
        assert_eq!(desktop_plan(&p, "pc", "sam", &[], &Used::new(), Some(&clock(12 * 60)), 0).lock, None);
        // A limit on messaging closes Telegram once used up.
        let sam = p.family.children.get_mut("Sam").unwrap();
        sam.screen.limits.insert("messaging".into(), 20);
        let used: Used = [("telegram-desktop".to_string(), 25)].into();
        let plan = desktop_plan(&p, "pc", "sam", &["telegram-desktop".into()], &used, Some(&clock(12 * 60)), 0);
        assert!(matches!(plan.close[0].1, AppVerdict::Screen(Stop::Limit { .. })));
    }

    #[test]
    fn parsers() {
        let s = parse_loginctl_sessions("     2 1000 luke seat0 tty2\n    c3 1001 sam  seat0 tty3\n");
        assert_eq!(s, vec![("2".into(), "luke".into()), ("c3".into(), "sam".into())]);
        assert!(parse_loginctl_show("Active=yes\nLockedHint=no\nIdleHint=no\nClass=user\n"));
        assert!(!parse_loginctl_show("Active=yes\nLockedHint=yes\nIdleHint=no\n"));
        assert!(!parse_loginctl_show("Active=no\n"));
        let q = " USERNAME              SESSIONNAME        ID  STATE   IDLE TIME  LOGON TIME\n>sam                   console             1  Active      none   11/25/2026 7:00 PM\n mum                                       2  Disc            5  11/25/2026 6:00 PM\n";
        let s = parse_query_user(q);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], Session { id: "1".into(), user: "sam".into(), active: true });
        assert!(!s[1].active);
        assert_eq!(parse_tasklist("\"Discord.exe\",\"1234\",\"Console\",\"1\",\"80,000 K\"\n\"explorer.exe\",\"99\",\"Console\",\"1\",\"9 K\"\n"), ["Discord.exe", "explorer.exe"]);
        assert_eq!(parse_passwd("root:x:0:0::/root:/bin/bash\nluke:x:1000:1000::/home/luke:/bin/zsh\nnobody:x:65534:65534::/:/usr/bin/nologin\nsam:x:1001:1001::/home/sam:/bin/bash\n"), ["luke", "sam"]);
        assert_eq!(catalog_program("C:\\\\Users\\\\sam\\\\AppData\\\\Local\\\\Discord\\\\Discord.exe").unwrap().label, "Discord");
    }
}
