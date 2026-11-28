//! Chrome's managed settings and the family DNS this phone uses, from the family policy and
//! this phone's plan (`android_plan`). Java checks what the network can reach and hands the
//! settings to Android (`DevicePolicyManager.setApplicationRestrictions`).

use serde_json::{json, Value};
use thurea_core::policy::doh_template;
use thurea_core::Policy;

fn is_child(plan: &Value) -> bool {
    plan["role"] == "child"
}

/// The family DNS host: a child's from its plan (CleanBrowsing's Family filter, which also
/// forces YouTube Restricted Mode in every app), otherwise the household policy's.
pub fn dns_host(policy: &Policy, plan: &Value) -> String {
    match plan["dns_host"].as_str() {
        Some(h) if is_child(plan) && !h.is_empty() => h.to_string(),
        _ => policy.family_dns.host.clone(),
    }
}

/// `{"host", "doh_host"}`: where Private DNS (port 853) and Chrome's DNS over HTTPS (443) go.
pub fn dns(policy: &Policy, plan: &Value) -> Value {
    let host = dns_host(policy, plan);
    let doh = doh_template(&host);
    let doh_host = doh.trim_start_matches("https://").split('/').next().unwrap_or_default().to_string();
    json!({"host": host, "doh_host": doh_host})
}

/// Chrome's managed settings. `doh_reachable`: the family DoH server answers on this network,
/// so Chrome can use "secure" (it never falls back); otherwise "automatic", or Chrome couldn't
/// open any page at all.
pub fn restrictions(policy: &Policy, plan: &Value, doh_reachable: bool) -> Value {
    let child = is_child(plan);
    let mut blocklist = policy.blocklist.clone();
    blocklist.extend(plan["blocked_sites"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from));
    json!({
        "DnsOverHttpsMode": if doh_reachable { "secure" } else { "automatic" },
        "DnsOverHttpsTemplates": doh_template(&dns_host(policy, plan)),
        "SafeSitesFilterBehavior": 1,
        "ForceGoogleSafeSearch": true,
        "ForceYouTubeRestrict": if child { plan["youtube_restrict"].as_u64().unwrap_or(2) } else { 1 },
        "IncognitoModeAvailability": 1,
        "URLBlocklist": blocklist,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adult() {
        let p = Policy::default();
        let plan = json!({"role": "adult", "dns_host": "ignored.example"});
        let d = dns(&p, &plan);
        assert_eq!(d["host"], p.family_dns.host.as_str());
        assert_eq!(d["doh_host"], p.family_dns.host.as_str());
        let r = restrictions(&p, &plan, false);
        assert_eq!(r["DnsOverHttpsMode"], "automatic");
        assert_eq!(r["DnsOverHttpsTemplates"], p.doh_template());
        assert_eq!(r["ForceYouTubeRestrict"], 1);
        assert_eq!(r["ForceGoogleSafeSearch"], true);
        assert_eq!(r["IncognitoModeAvailability"], 1);
        assert_eq!(r["URLBlocklist"].as_array().unwrap().len(), p.blocklist.len());
        assert_eq!(restrictions(&p, &json!({}), true)["DnsOverHttpsMode"], "secure");
    }

    #[test]
    fn child() {
        let p = Policy::default();
        let plan = json!({
            "role": "child",
            "dns_host": thurea_core::family::CHILD_DNS_HOST,
            "youtube_restrict": 2,
            "blocked_sites": ["discord.com", "reddit.com"],
        });
        let d = dns(&p, &plan);
        assert_eq!(d["host"], thurea_core::family::CHILD_DNS_HOST);
        assert_eq!(d["doh_host"], "doh.cleanbrowsing.org");
        let r = restrictions(&p, &plan, true);
        assert_eq!(r["DnsOverHttpsTemplates"], "https://doh.cleanbrowsing.org/doh/family-filter/");
        assert_eq!(r["ForceYouTubeRestrict"], 2);
        let list: Vec<&str> = r["URLBlocklist"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(list.len(), p.blocklist.len() + 2);
        assert!(list.ends_with(&["discord.com", "reddit.com"]));
        // A child's plan with no DNS of its own falls back to the household's.
        assert_eq!(dns_host(&p, &json!({"role": "child", "dns_host": ""})), p.family_dns.host);
        assert_eq!(restrictions(&p, &json!({"role": "child"}), true)["ForceYouTubeRestrict"], 2);
    }
}
