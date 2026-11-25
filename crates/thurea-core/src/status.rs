//! Protection checks. The rule the old app broke: a check with no evidence is `Unknown`,
//! and only a report where every check is `Ok` counts as protected.

use serde::{Deserialize, Serialize};

/// Ordered from best to worst, so a report's overall state is its worst check.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ok,
    Unknown,
    Degraded,
}

impl State {
    pub fn mark(self) -> &'static str {
        match self {
            State::Ok => "✓",
            State::Unknown => "?",
            State::Degraded => "!",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Check {
    pub id: String,
    pub state: State,
    pub detail: String,
}

impl Check {
    pub fn new(id: &str, state: State, detail: impl Into<String>) -> Self {
        Check { id: id.into(), state, detail: detail.into() }
    }
    pub fn ok(id: &str, detail: impl Into<String>) -> Self {
        Self::new(id, State::Ok, detail)
    }
    pub fn unknown(id: &str, detail: impl Into<String>) -> Self {
        Self::new(id, State::Unknown, detail)
    }
    pub fn degraded(id: &str, detail: impl Into<String>) -> Self {
        Self::new(id, State::Degraded, detail)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    pub fn push(&mut self, c: Check) {
        self.checks.push(c);
    }

    /// Worst state of all checks; an empty report proves nothing, so it is `Unknown`.
    pub fn overall(&self) -> State {
        self.checks.iter().map(|c| c.state).max().unwrap_or(State::Unknown)
    }

    pub fn problems(&self) -> impl Iterator<Item = &Check> {
        self.checks.iter().filter(|c| c.state != State::Ok)
    }

    pub fn render(&self) -> String {
        self.checks.iter().map(|c| format!("{} {}\n", c.state.mark(), c.detail)).collect()
    }
}

/// Strips an IPv6 zone ("fe80::1%wlan0") and brackets.
fn bare_ip(s: &str) -> &str {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    s.split('%').next().unwrap_or(s)
}

/// Verdict on a resolver list (a VPN's DNS, an adapter's DNS): every server must be a
/// family resolver, and an empty or unreadable list is `Unknown`, not fine.
pub fn dns_verdict(servers: &[String], family: &[String]) -> (State, Option<String>) {
    if servers.is_empty() {
        return (State::Unknown, None);
    }
    for s in servers {
        let ip = bare_ip(s);
        if !family.iter().any(|f| bare_ip(f).eq_ignore_ascii_case(ip)) {
            return (State::Degraded, Some(ip.to_string()));
        }
    }
    (State::Ok, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fam() -> Vec<String> {
        crate::Policy::default().family_dns.servers
    }

    #[test]
    fn empty_dns_is_unknown_not_ok() {
        assert_eq!(dns_verdict(&[], &fam()).0, State::Unknown);
    }

    #[test]
    fn dns_verdicts() {
        assert_eq!(dns_verdict(&["1.1.1.3".into(), "1.0.0.3".into()], &fam()), (State::Ok, None));
        assert_eq!(dns_verdict(&["[2606:4700:4700::1113]".into()], &fam()).0, State::Ok);
        assert_eq!(
            dns_verdict(&["1.1.1.3".into(), "8.8.8.8".into()], &fam()),
            (State::Degraded, Some("8.8.8.8".into()))
        );
        assert_eq!(dns_verdict(&["fe80::1%wlan0".into()], &fam()).1.as_deref(), Some("fe80::1"));
    }

    #[test]
    fn overall_is_worst_and_empty_is_unknown() {
        let mut r = Report::default();
        assert_eq!(r.overall(), State::Unknown);
        r.push(Check::ok("a", "fine"));
        assert_eq!(r.overall(), State::Ok);
        r.push(Check::unknown("b", "no data"));
        assert_eq!(r.overall(), State::Unknown);
        r.push(Check::degraded("c", "off"));
        assert_eq!(r.overall(), State::Degraded);
        assert_eq!(r.problems().count(), 2);
    }
}
