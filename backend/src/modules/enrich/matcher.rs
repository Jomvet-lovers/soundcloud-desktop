//! Единый алгоритм сопоставления (artist, title) между внешним источником
//! и SoundCloud-треком. Используется wanted_resolver, sc_account_scan и
//! артист-кролером для линковки.
//!
//! Идея — три ступени, score 0..1:
//!   - exact (нормализованный): 1.0
//!   - compact (без пробелов и пунктуации): 0.95
//!   - substring с порогом длины: 0.6..0.8
//!   - prefix-/suffix-/n-gram fuzzy: 0.4..0.6
//!
//! Отдельно — артист score (имя SC-аплоадера / имя в БД).
//!
//! Никаких внешних запросов; чистая функция.

use serde_json::Value;

use crate::modules::enrich::normalize::{
    compact_title, normalize_name, normalize_title, parse_sc_title,
};

/// Финальный результат matching'а одного SC-кандидата против wanted-трека.
#[derive(Debug, Clone)]
pub struct TrackMatch {
    pub title_score: f32,
    pub artist_score: f32,
    pub duration_match: DurationMatch,
    pub isrc_match: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationMatch {
    Exact,
    Close,
    Far,
    Unknown,
}

impl TrackMatch {
    /// Композитный score 0..1.
    /// ISRC совпадение → сразу 1.0.
    /// Иначе — title × 0.55 + artist × 0.35 + duration × 0.10.
    pub fn score(&self) -> f32 {
        if self.isrc_match {
            return 1.0;
        }
        let dur = match self.duration_match {
            DurationMatch::Exact => 1.0,
            DurationMatch::Close => 0.7,
            DurationMatch::Unknown => 0.4,
            DurationMatch::Far => 0.0,
        };
        (self.title_score * 0.55 + self.artist_score * 0.35 + dur * 0.10).min(1.0)
    }
}

/// Title score между «целевым» (то что мы ищем — например, wanted_track.title)
/// и «кандидатом» (то что нам отдал SC — может содержать "Artist - " префикс,
/// feat-блок, шум типа [Free DL]).
///
/// `cand_uploader_username` помогает parse_sc_title правильно отделить артиста
/// в случае если в title нет дефиса.
pub fn title_score(
    target_title: &str,
    cand_title: &str,
    cand_uploader_username: Option<&str>,
) -> f32 {
    let target_n = normalize_title(target_title);
    if target_n.is_empty() {
        return 0.0;
    }
    let target_compact = compact_title(target_title);

    let parsed = parse_sc_title(cand_title, cand_uploader_username);
    let cleaned_n = normalize_title(&parsed.cleaned_title);
    let cleaned_compact = compact_title(&parsed.cleaned_title);
    let raw_n = normalize_title(cand_title);
    let raw_compact = compact_title(cand_title);

    if cleaned_n == target_n || raw_n == target_n {
        return 1.0;
    }
    if !target_compact.is_empty()
        && (cleaned_compact == target_compact || raw_compact == target_compact)
    {
        return 0.95;
    }

    let min_len = 6;
    if target_compact.len() >= min_len {
        if cleaned_compact.contains(&target_compact) || raw_compact.contains(&target_compact) {
            // целевая короче кандидата — кандидат «покрывает» wanted
            return 0.75;
        }
        if !cleaned_compact.is_empty()
            && target_compact.contains(&cleaned_compact)
            && cleaned_compact.len() * 2 >= target_compact.len()
        {
            return 0.65;
        }
    }

    // n-gram (триграмм) пересечение, нормализованный по target.
    let trigram_ratio = trigram_overlap(&cleaned_compact, &target_compact)
        .max(trigram_overlap(&raw_compact, &target_compact));
    if trigram_ratio >= 0.7 {
        return 0.55;
    }
    if trigram_ratio >= 0.5 {
        return 0.4;
    }
    0.0
}

/// Artist score: target = ожидаемое имя артиста (как у нас в БД),
/// cand_uploader_username = SC username аплоадера,
/// cand_title_parsed_artist = первый primary_artist парсера тайтла кандидата.
/// Шкала похожести — единая, из `artist_names::name_similarity`.
pub fn artist_score(
    target_artist: &str,
    cand_uploader_username: Option<&str>,
    cand_title_parsed_artist: Option<&str>,
) -> f32 {
    if normalize_name(target_artist).is_empty() {
        // Без эталонного имени артиста сравнивать не с чем — даём нейтральный 0.5
        // чтобы не убивать score для wanted_tracks без artist'а.
        return 0.5;
    }
    let mut best = 0.0f32;
    for c in [cand_title_parsed_artist, cand_uploader_username]
        .into_iter()
        .flatten()
    {
        let s = crate::modules::enrich::artist_names::name_similarity(target_artist, c);
        if s > best {
            best = s;
        }
    }
    best
}

pub fn duration_match(target_ms: Option<i32>, cand_ms: Option<i64>) -> DurationMatch {
    match (target_ms, cand_ms) {
        (Some(t), Some(c)) => {
            let diff = (c - t as i64).abs();
            if diff <= 1500 {
                DurationMatch::Exact
            } else if diff <= 5000 {
                DurationMatch::Close
            } else {
                DurationMatch::Far
            }
        }
        _ => DurationMatch::Unknown,
    }
}

fn ngram_set(s: &str, n: usize) -> std::collections::HashSet<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < n {
        return std::collections::HashSet::new();
    }
    let mut set = std::collections::HashSet::with_capacity(chars.len().saturating_sub(n - 1));
    for w in chars.windows(n) {
        set.insert(w.iter().collect::<String>());
    }
    set
}

fn jaccard_ratio(
    a: &std::collections::HashSet<String>,
    b: &std::collections::HashSet<String>,
) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count() as f32;
    let union = a.union(b).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

fn trigram_overlap(a: &str, b: &str) -> f32 {
    jaccard_ratio(&ngram_set(a, 3), &ngram_set(b, 3))
}

/// Извлечь sc_track_id из urn-строки (`soundcloud:tracks:1234`).
pub fn sc_track_id_from_urn(urn: &str) -> Option<String> {
    urn.rsplit(':')
        .next()
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Удобный helper: посчитать TrackMatch для SC-сырого кандидата против wanted-задачи.
pub fn evaluate_sc_candidate(
    cand: &Value,
    wanted_title: &str,
    wanted_artist: &str,
    wanted_isrc: Option<&str>,
    wanted_duration_ms: Option<i32>,
) -> TrackMatch {
    let cand_title = cand.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let cand_uploader = cand
        .get("user")
        .and_then(|u| u.get("username"))
        .and_then(|v| v.as_str());
    let cand_isrc = cand
        .pointer("/publisher_metadata/isrc")
        .and_then(|v| v.as_str());
    let cand_duration_ms = cand.get("duration").and_then(|v| v.as_i64());

    let parsed = parse_sc_title(cand_title, cand_uploader);
    let cand_parsed_artist = parsed.primary_artists.first().map(|s| s.as_str());

    TrackMatch {
        title_score: title_score(wanted_title, cand_title, cand_uploader),
        artist_score: artist_score(wanted_artist, cand_uploader, cand_parsed_artist),
        duration_match: duration_match(wanted_duration_ms, cand_duration_ms),
        isrc_match: matches!((wanted_isrc, cand_isrc), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_exact() {
        assert!((title_score("Lose Yourself", "Eminem - Lose Yourself", None) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn title_compact_diff_punctuation() {
        // wanted: "1000-7?что ты сказал?" vs SC: "Psychosis, Pxlsdead - 1000 - 7что Ты Сказал"
        let s = title_score(
            "1000-7?что ты сказал?",
            "Psychosis, Pxlsdead - 1000 - 7что Ты Сказал",
            None,
        );
        assert!(s >= 0.95, "expected high compact match, got {s}");
    }

    #[test]
    fn title_substring_short_in_long() {
        let s = title_score("100-7", "psychosis - 100-7 (slowed reverb)", None);
        // wanted "100 7" короче, кандидат содержит → 0.75 минимум
        assert!(s >= 0.75, "expected substring match >= 0.75, got {s}");
    }

    #[test]
    fn title_no_match() {
        let s = title_score("totally different", "another song completely", None);
        assert!(s < 0.4, "expected low score, got {s}");
    }

    #[test]
    fn artist_exact_via_uploader() {
        let s = artist_score("Ultimathule", Some("ultimathule"), None);
        assert!((s - 1.0).abs() < 1e-6);
    }

    #[test]
    fn artist_substring_with_suffix() {
        // в БД "ultimathule (RUS)", uploader "ultimathule"
        let s = artist_score("ultimathule (RUS)", Some("ultimathule"), None);
        assert!(s >= 0.85, "expected substring artist match, got {s}");
    }

    #[test]
    fn artist_no_target_neutral() {
        // wanted без артиста — не убиваем score
        let s = artist_score("", Some("anyone"), None);
        assert!((s - 0.5).abs() < 1e-6);
    }

    #[test]
    fn artist_uploader_unrelated() {
        let s = artist_score("Drake", Some("RandomReuploader42"), None);
        assert!(s < 0.4, "unrelated uploader must score low, got {s}");
    }

    #[test]
    fn duration_buckets() {
        assert_eq!(
            duration_match(Some(180_000), Some(180_500)),
            DurationMatch::Exact
        );
        assert_eq!(
            duration_match(Some(180_000), Some(183_000)),
            DurationMatch::Close
        );
        assert_eq!(
            duration_match(Some(180_000), Some(220_000)),
            DurationMatch::Far
        );
        assert_eq!(duration_match(None, Some(180_000)), DurationMatch::Unknown);
    }

    #[test]
    fn isrc_pins_score_to_one() {
        let m = TrackMatch {
            title_score: 0.0,
            artist_score: 0.0,
            duration_match: DurationMatch::Far,
            isrc_match: true,
        };
        assert!((m.score() - 1.0).abs() < 1e-6);
    }
}
