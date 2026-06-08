use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::Pool as RedisPool;
use futures::future::join_all;
use serde::Serialize;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tracing::warn;

use crate::error::AppResult;
use crate::modules::auth::AuthService;
use crate::sc::{self, ScClient};

use super::actions::{self, ActionCtx};

const BATCH_SIZE: i64 = 50;
const FLUSH_CONCURRENCY: usize = 16;
const LOCK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const MAX_RETRIES: i32 = 5;
const BACKOFF_BAN_SEC: i64 = 30 * 60;
const BACKOFF_RATE_LIMIT_SEC: i64 = 5 * 60;
const BACKOFF_CAP_SEC: i64 = 60 * 60;
const COUNTS_CACHE_TTL_SEC: usize = 5;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct SyncQueueRow {
    pub id: Uuid,
    pub user_id: String,
    pub action_type: String,
    pub target_urn: String,
    pub payload: Option<Value>,
    pub locked_at: Option<DateTime<Utc>>,
    pub retry_count: i32,
    pub last_error: Option<String>,
    pub next_run_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub dead: bool,
    pub failed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct FlushStats {
    pub synced: usize,
    pub failed: usize,
}

pub struct SyncQueueService {
    pg: PgPool,
    sc: ScClient,
    auth: Arc<AuthService>,
    redis: RedisPool,
}

impl SyncQueueService {
    pub fn new(pg: PgPool, sc: ScClient, auth: Arc<AuthService>, redis: RedisPool) -> Arc<Self> {
        Arc::new(Self {
            pg,
            sc,
            auth,
            redis,
        })
    }

    /// `(pending, failed)` для UI-индикатора в /auth/status. Кешируем в Redis
    /// на 5 секунд: при поллинге фронта раз в 30 сек и сотнях тысяч активных
    /// сессий иначе получаем тысячи SELECT/sec по `sync_queue`. Лаг до 5 сек
    /// для бейджа синка некритичен.
    pub async fn pending_counts_for_user(&self, sc_user_id: &str) -> AppResult<(i64, i64)> {
        if sc_user_id.is_empty() {
            return Ok((0, 0));
        }
        let key = format!("sync_queue:counts:{sc_user_id}");

        if let Ok(mut conn) = self.redis.get().await {
            let raw: Option<String> = conn.get(&key).await.ok().flatten();
            if let Some(s) = raw {
                if let Some((p, f)) = parse_counts(&s) {
                    return Ok((p, f));
                }
            }
        }

        let (pending, failed): (i64, i64) = sqlx::query_as(
            "SELECT \
                 COUNT(*) FILTER (WHERE retry_count = 0 AND dead = false)::bigint, \
                 COUNT(*) FILTER (WHERE retry_count > 0 OR dead = true)::bigint \
             FROM sync_queue WHERE user_id = ANY($1)",
        )
            .bind(crate::common::sc_ids::user_id_variants(sc_user_id))
        .fetch_one(&self.pg)
        .await?;

        if let Ok(mut conn) = self.redis.get().await {
            let payload = format!("{pending}:{failed}");
            let _: Result<(), _> = conn
                .set_ex(&key, payload, COUNTS_CACHE_TTL_SEC as u64)
                .await;
        }
        Ok((pending, failed))
    }

    /// Поставить мутацию в очередь.
    /// - Если есть обратное действие (like → unlike) на тот же target — удаляем
    ///   его, новую запись не пишем: пользователь успел отменить намерение.
    /// - Иначе INSERT с дедупом через UNIQUE(user_id, action_type, target_urn).
    ///   Повторный enqueue того же действия — no-op (DO NOTHING).
    pub async fn enqueue(
        &self,
        user_id: &str,
        action_type: &str,
        target_urn: &str,
        payload: Option<&Value>,
    ) -> AppResult<()> {
        if let Some(inv) = actions::inverse(action_type) {
            let cancelled = sqlx::query(
                "DELETE FROM sync_queue \
                 WHERE user_id = $1 AND action_type = $2 AND target_urn = $3 AND locked_at IS NULL",
            )
            .bind(user_id)
            .bind(inv)
            .bind(target_urn)
            .execute(&self.pg)
            .await?;
            if cancelled.rows_affected() > 0 {
                return Ok(());
            }
        }

        sqlx::query(
            "INSERT INTO sync_queue (user_id, action_type, target_urn, payload) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (user_id, action_type, target_urn) DO UPDATE SET \
                 payload = COALESCE(EXCLUDED.payload, sync_queue.payload), \
                 locked_at = NULL, \
                 retry_count = 0, \
                 last_error = NULL, \
                 next_run_at = now()",
        )
        .bind(user_id)
        .bind(action_type)
        .bind(target_urn)
        .bind(payload)
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    /// Cron-таска. Атомарно захватывает батч через FOR UPDATE SKIP LOCKED и
    /// проводит SC-вызовы. На успехе — DELETE. На ошибке — backoff:
    /// - ban/rate-limit: ждём фикс. интервал, retry_count НЕ растёт
    /// - прочее: retry_count++, exp backoff; на MAX_RETRIES — DELETE + warn
    pub async fn flush(&self) -> AppResult<FlushStats> {
        let claimed = self.claim_batch(BATCH_SIZE).await?;
        // Конкурентно (bounded): один забаненный/медленный юзер в голове батча
        // не должен блокировать write-back остальным. Backoff-строки сюда не
        // попадают (claim фильтрует next_run_at <= now()).
        let sem = Arc::new(Semaphore::new(FLUSH_CONCURRENCY));
        let results = join_all(claimed.into_iter().map(|row| {
            let sem = sem.clone();
            async move {
                let _permit = sem.acquire().await;
                match self.execute_one(&row).await {
                    Ok(()) => {
                        // Optimistic delete: только если строку не «тронул» enqueue
                        // конкурентной правки (locked_at не изменился с момента
                        // claim). Иначе строка переживает и переотправит свежий
                        // стейт следующим тиком — фикс lost-write под гонкой
                        // (в т.ч. playlist_sync при правке во время in-flight PUT).
                        if let Err(e) =
                            sqlx::query("DELETE FROM sync_queue WHERE id = $1 AND locked_at = $2")
                                .bind(row.id)
                                .bind(row.locked_at)
                                .execute(&self.pg)
                                .await
                        {
                            warn!(error = %e, "sync_queue delete failed");
                        }
                        true
                    }
                    Err(err) => {
                        if let Err(e) = self.record_failure(&row, &err).await {
                            warn!(error = %e, "sync_queue record_failure failed");
                        }
                        false
                    }
                }
            }
        }))
            .await;
        let synced = results.iter().filter(|&&ok| ok).count();
        let failed = results.len() - synced;
        Ok(FlushStats { synced, failed })
    }

    /// Heal-свип (отдельный тик, не из flush): делает permanent loss
    /// невозможным. Реэнкюивает намерение из mirror/desired-state, которое могло
    /// не доехать (потерянный когда-то action или зависший progress=true), и
    /// оживляет dead-строки, пока их намерение ещё актуально (ON CONFLICT).
    /// NOT EXISTS гейтит только по ЖИВЫМ (dead=false) queue-row, поэтому
    /// конфликт всегда попадает на dead-строку → полное оживление. Каждый
    /// стейтмент с LIMIT — тик дёшев.
    pub async fn heal(&self) -> AppResult<()> {
        let revive = "ON CONFLICT (user_id, action_type, target_urn) DO UPDATE SET \
                      next_run_at = now(), locked_at = NULL, dead = false, \
                      retry_count = 0, last_error = NULL";

        // Лайки треков (bare sc_track_id → urn для совпадения с enqueue call-site).
        sqlx::query(&format!(
            "INSERT INTO sync_queue (user_id, action_type, target_urn) \
             SELECT m.user_id, \
                    CASE WHEN m.wanted_state THEN 'like_track' ELSE 'unlike_track' END, \
                    'soundcloud:tracks:' || m.sc_track_id \
             FROM user_likes_tracks m \
             WHERE m.progress = true \
               AND COALESCE(m.synced_at, m.created_at) < now() - interval '15 minutes' \
               AND NOT EXISTS ( SELECT 1 FROM sync_queue q \
                   WHERE q.user_id = m.user_id AND q.dead = false \
                     AND q.target_urn = 'soundcloud:tracks:' || m.sc_track_id \
                     AND q.action_type IN ('like_track','unlike_track') ) \
             LIMIT 500 {revive}"
        ))
            .execute(&self.pg)
            .await?;

        // Лайки плейлистов (key = playlist_urn).
        sqlx::query(&format!(
            "INSERT INTO sync_queue (user_id, action_type, target_urn) \
             SELECT m.user_id, \
                    CASE WHEN m.wanted_state THEN 'like_playlist' ELSE 'unlike_playlist' END, \
                    m.playlist_urn \
             FROM user_likes_playlists m \
             WHERE m.progress = true \
               AND COALESCE(m.synced_at, m.created_at) < now() - interval '15 minutes' \
               AND NOT EXISTS ( SELECT 1 FROM sync_queue q \
                   WHERE q.user_id = m.user_id AND q.dead = false \
                     AND q.target_urn = m.playlist_urn \
                     AND q.action_type IN ('like_playlist','unlike_playlist') ) \
             LIMIT 500 {revive}"
        ))
            .execute(&self.pg)
            .await?;

        // Фолловинги (key = target_user_urn).
        sqlx::query(&format!(
            "INSERT INTO sync_queue (user_id, action_type, target_urn) \
             SELECT m.user_id, \
                    CASE WHEN m.wanted_state THEN 'follow_user' ELSE 'unfollow_user' END, \
                    m.target_user_urn \
             FROM user_followings m \
             WHERE m.progress = true \
               AND COALESCE(m.synced_at, m.created_at) < now() - interval '15 minutes' \
               AND NOT EXISTS ( SELECT 1 FROM sync_queue q \
                   WHERE q.user_id = m.user_id AND q.dead = false \
                     AND q.target_urn = m.target_user_urn \
                     AND q.action_type IN ('follow_user','unfollow_user') ) \
             LIMIT 500 {revive}"
        ))
            .execute(&self.pg)
            .await?;

        // Owned-плейлисты с pending desired_rev > synced_rev без живого sync.
        sqlx::query(&format!(
            "INSERT INTO sync_queue (user_id, action_type, target_urn) \
             SELECT p.owner_sc_user_id, 'playlist_sync', p.urn \
             FROM playlists p \
             WHERE p.desired_rev > p.synced_rev AND p.owner_sc_user_id IS NOT NULL \
               AND NOT EXISTS ( SELECT 1 FROM sync_queue q \
                   WHERE q.user_id = p.owner_sc_user_id AND q.dead = false \
                     AND q.target_urn = p.urn AND q.action_type = 'playlist_sync' ) \
             LIMIT 500 {revive}"
        ))
            .execute(&self.pg)
            .await?;

        // Гигиена: очень старые dead-строки (>30 дней) — аудит-след исчерпан.
        let _ = sqlx::query(
            "DELETE FROM sync_queue WHERE dead = true AND failed_at < now() - interval '30 days'",
        )
            .execute(&self.pg)
            .await;

        Ok(())
    }

    async fn claim_batch(&self, limit: i64) -> AppResult<Vec<SyncQueueRow>> {
        let lock_timeout = Utc::now() - chrono::Duration::from_std(LOCK_TIMEOUT).unwrap();
        // Не берём dead-строки; не берём таргет, у которого уже есть живой lease
        // другого воркера (per-(user,target) сериализация: like→unlike и
        // последовательные правки одного таргета не выполняются параллельно).
        let rows: Vec<SyncQueueRow> = sqlx::query_as(
            "UPDATE sync_queue SET locked_at = now() \
             WHERE id IN ( \
                 SELECT q.id FROM sync_queue q \
                 WHERE q.dead = false \
                   AND (q.locked_at IS NULL OR q.locked_at < $1) \
                   AND q.next_run_at <= now() \
                   AND NOT EXISTS ( \
                       SELECT 1 FROM sync_queue s \
                       WHERE s.user_id = q.user_id AND s.target_urn = q.target_urn \
                         AND s.id <> q.id \
                         AND s.locked_at IS NOT NULL AND s.locked_at >= $1 ) \
                 ORDER BY q.next_run_at ASC, q.created_at ASC \
                 FOR UPDATE SKIP LOCKED \
                 LIMIT $2 \
             ) RETURNING *",
        )
        .bind(lock_timeout)
        .bind(limit)
        .fetch_all(&self.pg)
        .await?;

        // В пределах одного батча anti-join не спасает (ни одна строка ещё не
        // была locked в снапшоте). Оставляем на исполнение только самую раннюю
        // строку на (user_id, target_urn), остальным сразу снимаем lock —
        // выполнятся следующим тиком после первой.
        let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
        let mut keep: Vec<SyncQueueRow> = Vec::with_capacity(rows.len());
        let mut release: Vec<Uuid> = Vec::new();
        for row in rows {
            if seen.insert((row.user_id.clone(), row.target_urn.clone())) {
                keep.push(row);
            } else {
                release.push(row.id);
            }
        }
        if !release.is_empty() {
            let _ = sqlx::query("UPDATE sync_queue SET locked_at = NULL WHERE id = ANY($1)")
                .bind(&release)
                .execute(&self.pg)
                .await;
        }
        Ok(keep)
    }

    async fn execute_one(&self, row: &SyncQueueRow) -> AppResult<()> {
        let token = self
            .auth
            .get_valid_access_token_for_user(&row.user_id)
            .await?;
        // Канон user_id для mirror-апдейтов экшенов — bare (совпадает с тем, что
        // пишут set_wanted/refresh). Token lookup выше берёт raw (variant-tolerant).
        let action_user_id = crate::common::sc_ids::extract_sc_id(&row.user_id);
        let ctx = ActionCtx {
            sc: &self.sc,
            pg: &self.pg,
            token: &token,
            user_id: action_user_id,
            target_urn: &row.target_urn,
            payload: row.payload.as_ref(),
        };
        actions::dispatch(&ctx, &row.action_type).await
    }

    async fn record_failure(
        &self,
        row: &SyncQueueRow,
        err: &crate::error::AppError,
    ) -> AppResult<()> {
        let mut msg = err.to_string();
        msg.truncate(500);

        // Внешние блокировки SC (ban/rate-limit) — не наш баг, ретраить чаще
        // нет смысла, и инкремент retry_count в таких случаях быстро убьёт
        // легитимные действия. Отложить и оставить retry_count.
        let backoff_sec = if sc::is_ban_error(err) {
            BACKOFF_BAN_SEC
        } else if sc::is_rate_limited(err) {
            BACKOFF_RATE_LIMIT_SEC
        } else {
            // 2,4,8,16,32 мин (cap 60). retry_count берём из строки до
            // инкремента, чтобы первая ошибка дала 2 мин, не 1.
            let next = row.retry_count + 1;
            if next >= MAX_RETRIES {
                // НЕ удаляем — паркуем (dead). Намерение durable, видно в admin/
                // badge, heal-свип оживит его пока desired-state его хочет.
                sqlx::query(
                    "UPDATE sync_queue SET \
                        dead = true, failed_at = now(), locked_at = NULL, \
                        last_error = $1, retry_count = $2, next_run_at = 'infinity' \
                     WHERE id = $3",
                )
                    .bind(&msg)
                    .bind(next)
                    .bind(row.id)
                    .execute(&self.pg)
                    .await?;
                warn!(
                    action = %row.action_type,
                    target = %row.target_urn,
                    user = %row.user_id,
                    retries = next,
                    error = %msg,
                    "sync_queue action parked as dead after MAX_RETRIES"
                );
                return Ok(());
            }
            let secs = (60i64.saturating_mul(1 << next)).min(BACKOFF_CAP_SEC);
            sqlx::query(
                "UPDATE sync_queue SET \
                    locked_at = NULL, \
                    retry_count = retry_count + 1, \
                    last_error = $1, \
                    next_run_at = now() + ($2 || ' seconds')::interval \
                 WHERE id = $3",
            )
            .bind(&msg)
            .bind(secs)
            .bind(row.id)
            .execute(&self.pg)
            .await?;
            warn!(
                action = %row.action_type,
                target = %row.target_urn,
                retry = next,
                error = %msg,
                "sync_queue action failed, will retry"
            );
            return Ok(());
        };

        sqlx::query(
            "UPDATE sync_queue SET \
                locked_at = NULL, \
                last_error = $1, \
                next_run_at = now() + ($2 || ' seconds')::interval \
             WHERE id = $3",
        )
        .bind(&msg)
        .bind(backoff_sec)
        .bind(row.id)
        .execute(&self.pg)
        .await?;
        warn!(
            action = %row.action_type,
            target = %row.target_urn,
            backoff_sec,
            error = %msg,
            "sync_queue action blocked by SC (ban/rate-limit), backoff"
        );
        Ok(())
    }
}

fn parse_counts(s: &str) -> Option<(i64, i64)> {
    let (a, b) = s.split_once(':')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}
