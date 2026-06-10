use std::sync::Arc;

use tracing::{debug, warn};

use crate::error::AppResult;
use crate::modules::enrich::ai::AiResolverClient;
use crate::modules::enrich::artist_names;
use crate::modules::enrich::genius as genius_stage;
use crate::modules::enrich::mb::{MbArtist, MbClient, MbRecording};
use crate::modules::enrich::normalize::{normalize_name, parse_sc_title, ParsedTitle};
use crate::modules::lyrics::genius::GeniusService;

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
    let mut parsed = parse_sc_title(&ctx.title, ctx.uploader_username.as_deref());
    let meta_names = ctx
        .metadata_artist
        .as_deref()
        .map(artist_names::meta_artist_names)
        .unwrap_or_default();
    maybe_unreverse_with_meta(&mut parsed, &meta_names);
    let heuristic = heuristic_result(ctx, &parsed, &meta_names);

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
                    &meta_names,
                ))
            }
            Ok(None) => debug!(isrc, "ISRC lookup empty"),
            Err(e) => debug!(error = %e, isrc, "ISRC lookup failed"),
        }
    }

    let primary_hint = parsed
        .primary_artists
        .first()
        .filter(|_| parsed.primary_from_title)
        .cloned()
        .or_else(|| meta_names.first().cloned())
        .or_else(|| ctx.uploader_username.clone());
    let title_q = if parsed.cleaned_title.is_empty() {
        ctx.title.clone()
    } else {
        parsed.cleaned_title.clone()
    };

    // MB throttle (1.1с) сериализует enrich, а для SC-аплоадов MB почти всегда
    // пуст — ходим туда только для лейбловых треков (ISRC / живая мета).
    let try_mb = ctx.isrc.is_some() || !meta_names.is_empty();
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
                if let Some(meta_a) = meta_names.first().map(|s| s.as_str()) {
                    if normalize_name(meta_a) != normalize_name(artist) {
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
                    &meta_names,
                ));
            }
        }
    }

    match genius_stage::search(&deps.genius, ctx, primary_hint.as_deref(), &title_q).await {
        Ok(Some(res)) => return Ok(merge_with(heuristic, res, ctx, &meta_names)),
        Ok(None) => debug!(title_q, "Genius search empty"),
        Err(e) => warn!(error = %e, "Genius search failed"),
    }

    if let Some(meta_a) = meta_names.first().map(|s| s.as_str()) {
        let differs = primary_hint
            .as_deref()
            .map(|h| normalize_name(meta_a) != normalize_name(h))
            .unwrap_or(true);
        if differs {
            match genius_stage::search(&deps.genius, ctx, Some(meta_a), &title_q).await {
                Ok(Some(res)) => return Ok(merge_with(heuristic, res, ctx, &meta_names)),
                Ok(None) => debug!(meta_a, "Genius search empty (metadata_artist)"),
                Err(e) => warn!(error = %e, "Genius search failed (metadata_artist)"),
            }
        }
    }

    if let Some(ai) = deps.ai.as_ref() {
        match ai.resolve(ctx).await {
            Ok(Some(res)) => return Ok(merge_with(heuristic, res, ctx, &meta_names)),
            Ok(None) => debug!("AI resolve empty"),
            Err(e) => debug!(error = %e, "AI resolve failed"),
        }
    }

    Ok(heuristic)
}

/// Перевёрнутая разметка "Track - Artist" (~4% дефисных тайтлов по корпусу):
/// левая часть мете неизвестна, а правая — ровно артист из меты. Откатываем:
/// "505 - arctic monkeys" + мета "Arctic Monkeys" → артист справа.
fn maybe_unreverse_with_meta(parsed: &mut ParsedTitle, meta_names: &[String]) {
    if !parsed.primary_from_title || meta_names.is_empty() || parsed.cleaned_title.is_empty() {
        return;
    }
    let meta_strs = || meta_names.iter().map(|s| s.as_str());
    let left_known = parsed
        .primary_artists
        .iter()
        .any(|t| artist_names::name_in(t, meta_strs()));
    if left_known || !artist_names::name_in(&parsed.cleaned_title, meta_strs()) {
        return;
    }
    let new_title = parsed
        .raw_artist_part
        .take()
        .unwrap_or_else(|| parsed.primary_artists.join(", "));
    parsed.primary_artists = vec![std::mem::replace(&mut parsed.cleaned_title, new_title)];
}

/// Локальный резолв без внешних API. Иерархия доверия:
///   1. явная разметка "Artist - Title" в заголовке (авторская),
///   2. лейбловая `metadata_artist` (дистрибьюторская, уже без мусора),
///   3. загрузчик.
///
/// Мета и разметка согласны → объединяем составы (мета знает co-артистов,
/// которых в заголовке поленились перечислить, и наоборот).
fn heuristic_result(
    ctx: &TrackContext,
    parsed: &ParsedTitle,
    meta_names: &[String],
) -> ResolveResult {
    let to_candidate = |name: &str, sc_user_id: Option<String>| ArtistCandidate {
        name: name.to_string(),
        mb_id: None,
        genius_id: None,
        sc_user_id,
    };
    let attach_uploader_sc = |n: &str| {
        if name_matches_uploader(n, ctx.uploader_username.as_deref()) {
            ctx.uploader_sc_user_id.clone()
        } else {
            None
        }
    };

    let mut source = ResolveSource::Heuristic;
    let mut primary: Vec<ArtistCandidate> = if parsed.primary_from_title {
        parsed
            .primary_artists
            .iter()
            .map(|n| to_candidate(n, attach_uploader_sc(n)))
            .collect()
    } else {
        Vec::new()
    };

    if primary.is_empty() {
        if !meta_names.is_empty() {
            source = ResolveSource::Meta;
            primary = meta_names
                .iter()
                .map(|n| to_candidate(n, attach_uploader_sc(n)))
                .collect();
        } else if let Some(u) = ctx.uploader_username.as_deref() {
            primary.push(to_candidate(u, ctx.uploader_sc_user_id.clone()));
        }
    } else if !meta_names.is_empty() {
        let title_names: Vec<&str> = parsed.primary_artists.iter().map(|s| s.as_str()).collect();
        // Сначала склейка: "ALUCIFYxBACKW666S - трек" при мете
        // "alucify, backw666s" дословно равна мете — раскрываем по ней.
        // (Проверка идёт до agree-union: substring-похожесть считает кусок
        // склейки «тем же артистом» и уводит в неверную ветку.)
        if let Some(chain) = unspaced_chain_matches_meta(&title_names, meta_names) {
            source = ResolveSource::Meta;
            primary = chain
                .iter()
                .map(|n| to_candidate(n, attach_uploader_sc(n)))
                .collect();
        } else {
            let agrees = meta_names
                .iter()
                .any(|m| artist_names::name_in(m, title_names.iter().copied()));
            if agrees {
                source = ResolveSource::Meta;
                let known: Vec<&str> = parsed
                    .primary_artists
                    .iter()
                    .chain(parsed.featured.iter())
                    .chain(parsed.producers.iter())
                    .chain(parsed.remixers.iter())
                    .map(|s| s.as_str())
                    .collect();
                for m in meta_names {
                    if !artist_names::name_in(m, known.iter().copied()) {
                        primary.push(to_candidate(m, attach_uploader_sc(m)));
                    }
                }
            }
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

    let self_upload = primary.iter().any(|p| p.sc_user_id.is_some());
    let confidence = if primary.is_empty() {
        0.05
    } else {
        match (source, self_upload) {
            (ResolveSource::Meta, true) => 0.65,
            (ResolveSource::Meta, false) => 0.5,
            (_, true) => 0.55,
            (_, false) => 0.2,
        }
    };
    ResolveResult {
        source,
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
    ext_res: ResolveResult,
    ctx: &TrackContext,
    meta_names: &[String],
) -> ResolveResult {
    let mut out = ext_res;
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

    // Внешние источники часто возвращают только первого исполнителя, а мета
    // знает полный состав ("Psychosis, killaheelz"). Добираем недостающих
    // co-primary — но только когда мета пересекается с найденным составом,
    // иначе она про другой релиз/мусор и доверять ей нельзя.
    if !out.primary.is_empty() && !meta_names.is_empty() {
        let primary_names: Vec<String> = out.primary.iter().map(|c| c.name.clone()).collect();
        let agrees = meta_names
            .iter()
            .any(|m| artist_names::name_in(m, primary_names.iter().map(|s| s.as_str())));
        if agrees {
            let known: Vec<String> = out
                .primary
                .iter()
                .chain(out.featured.iter())
                .chain(out.producers.iter())
                .chain(out.remixers.iter())
                .map(|c| c.name.clone())
                .collect();
            for m in meta_names {
                if !artist_names::name_in(m, known.iter().map(|s| s.as_str())) {
                    out.primary.push(ArtistCandidate {
                        name: m.clone(),
                        mb_id: None,
                        genius_id: None,
                        sc_user_id: None,
                    });
                }
            }
        }
    }
    out
}

fn name_matches_uploader(name: &str, uploader: Option<&str>) -> bool {
    let Some(u) = uploader else { return false };
    crate::modules::enrich::normalize::normalize_name(name)
        == crate::modules::enrich::normalize::normalize_name(u)
}

/// Один title-токен, плотная склейка которого равна склейке ВСЕХ имён меты
/// (просто подряд или через x-джойнер) → вернуть мету как состав.
fn unspaced_chain_matches_meta<'a>(
    title_names: &[&str],
    meta_names: &'a [String],
) -> Option<&'a [String]> {
    if title_names.len() != 1 || meta_names.len() < 2 {
        return None;
    }
    let left = artist_names::compact_key(title_names[0]);
    if left.is_empty() {
        return None;
    }
    let keys: Vec<String> = meta_names
        .iter()
        .map(|m| artist_names::compact_key(m))
        .collect();
    if keys.iter().any(|k| k.is_empty()) {
        return None;
    }
    let plain = keys.concat();
    // Джойнер бывает латинским и кириллическим: AxB / AхB.
    if left == plain || left == keys.join("x") || left == keys.join("х") {
        Some(meta_names)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(title: &str, uploader: Option<&str>, meta: Option<&str>) -> TrackContext {
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

    fn run_heuristic(c: &TrackContext) -> ResolveResult {
        let parsed = parse_sc_title(&c.title, c.uploader_username.as_deref());
        let meta_names = c
            .metadata_artist
            .as_deref()
            .map(artist_names::meta_artist_names)
            .unwrap_or_default();
        heuristic_result(c, &parsed, &meta_names)
    }

    fn names(r: &ResolveResult) -> Vec<&str> {
        r.primary.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn meta_beats_uploader_when_title_has_no_artist() {
        // Реальный кейс: "benz" залит юзером "4", мета знает авторов.
        let c = ctx("benz", Some("4"), Some("ghasaii, psychosis"));
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["ghasaii", "psychosis"]);
        assert_eq!(r.source, ResolveSource::Meta);
        assert!((r.confidence - 0.5).abs() < 1e-6);
    }

    #[test]
    fn stylized_uploader_matches_meta_and_keeps_sc_link() {
        // "psychosis" от ᴍᴏɴᴀʀᴄʜ, мета "Monarch, johnertekker": оба артиста
        // в primary, аплоадер прилинкован к Monarch несмотря на смолкапсы.
        let c = ctx("psychosis", Some("ᴍᴏɴᴀʀᴄʜ"), Some("Monarch, johnertekker"));
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["Monarch", "johnertekker"]);
        assert_eq!(r.primary[0].sc_user_id.as_deref(), Some("42"));
        assert_eq!(r.primary[1].sc_user_id, None);
        assert!((r.confidence - 0.65).abs() < 1e-6);
    }

    #[test]
    fn reupload_with_label_meta_credits_real_artist() {
        // "Каждый день LIL KRYSTALLL" залит Sport1kk: мета должна победить
        // догадку «артист = загрузчик».
        let c = ctx(
            "Каждый день LIL KRYSTALLL",
            Some("Sport1kk"),
            Some("lil krystalll"),
        );
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["lil krystalll"]);
        assert_eq!(r.source, ResolveSource::Meta);
    }

    #[test]
    fn title_and_meta_union_coartists() {
        let c = ctx(
            "Dave Childz - Wish You Were Here",
            Some("JBroadway"),
            Some("Dave Childz, JBroadway"),
        );
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["Dave Childz", "JBroadway"]);
        assert_eq!(r.source, ResolveSource::Meta);
    }

    #[test]
    fn title_wins_when_meta_disjoint() {
        let c = ctx("Senso - minimal", Some("BakedEye"), Some("SUICIDAL AVENUE"));
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["Senso"]);
        assert_eq!(r.source, ResolveSource::Heuristic);
    }

    #[test]
    fn junk_meta_falls_back_to_uploader() {
        let c = ctx("ДВА КУСОЧКА ПИЦЦЫ", Some("GONE.Fludd"), Some("muzok.net"));
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["GONE.Fludd"]);
        assert_eq!(r.source, ResolveSource::Heuristic);
        assert!((r.confidence - 0.55).abs() < 1e-6);
    }

    #[test]
    fn merge_appends_missing_meta_coprimary() {
        // Genius нашёл только Psychosis, мета знает второго.
        let c = ctx("паралич", Some("Psychosis"), Some("Psychosis, killaheelz"));
        let meta_names = artist_names::meta_artist_names("Psychosis, killaheelz");
        let heuristic = run_heuristic(&c);
        let ext = ResolveResult {
            source: ResolveSource::Genius,
            confidence: 0.8,
            primary: vec![ArtistCandidate {
                name: "Psychosis".into(),
                mb_id: None,
                genius_id: Some("123".into()),
                sc_user_id: None,
            }],
            ..Default::default()
        };
        let merged = merge_with(heuristic, ext, &c, &meta_names);
        assert_eq!(names(&merged), vec!["Psychosis", "killaheelz"]);
        assert_eq!(merged.source, ResolveSource::Genius);
    }

    #[test]
    fn merge_skips_meta_when_disjoint_from_external() {
        let c = ctx("song", Some("up"), Some("Akio Ohmori, Ritsuo Kamimura"));
        let meta_names = artist_names::meta_artist_names("Akio Ohmori, Ritsuo Kamimura");
        let heuristic = run_heuristic(&c);
        let ext = ResolveResult {
            source: ResolveSource::Genius,
            confidence: 0.7,
            primary: vec![ArtistCandidate {
                name: "Cyalm".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let merged = merge_with(heuristic, ext, &c, &meta_names);
        assert_eq!(names(&merged), vec!["Cyalm"]);
    }

    #[test]
    fn unspaced_chain_expands_via_meta() {
        // "ALUCIFYxBACKW666S - трек" + мета "alucify, backw666s".
        let c = ctx(
            "ALUCIFYxBACKW666S - sin city",
            Some("alucify"),
            Some("alucify, backw666s"),
        );
        let r = run_heuristic(&c);
        assert_eq!(names(&r), vec!["alucify", "backw666s"]);
        assert_eq!(r.source, ResolveSource::Meta);

        // Кириллический джойнер.
        let c2 = ctx("СОЛНЦЕхЛУНА - ночь", Some("кто-то"), Some("СОЛНЦЕ, ЛУНА"));
        let r2 = run_heuristic(&c2);
        assert_eq!(names(&r2), vec!["СОЛНЦЕ", "ЛУНА"]);
    }

    #[test]
    fn reversed_markup_unreversed_by_meta() {
        // "505 - arctic monkeys": мета знает правую часть, левая — номер/название.
        let mut parsed = parse_sc_title("505 - arctic monkeys", Some("reposter"));
        assert_eq!(parsed.primary_artists, vec!["505"]);
        let meta = artist_names::meta_artist_names("Arctic Monkeys");
        maybe_unreverse_with_meta(&mut parsed, &meta);
        assert_eq!(parsed.primary_artists, vec!["arctic monkeys"]);
        assert_eq!(parsed.cleaned_title, "505");

        // Обычная разметка метой не переворачивается.
        let mut ok = parse_sc_title("Psychosis - x-ray", None);
        let meta2 = artist_names::meta_artist_names("Psychosis");
        maybe_unreverse_with_meta(&mut ok, &meta2);
        assert_eq!(ok.primary_artists, vec!["Psychosis"]);
        assert_eq!(ok.cleaned_title, "x-ray");
    }

    #[test]
    fn merge_does_not_duplicate_featured_from_meta() {
        // Мета перечисляет и фитующих — они уже в featured, в primary не дублируем.
        let c = ctx(
            "GLAM GO! - ГЛЯНЬ ЕЙ НА ЛИЦО (feat. Gone.Fludd)",
            Some("glamgo"),
            Some("Glam Go, Gone.Fludd"),
        );
        let meta_names = artist_names::meta_artist_names("Glam Go, Gone.Fludd");
        let heuristic = run_heuristic(&c);
        let ext = ResolveResult {
            source: ResolveSource::Genius,
            confidence: 0.8,
            primary: vec![ArtistCandidate {
                name: "GLAM GO GANG!".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let merged = merge_with(heuristic, ext, &c, &meta_names);
        assert_eq!(names(&merged), vec!["GLAM GO GANG!"]);
        assert!(merged
            .featured
            .iter()
            .any(|f| f.name.to_lowercase().contains("fludd")));
    }
}
