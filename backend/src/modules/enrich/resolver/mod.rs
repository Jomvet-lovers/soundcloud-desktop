//! Резолв артиста/альбома одного трека.
//!
//! Путь трека: [`resolve_track`] → быстрый клейм verified-аккаунта
//! ([`verified`]) или каскад внешних источников (ISRC → MB → Genius → AI →
//! локальная эвристика). Любой внешний результат прогоняется через
//! [`merge::merge_with`] — недостающих co-primary добирают разметка заголовка
//! и лейбловая мета ([`signals::LocalSignals`]).
//!
//! Транзиентный отказ источника (сеть/429) помечает результат `degraded` —
//! сервис тогда не перезаписывает более сильный прошлый результат, а отдаёт
//! трек в ретрай по бэкоффу.

mod mb_stage;
mod merge;
mod signals;
mod verified;

use std::sync::Arc;

use tracing::{debug, warn};

use crate::error::AppResult;
use crate::modules::enrich::ai::AiResolverClient;
use crate::modules::enrich::genius as genius_stage;
use crate::modules::enrich::mb::MbClient;
use crate::modules::enrich::normalize::normalize_name;
use crate::modules::lyrics::genius::GeniusService;

pub use merge::enrich_with_local_signals;
pub use signals::LocalSignals;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResolveSource {
    #[default]
    Heuristic,
    /// Лейбловая `metadata_artist` подтвердила/дала состав (без внешнего API).
    Meta,
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
            Self::Meta => "meta",
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
            Self::Meta => 2,
            Self::Ai => 3,
            Self::Genius => 4,
            Self::Mb => 5,
            Self::Isrc => 6,
            Self::ScVerified => 7,
        }
    }
    pub fn from_db(s: &str) -> Self {
        match s {
            "sc_verified" => Self::ScVerified,
            "isrc" => Self::Isrc,
            "mb" => Self::Mb,
            "genius" => Self::Genius,
            "ai" => Self::Ai,
            "meta" => Self::Meta,
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
    /// Во время резолва внешний источник падал транзиентно (сеть/429/прокси):
    /// результат — лучшее из доступного, но НЕ повод затирать более сильный
    /// прошлый. Сервис в этом случае не даунгрейдит, а ретраит по бэкоффу.
    pub degraded: bool,
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
    /// Словарная сегментация склеек + verified-клейм по каталогу.
    pub pg: sqlx::PgPool,
}

/// Полный резолв одного трека: локальные сигналы → verified fast-path или
/// каскад внешних источников → AI-проверка существования для голой эвристики.
pub async fn resolve_track(
    track: &crate::modules::tracks::TrackRow,
    deps: &ResolverDeps,
) -> AppResult<ResolveResult> {
    let ctx = TrackContext::from_row(track);
    let signals = LocalSignals::build(&ctx, &deps.pg).await;

    let mut result = match verified::claim(&ctx, &signals, &deps.pg).await? {
        Some(fast) => enrich_with_local_signals(fast, &ctx, &signals),
        None => resolve(&ctx, &signals, deps).await?,
    };

    if matches!(result.source, ResolveSource::Heuristic) {
        verify_heuristic_existence(&mut result, &ctx, &signals, deps).await;
    }
    Ok(result)
}

/// Каскад внешних источников. Порядок = убывание доверия; каждый результат
/// сливается с локальными сигналами через `merge_with`.
async fn resolve(
    ctx: &TrackContext,
    signals: &LocalSignals,
    deps: &ResolverDeps,
) -> AppResult<ResolveResult> {
    let heuristic = signals.heuristic(ctx);
    let mut degraded = false;

    // `(cover)` в title → uploader сделал кавер. Резолвим в MB/Genius чтобы
    // найти ОРИГИНАЛЬНОГО артиста; persist запишет его в `cover_of_artist_id`,
    // primary_artist_id у трека останется NULL, upload_kind = 'cover'.

    if let Some(isrc) = ctx.isrc.as_ref() {
        match deps.mb.lookup_by_isrc(isrc).await {
            Ok(Some(rec)) => {
                let ext = mb_stage::from_mb(rec, ResolveSource::Isrc, 0.95, Some(isrc.clone()));
                return Ok(merge::merge_with(heuristic, ext, ctx, signals));
            }
            Ok(None) => debug!(isrc, "ISRC lookup empty"),
            Err(e) => {
                debug!(error = %e, isrc, "ISRC lookup failed");
                degraded = true;
            }
        }
    }

    let primary_hint = signals.primary_hint(ctx);
    let title_q = signals.title_query(ctx);

    if let Some(ext) = mb_stage::search(ctx, signals, deps, &primary_hint, &title_q).await {
        let mut out = merge::merge_with(heuristic, ext, ctx, signals);
        out.degraded = degraded;
        return Ok(out);
    }

    match genius_stage::search(&deps.genius, ctx, primary_hint.as_deref(), &title_q).await {
        Ok(Some(res)) => {
            let mut out = merge::merge_with(heuristic, res, ctx, signals);
            out.degraded = degraded;
            return Ok(out);
        }
        Ok(None) => debug!(title_q, "Genius search empty"),
        Err(e) => {
            warn!(error = %e, "Genius search failed");
            degraded = true;
        }
    }

    if let Some(meta_a) = signals.meta_names.first().map(|s| s.as_str()) {
        let differs = primary_hint
            .as_deref()
            .map(|h| normalize_name(meta_a) != normalize_name(h))
            .unwrap_or(true);
        if differs {
            match genius_stage::search(&deps.genius, ctx, Some(meta_a), &title_q).await {
                Ok(Some(res)) => {
                    let mut out = merge::merge_with(heuristic, res, ctx, signals);
                    out.degraded = degraded;
                    return Ok(out);
                }
                Ok(None) => debug!(meta_a, "Genius search empty (metadata_artist)"),
                Err(e) => {
                    warn!(error = %e, "Genius search failed (metadata_artist)");
                    degraded = true;
                }
            }
        }
    }

    if let Some(ai) = deps.ai.as_ref() {
        match ai.resolve(ctx).await {
            Ok(Some(res)) => {
                let mut out = merge::merge_with(heuristic, res, ctx, signals);
                out.degraded = degraded;
                return Ok(out);
            }
            Ok(None) => debug!("AI resolve empty"),
            Err(e) => {
                debug!(error = %e, "AI resolve failed");
                degraded = true;
            }
        }
    }

    let mut out = heuristic;
    out.degraded = degraded;
    Ok(out)
}

/// Для чистой эвристики спрашиваем у AI «такой артист с таким треком вообще
/// существует?» и двигаем confidence. Не источник, а калибровка.
async fn verify_heuristic_existence(
    result: &mut ResolveResult,
    ctx: &TrackContext,
    signals: &LocalSignals,
    deps: &ResolverDeps,
) {
    let Some(ai) = deps.ai.as_ref() else { return };
    let Some(name) = result.primary.first().map(|a| a.name.clone()) else {
        return;
    };
    let title_q = signals.title_query(ctx);
    match ai.verify_existence(&name, &title_q).await {
        Ok(Some(true)) => result.confidence = result.confidence.max(0.4),
        Ok(Some(false)) => result.confidence = result.confidence.min(0.05),
        _ => {}
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub fn ctx(title: &str, uploader: Option<&str>, meta: Option<&str>) -> TrackContext {
        TrackContext {
            title: title.to_string(),
            uploader_username: uploader.map(String::from),
            uploader_sc_user_id: uploader.map(|_| "42".to_string()),
            duration_ms: Some(180_000),
            isrc: None,
            metadata_artist: meta.map(String::from),
            description: None,
        }
    }

    pub fn signals_no_dict(c: &TrackContext) -> LocalSignals {
        LocalSignals::build_with_dictionary(c, &std::collections::HashSet::new())
    }

    pub fn run_heuristic(c: &TrackContext) -> ResolveResult {
        signals_no_dict(c).heuristic(c)
    }

    pub fn names(r: &ResolveResult) -> Vec<&str> {
        r.primary.iter().map(|c| c.name.as_str()).collect()
    }
}
