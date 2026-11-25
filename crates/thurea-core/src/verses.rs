//! Verse of the day, the same on every device: `policy/verses.json` (World English Bible,
//! public domain), one verse per day in order.

use serde::Deserialize;

pub const JSON: &str = include_str!("../../../policy/verses.json");

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Verse {
    #[serde(rename = "ref")]
    pub reference: String,
    pub text: String,
}

#[derive(Deserialize)]
struct File {
    abbreviation: String,
    notice: String,
    verses: Vec<Verse>,
}

fn file() -> File {
    serde_json::from_str(JSON).expect("policy/verses.json is valid")
}

/// The translation's short name ("WEB") and its notice, for credits.
pub fn translation() -> (String, String) {
    let f = file();
    (f.abbreviation, f.notice)
}

/// The verse for a given day (days since 1970-01-01), as Android, Windows and Linux pick it.
pub fn for_day(day: i64) -> Verse {
    let v = file().verses;
    v[day.rem_euclid(v.len() as i64) as usize].clone()
}

/// Today's verse, by the local date when the platform says what it is (TZ offset in seconds).
pub fn today(utc_offset_secs: i64) -> Verse {
    for_day((crate::now() as i64 + utc_offset_secs).div_euclid(86_400))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_verse_is_there_and_public_domain() {
        let f = file();
        assert!(f.verses.len() >= 30);
        assert_eq!(f.abbreviation, "WEB");
        assert!(f.notice.contains("public domain"));
        assert!(f.verses.iter().all(|v| !v.reference.is_empty() && v.text.len() > 20));
        assert_eq!(for_day(0), for_day(f.verses.len() as i64));
        assert_eq!(for_day(-1), for_day(f.verses.len() as i64 - 1));
    }
}
