use std::sync::Arc;

use chrono::Datelike;

use crate::error::AppResult;
use crate::modules::enrich::normalize::normalize_name;
use crate::modules::enrich::resolver::{
    AlbumCandidate, ArtistCandidate, ResolveResult, ResolveSource, TrackContext,
};
use crate::modules::lyrics::genius::{GeniusArtistRef, GeniusService, GeniusSongMeta};

pub async fn search(
    genius: &Arc<GeniusService>,
    ctx: &TrackContext,
    primary_hint: Option<&str>,
    cleaned_title: &str,
) -> AppResult<Option<ResolveResult>> {
    if cleaned_title.trim().is_empty() {
        return Ok(None);
    }
    let q = match primary_hint {
        Some(a) if !a.trim().is_empty() => format!("{a} {cleaned_title}"),
        _ => cleaned_title.to_string(),
    };

    // Err = Genius недоступен (оба канала) — пробрасываем: caller не должен
    // путать это с «песни нет» и сваливаться в heuristic.
    let candidates = genius.search_song_meta(&q, 5).await?;
    if candidates.is_empty() {
        return Ok(None);
    }

    let target_title = normalize_name(cleaned_title);
    let target_artist = primary_hint
        .or(ctx.uploader_username.as_deref())
        .map(normalize_name)
        .unwrap_or_default();

    let scored = candidates
        .into_iter()
        .filter_map(|c| {
            let pa_name = c
                .primary_artist
                .as_ref()
                .map(|a| normalize_name(&a.name))
                .unwrap_or_default();
            if pa_name.is_empty() {
                return None;
            }
            let title_norm = normalize_name(&c.title);
            let normal = score_match(&title_norm, &target_title) * 0.6
                + score_match(&pa_name, &target_artist) * 0.4;
            let flipped = score_match(&title_norm, &target_artist) * 0.6
                + score_match(&pa_name, &target_title) * 0.4;
            let best = normal.max(flipped);
            Some((best, c))
        })
        .filter(|(s, _)| *s >= 0.55)
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let Some((score, meta)) = scored else {
        return Ok(None);
    };
    let details = match meta.genius_song_id {
        Some(id) => genius.lookup_song(id).await,
        None => None,
    };
    let song_year = details.as_ref().and_then(|d| d.year);
    let song_date = details.as_ref().and_then(|d| d.release_date);
    let album_pair = details.and_then(|d| d.album.map(|a| (a, song_year)));
    Ok(Some(into_result(
        meta,
        score,
        ctx.isrc.clone(),
        album_pair,
        song_date,
        song_year,
    )))
}

fn into_result(
    meta: GeniusSongMeta,
    score: f32,
    isrc: Option<String>,
    album: Option<(crate::modules::lyrics::genius::GeniusAlbumRef, Option<i16>)>,
    song_release_date: Option<chrono::NaiveDate>,
    song_release_year: Option<i16>,
) -> ResolveResult {
    let primary_meta = meta.primary_artist.clone();
    let primary = meta.primary_artist.into_iter().map(map_ref).collect();
    let featured = meta.featured.into_iter().map(map_ref).collect();
    let confidence = (score * 0.85).clamp(0.5, 0.85);
    // Если у song нет даты — берём из альбома (часто там точнее, особенно
    // у синглов, где Genius даёт точный day у release, но у song только year).
    let album_date = album.as_ref().and_then(|(a, _)| a.release_date);
    let album_year = album.as_ref().and_then(|(a, sy)| a.year.or(*sy));
    let release_date = song_release_date.or(album_date);
    let release_year = song_release_year
        .or(album_year)
        .or_else(|| release_date.and_then(|d| i16::try_from(d.year()).ok()));
    let album = album.map(|(a, song_year)| AlbumCandidate {
        title: a.name,
        year: a.year.or(song_year),
        mb_id: None,
        genius_id: Some(a.genius_album_id.to_string()),
        cover_url: a.cover_url,
        release_type: None,
        primary_artist: primary_meta.map(map_ref),
    });
    ResolveResult {
        source: ResolveSource::Genius,
        confidence,
        primary,
        featured,
        album,
        isrc,
        release_date,
        release_year,
        ..Default::default()
    }
}

fn score_match(a: &str, target: &str) -> f32 {
    if target.is_empty() {
        return 0.4;
    }
    if a == target {
        return 1.0;
    }
    if a.contains(target) || target.contains(a) {
        return 0.6;
    }
    0.2
}

fn map_ref(a: GeniusArtistRef) -> ArtistCandidate {
    ArtistCandidate {
        name: a.name,
        mb_id: None,
        genius_id: a.genius_artist_id.map(|i| i.to_string()),
        sc_user_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::enrich::normalize::normalize_name;

    #[test]
    fn score_match_basic() {
        assert_eq!(score_match("eminem", "eminem"), 1.0);
        assert_eq!(score_match("eminem slim shady", "eminem"), 0.6);
        assert_eq!(score_match("eminem", "eminem slim shady"), 0.6);
        assert_eq!(score_match("eminem", "drake"), 0.2);
        assert_eq!(score_match("anything", ""), 0.4);
    }

    #[test]
    fn psychosis_x_ray_scores_full() {
        let target_title = normalize_name("x-ray");
        let target_artist = normalize_name("Psychosis");
        let cand_title = normalize_name("x-ray");
        let cand_artist = normalize_name("Psychosis");

        let normal = score_match(&cand_title, &target_title) * 0.6
            + score_match(&cand_artist, &target_artist) * 0.4;
        let flipped = score_match(&cand_title, &target_artist) * 0.6
            + score_match(&cand_artist, &target_title) * 0.4;

        assert!(normal >= 0.95, "normal score too low: {normal}");
        assert!(normal > flipped, "normal must beat flipped");
    }

    #[test]
    fn flipped_order_detected() {
        // user wrote "x-ray - Psychosis" instead of "Psychosis - x-ray"
        let target_title = normalize_name("Psychosis");
        let target_artist = normalize_name("x-ray");
        let cand_title = normalize_name("x-ray");
        let cand_artist = normalize_name("Psychosis");

        let normal = score_match(&cand_title, &target_title) * 0.6
            + score_match(&cand_artist, &target_artist) * 0.4;
        let flipped = score_match(&cand_title, &target_artist) * 0.6
            + score_match(&cand_artist, &target_title) * 0.4;

        assert!(flipped > normal, "flipped must beat normal in this case");
        assert!(flipped >= 0.95, "flipped score: {flipped}");
    }

    #[test]
    fn fake_claim_rejected() {
        // "lil peed" claim — neither artist nor title match anything real
        let target_title = normalize_name("я долбаеб");
        let target_artist = normalize_name("lil peed");
        // suppose Genius returned some unrelated result
        let cand_title = normalize_name("Some Other Song");
        let cand_artist = normalize_name("Other Artist");

        let best = (score_match(&cand_title, &target_title) * 0.6
            + score_match(&cand_artist, &target_artist) * 0.4)
            .max(
                score_match(&cand_title, &target_artist) * 0.6
                    + score_match(&cand_artist, &target_title) * 0.4,
            );
        assert!(
            best < 0.55,
            "fake claim should not pass threshold; got {best}"
        );
    }
}
