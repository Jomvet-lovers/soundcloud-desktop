use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::cache::extract_sc_cursor;
use crate::cache::CacheService;
use crate::common::sc_ids::extract_sc_id;
use crate::config::ColdCfg;
use crate::error::AppResult;
use crate::sc::ScClient;

const EVICT_TICK: Duration = Duration::from_secs(3600);
const REFRESH_BATCH_SIZE: usize = 500;
const EVICT_CHUNK_SIZE: i64 = 10_000;
const EVICT_BETWEEN_MS: u64 = 100;

/// Какую SC-коллекцию синхронизируем + куда писать данные.
#[derive(Debug, Clone, Copy)]
pub struct UserCollection {
    /// SC-эндпоинт без query-параметров (`/me/likes/tracks`, `/me/followings`, …).
    pub sc_path: &'static str,
    /// Lock-ключ в Redis. Без юзера — добавится в `key_for`.
    pub lock_kind: &'static str,
    /// Зеркало состояния юзера в PG.
    pub mirror_table: &'static str,
    /// Имя колонки-ключа в `mirror_table`: `sc_track_id` | `playlist_urn` | `target_user_urn`.
    pub mirror_key_col: &'static str,
    /// Тип общедоступного кеша для public-сущностей. Owned-коллекции пишут
    /// payload только в mirror (`mirror_payload_col`), потому что приватные
    /// поля владельца нельзя класть в shared cache, который читают все.
    /// Public-копию owned-сущностей всё равно зеркалируем в shared cache,
    /// чтобы read-path по `/tracks/{urn}` / `/playlists/{urn}` для других
    /// юзеров находил трек без обращения к SC.
    pub shared_cache: SharedCache,
    /// Если задано — payload юзера пишется в эту колонку зеркала (`payload`
    /// для owned-коллекций). Иначе payload идёт только в shared cache.
    pub mirror_payload_col: Option<&'static str>,
    /// Если задано — payload пишется в shared cache **только** когда у объекта
    /// `sharing == "public"`. None — пишем всегда (likes/follows: мы видим
    /// чужие public-объекты, приватных там по определению быть не может).
    pub public_only_to_shared: bool,
    /// `true` — у строк есть `wanted_state` (likes/follows). `false` — owned, без отмены.
    pub has_wanted_state: bool,
    /// Если задано — INSERT в mirror'е скипается при наличии pending-удаления
    /// в sync_queue (нужно owned_playlists, чтобы refresh не воскресил
    /// удалённый плейлист до flush'а).
    pub guard_pending_delete_action: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub enum SharedCache {
    /// `indexed_tracks` (sc_track_id, raw_sc_data). Извлекаем sc_track_id из urn.
    Tracks,
    /// `cached_playlists` (playlist_urn, payload). Ключ — целый urn.
    Playlists,
    /// `cached_users` (user_urn, payload). Ключ — целый urn.
    Users,
}

pub const LIKED_TRACKS: UserCollection = UserCollection {
    sc_path: "/me/likes/tracks",
    lock_kind: "liked-tracks",
    mirror_table: "user_likes_tracks",
    mirror_key_col: "sc_track_id",
    shared_cache: SharedCache::Tracks,
    mirror_payload_col: None,
    public_only_to_shared: false,
    has_wanted_state: true,
    guard_pending_delete_action: None,
};

pub const LIKED_PLAYLISTS: UserCollection = UserCollection {
    sc_path: "/me/likes/playlists",
    lock_kind: "liked-playlists",
    mirror_table: "user_likes_playlists",
    mirror_key_col: "playlist_urn",
    shared_cache: SharedCache::Playlists,
    mirror_payload_col: None,
    public_only_to_shared: false,
    has_wanted_state: true,
    guard_pending_delete_action: None,
};

pub const FOLLOWINGS: UserCollection = UserCollection {
    sc_path: "/me/followings",
    lock_kind: "followings",
    mirror_table: "user_followings",
    mirror_key_col: "target_user_urn",
    shared_cache: SharedCache::Users,
    mirror_payload_col: None,
    public_only_to_shared: false,
    has_wanted_state: true,
    guard_pending_delete_action: None,
};

pub const OWNED_PLAYLISTS: UserCollection = UserCollection {
    sc_path: "/me/playlists",
    lock_kind: "owned-playlists",
    mirror_table: "user_owned_playlists",
    mirror_key_col: "playlist_urn",
    shared_cache: SharedCache::Playlists,
    mirror_payload_col: Some("payload"),
    public_only_to_shared: true,
    has_wanted_state: false,
    guard_pending_delete_action: Some("playlist_delete"),
};

pub const OWNED_TRACKS: UserCollection = UserCollection {
    sc_path: "/me/tracks",
    lock_kind: "owned-tracks",
    mirror_table: "user_owned_tracks",
    mirror_key_col: "sc_track_id",
    shared_cache: SharedCache::Tracks,
    mirror_payload_col: Some("payload"),
    public_only_to_shared: true,
    has_wanted_state: false,
    guard_pending_delete_action: None,
};

/// Сервис фоновых refresh'ей cold-cache.
/// - Дедуп параллельных обновлений: Redis SETNX-лок на ключ ресурса с TTL
///   (`COLD_REFRESH_LOCK_TTL_SEC`). Другой воркер, видя занятый лок, тихо
///   отваливается.
/// - Bounded concurrency: глобальный `Semaphore` ограничивает число живых
///   SC-fetch'ей (`COLD_REFRESH_CONCURRENCY`), чтобы при пике reads не
///   выжечь токены SC.
pub struct ColdRefreshService {
    sc: ScClient,
    pg: PgPool,
    cache: Arc<CacheService>,
    cfg: ColdCfg,
    sem: Arc<Semaphore>,
}

impl ColdRefreshService {
    pub fn new(sc: ScClient, pg: PgPool, cache: Arc<CacheService>, cfg: ColdCfg) -> Arc<Self> {
        let sem = Arc::new(Semaphore::new(cfg.refresh_concurrency));
        Arc::new(Self {
            sc,
            pg,
            cache,
            cfg,
            sem,
        })
    }

    pub fn is_track_stale(&self, synced_at: Option<DateTime<Utc>>) -> bool {
        is_stale(synced_at, self.cfg.track_ttl_sec)
    }

    pub fn is_user_stale(&self, synced_at: Option<DateTime<Utc>>) -> bool {
        is_stale(synced_at, self.cfg.user_ttl_sec)
    }

    pub fn is_playlist_stale(&self, synced_at: Option<DateTime<Utc>>) -> bool {
        is_stale(synced_at, self.cfg.playlist_ttl_sec)
    }

    fn ttl_for(&self, coll: &UserCollection) -> u64 {
        match coll.sc_path {
            p if p == LIKED_TRACKS.sc_path => self.cfg.liked_tracks_ttl_sec,
            p if p == LIKED_PLAYLISTS.sc_path => self.cfg.liked_playlists_ttl_sec,
            p if p == FOLLOWINGS.sc_path => self.cfg.followings_ttl_sec,
            _ => self.cfg.owned_ttl_sec,
        }
    }

    /// Гарантирует, что зеркало `mirror_table` для данного юзера достаточно
    /// свежо. На пустом зеркале — синхронно тянет всё из SC (seed). На stale —
    /// спавнит фоновую задачу: первый клиент после TTL заплатит за refresh,
    /// остальные читают параллельно текущий снапшот. На свежем — no-op.
    /// `extra_params` прокидываются в SC (например `access` для liked tracks).
    pub async fn ensure_collection(
        self: &Arc<Self>,
        coll: UserCollection,
        sc_user_id: &str,
        token: &str,
        extra_params: &[(String, String)],
    ) -> AppResult<()> {
        let max_synced: Option<DateTime<Utc>> = sqlx::query_scalar(&format!(
            "SELECT MAX(synced_at) FROM {} WHERE user_id = $1",
            coll.mirror_table
        ))
        .bind(sc_user_id)
        .fetch_one(&self.pg)
        .await?;

        if max_synced.is_none() {
            self.refresh_collection(coll, sc_user_id, token, extra_params)
                .await?;
            return Ok(());
        }

        if !is_stale(max_synced, self.ttl_for(&coll)) {
            return Ok(());
        }

        let me = Arc::clone(self);
        let user = sc_user_id.to_string();
        let tok = token.to_string();
        let extra = extra_params.to_vec();
        tokio::spawn(async move {
            if let Err(e) = me.refresh_collection(coll, &user, &tok, &extra).await {
                debug!(error = %e, user = %user, path = %coll.sc_path, "background refresh failed");
            }
        });
        Ok(())
    }

    /// Полный refresh per-user коллекции из SC. Тянет все страницы, UPSERT'ит
    /// shared cache + mirror'ы. Локальные строки, которых нет в SC и которые
    /// не pending — удаляются (SC побеждает). Pending строки (progress=true
    /// или wanted_state=false) — не трогаем: пусть sync_queue разберётся.
    pub async fn refresh_collection(
        &self,
        coll: UserCollection,
        sc_user_id: &str,
        token: &str,
        extra_params: &[(String, String)],
    ) -> AppResult<()> {
        let key = format!("refresh:{}:{sc_user_id}", coll.lock_kind);
        let Some(_lock) = self.try_lock(&key).await? else {
            return Ok(());
        };
        let _permit = self.sem.acquire().await.ok();
        let items = self
            .fetch_all_pages(coll.sc_path, token, extra_params)
            .await?;

        // SC отдаёт новые записи первыми; разворачиваем в (старые→новые) порядок,
        // чтобы `created_at` рос в нашем порядке и `ORDER BY created_at DESC`
        // потом отдавал новые сверху.
        let mut ordered: Vec<(String, &Value)> = Vec::with_capacity(items.len());
        for item in items.iter().rev() {
            let Some(urn) = item.get("urn").and_then(|v| v.as_str()) else {
                continue;
            };
            if urn.is_empty() {
                continue;
            }
            let key_value = match coll.shared_cache {
                SharedCache::Tracks => extract_sc_id(urn).to_string(),
                SharedCache::Playlists | SharedCache::Users => urn.to_string(),
            };
            ordered.push((key_value, item));
        }

        let seen: Vec<String> = ordered.iter().map(|(k, _)| k.clone()).collect();

        // Чанковые bulk UPSERT'ы: на юзере с 10к лайков это 10к/REFRESH_BATCH_SIZE
        // транзакций вместо 10к. fsync per-commit, lock contention в shared
        // таблицах — минимальные.
        for chunk in ordered.chunks(REFRESH_BATCH_SIZE) {
            let mut shared_keys: Vec<String> = Vec::with_capacity(chunk.len());
            let mut shared_payloads: Vec<Value> = Vec::with_capacity(chunk.len());
            let mut mirror_keys: Vec<String> = Vec::with_capacity(chunk.len());
            let mut mirror_payloads: Vec<Value> = Vec::with_capacity(chunk.len());

            for (k, item) in chunk {
                if !coll.public_only_to_shared || is_public(item) {
                    shared_keys.push(k.clone());
                    shared_payloads.push((*item).clone());
                }
                mirror_keys.push(k.clone());
                mirror_payloads.push((*item).clone());
            }

            let mut tx = self.pg.begin().await?;
            if !shared_keys.is_empty() {
                batch_upsert_shared_cache(
                    &mut tx,
                    coll.shared_cache,
                    &shared_keys,
                    &shared_payloads,
                )
                .await?;
            }
            if !mirror_keys.is_empty() {
                batch_upsert_mirror(&mut tx, &coll, sc_user_id, &mirror_keys, &mirror_payloads)
                    .await?;
            }
            tx.commit().await?;
        }

        if !items.is_empty() {
            delete_orphans(&self.pg, &coll, sc_user_id, &seen).await?;
        }
        Ok(())
    }

    pub async fn refresh_track(&self, track_urn: &str, token: &str) -> AppResult<()> {
        let key = format!("refresh:track:{track_urn}");
        let Some(_lock) = self.try_lock(&key).await? else {
            return Ok(());
        };
        let _permit = self.sem.acquire().await.ok();
        let fetched: Value = self
            .sc
            .api_get_value(&format!("/tracks/{track_urn}"), token, None)
            .await?;
        let sc_track_id = extract_sc_id(track_urn);
        upsert_track_cache(&self.pg, sc_track_id, &fetched).await?;
        debug!(urn = %track_urn, "track refreshed");
        Ok(())
    }

    pub async fn refresh_user(&self, user_urn: &str, token: &str) -> AppResult<()> {
        let key = format!("refresh:user:{user_urn}");
        let Some(_lock) = self.try_lock(&key).await? else {
            return Ok(());
        };
        let _permit = self.sem.acquire().await.ok();
        let fetched: Value = self
            .sc
            .api_get_value(&format!("/users/{user_urn}"), token, None)
            .await?;
        upsert_user_cache(&self.pg, user_urn, &fetched).await?;
        debug!(urn = %user_urn, "user refreshed");
        Ok(())
    }

    pub async fn refresh_playlist(&self, playlist_urn: &str, token: &str) -> AppResult<()> {
        let key = format!("refresh:playlist:{playlist_urn}");
        let Some(_lock) = self.try_lock(&key).await? else {
            return Ok(());
        };
        let _permit = self.sem.acquire().await.ok();
        let fetched: Value = self
            .sc
            .api_get_value(&format!("/playlists/{playlist_urn}"), token, None)
            .await?;
        upsert_playlist_cache(&self.pg, playlist_urn, &fetched).await?;
        debug!(urn = %playlist_urn, "playlist refreshed");
        Ok(())
    }

    async fn fetch_all_pages(
        &self,
        path: &str,
        token: &str,
        extra_params: &[(String, String)],
    ) -> AppResult<Vec<Value>> {
        let mut cursor: Option<String> = None;
        let mut acc: Vec<Value> = Vec::new();
        loop {
            let mut params: Vec<(String, String)> = Vec::with_capacity(2 + extra_params.len());
            params.extend(extra_params.iter().cloned());
            params.push(("limit".into(), "200".into()));
            params.push(("linked_partitioning".into(), "true".into()));
            if let Some(c) = &cursor {
                params.push(("cursor".into(), c.clone()));
            }
            let resp: Value = self.sc.api_get_value(path, token, Some(&params)).await?;
            let items: Vec<Value> = resp
                .get("collection")
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            if items.is_empty() {
                break;
            }
            acc.extend(items);
            let Some(next) = resp
                .get("next_href")
                .and_then(|v| v.as_str())
                .map(String::from)
            else {
                break;
            };
            match extract_sc_cursor(Some(&next)) {
                Some(c) if Some(&c) != cursor.as_ref() => cursor = Some(c),
                _ => break,
            }
        }
        Ok(acc)
    }

    async fn try_lock(&self, key: &str) -> AppResult<Option<()>> {
        let acquired = self
            .cache
            .try_acquire_lock(key, self.cfg.refresh_lock_ttl_sec)
            .await?;
        Ok(if acquired { Some(()) } else { None })
    }

    /// Cron-петля очистки давно нечитанных shared-entity-кешей: cached_users
    /// и cached_playlists. indexed_tracks НЕ трогаем — на ней живёт enrich
    /// pipeline + наши собственные сущности (artists/albums).
    pub fn spawn_evict_loop(self: Arc<Self>, shutdown: CancellationToken) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(EVICT_TICK);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = ticker.tick() => {
                        if let Err(e) = self.evict_once().await {
                            warn!(error = %e, "cold_refresh evict failed");
                        }
                    }
                }
            }
        });
    }

    async fn evict_once(&self) -> AppResult<()> {
        let cutoff = Utc::now() - chrono::Duration::seconds(self.cfg.evict_after_sec as i64);
        let users_n = evict_chunked(
            &self.pg,
            "DELETE FROM cached_users WHERE user_urn IN ( \
                 SELECT user_urn FROM cached_users \
                 WHERE last_read_at IS NOT NULL AND last_read_at < $1 \
                 LIMIT $2 \
             )",
            cutoff,
        )
        .await?;
        let playlists_n = evict_chunked(
            &self.pg,
            "DELETE FROM cached_playlists WHERE playlist_urn IN ( \
                 SELECT playlist_urn FROM cached_playlists \
                 WHERE last_read_at IS NOT NULL AND last_read_at < $1 \
                 LIMIT $2 \
             )",
            cutoff,
        )
        .await?;
        // cached_playlist_tracks — справочник позиций. После эвикции родителей
        // подбираем оставшихся «сирот» теми же чанками: ANTI JOIN дешевле
        // NOT IN на больших наборах.
        let tracks_n = evict_orphan_playlist_tracks(&self.pg).await?;
        if users_n > 0 || playlists_n > 0 || tracks_n > 0 {
            info!(
                users = users_n,
                playlists = playlists_n,
                playlist_tracks = tracks_n,
                "cold_refresh evicted stale cache rows"
            );
        }
        Ok(())
    }
}

/// Chunked-DELETE с короткой паузой между итерациями. Защита от длинных
/// эксклюзивных блокировок на cached_*-таблицах при большом cutoff'е.
async fn evict_chunked(pg: &PgPool, sql: &str, cutoff: DateTime<Utc>) -> AppResult<u64> {
    let mut total: u64 = 0;
    loop {
        let n = sqlx::query(sql)
            .bind(cutoff)
            .bind(EVICT_CHUNK_SIZE)
            .execute(pg)
            .await?
            .rows_affected();
        total += n;
        if (n as i64) < EVICT_CHUNK_SIZE {
            break;
        }
        tokio::time::sleep(Duration::from_millis(EVICT_BETWEEN_MS)).await;
    }
    Ok(total)
}

async fn evict_orphan_playlist_tracks(pg: &PgPool) -> AppResult<u64> {
    let mut total: u64 = 0;
    loop {
        let n = sqlx::query(
            "DELETE FROM cached_playlist_tracks \
             WHERE (playlist_urn, position) IN ( \
                 SELECT cpt.playlist_urn, cpt.position FROM cached_playlist_tracks cpt \
                 LEFT JOIN cached_playlists cp ON cp.playlist_urn = cpt.playlist_urn \
                 WHERE cp.playlist_urn IS NULL \
                 LIMIT $1 \
             )",
        )
        .bind(EVICT_CHUNK_SIZE)
        .execute(pg)
        .await?
        .rows_affected();
        total += n;
        if (n as i64) < EVICT_CHUNK_SIZE {
            break;
        }
        tokio::time::sleep(Duration::from_millis(EVICT_BETWEEN_MS)).await;
    }
    Ok(total)
}

fn is_stale(synced_at: Option<DateTime<Utc>>, ttl_sec: u64) -> bool {
    match synced_at {
        None => true,
        Some(t) => {
            let age = Utc::now().signed_duration_since(t).num_seconds();
            age < 0 || age as u64 > ttl_sec
        }
    }
}

async fn batch_upsert_shared_cache(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    kind: SharedCache,
    keys: &[String],
    payloads: &[Value],
) -> AppResult<()> {
    let sql = match kind {
        SharedCache::Tracks => {
            "INSERT INTO indexed_tracks (sc_track_id, raw_sc_data, synced_at) \
             SELECT k, p, now() \
             FROM UNNEST($1::text[], $2::jsonb[]) AS t(k, p) \
             ON CONFLICT (sc_track_id) DO UPDATE SET \
                 raw_sc_data = EXCLUDED.raw_sc_data, synced_at = now()"
        }
        SharedCache::Playlists => {
            "INSERT INTO cached_playlists (playlist_urn, payload, synced_at) \
             SELECT k, p, now() \
             FROM UNNEST($1::text[], $2::jsonb[]) AS t(k, p) \
             ON CONFLICT (playlist_urn) DO UPDATE SET \
                 payload = EXCLUDED.payload, synced_at = now()"
        }
        SharedCache::Users => {
            "INSERT INTO cached_users (user_urn, payload, synced_at) \
             SELECT k, p, now() \
             FROM UNNEST($1::text[], $2::jsonb[]) AS t(k, p) \
             ON CONFLICT (user_urn) DO UPDATE SET \
                 payload = EXCLUDED.payload, synced_at = now()"
        }
    };
    sqlx::query(sql)
        .bind(keys)
        .bind(payloads)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn batch_upsert_mirror(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    coll: &UserCollection,
    sc_user_id: &str,
    keys: &[String],
    payloads: &[Value],
) -> AppResult<()> {
    let key_col = coll.mirror_key_col;
    let table = coll.mirror_table;

    // Источники значений в SELECT-проекции и блок ON CONFLICT — три варианта
    // mirror-форм (owned-с-payload, likes/followings, плейн-owned). Имена
    // подставляются через format!() из &'static str — user-input в SQL не
    // попадает.
    let (select_cols, update_set) = if let Some(p) = coll.mirror_payload_col {
        (
            format!("$1, t.k, t.p, false, now()"),
            format!("{p} = EXCLUDED.{p}, synced_at = now()"),
        )
    } else if coll.has_wanted_state {
        (
            "$1, t.k, true, false, now()".to_string(),
            "synced_at = now()".to_string(),
        )
    } else {
        (
            "$1, t.k, false, now()".to_string(),
            "synced_at = now()".to_string(),
        )
    };

    let insert_cols = if coll.mirror_payload_col.is_some() {
        format!(
            "user_id, {key_col}, {}, progress, synced_at",
            coll.mirror_payload_col.unwrap()
        )
    } else if coll.has_wanted_state {
        format!("user_id, {key_col}, wanted_state, progress, synced_at")
    } else {
        format!("user_id, {key_col}, progress, synced_at")
    };

    let from_clause = if coll.mirror_payload_col.is_some() {
        "FROM UNNEST($2::text[], $3::jsonb[]) AS t(k, p)"
    } else {
        "FROM UNNEST($2::text[]) AS t(k)"
    };

    let guard_clause = if let Some(g) = coll.guard_pending_delete_action {
        // owned + pending delete защита: не воскрешаем строку, которую юзер
        // уже удалил, но воркер sync_queue ещё не отправил в SC.
        format!(
            "WHERE NOT EXISTS ( \
                 SELECT 1 FROM sync_queue \
                 WHERE user_id = $1 AND action_type = '{g}' AND target_urn = t.k \
             )"
        )
    } else {
        String::new()
    };

    let sql = format!(
        "INSERT INTO {table} ({insert_cols}) \
         SELECT {select_cols} {from_clause} {guard_clause} \
         ON CONFLICT (user_id, {key_col}) DO UPDATE SET {update_set}"
    );

    let q = sqlx::query(&sql).bind(sc_user_id).bind(keys);
    if coll.mirror_payload_col.is_some() {
        q.bind(payloads).execute(&mut **tx).await?;
    } else {
        q.execute(&mut **tx).await?;
    }
    Ok(())
}

fn is_public(item: &Value) -> bool {
    item.get("sharing").and_then(|v| v.as_str()) == Some("public")
}

async fn delete_orphans(
    pg: &PgPool,
    coll: &UserCollection,
    sc_user_id: &str,
    seen: &[String],
) -> AppResult<()> {
    let extra_filter = if coll.has_wanted_state {
        "AND wanted_state = true AND progress = false"
    } else {
        "AND progress = false"
    };
    let sql = format!(
        "DELETE FROM {table} \
         WHERE user_id = $1 {extra_filter} \
           AND NOT ({key_col} = ANY($2))",
        table = coll.mirror_table,
        key_col = coll.mirror_key_col,
    );
    sqlx::query(&sql)
        .bind(sc_user_id)
        .bind(seen)
        .execute(pg)
        .await?;
    Ok(())
}

pub async fn upsert_track_cache(pg: &PgPool, sc_track_id: &str, raw: &Value) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO indexed_tracks (sc_track_id, raw_sc_data, synced_at, last_read_at) \
         VALUES ($1, $2, now(), now()) \
         ON CONFLICT (sc_track_id) DO UPDATE SET \
             raw_sc_data = EXCLUDED.raw_sc_data, synced_at = now(), last_read_at = now()",
    )
    .bind(sc_track_id)
    .bind(raw)
    .execute(pg)
    .await?;
    Ok(())
}

pub async fn upsert_playlist_cache(
    pg: &PgPool,
    playlist_urn: &str,
    payload: &Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO cached_playlists (playlist_urn, payload, synced_at, last_read_at) \
         VALUES ($1, $2, now(), now()) \
         ON CONFLICT (playlist_urn) DO UPDATE SET \
             payload = EXCLUDED.payload, synced_at = now(), last_read_at = now()",
    )
    .bind(playlist_urn)
    .bind(payload)
    .execute(pg)
    .await?;
    Ok(())
}

pub async fn upsert_user_cache(pg: &PgPool, user_urn: &str, payload: &Value) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO cached_users (user_urn, payload, synced_at, last_read_at) \
         VALUES ($1, $2, now(), now()) \
         ON CONFLICT (user_urn) DO UPDATE SET \
             payload = EXCLUDED.payload, synced_at = now(), last_read_at = now()",
    )
    .bind(user_urn)
    .bind(payload)
    .execute(pg)
    .await?;
    Ok(())
}
