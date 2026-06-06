//! Сетка близости артистов вокруг вкуса + аддитивное распространение.
//!
//! Модель:
//! 1. TIER A (сиды) = участники последних лайков (`primary`+`featured`+
//!    `remixer`; если у трека нет кредитов — фолбэк через `album_artists`).
//!    Вес сида ∝ частота лайков × свежесть × сколько реально слушаешь (плеи).
//!    Так доминантный артист (много лайков+плеёв) перевешивает случайный
//!    одноразовый лайк.
//! 2. Рёбра «близости %» = `artist_coplay` (коллаборации), нормализованные
//!    ПО ИСТОЧНИКУ: ближайший коллаб артиста = 1.0.
//! 3. `affinity(v) = Σ по всем путям Π(рёбра)` — затухающее spreading-
//!    activation на [HOPS] хопов. Вклады РАЗНЫХ путей к одному артисту
//!    СУММИРУЮТСЯ: psychosis→мокери(.9)→shadow(.5) + psychosis→гуль(.5)→
//!    shadow(.1) = 0.45 + 0.05 = 0.50.
//! 4. Диз-артист (≥ [DISLIKE_ARTIST_MIN] дизов на его треки) выкидывается из
//!    графа и гасит близких соседей (анти-спред).

use std::collections::HashMap;

use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;
use tracing::debug;
use uuid::Uuid;

use crate::modules::recommendations::service::util::user_id_variants;
use crate::modules::recommendations::service::RecommendationsService;

const SEED_LIMIT: i64 = 48;
const LIKES_WINDOW_DAYS: i32 = 365;
const PLAYS_WINDOW_DAYS: i32 = 120;
/// Во сколько плеи весят относительно лайков при сборке сида.
const PLAY_BOOST: f32 = 0.6;
const HOPS: usize = 3;
/// Глобальный демпинг хопа: ≈1, чтобы честно повторять модель «45+5%», но <1,
/// чтобы дальние хопы затухали и пропагация сходилась.
const GAMMA: f32 = 0.9;
/// Прунинг: активация ниже порога не распространяется дальше.
const EPS: f32 = 0.004;
/// Кап фронтира на хоп (highload: ограничивает размер ANY-массива в SQL).
const FRONTIER_CAP: usize = 320;
/// Кап итогового графа.
const TOTAL_CAP: usize = 1500;
const DISLIKE_ARTIST_MIN: i64 = 3;
/// Сколько вычитаем у соседей диз-артиста (анти-хотелка).
const ANTISPREAD_MU: f32 = 0.6;
const CACHE_TTL_SECS: u64 = 90;

/// Чем затравливаем сетку. Юзер — взвешенными лайками+плеями; трек — его
/// участниками; артист — самим собой.
pub enum GraphSeed {
    User,
    Track(u64),
    Artist(Uuid),
}

/// Карта `artist_id → affinity`. Сиды ≈1.0, дальше затухает; диз-артистов нет.
pub type Affinity = HashMap<Uuid, f32>;

/// Сетка + кого юзер «задизил» как артиста (нужно жёстко резать их треки даже
/// в чистом MERT-хвосте, где affinity уже 0).
pub struct GraphResult {
    pub affinity: Affinity,
    pub disliked_artists: Vec<Uuid>,
}

pub async fn build_affinity(
    svc: &RecommendationsService,
    sc_user_id: &str,
    seed: GraphSeed,
) -> GraphResult {
    let variants = user_id_variants(sc_user_id);
    let disliked = load_disliked_artists(&svc.pg, &variants).await;

    if let GraphSeed::User = seed {
        if let Some(cached) = read_cache(&svc.redis, sc_user_id).await {
            return GraphResult {
                affinity: cached,
                disliked_artists: disliked,
            };
        }
    }

    let mut seeds = match seed {
        GraphSeed::User => load_user_seeds(&svc.pg, &variants).await,
        GraphSeed::Track(t) => load_track_seeds(&svc.pg, t).await,
        GraphSeed::Artist(a) => {
            let mut m = HashMap::new();
            m.insert(a, 1.0f32);
            m
        }
    };
    for d in &disliked {
        seeds.remove(d);
    }
    if seeds.is_empty() {
        return GraphResult {
            affinity: HashMap::new(),
            disliked_artists: disliked,
        };
    }
    normalize_by_max(&mut seeds);

    let mut affinity = propagate(&svc.pg, &seeds, &disliked).await;
    anti_spread(&svc.pg, &mut affinity, &disliked).await;
    cap_top(&mut affinity, TOTAL_CAP);

    if let GraphSeed::User = seed {
        write_cache(&svc.redis, sc_user_id, &affinity).await;
    }
    GraphResult {
        affinity,
        disliked_artists: disliked,
    }
}

/// Spreading-activation: `total = s + γMs + γ²M²s + ...`, вклады путей
/// суммируются. Сиды держат свой вес; в диз-артистов активация не течёт.
async fn propagate(pg: &PgPool, seeds: &Affinity, disliked: &[Uuid]) -> Affinity {
    let disliked_set: std::collections::HashSet<Uuid> = disliked.iter().copied().collect();
    let mut total = seeds.clone();
    let mut activation = seeds.clone();

    for _ in 0..HOPS {
        if activation.is_empty() {
            break;
        }
        let frontier = top_keys(&activation, FRONTIER_CAP);
        let edges = load_coplay_edges(pg, &frontier).await;
        if edges.is_empty() {
            break;
        }

        // adjacency[src] = (dst, raw_weight); src — узел фронтира.
        let frontier_set: std::collections::HashSet<Uuid> = frontier.iter().copied().collect();
        let mut adjacency: HashMap<Uuid, Vec<(Uuid, f32)>> = HashMap::new();
        for (a, b, w) in edges {
            if frontier_set.contains(&a) {
                adjacency.entry(a).or_default().push((b, w));
            }
            if frontier_set.contains(&b) {
                adjacency.entry(b).or_default().push((a, w));
            }
        }

        let mut next: Affinity = HashMap::new();
        for (src, dsts) in &adjacency {
            let Some(&act) = activation.get(src) else {
                continue;
            };
            let max_w = dsts.iter().map(|(_, w)| *w).fold(0f32, f32::max).max(1e-6);
            for (dst, w) in dsts {
                // Сиды остаются на своём весе; в диз-артистов не льём.
                if seeds.contains_key(dst) || disliked_set.contains(dst) {
                    continue;
                }
                let e = (w / max_w).clamp(0.0, 1.0); // близость %
                *next.entry(*dst).or_insert(0.0) += act * e * GAMMA;
            }
        }
        next.retain(|_, v| *v >= EPS);
        if next.is_empty() {
            break;
        }
        for (k, v) in &next {
            *total.entry(*k).or_insert(0.0) += *v;
        }
        activation = next;
    }
    total
}

/// Диз-артист радиирует «анти-хотелку»: соседи по коллабу слегка глушатся.
async fn anti_spread(pg: &PgPool, affinity: &mut Affinity, disliked: &[Uuid]) {
    if disliked.is_empty() {
        return;
    }
    let edges = load_coplay_edges(pg, disliked).await;
    let disliked_set: std::collections::HashSet<Uuid> = disliked.iter().copied().collect();
    // max-нормализация по диз-источнику.
    let mut by_src: HashMap<Uuid, Vec<(Uuid, f32)>> = HashMap::new();
    for (a, b, w) in edges {
        if disliked_set.contains(&a) {
            by_src.entry(a).or_default().push((b, w));
        }
        if disliked_set.contains(&b) {
            by_src.entry(b).or_default().push((a, w));
        }
    }
    for (_, dsts) in by_src {
        let max_w = dsts.iter().map(|(_, w)| *w).fold(0f32, f32::max).max(1e-6);
        for (dst, w) in dsts {
            if let Some(v) = affinity.get_mut(&dst) {
                *v = (*v - ANTISPREAD_MU * (w / max_w)).max(0.0);
            }
        }
    }
    for d in disliked {
        affinity.remove(d);
    }
    affinity.retain(|_, v| *v > 0.0);
}

async fn load_user_seeds(pg: &PgPool, variants: &[String]) -> Affinity {
    let rows: Vec<(Uuid, f32)> = sqlx::query_as(
        "WITH rl AS ( \
             SELECT sc_track_id, \
                 EXP(-EXTRACT(EPOCH FROM (now()-created_at))/86400.0/60.0)::real AS rec \
             FROM user_likes_tracks \
             WHERE user_id = ANY($1) AND wanted_state = true \
               AND created_at > now() - make_interval(days => $2::int) \
             ORDER BY created_at DESC, ctid DESC \
             LIMIT 200 \
         ), \
         parts AS ( \
             SELECT ta.artist_id, rl.rec, \
                 (CASE ta.role WHEN 'primary' THEN 1.0 WHEN 'featured' THEN 0.6 ELSE 0.5 END)::real AS rw \
             FROM rl \
             JOIN tracks it ON it.sc_track_id = rl.sc_track_id \
             JOIN track_artists ta ON ta.track_id = it.id AND ta.role IN ('primary','featured','remixer') \
             UNION ALL \
             SELECT aa.artist_id, rl.rec, 0.8::real \
             FROM rl \
             JOIN tracks it ON it.sc_track_id = rl.sc_track_id \
             JOIN album_artists aa ON aa.album_id = it.album_id \
             WHERE NOT EXISTS ( \
                 SELECT 1 FROM track_artists ta2 \
                 WHERE ta2.track_id = it.id AND ta2.role IN ('primary','featured','remixer') \
             ) \
         ), \
         like_w AS (SELECT artist_id, SUM(rec*rw)::real AS w FROM parts GROUP BY artist_id), \
         play_w AS ( \
             SELECT ta.artist_id, \
                 SUM(EXP(-EXTRACT(EPOCH FROM (now()-ue.created_at))/86400.0/60.0))::real AS w \
             FROM user_events ue \
             JOIN tracks it ON it.sc_track_id = ue.sc_track_id \
             JOIN track_artists ta ON ta.track_id = it.id AND ta.role = 'primary' \
             WHERE ue.sc_user_id = ANY($1) \
               AND ue.event_type IN ('full_play','play_complete') \
               AND ue.created_at > now() - make_interval(days => $3::int) \
             GROUP BY ta.artist_id \
         ), \
         merged AS ( \
             SELECT COALESCE(l.artist_id, p.artist_id) AS artist_id, \
                 (COALESCE(l.w,0) + $4::real * COALESCE(p.w,0))::real AS weight \
             FROM like_w l FULL OUTER JOIN play_w p ON p.artist_id = l.artist_id \
         ) \
         SELECT m.artist_id, m.weight \
         FROM merged m JOIN artists a ON a.id = m.artist_id \
         WHERE a.merged_into IS NULL AND m.weight > 0 \
         ORDER BY m.weight DESC \
         LIMIT $5",
    )
        .bind(variants)
        .bind(LIKES_WINDOW_DAYS)
        .bind(PLAYS_WINDOW_DAYS)
        .bind(PLAY_BOOST)
        .bind(SEED_LIMIT)
        .fetch_all(pg)
        .await
        .unwrap_or_default();
    rows.into_iter().collect()
}

async fn load_track_seeds(pg: &PgPool, sc_track_id: u64) -> Affinity {
    let rows: Vec<(Uuid, f32)> = sqlx::query_as(
        "SELECT ta.artist_id, \
             (CASE ta.role WHEN 'primary' THEN 1.0 WHEN 'featured' THEN 0.6 ELSE 0.5 END)::real AS w \
         FROM tracks it \
         JOIN track_artists ta ON ta.track_id = it.id AND ta.role IN ('primary','featured','remixer') \
         JOIN artists a ON a.id = ta.artist_id \
         WHERE it.sc_track_id = $1 AND a.merged_into IS NULL",
    )
        .bind(sc_track_id.to_string())
        .fetch_all(pg)
        .await
        .unwrap_or_default();
    if !rows.is_empty() {
        return rows.into_iter().collect();
    }
    // Фолбэк: трек без кредитов → через альбом.
    let rows: Vec<(Uuid, f32)> = sqlx::query_as(
        "SELECT aa.artist_id, 0.8::real AS w \
         FROM tracks it \
         JOIN album_artists aa ON aa.album_id = it.album_id \
         JOIN artists a ON a.id = aa.artist_id \
         WHERE it.sc_track_id = $1 AND a.merged_into IS NULL",
    )
        .bind(sc_track_id.to_string())
        .fetch_all(pg)
        .await
        .unwrap_or_default();
    rows.into_iter().collect()
}

async fn load_disliked_artists(pg: &PgPool, variants: &[String]) -> Vec<Uuid> {
    // Артист «дизнут» только если дизов >= порога И дизов БОЛЬШЕ, чем лайков на
    // нём: 0 лайков + 3 диза → дизнут; 5 лайков + 3 диза → нет (ты его любишь).
    sqlx::query_scalar(
        "WITH disl AS ( \
             SELECT ta.artist_id, COUNT(*) AS dc \
             FROM disliked_tracks dt \
             JOIN tracks it ON it.sc_track_id = dt.sc_track_id \
             JOIN track_artists ta ON ta.track_id = it.id AND ta.role = 'primary' \
             WHERE dt.sc_user_id = ANY($1) \
             GROUP BY ta.artist_id \
         ), \
         lik AS ( \
             SELECT ta.artist_id, COUNT(*) AS lc \
             FROM user_likes_tracks ul \
             JOIN tracks it ON it.sc_track_id = ul.sc_track_id \
             JOIN track_artists ta ON ta.track_id = it.id AND ta.role = 'primary' \
             WHERE ul.user_id = ANY($1) AND ul.wanted_state = true \
             GROUP BY ta.artist_id \
         ) \
         SELECT d.artist_id \
         FROM disl d \
         LEFT JOIN lik l ON l.artist_id = d.artist_id \
         WHERE d.dc >= $2 AND d.dc > COALESCE(l.lc, 0)",
    )
        .bind(variants)
        .bind(DISLIKE_ARTIST_MIN)
        .fetch_all(pg)
        .await
        .unwrap_or_default()
}

async fn load_coplay_edges(pg: &PgPool, nodes: &[Uuid]) -> Vec<(Uuid, Uuid, f32)> {
    if nodes.is_empty() {
        return Vec::new();
    }
    sqlx::query_as(
        "SELECT a_id, b_id, weight FROM artist_coplay \
         WHERE (a_id = ANY($1) OR b_id = ANY($1)) AND weight > 0",
    )
        .bind(nodes)
        .fetch_all(pg)
        .await
        .unwrap_or_default()
}

fn normalize_by_max(map: &mut Affinity) {
    let max = map.values().copied().fold(0f32, f32::max);
    if max <= 0.0 {
        return;
    }
    for v in map.values_mut() {
        *v = (*v / max).clamp(0.0, 1.0);
    }
}

fn top_keys(map: &Affinity, n: usize) -> Vec<Uuid> {
    let mut pairs: Vec<(Uuid, f32)> = map.iter().map(|(k, v)| (*k, *v)).collect();
    if pairs.len() > n {
        pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        pairs.truncate(n);
    }
    pairs.into_iter().map(|(k, _)| k).collect()
}

fn cap_top(map: &mut Affinity, n: usize) {
    if map.len() <= n {
        return;
    }
    let mut pairs: Vec<(Uuid, f32)> = map.drain().collect();
    pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    pairs.truncate(n);
    *map = pairs.into_iter().collect();
}

fn cache_key(sc_user_id: &str) -> String {
    format!("wave:graph:{sc_user_id}")
}

async fn read_cache(redis: &RedisPool, sc_user_id: &str) -> Option<Affinity> {
    let mut conn = redis.get().await.ok()?;
    let raw: Option<String> = conn.get(cache_key(sc_user_id)).await.ok().flatten();
    let pairs: Vec<(Uuid, f32)> = serde_json::from_str(&raw?).ok()?;
    Some(pairs.into_iter().collect())
}

async fn write_cache(redis: &RedisPool, sc_user_id: &str, affinity: &Affinity) {
    let pairs: Vec<(Uuid, f32)> = affinity.iter().map(|(k, v)| (*k, *v)).collect();
    let Ok(payload) = serde_json::to_string(&pairs) else {
        return;
    };
    let Ok(mut conn) = redis.get().await else {
        return;
    };
    let _: Result<(), _> = conn
        .set_ex::<_, _, ()>(cache_key(sc_user_id), payload, CACHE_TTL_SECS)
        .await;
    debug!(user = %sc_user_id, artists = affinity.len(), "wave graph cached");
}
