//! Blocklist matching. A rule "example.com" blocks example.com and every subdomain of it.

use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
pub struct DomainSet {
    rules: HashSet<String>,
}

impl DomainSet {
    pub fn new<I, S>(rules: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        DomainSet { rules: rules.into_iter().filter_map(|r| normalize_host(r.as_ref())).collect() }
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The rule that blocks this host or URL, if any.
    pub fn matching_rule(&self, input: &str) -> Option<&str> {
        let host = normalize_host(input)?;
        let mut rest = host.as_str();
        loop {
            if let Some(r) = self.rules.get(rest) {
                return Some(r.as_str());
            }
            match rest.find('.') {
                Some(i) => rest = &rest[i + 1..],
                None => return None,
            }
        }
    }

    pub fn is_blocked(&self, input: &str) -> bool {
        self.matching_rule(input).is_some()
    }
}

/// Reduces a host or URL to a bare lowercase host name: drops the scheme, user info, port,
/// path, a leading "*." and the trailing dot. Returns `None` for anything without a host.
pub fn normalize_host(input: &str) -> Option<String> {
    let mut s = input.trim();
    if let Some(i) = s.find("://") {
        s = &s[i + 3..];
    }
    if let Some(i) = s.find(['/', '?', '#']) {
        s = &s[..i];
    }
    if let Some(i) = s.rfind('@') {
        s = &s[i + 1..];
    }
    if s.starts_with('[') {
        return None; // IPv6 literal: never a blocklist name
    }
    if let Some(i) = s.rfind(':') {
        s = &s[..i];
    }
    let s = s.strip_prefix("*.").unwrap_or(s).trim_end_matches('.');
    let ok = !s.is_empty()
        && s.split('.').all(|l| !l.is_empty() && l.len() <= 63)
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_');
    ok.then(|| s.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_domain_and_subdomains_only() {
        let set = DomainSet::new(["pornhub.com", "Rule34.XXX"]);
        assert!(set.is_blocked("pornhub.com"));
        assert!(set.is_blocked("www.pornhub.com"));
        assert!(set.is_blocked("https://de.pornhub.com/view?x=1"));
        assert!(set.is_blocked("RULE34.xxx."));
        assert!(!set.is_blocked("notpornhub.com"));
        assert!(!set.is_blocked("pornhub.com.example.org"));
        assert!(!set.is_blocked("example.com"));
        assert_eq!(set.matching_rule("a.b.pornhub.com"), Some("pornhub.com"));
    }

    #[test]
    fn normalizes_urls() {
        assert_eq!(normalize_host("HTTPS://user:pw@Example.com:8443/a").as_deref(), Some("example.com"));
        assert_eq!(normalize_host("*.example.com").as_deref(), Some("example.com"));
        assert_eq!(normalize_host("  "), None);
        assert_eq!(normalize_host("[::1]:80"), None);
        assert_eq!(normalize_host("a..b"), None);
    }
}
