//! Reading GitHub's "latest release" answers for the apps Thurea looks after on Android
//! (MicroG-RE, the Morphe patches). Java fetches the JSON and does the install and signature
//! checks with Android's APIs; this turns the answer into a version and a download address.

use serde_json::{json, Value};
use std::cmp::Ordering;

/// Is version `a` newer than `b`? Number by number, as `update::compare`.
pub fn newer(a: &str, b: &str) -> bool {
    crate::update::compare(a, b) == Ordering::Greater
}

/// `{"tag"}` from a GitHub release's JSON (`tag_name`, without a leading "v"), plus `"url"`: the
/// first `.apk` asset whose download address starts with `asset_prefix`, when one is given.
/// `{}` if the answer isn't a release with a plain version number (or has no such asset).
pub fn latest(body: &str, asset_prefix: &str) -> Value {
    let Ok(r) = serde_json::from_str::<Value>(body) else { return json!({}) };
    let Some(tag) = r["tag_name"].as_str() else { return json!({}) };
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return json!({});
    }
    if asset_prefix.is_empty() {
        return json!({"tag": tag});
    }
    let url = r["assets"].as_array().into_iter().flatten().filter_map(|a| a["browser_download_url"].as_str()).find(|u| {
        u.len() > asset_prefix.len() + 4
            && u.starts_with(asset_prefix)
            && u.ends_with(".apk")
            && !u[asset_prefix.len()..].contains(['"', '\\', '?', '#'])
            && !u[asset_prefix.len()..].contains("..")
    });
    match url {
        Some(u) => json!({"tag": tag, "url": u}),
        None => json!({}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MICROG: &str = "https://github.com/MorpheApp/MicroG-RE/releases/download/";

    #[test]
    fn versions() {
        assert!(newer("6.1.0", "6.0.9"));
        assert!(newer("0.3.10", "0.3.9"));
        assert!(!newer("6.0", "6.0.0"));
        assert!(!newer("6.0.0", "6.0.1"));
    }

    #[test]
    fn microg_release() {
        let body = json!({
            "tag_name": "v6.1.2",
            "assets": [
                {"browser_download_url": "https://github.com/MorpheApp/MicroG-RE/releases/download/v6.1.2/notes.txt"},
                {"browser_download_url": "https://example.com/MicroG-RE/evil.apk"},
                {"browser_download_url": "https://github.com/MorpheApp/MicroG-RE/releases/download/v6.1.2/microg-re.apk"},
            ]
        })
        .to_string();
        let r = latest(&body, MICROG);
        assert_eq!(r["tag"], "6.1.2");
        assert_eq!(r["url"], "https://github.com/MorpheApp/MicroG-RE/releases/download/v6.1.2/microg-re.apk");
        // Only the tag (Morphe patches), with or without a "v".
        assert_eq!(latest(r#"{"tag_name":"1.4.0"}"#, ""), json!({"tag": "1.4.0"}));
    }

    #[test]
    fn refused() {
        assert_eq!(latest("not json", ""), json!({}));
        assert_eq!(latest(r#"{"message":"API rate limit exceeded"}"#, ""), json!({}));
        assert_eq!(latest(r#"{"tag_name":"v1.4.0-beta"}"#, ""), json!({}));
        assert_eq!(latest(r#"{"tag_name":"v"}"#, ""), json!({}));
        // A release with no MicroG-RE APK on GitHub's download address.
        let none = json!({"tag_name": "v6.1.2", "assets": [
            {"browser_download_url": "https://example.com/x.apk"},
            {"browser_download_url": "https://github.com/MorpheApp/MicroG-RE/releases/download/.apk"},
            {"browser_download_url": "https://github.com/MorpheApp/MicroG-RE/releases/download/../../x/y.apk"},
            {"browser_download_url": "https://github.com/MorpheApp/MicroG-RE/releases/download/v1/a.apk?x=.apk"},
        ]})
        .to_string();
        assert_eq!(latest(&none, MICROG), json!({}));
        assert_eq!(latest(r#"{"tag_name":"v6.1.2"}"#, MICROG), json!({}));
    }
}
