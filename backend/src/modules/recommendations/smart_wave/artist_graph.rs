//! Сетка артистов вокруг вкуса юзера.
//!
//! Идея:
//! 1. seeds = top-K артистов, которых юзер часто лайкает / слушает.
//! 2. direct (hop=1) — соседи seeds через `artist_coplay`, weight = seed × edge.
//! 3. indirect (hop=2) — соседи соседей, weight = seed × edge1 × edge2.
//! 4. artist уже найденный в seeds∪direct **не дублируется** в indirect —
//!    оставляем самую короткую дорогу с её весом. Это и есть правило из тз.
//!
//! Кеш в Redis с TTL 60 сек — пересчёт стоит дешевле, чем гонять SQL
//! на каждый запрос /wave. Redis может вырубаться — это OK, тогда graph
//! пересчитывается на холодную.

use std::collections::{HashMap, HashSet};

use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::Pool as RedisPool;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::debug;
use uuid::Uuid;

use crate::error::AppResult;

const SEED_LIMIT: i64 = 20;
const SEED_WINDOW_DAYS: i32 = 90;
const TOTAL_LIMIT: usize = 60;
const DIRECT_PER_SEED: i64 = 12;
const INDIRECT_PER_NODE: i64 = 8;
const CACHE_TTL_SECS: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistAffinity {
    pub artist_id: Uuid,
    /// Нормализованный вес в [0,1]. seed = 1.0; direct/indirect — пропорционально
    /// произведению seed-weight × edge-weight.
    pub weight: f32,
    /// 0 = сам юзер слушает; 1 = direct; 2 = indirect.
    pub hops: u8,
}

pub async fn build_artist_affinity(
    pg: &PgPool,
    redis: &RedisPool,
    sc_user_id: &str,
) -> Vec<ArtistAffinity> {
    if let Some(cached) = read_cache(redis, sc_user_id).await {
        return cached;
    }
    let fresh = match compute(pg, sc_user_id).await {
        Ok(v) => v,
        Err(e) => {
            debug!(user = %sc_user_id, error = %e, "artist_graph: compute failed");
            return Vec::new();
        }
    };
    write_cache(redis, sc_user_id, &fresh).await;
    fresh
}

#[derive(Debug, sqlx::FromRow)]
struct SeedRow {
    artist_id: Uuid,
    weight: f32,
}

#[derive(Debug, sqlx::FromRow)]
struct EdgeRow {
    src: Uuid,
    dst: Uuid,
    weight: f32,
}

async fn compute(pg: &PgPool, sc_user_id: &str) -> AppResult<Vec<ArtistAffinity>> {
    let seeds = load_seed_artists(pg, sc_user_id).await?;
    if seeds.is_empty() {
        return Ok(Vec::new());
    }

    // Нормализуем seed-веса в [0,1] по максимуму.
    let max_seed = seeds.iter().map(|s| s.weight).fold(0f32, f32::max);
    let seed_norm: HashMap<Uuid, f32> = seeds
        .iter()
        .map(|s| {
            let w = if max_seed > 0.0 {
                s.weight / max_seed
            } else {
                1.0
            };
            (s.artist_id, w.clamp(0.0, 1.0))
        })
        .collect();

    let seed_ids: Vec<Uuid> = seeds.iter().map(|s| s.artist_id).collect();
    let direct = load_direct_neighbors(pg, &seed_ids).await?;

    // direct max edge для нормализации.
    let max_edge = direct.iter().map(|e| e.weight).fold(0f32, f32::max).max(1.0);

    // Собираем известные узлы: seeds + direct.
    let mut known: HashMap<Uuid, ArtistAffinity> = HashMap::new();
    for s in &seeds {
        let w = *seed_norm.get(&s.artist_id).unwrap_or(&1.0);
        known.insert(
            s.artist_id,
            ArtistAffinity {
                artist_id: s.artist_id,
                weight: w,
                hops: 0,
            },
        );
    }
    for e in &direct {
        let seed_w = *seed_norm.get(&e.src).unwrap_or(&0.0);
        let edge_w = (e.weight / max_edge).clamp(0.0, 1.0);
        let w = seed_w * edge_w;
        upsert_better(&mut known, e.dst, w, 1);
    }

    // indirect = соседи direct'ов, но **не** seeds и не уже-direct (по правилу
    // user'а: если нашёлся напрямую, не идём через 2 hop).
    let direct_ids: Vec<Uuid> = known
        .iter()
        .filter(|(_, v)| v.hops == 1)
        .map(|(k, _)| *k)
        .collect();
    if !direct_ids.is_empty() {
        let indirect = load_indirect_neighbors(pg, &direct_ids).await?;
        let already: HashSet<Uuid> = known.keys().copied().collect();
        for e in &indirect {
            if already.contains(&e.dst) {
                continue;
            }
            let direct_aff = known.get(&e.src);
            let Some(da) = direct_aff else { continue };
            let edge_w = (e.weight / max_edge).clamp(0.0, 1.0);
            let w = da.weight * edge_w;
            upsert_better(&mut known, e.dst, w, 2);
        }
    }

    let mut out: Vec<ArtistAffinity> = known
        .into_values()
        .filter(|a| a.weight > 0.001)
        .collect();
    out.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(TOTAL_LIMIT);
    Ok(out)
}

fn upsert_better(known: &mut HashMap<Uuid, ArtistAffinity>, id: Uuid, w: f32, hops: u8) {
    let entry = known.entry(id).or_insert(ArtistAffinity {
        artist_id: id,
        weight: 0.0,
        hops,
    });
    if w > entry.weight {
        entry.weight = w;
        entry.hops = hops;
    }
}

async fn load_seed_artists(pg: &PgPool, sc_user_id: &str) -> AppResult<Vec<SeedRow>> {
    let rows: Vec<SeedRow> = sqlx::query_as(
        "WITH likes AS ( \
             SELECT ulk.sc_track_id, \
                 EXP(-EXTRACT(EPOCH FROM (NOW() - ulk.created_at))/86400.0/60.0)::real AS w \
             FROM user_likes_tracks ulk \
             WHERE ulk.user_id = $1 AND ulk.wanted_state = true \
               AND ulk.created_at > NOW() - make_interval(days => $2::int) \
         ), \
         plays AS ( \
             SELECT ue.sc_track_id, \
                 (0.3 * EXP(-EXTRACT(EPOCH FROM (NOW() - ue.created_at))/86400.0/60.0))::real AS w \
             FROM user_events ue \
             WHERE ue.sc_user_id = $1 \
               AND ue.event_type IN ('full_play', 'play_complete') \
               AND ue.created_at > NOW() - make_interval(days => $2::int) \
         ), \
         signals AS ( \
             SELECT sc_track_id, w FROM likes \
             UNION ALL \
             SELECT sc_track_id, w FROM plays \
         ) \
         SELECT a.id AS artist_id, SUM(s.w)::real AS weight \
         FROM signals s \
         JOIN tracks it ON it.sc_track_id = s.sc_track_id \
         JOIN track_artists ta ON ta.track_id = it.id AND ta.role = 'primary' \
         JOIN artists a ON a.id = ta.artist_id \
         WHERE a.merged_into IS NULL \
         GROUP BY a.id \
         ORDER BY weight DESC \
         LIMIT $3",
    )
    .bind(sc_user_id)
    .bind(SEED_WINDOW_DAYS)
    .bind(SEED_LIMIT)
    .fetch_all(pg)
    .await?;
    Ok(rows)
}

async fn load_direct_neighbors(pg: &PgPool, seeds: &[Uuid]) -> AppResult<Vec<EdgeRow>> {
    if seeds.is_empty() {
        return Ok(Vec::new());
    }
    // Берём top-DIRECT_PER_SEED соседей для каждого seed через ROW_NUMBER.
    let rows: Vec<EdgeRow> = sqlx::query_as(
        "WITH neighbors AS ( \
             SELECT \
                 (CASE WHEN ac.a_id = ANY($1) THEN ac.a_id ELSE ac.b_id END) AS src, \
                 (CASE WHEN ac.a_id = ANY($1) THEN ac.b_id ELSE ac.a_id END) AS dst, \
                 ac.weight \
             FROM artist_coplay ac \
             WHERE (ac.a_id = ANY($1) OR ac.b_id = ANY($1)) \
         ), \
         ranked AS ( \
             SELECT src, dst, weight, \
                 ROW_NUMBER() OVER (PARTITION BY src ORDER BY weight DESC) AS rn \
             FROM neighbors \
             WHERE NOT (dst = ANY($1)) \
         ) \
         SELECT r.src, r.dst, r.weight FROM ranked r \
         JOIN artists a ON a.id = r.dst \
         WHERE r.rn <= $2 AND a.merged_into IS NULL",
    )
    .bind(seeds)
    .bind(DIRECT_PER_SEED)
    .fetch_all(pg)
    .await?;
    Ok(rows)
}

async fn load_indirect_neighbors(pg: &PgPool, direct: &[Uuid]) -> AppResult<Vec<EdgeRow>> {
    if direct.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<EdgeRow> = sqlx::query_as(
        "WITH neighbors AS ( \
             SELECT \
                 (CASE WHEN ac.a_id = ANY($1) THEN ac.a_id ELSE ac.b_id END) AS src, \
                 (CASE WHEN ac.a_id = ANY($1) THEN ac.b_id ELSE ac.a_id END) AS dst, \
                 ac.weight \
             FROM artist_coplay ac \
             WHERE (ac.a_id = ANY($1) OR ac.b_id = ANY($1)) \
         ), \
         ranked AS ( \
             SELECT src, dst, weight, \
                 ROW_NUMBER() OVER (PARTITION BY src ORDER BY weight DESC) AS rn \
             FROM neighbors \
             WHERE NOT (dst = ANY($1)) \
         ) \
         SELECT r.src, r.dst, r.weight FROM ranked r \
         JOIN artists a ON a.id = r.dst \
         WHERE r.rn <= $2 AND a.merged_into IS NULL",
    )
    .bind(direct)
    .bind(INDIRECT_PER_NODE)
    .fetch_all(pg)
    .await?;
    Ok(rows)
}

fn cache_key(sc_user_id: &str) -> String {
    format!("smartwave:graph:{sc_user_id}")
}

async fn read_cache(redis: &RedisPool, sc_user_id: &str) -> Option<Vec<ArtistAffinity>> {
    let mut conn = redis.get().await.ok()?;
    let raw: Option<String> = conn.get(cache_key(sc_user_id)).await.ok().flatten();
    let raw = raw?;
    serde_json::from_str(&raw).ok()
}

async fn write_cache(redis: &RedisPool, sc_user_id: &str, data: &[ArtistAffinity]) {
    let Ok(payload) = serde_json::to_string(data) else {
        return;
    };
    let Ok(mut conn) = redis.get().await else {
        return;
    };
    let _: Result<(), _> = conn
        .set_ex::<_, _, ()>(cache_key(sc_user_id), payload, CACHE_TTL_SECS)
        .await;
}
