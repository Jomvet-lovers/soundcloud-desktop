use std::sync::Arc;

use tracing::{debug, warn};

use crate::error::AppResult;
use crate::modules::enrich::ai::AiResolverClient;
use crate::modules::enrich::genius as genius_stage;
use crate::modules::enrich::mb::{MbArtist, MbClient, MbRecording};
use crate::modules::enrich::normalize::{normalize_name, parse_sc_title, ParsedTitle};
use crate::modules::lyrics::genius::GeniusService;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResolveSource {
    #[default]
    Heuristic,
    Ai,
    Genius,
    Mb,
    Isrc,
    ScVerified,
}

impl ResolveSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Heuristic => "heuristic",
            Self::Ai => "ai",
            Self::Genius => "genius",
            Self::Mb => "mb",
            Self::Isrc => "isrc",
            Self::ScVerified => "sc_verified",
        }
    }
    pub fn priority(&self) -> u8 {
        match self {
            Self::Heuristic => 1,
            Self::Ai => 2,
            Self::Genius => 3,
            Self::Mb => 4,
            Self::Isrc => 5,
            Self::ScVerified => 6,
        }
    }
    pub fn from_db(s: &str) -> Self {
        match s {
            "sc_verified" => Self::ScVerified,
            "isrc" => Self::Isrc,
            "mb" => Self::Mb,
            "genius" => Self::Genius,
            "ai" => Self::Ai,
            _ => Self::Heuristic,
        }
    }
    pub fn priority_of(s: &str) -> u8 {
        Self::from_db(s).priority()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ArtistCandidate {
    pub name: String,
    pub mb_id: Option<String>,
    pub genius_id: Option<String>,
    pub sc_user_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AlbumCandidate {
    pub title: String,
    pub year: Option<i16>,
    pub mb_id: Option<String>,
    pub genius_id: Option<String>,
    pub cover_url: Option<String>,
    pub release_type: Option<String>,
    pub primary_artist: Option<ArtistCandidate>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolveResult {
    pub source: ResolveSource,
    pub confidence: f32,
    pub primary: Vec<ArtistCandidate>,
    pub featured: Vec<ArtistCandidate>,
    pub producers: Vec<ArtistCandidate>,
    pub remixers: Vec<ArtistCandidate>,
    pub album: Option<AlbumCandidate>,
    pub isrc: Option<String>,
    /// Релиз-дата трека (Genius song / fallback album). Если есть — persist
    /// перезапишет `tracks.release_date` + `release_year`. Когда None —
    /// fallback на `sc_created_at` (заливка на SC).
    pub release_date: Option<chrono::NaiveDate>,
    pub release_year: Option<i16>,
    pub is_cover: bool,
}

pub struct TrackContext {
    pub title: String,
    pub uploader_username: Option<String>,
    pub uploader_sc_user_id: Option<String>,
    pub duration_ms: Option<i32>,
    pub isrc: Option<String>,
    pub metadata_artist: Option<String>,
    pub description: Option<String>,
}

impl TrackContext {
    pub fn from_row(row: &crate::modules::tracks::TrackRow) -> Self {
        Self {
            title: row.title.clone(),
            uploader_username: row.uploader_username.clone(),
            uploader_sc_user_id: row.uploader_sc_user_id.clone(),
            duration_ms: Some(row.duration_ms),
            isrc: row.isrc.clone(),
            metadata_artist: row.metadata_artist.clone(),
            description: row.description.clone(),
        }
    }
}

pub struct ResolverDeps {
    pub mb: Arc<MbClient>,
    pub genius: Arc<GeniusService>,
    pub ai: Option<Arc<AiResolverClient>>,
}

pub async fn resolve(ctx: &TrackContext, deps: &ResolverDeps) -> AppResult<ResolveResult> {
    let parsed = parse_sc_title(&ctx.title, ctx.uploader_username.as_deref());
    let heuristic = heuristic_result(ctx, &parsed);

    // `(cover)` в title → uploader сделал кавер. Полноценно резолвим
    // в MB/Genius чтобы найти ОРИГИНАЛЬНОГО артиста, но дальше persist
    // запишет его в `cover_of_artist_id`, primary_artist_id у трека
    // останется NULL (uploader не равен оригиналу), upload_kind = 'cover'.
    // Страница оригинала получит вкладку "Covers" одним SQL.
    // `is_cover` уже прокинут в heuristic.is_cover (см. heuristic_result).

    if let Some(isrc) = ctx.isrc.as_ref() {
        match deps.mb.lookup_by_isrc(isrc).await {
            Ok(Some(rec)) => {
                return Ok(merge_with(
                    heuristic,
                    from_mb(rec, ResolveSource::Isrc, 0.95, Some(isrc.clone())),
                    ctx,
                ))
            }
            Ok(None) => debug!(isrc, "ISRC lookup empty"),
            Err(e) => debug!(error = %e, isrc, "ISRC lookup failed"),
        }
    }

    let primary_hint = parsed
        .primary_artists
        .first()
        .cloned()
        .or_else(|| ctx.uploader_username.clone());
    let title_q = if parsed.cleaned_title.is_empty() {
        ctx.title.clone()
    } else {
        parsed.cleaned_title.clone()
    };

    // MB throttle (1.1с) сериализует enrich, а для SC-аплоадов MB почти всегда
    // пуст — ходим туда только для лейбловых треков (ISRC / metadata_artist).
    let try_mb = ctx.isrc.is_some()
        || ctx
        .metadata_artist
        .as_deref()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if let Some(artist) = primary_hint.as_deref().filter(|_| try_mb) {
        if !artist.is_empty() && !title_q.is_empty() {
            let mut found: Option<MbRecording> = None;
            match deps
                .mb
                .search_recording(artist, &title_q, ctx.duration_ms)
                .await
            {
                Ok(Some(rec)) => found = Some(rec),
                Ok(None) => debug!(artist, title_q, "MB search empty"),
                Err(e) => debug!(error = %e, "MB search failed"),
            }
            if found.is_none() && artist != title_q {
                match deps
                    .mb
                    .search_recording(&title_q, artist, ctx.duration_ms)
                    .await
                {
                    Ok(Some(rec)) => found = Some(rec),
                    Ok(None) => debug!(artist, title_q, "MB search empty (flipped)"),
                    Err(e) => debug!(error = %e, "MB search failed (flipped)"),
                }
            }
            if found.is_none() {
                if let Some(meta_a) = ctx.metadata_artist.as_deref() {
                    if !meta_a.is_empty() && normalize_name(meta_a) != normalize_name(artist) {
                        match deps
                            .mb
                            .search_recording(meta_a, &title_q, ctx.duration_ms)
                            .await
                        {
                            Ok(Some(rec)) => found = Some(rec),
                            Ok(None) => debug!(meta_a, "MB search empty (metadata_artist)"),
                            Err(e) => debug!(error = %e, "MB search failed (metadata_artist)"),
                        }
                    }
                }
            }
            if let Some(rec) = found {
                let mut conf = ((rec.score as f32) / 100.0).clamp(0.7, 0.9);
                if let Some(mb_primary) = rec.primary_artist.as_ref() {
                    if !mb_primary.name.is_empty() && !title_q.is_empty() {
                        if let Ok(Some(g_res)) = genius_stage::search(
                            &deps.genius,
                            ctx,
                            Some(&mb_primary.name),
                            &title_q,
                        )
                        .await
                        {
                            if let Some(g_primary) = g_res.primary.first() {
                                if normalize_name(&g_primary.name)
                                    != normalize_name(&mb_primary.name)
                                {
                                    debug!(
                                        mb = %mb_primary.name,
                                        genius = %g_primary.name,
                                        "Genius disagrees with MB primary; downgrading"
                                    );
                                    conf *= 0.7;
                                }
                            }
                        }
                    }
                }
                return Ok(merge_with(
                    heuristic,
                    from_mb(rec, ResolveSource::Mb, conf, ctx.isrc.clone()),
                    ctx,
                ));
            }
        }
    }

    match genius_stage::search(&deps.genius, ctx, primary_hint.as_deref(), &title_q).await {
        Ok(Some(res)) => return Ok(merge_with(heuristic, res, ctx)),
        Ok(None) => debug!(title_q, "Genius search empty"),
        Err(e) => warn!(error = %e, "Genius search failed"),
    }

    if let Some(meta_a) = ctx.metadata_artist.as_deref() {
        let differs = primary_hint
            .as_deref()
            .map(|h| normalize_name(meta_a) != normalize_name(h))
            .unwrap_or(true);
        if !meta_a.is_empty() && differs {
            match genius_stage::search(&deps.genius, ctx, Some(meta_a), &title_q).await {
                Ok(Some(res)) => return Ok(merge_with(heuristic, res, ctx)),
                Ok(None) => debug!(meta_a, "Genius search empty (metadata_artist)"),
                Err(e) => warn!(error = %e, "Genius search failed (metadata_artist)"),
            }
        }
    }

    if let Some(ai) = deps.ai.as_ref() {
        match ai.resolve(ctx).await {
            Ok(Some(res)) => return Ok(merge_with(heuristic, res, ctx)),
            Ok(None) => debug!("AI resolve empty"),
            Err(e) => debug!(error = %e, "AI resolve failed"),
        }
    }

    Ok(heuristic)
}

fn heuristic_result(ctx: &TrackContext, parsed: &ParsedTitle) -> ResolveResult {
    let to_candidate = |name: &str, sc_user_id: Option<String>| ArtistCandidate {
        name: name.to_string(),
        mb_id: None,
        genius_id: None,
        sc_user_id,
    };

    let mut primary: Vec<ArtistCandidate> = parsed
        .primary_artists
        .iter()
        .map(|n| {
            let sc = if name_matches_uploader(n, ctx.uploader_username.as_deref()) {
                ctx.uploader_sc_user_id.clone()
            } else {
                None
            };
            to_candidate(n, sc)
        })
        .collect();
    if primary.is_empty() {
        if let Some(u) = ctx.uploader_username.as_deref() {
            primary.push(to_candidate(u, ctx.uploader_sc_user_id.clone()));
        }
    }

    let featured = parsed
        .featured
        .iter()
        .map(|n| to_candidate(n, None))
        .collect();
    let producers = parsed
        .producers
        .iter()
        .map(|n| to_candidate(n, None))
        .collect();
    let remixers = parsed
        .remixers
        .iter()
        .map(|n| to_candidate(n, None))
        .collect();

    let primary_self_upload = primary
        .first()
        .map(|p| p.sc_user_id.is_some())
        .unwrap_or(false);
    let confidence = if primary.is_empty() {
        0.05
    } else if primary_self_upload {
        0.55
    } else {
        0.2
    };
    ResolveResult {
        source: ResolveSource::Heuristic,
        confidence,
        primary,
        featured,
        producers,
        remixers,
        album: None,
        isrc: ctx.isrc.clone(),
        release_date: None,
        release_year: None,
        is_cover: parsed.is_cover,
    }
}

fn from_mb(
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
        producers: Vec::new(),
        remixers: Vec::new(),
        album,
        isrc,
        release_date: None,
        release_year: None,
        is_cover: false,
    }
}

fn merge_with(
    heuristic: ResolveResult,
    mb_res: ResolveResult,
    ctx: &TrackContext,
) -> ResolveResult {
    let mut out = mb_res;
    if out.primary.is_empty() {
        out.primary = heuristic.primary.clone();
    } else if let Some(first) = out.primary.first_mut() {
        if first.sc_user_id.is_none() {
            if let Some(scid) = ctx.uploader_sc_user_id.clone() {
                if name_matches_uploader(&first.name, ctx.uploader_username.as_deref()) {
                    first.sc_user_id = Some(scid);
                }
            }
        }
    }
    if out.producers.is_empty() {
        out.producers = heuristic.producers;
    }
    if out.remixers.is_empty() {
        out.remixers = heuristic.remixers;
    }
    if out.featured.is_empty() {
        out.featured = heuristic.featured;
    }
    out
}

fn name_matches_uploader(name: &str, uploader: Option<&str>) -> bool {
    let Some(u) = uploader else { return false };
    crate::modules::enrich::normalize::normalize_name(name)
        == crate::modules::enrich::normalize::normalize_name(u)
}
