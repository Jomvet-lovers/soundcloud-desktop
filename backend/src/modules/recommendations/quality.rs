use serde_json::Value;

pub const MIN_DURATION_MS: i64 = 30_000;
pub const MAX_DURATION_MS: i64 = 30 * 60_000;
pub const MIN_PLAYS_DEFAULT: i64 = 50;

pub fn passes(raw_sc_data: Option<&Value>, plays: i64, min_plays: i64) -> bool {
    if plays < min_plays {
        return false;
    }
    let Some(raw) = raw_sc_data else { return true };
    let duration = raw.get("duration").and_then(|v| v.as_i64()).unwrap_or(0);
    if duration < MIN_DURATION_MS || duration > MAX_DURATION_MS {
        return false;
    }
    let title = raw
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();
    if title.contains("preview") || title.contains("teaser") {
        return false;
    }
    let kind = raw.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    if !kind.is_empty() && kind != "track" {
        return false;
    }
    true
}

