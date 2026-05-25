//! SmartWave — бесконечная волна. Один и тот же движок умеет три режима:
//! `User` (home wave), `Track` (страница трека), `Artist` (страница артиста).
//!
//! Внутри:
//! 1. Собираем сигналы юзера (свежие лайки, дизы, скипы, played) —
//!    [signals::load_recent_signals].
//! 2. Строим граф артистов вокруг вкуса — [artist_graph::build_artist_affinity].
//! 3. Параллельно гоняем три arm'а: track-arm (clap+lyrics+mert от seed-треков),
//!    artist-arm (треки из графа), collab-arm (track2vec).
//! 4. Blender смешивает с весами, адаптивными к недавнему фидбеку.
//! 5. Курсор в Redis помнит уже отданное и негативные исходы для следующего
//!    вызова — отсюда и "бесконечность" + "умнеет за сессию".

pub mod artist_arm;
pub mod artist_graph;
pub mod blender;
pub mod cursor;
pub mod signals;
pub mod track_arm;

use std::collections::{HashMap, HashSet};

use sqlx::PgPool;
use tracing::{debug, info};
use uuid::Uuid;

use crate::error::AppResult;
use crate::modules::recommendations::clusters::recommend_id_str;
use crate::modules::recommendations::service::{RecommendResult, RecommendationsService};

use blender::{BlendWeights, BlendedCandidate};
use cursor::{SeedKind, WaveCursor};
use signals::UserSignals;

const ARTIST_CAP_IN_WINDOW: usize = 2;
const SEED_TRACKS_FOR_USER_ARM: usize = 12;

pub enum SmartWaveSeed<'a> {
    /// Home — собираем волну вокруг свежих сигналов юзера.
    User,
    /// Страница трека — якорь = seed_track_id.
    Track(u64),
    /// Страница артиста — якорь = artist_id с его top-N треками.
    Artist(Uuid, &'a [u64]),
}

pub struct SmartWaveRequest<'a> {
    pub sc_user_id: &'a str,
    pub languages: Option<&'a [String]>,
    pub limit: usize,
    pub cursor_token: Option<&'a str>,
    pub seed: SmartWaveSeed<'a>,
}

pub struct SmartWaveResponse {
    pub tracks: Vec<RecommendResult>,
    pub cursor: String,
}

pub async fn build(
    svc: &RecommendationsService,
    req: SmartWaveRequest<'_>,
) -> AppResult<SmartWaveResponse> {
    let signals = signals::load_recent_signals(&svc.pg, req.sc_user_id).await?;
    let seed_kind = match req.seed {
        SmartWaveSeed::User => SeedKind::User,
        SmartWaveSeed::Track(_) => SeedKind::Track,
        SmartWaveSeed::Artist(_, _) => SeedKind::Artist,
    };
    let seed_key = match &req.seed {
        SmartWaveSeed::User => req.sc_user_id.to_string(),
        SmartWaveSeed::Track(t) => format!("t{t}"),
        SmartWaveSeed::Artist(a, _) => format!("a{a}"),
    };

    let owner = if req.sc_user_id.is_empty() {
        "anon"
    } else {
        req.sc_user_id
    };
    let mut wave_cursor = cursor::load_or_new(
        &svc.redis,
        owner,
        req.cursor_token,
        seed_kind,
        &seed_key,
    )
    .await;

    let exclude = build_exclude(&signals, &wave_cursor, &req.seed);

    let affinity_fut = async {
        if matches!(req.seed, SmartWaveSeed::Track(_)) {
            // Для track-seed артист-граф пользователя играет вспомогательную
            // роль и весит меньше — но всё равно даёт расширение в "близкие миры".
            artist_graph::build_artist_affinity(&svc.pg, &svc.redis, req.sc_user_id).await
        } else {
            artist_graph::build_artist_affinity(&svc.pg, &svc.redis, req.sc_user_id).await
        }
    };

    let seeds_for_track_arm = pick_track_seeds(&req.seed, &signals);
    let negative_ids = negative_ids_for_qdrant(&signals);
    let filter = svc.build_filter(&exclude, req.languages);
    let exclude_set: HashSet<String> = exclude.iter().cloned().collect();

    let track_arm_fut =
        track_arm::recommend_from_many(svc, &seeds_for_track_arm, &negative_ids, filter.as_ref(), 80);
    let collab_arm_fut = blender::collab_for_user(svc, req.sc_user_id, &exclude_set, 80);

    let (affinity, track_cands, collab_cands) =
        tokio::join!(affinity_fut, track_arm_fut, collab_arm_fut);

    let artist_cands =
        artist_arm::pick_tracks(&svc.pg, &affinity, &exclude_set, 80).await;

    let weights = pick_weights(&req.seed, wave_cursor.neg_rate());
    let blended = blender::blend(&track_cands, &artist_cands, &collab_cands, weights);

    let artist_by_track =
        load_artist_by_track(&svc.pg, &blended).await;

    let picked = blender::pick_with_cap(
        blended,
        &artist_by_track,
        &wave_cursor,
        req.limit,
        ARTIST_CAP_IN_WINDOW,
    );

    // S3-verify и quality-filter тут же — за поведением остаётся правило
    // "никаких треков, которых нет в storage".
    let ids: Vec<String> = picked.iter().map(|c| c.sc_track_id.to_string()).collect();
    let missing = svc.s3.find_missing(&ids).await.unwrap_or_default();
    let mut tracks: Vec<RecommendResult> = Vec::with_capacity(picked.len());
    for c in &picked {
        let id_str = c.sc_track_id.to_string();
        if missing.contains(&id_str) {
            continue;
        }
        tracks.push(RecommendResult {
            id: serde_json::json!(c.sc_track_id),
            score: Some(c.score),
            payload: None,
            artist: None,
            genre: None,
            playback_count: None,
            features: None,
        });
        wave_cursor.mark_served(c.sc_track_id, artist_by_track.get(&c.sc_track_id).copied());
    }

    let handle = cursor::save(&svc.redis, owner, &wave_cursor)
        .await
        .unwrap_or_else(|| wave_cursor.handle.clone());
    cursor::register_handle(&svc.redis, owner, &wave_cursor).await;

    info!(
        user = %req.sc_user_id,
        kind = ?seed_kind,
        served_total = wave_cursor.served,
        returned = tracks.len(),
        neg_rate = wave_cursor.neg_rate(),
        weights = ?weights,
        graph = affinity.len(),
        "smartwave built"
    );

    Ok(SmartWaveResponse {
        tracks,
        cursor: handle,
    })
}

/// feedback от клиента: какие треки из текущего окна получили dislike/skip.
/// Записываем в курсор, чтобы следующий build умнел.
pub async fn record_feedback(
    svc: &RecommendationsService,
    sc_user_id: &str,
    cursor_token: &str,
    negatives: usize,
    positives: usize,
) -> Option<String> {
    let owner = if sc_user_id.is_empty() {
        "anon"
    } else {
        sc_user_id
    };
    let mut wave_cursor = cursor::load_or_new(
        &svc.redis,
        owner,
        Some(cursor_token),
        SeedKind::User,
        sc_user_id,
    )
    .await;
    wave_cursor.record_outcomes(negatives, positives);
    let handle = cursor::save(&svc.redis, owner, &wave_cursor).await?;
    cursor::register_handle(&svc.redis, owner, &wave_cursor).await;
    Some(handle)
}

fn build_exclude(
    signals: &UserSignals,
    cursor: &WaveCursor,
    seed: &SmartWaveSeed,
) -> Vec<String> {
    let mut excl = signals.exclude_set();
    for t in cursor.seen_tracks.iter() {
        excl.push(t.to_string());
    }
    if let SmartWaveSeed::Track(t) = seed {
        excl.push(t.to_string());
    }
    excl.sort();
    excl.dedup();
    excl
}

fn pick_track_seeds(seed: &SmartWaveSeed, signals: &UserSignals) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    match seed {
        SmartWaveSeed::Track(t) => {
            out.push(*t);
            // Добавляем 3-5 свежих лайков как "контекст" — даёт волне
            // расширение, а не только клон seed-трека.
            for id in signals.fresh_likes.iter().take(5) {
                if let Ok(n) = id.parse::<u64>() {
                    if n != *t && !out.contains(&n) {
                        out.push(n);
                    }
                }
            }
        }
        SmartWaveSeed::Artist(_, tracks) => {
            for t in tracks.iter().take(8) {
                out.push(*t);
            }
            for id in signals.fresh_likes.iter().take(3) {
                if let Ok(n) = id.parse::<u64>() {
                    if !out.contains(&n) {
                        out.push(n);
                    }
                }
            }
        }
        SmartWaveSeed::User => {
            for id in signals.fresh_likes.iter().take(SEED_TRACKS_FOR_USER_ARM) {
                if let Ok(n) = id.parse::<u64>() {
                    out.push(n);
                }
            }
            // Если лайков мало — добиваем свежим played.
            for id in signals.recent_played.iter() {
                if out.len() >= SEED_TRACKS_FOR_USER_ARM {
                    break;
                }
                if let Ok(n) = id.parse::<u64>() {
                    if !out.contains(&n) {
                        out.push(n);
                    }
                }
            }
        }
    }
    out
}

fn negative_ids_for_qdrant(signals: &UserSignals) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    for id in signals.disliked_ids.iter().chain(signals.recent_skips.iter()) {
        if let Ok(n) = id.parse::<u64>() {
            out.push(n);
        }
    }
    out.sort();
    out.dedup();
    // Чтобы не раздуть payload запроса в qdrant — ограничиваем.
    out.truncate(40);
    out
}

fn pick_weights(seed: &SmartWaveSeed, neg_rate: f32) -> BlendWeights {
    let base = match seed {
        SmartWaveSeed::User => BlendWeights::default_user(),
        SmartWaveSeed::Track(_) => BlendWeights::for_track_seed(),
        SmartWaveSeed::Artist(_, _) => BlendWeights::for_artist_seed(),
    };
    base.adapt_to_negative(neg_rate)
}

async fn load_artist_by_track(
    pg: &PgPool,
    blended: &[BlendedCandidate],
) -> HashMap<u64, Uuid> {
    if blended.is_empty() {
        return HashMap::new();
    }
    let ids: Vec<String> = blended.iter().map(|c| c.sc_track_id.to_string()).collect();
    let rows: Vec<(String, Option<Uuid>)> = match sqlx::query_as(
        "SELECT sc_track_id, primary_artist_id FROM tracks \
         WHERE sc_track_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(pg)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            debug!(error = %e, "smartwave: artist map load failed");
            return HashMap::new();
        }
    };
    let mut out: HashMap<u64, Uuid> = HashMap::new();
    for (sc_id, artist) in rows {
        let Ok(n) = sc_id.parse::<u64>() else {
            continue;
        };
        if let Some(a) = artist {
            out.insert(n, a);
        }
    }
    out
}

/// Cluster-friendly обёртка: возвращает только track_ids, без cursor.
/// Используется home/similar/artist wave при сборке cluster `wave` сверху.
pub async fn cluster_track_ids(
    svc: &RecommendationsService,
    sc_user_id: &str,
    languages: Option<&[String]>,
    seed: SmartWaveSeed<'_>,
    limit: usize,
) -> Vec<String> {
    let req = SmartWaveRequest {
        sc_user_id,
        languages,
        limit,
        cursor_token: None,
        seed,
    };
    match build(svc, req).await {
        Ok(resp) => resp
            .tracks
            .iter()
            .map(|r| recommend_id_str(&r.id))
            .filter(|s| !s.is_empty())
            .collect(),
        Err(e) => {
            debug!(error = %e, "smartwave: cluster_track_ids failed");
            Vec::new()
        }
    }
}
