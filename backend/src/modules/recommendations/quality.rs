pub const MIN_DURATION_MS: i32 = 30_000;
pub const MAX_DURATION_MS: i32 = 30 * 60_000;
pub const MIN_PLAYS_DEFAULT: i64 = 50;

pub struct QualityCheck<'a> {
    pub duration_ms: i32,
    pub title: &'a str,
    pub plays: i64,
}

pub fn passes(check: QualityCheck<'_>, min_plays: i64) -> bool {
    if check.plays < min_plays {
        return false;
    }
    if check.duration_ms < MIN_DURATION_MS || check.duration_ms > MAX_DURATION_MS {
        return false;
    }
    let lower = check.title.to_lowercase();
    if lower.contains("preview") || lower.contains("teaser") {
        return false;
    }
    true
}
