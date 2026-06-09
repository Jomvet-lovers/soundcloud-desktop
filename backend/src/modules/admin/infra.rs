use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::common::admin::AdminAuth;
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Serialize)]
pub struct PgPoolStats {
    pub connections: u32,
    pub idle: i64,
    pub max: u32,
}

#[derive(Serialize)]
pub struct RedisPoolStats {
    pub ok: bool,
    pub size: i64,
    pub available: i64,
    pub max: i64,
}

#[derive(Serialize)]
pub struct InfraStats {
    pub pg: PgPoolStats,
    pub redis: RedisPoolStats,
}

/// GET /admin/infra — internal connection-pool health of the main backend
/// (Postgres + Redis) — the bits the BFF can't see from outside.
#[tracing::instrument(skip_all)]
pub async fn get_infra(_: AdminAuth, State(state): State<AppState>) -> AppResult<Json<InfraStats>> {
    let pg = PgPoolStats {
        connections: state.pg.size(),
        idle: state.pg.num_idle() as i64,
        max: state.pg.options().get_max_connections(),
    };

    let (size, available, max) = state.cache.pool_status();
    let redis = RedisPoolStats {
        ok: state.cache.ping().await,
        size: size as i64,
        available: available as i64,
        max: max as i64,
    };

    Ok(Json(InfraStats { pg, redis }))
}

// ───────────────────────── HTTP RPS / latency ─────────────────────────

#[derive(Serialize)]
pub struct EndpointStat {
    pub route: String,
    pub count: u64,
    pub avg_ms: u64,
    pub max_ms: u64,
    pub errors: u64,
}

#[derive(Serialize)]
pub struct HttpStats {
    pub uptime_secs: u64,
    pub total_requests: u64,
    pub rps: f64,
    pub endpoints: Vec<EndpointStat>,
}

/// GET /admin/http-stats — per-route request counts + latency since boot,
/// captured by the router's tracking middleware (top 30 by volume).
#[tracing::instrument(skip_all)]
pub async fn http_stats(_: AdminAuth, State(state): State<AppState>) -> AppResult<Json<HttpStats>> {
    let snap = state.http_metrics.snapshot();
    let uptime = state.http_metrics.uptime_secs();
    let total: u64 = snap.iter().map(|(_, s)| s.count).sum();

    let mut endpoints: Vec<EndpointStat> = snap
        .into_iter()
        .map(|(route, s)| EndpointStat {
            route,
            count: s.count,
            avg_ms: s.total_ms.checked_div(s.count).unwrap_or(0),
            max_ms: s.max_ms,
            errors: s.errors,
        })
        .collect();
    endpoints.sort_by_key(|b| std::cmp::Reverse(b.count));
    endpoints.truncate(30);

    Ok(Json(HttpStats {
        uptime_secs: uptime,
        total_requests: total,
        rps: total as f64 / uptime.max(1) as f64,
        endpoints,
    }))
}

// ───────────────────────── slow queries (pg_stat_statements) ─────────────────────────

#[derive(Serialize, sqlx::FromRow)]
pub struct SlowQuery {
    pub query: String,
    pub calls: i64,
    pub mean_ms: f64,
    pub total_ms: f64,
    pub rows: i64,
}

#[derive(Serialize)]
pub struct SlowQueries {
    pub enabled: bool,
    pub queries: Vec<SlowQuery>,
}

/// GET /admin/slow-queries — top statements by mean execution time from
/// pg_stat_statements. Degrades to `enabled:false` when the extension isn't
/// preloaded (set `shared_preload_libraries=pg_stat_statements` on Postgres).
#[tracing::instrument(skip_all)]
pub async fn slow_queries(
    _: AdminAuth,
    State(state): State<AppState>,
) -> AppResult<Json<SlowQueries>> {
    // Best-effort: no-op once enabled, errors (ignored) if the lib isn't preloaded.
    let _ = sqlx::query("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
        .execute(&state.pg)
        .await;

    let res = sqlx::query_as::<_, SlowQuery>(
        "SELECT query, calls::int8 AS calls, mean_exec_time AS mean_ms, \
                total_exec_time AS total_ms, rows::int8 AS rows \
         FROM pg_stat_statements ORDER BY mean_exec_time DESC LIMIT 30",
    )
    .fetch_all(&state.pg)
    .await;

    match res {
        Ok(queries) => Ok(Json(SlowQueries {
            enabled: true,
            queries,
        })),
        Err(_) => Ok(Json(SlowQueries {
            enabled: false,
            queries: Vec::new(),
        })),
    }
}

// ───────────────────────── index usage (pg_stat_user_indexes) ─────────────────────────

#[derive(Serialize, sqlx::FromRow)]
pub struct IndexUsage {
    pub table: String,
    pub index: String,
    pub idx_scan: i64,
    pub size_bytes: i64,
    pub is_unique: bool,
    pub is_primary: bool,
}

#[derive(Serialize)]
pub struct IndexUsageReport {
    pub indexes: Vec<IndexUsage>,
}

/// GET /admin/index-usage — per-index scan counts + size from pg_stat_user_indexes,
/// least-used first. A non-unique/non-pk index with idx_scan≈0 is a drop candidate:
/// it earns nothing on reads but is rewritten on every insert/update of a hot table.
#[tracing::instrument(skip_all)]
pub async fn index_usage(
    _: AdminAuth,
    State(state): State<AppState>,
) -> AppResult<Json<IndexUsageReport>> {
    let indexes = sqlx::query_as::<_, IndexUsage>(
        "SELECT relname AS \"table\", indexrelname AS \"index\", \
                idx_scan::int8 AS idx_scan, \
                pg_relation_size(indexrelid)::int8 AS size_bytes, \
                indisunique AS is_unique, indisprimary AS is_primary \
         FROM pg_stat_user_indexes JOIN pg_index USING (indexrelid) \
         ORDER BY idx_scan ASC, pg_relation_size(indexrelid) DESC",
    )
    .fetch_all(&state.pg)
    .await?;
    Ok(Json(IndexUsageReport { indexes }))
}
