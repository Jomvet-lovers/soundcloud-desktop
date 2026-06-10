//! MusicBrainz-стадия каскада. MB throttle (1.1с) сериализует enrich, а для
//! SC-аплоадов MB почти всегда пуст — ходим туда только для лейбловых треков
//! (ISRC / живая мета). Fuzzy-search MB умеет вернуть постороннего ("SID" на
//! запрос "SIDODGI DUBOSHIT") — результат гейтится пересечением с локальными
//! сигналами.

use tracing::debug;

use crate::modules::enrich::artist_names;
use crate::modules::enrich::genius as genius_stage;
use crate::modules::enrich::mb::{MbArtist, MbRecording};
use crate::modules::enrich::normalize::normalize_name;

use super::signals::LocalSignals;
use super::{
    AlbumCandidate, ArtistCandidate, ResolveResult, ResolveSource, ResolverDeps, TrackContext,
};

pub(super) async fn search(
    ctx: &TrackContext,
    signals: &LocalSignals,
    deps: &ResolverDeps,
    primary_hint: &Option<String>,
    title_q: &str,
) -> Option<ResolveResult> {
    let try_mb = ctx.isrc.is_some() || !signals.meta_names.is_empty();
    let artist = primary_hint.as_deref().filter(|_| try_mb)?;
    if artist.is_empty() || title_q.is_empty() {
        return None;
    }

    let mut found = search_attempts(ctx, signals, deps, artist, title_q).await;

    // Принимаем только результат, чей primary пересекается с локальными
    // сигналами: разметка / мета / uploader.
    if let Some(rec) = found.as_ref() {
        if let Some(mb_primary) = rec.primary_artist.as_ref() {
            let local: Vec<&str> = signals
                .parsed
                .primary_artists
                .iter()
                .map(|s| s.as_str())
                .chain(signals.meta_names.iter().map(|s| s.as_str()))
                .chain(ctx.uploader_username.as_deref())
                .collect();
            if !artist_names::name_in(&mb_primary.name, local.iter().copied()) {
                debug!(mb = %mb_primary.name, "MB result rejected: no overlap with local signals");
                found = None;
            }
        }
    }

    let rec = found?;
    let mut conf = ((rec.score as f32) / 100.0).clamp(0.7, 0.9);
    if let Some(mb_primary) = rec.primary_artist.as_ref() {
        if !mb_primary.name.is_empty() && !title_q.is_empty() {
            match genius_stage::search(&deps.genius, ctx, Some(&mb_primary.name), title_q).await {
                Ok(Some(g_res)) => {
                    if let Some(g_primary) = g_res.primary.first() {
                        if normalize_name(&g_primary.name) != normalize_name(&mb_primary.name) {
                            debug!(
                                mb = %mb_primary.name,
                                genius = %g_primary.name,
                                "Genius disagrees with MB primary; downgrading"
                            );
                            conf *= 0.7;
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => debug!(error = %e, "Genius cross-check failed; keeping MB confidence"),
            }
        }
    }
    Some(from_mb(rec, ResolveSource::Mb, conf, ctx.isrc.clone()))
}

/// Прямой, перевёрнутый и по-метовый заходы в MB search.
async fn search_attempts(
    ctx: &TrackContext,
    signals: &LocalSignals,
    deps: &ResolverDeps,
    artist: &str,
    title_q: &str,
) -> Option<MbRecording> {
    match deps
        .mb
        .search_recording(artist, title_q, ctx.duration_ms)
        .await
    {
        Ok(Some(rec)) => return Some(rec),
        Ok(None) => debug!(artist, title_q, "MB search empty"),
        Err(e) => debug!(error = %e, "MB search failed"),
    }
    if artist != title_q {
        match deps
            .mb
            .search_recording(title_q, artist, ctx.duration_ms)
            .await
        {
            Ok(Some(rec)) => return Some(rec),
            Ok(None) => debug!(artist, title_q, "MB search empty (flipped)"),
            Err(e) => debug!(error = %e, "MB search failed (flipped)"),
        }
    }
    if let Some(meta_a) = signals.meta_names.first().map(|s| s.as_str()) {
        if normalize_name(meta_a) != normalize_name(artist) {
            match deps
                .mb
                .search_recording(meta_a, title_q, ctx.duration_ms)
                .await
            {
                Ok(Some(rec)) => return Some(rec),
                Ok(None) => debug!(meta_a, "MB search empty (metadata_artist)"),
                Err(e) => debug!(error = %e, "MB search failed (metadata_artist)"),
            }
        }
    }
    None
}

pub(super) fn from_mb(
    rec: MbRecording,
    source: ResolveSource,
    confidence: f32,
    isrc: Option<String>,
) -> ResolveResult {
    let map_artist = |a: MbArtist, sc: Option<String>| ArtistCandidate {
        name: a.name,
        mb_id: Some(a.mb_id),
        genius_id: None,
        sc_user_id: sc,
    };
    let primary: Vec<ArtistCandidate> = rec
        .primary_artist
        .into_iter()
        .map(|a| map_artist(a, None))
        .collect();
    let featured: Vec<ArtistCandidate> = rec
        .featured
        .into_iter()
        .map(|a| map_artist(a, None))
        .collect();

    let album = rec.release.map(|rel| AlbumCandidate {
        title: rel.title,
        year: rel.year,
        mb_id: Some(rel.mb_id),
        genius_id: None,
        cover_url: None,
        release_type: rel.release_type,
        primary_artist: rel.primary_artist.map(|a| map_artist(a, None)),
    });

    ResolveResult {
        source,
        confidence,
        primary,
        featured,
        album,
        isrc,
        ..Default::default()
    }
}
