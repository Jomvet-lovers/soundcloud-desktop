use chrono::NaiveDate;
use serde_json::Value;

const KEYS: &[&str] = &["release_date", "display_date", "created_at"];

pub fn extract(track: &Value) -> (Option<i16>, Option<NaiveDate>) {
    for key in KEYS {
        let Some(s) = track.get(*key).and_then(|v| v.as_str()) else {
            continue;
        };
        if s.len() < 10 {
            continue;
        }
        if let Ok(date) = NaiveDate::parse_from_str(&s[..10], "%Y-%m-%d") {
            let year = date.format("%Y").to_string().parse::<i16>().ok();
            return (year, Some(date));
        }
    }
    let release_year = track
        .get("release_year")
        .and_then(|v| v.as_i64())
        .filter(|y| (1900..=2100).contains(y))
        .map(|y| y as i16);
    (release_year, None)
}
