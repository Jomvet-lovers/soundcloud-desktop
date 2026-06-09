//! Нормализованные `playlists` + `playlist_tracks` (без raw payload).

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Map, Value};
use sqlx::FromRow;
use sqlx::PgPool;

use crate::common::release_date;
use crate::common::sc_payload::{parse_dt, parse_id_or_string, string_field};
use crate::error::{AppError, AppResult};
use crate::modules::tracks::normalize::normalize_title;

#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct PlaylistRow {
    pub urn: String,
    pub sc_playlist_id: String,
    pub title: String,
    pub title_normalized: String,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub tags: Vec<String>,
    pub artwork_url: Option<String>,
    pub permalink_url: Option<String>,
    pub owner_sc_user_id: Option<String>,
    pub owner_urn: Option<String>,
    pub owner_username: Option<String>,
    pub track_count: i32,
    pub duration_ms: Option<i64>,
    pub playlist_type: Option<String>,
    pub kind: Option<String>,
    pub sharing: String,
    pub release_year: Option<i16>,
    pub release_date: Option<NaiveDate>,
    pub label_name: Option<String>,
    pub likes_count_sc: Option<i64>,
    pub reposts_count_sc: Option<i64>,
    pub sc_created_at: Option<DateTime<Utc>>,
    pub sc_last_modified: Option<DateTime<Utc>>,
    pub tracks_synced_at: Option<DateTime<Utc>>,
    pub sc_synced_at: DateTime<Utc>,
    pub last_read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub desired_rev: i64,
    pub synced_rev: i64,
}

pub struct PlaylistRepository {
    pg: PgPool,
}

impl PlaylistRepository {
    pub fn new(pg: PgPool) -> Self {
        Self { pg }
    }

    pub async fn find_by_urn(&self, urn: &str) -> AppResult<Option<PlaylistRow>> {
        let row = sqlx::query_file_as!(PlaylistRow, "queries/playlists/find_by_urn.sql", urn)
            .fetch_optional(&self.pg)
            .await?;
        Ok(row)
    }

    pub async fn touch_last_read(&self, urn: &str) -> AppResult<()> {
        sqlx::query_file!("queries/playlists/touch_last_read.sql", urn)
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    /// UPSERT playlist-метаданных из SC payload. Возвращает true если строка
    /// только что создана.
    pub async fn upsert_from_sc(&self, payload: &Value) -> AppResult<bool> {
        let Some(fields) = ScPlaylistFields::from_sc(payload) else {
            return Ok(false);
        };
        let row: (bool,) = sqlx::query_as(
            "INSERT INTO playlists (
                urn, sc_playlist_id, title, title_normalized, description, genre, tags,
                artwork_url, permalink_url, owner_sc_user_id, owner_urn, owner_username,
                track_count, duration_ms, playlist_type, kind, sharing,
                release_year, release_date, label_name, likes_count_sc, reposts_count_sc,
                sc_created_at, sc_last_modified, sc_synced_at
             ) VALUES (
                $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24, now()
             )
             ON CONFLICT (urn) DO UPDATE SET
                sc_playlist_id = EXCLUDED.sc_playlist_id,
                title = EXCLUDED.title,
                title_normalized = EXCLUDED.title_normalized,
                description = EXCLUDED.description,
                genre = EXCLUDED.genre,
                tags = EXCLUDED.tags,
                artwork_url = EXCLUDED.artwork_url,
                permalink_url = EXCLUDED.permalink_url,
                owner_sc_user_id = COALESCE(EXCLUDED.owner_sc_user_id, playlists.owner_sc_user_id),
                owner_urn = COALESCE(EXCLUDED.owner_urn, playlists.owner_urn),
                owner_username = COALESCE(EXCLUDED.owner_username, playlists.owner_username),
                track_count = EXCLUDED.track_count,
                duration_ms = COALESCE(EXCLUDED.duration_ms, playlists.duration_ms),
                playlist_type = COALESCE(EXCLUDED.playlist_type, playlists.playlist_type),
                kind = COALESCE(EXCLUDED.kind, playlists.kind),
                sharing = EXCLUDED.sharing,
                release_year = COALESCE(EXCLUDED.release_year, playlists.release_year),
                release_date = COALESCE(EXCLUDED.release_date, playlists.release_date),
                label_name = COALESCE(EXCLUDED.label_name, playlists.label_name),
                likes_count_sc = COALESCE(EXCLUDED.likes_count_sc, playlists.likes_count_sc),
                reposts_count_sc = COALESCE(EXCLUDED.reposts_count_sc, playlists.reposts_count_sc),
                sc_created_at = COALESCE(EXCLUDED.sc_created_at, playlists.sc_created_at),
                sc_last_modified = COALESCE(EXCLUDED.sc_last_modified, playlists.sc_last_modified),
                sc_synced_at = now(),
                updated_at = now()
             RETURNING (xmax = 0) AS was_new",
        )
        .bind(&fields.urn)
        .bind(&fields.sc_playlist_id)
        .bind(&fields.title)
        .bind(&fields.title_normalized)
        .bind(&fields.description)
        .bind(&fields.genre)
        .bind(&fields.tags)
        .bind(&fields.artwork_url)
        .bind(&fields.permalink_url)
        .bind(&fields.owner_sc_user_id)
        .bind(&fields.owner_urn)
        .bind(&fields.owner_username)
        .bind(fields.track_count)
        .bind(fields.duration_ms)
        .bind(&fields.playlist_type)
        .bind(&fields.kind)
        .bind(&fields.sharing)
        .bind(fields.release_year)
        .bind(fields.release_date)
        .bind(&fields.label_name)
        .bind(fields.likes_count_sc)
        .bind(fields.reposts_count_sc)
        .bind(fields.sc_created_at)
        .bind(fields.sc_last_modified)
        .fetch_one(&self.pg)
        .await?;
        Ok(row.0)
    }

    /// Атомарная замена track-list плейлиста: DELETE по playlist_urn + bulk
    /// INSERT новой расстановки в одной транзакции. Используется
    /// ingest-цепочкой после того как все страницы /playlists/{urn}/tracks
    /// собраны (нельзя класть инкрементально — мы потеряем reorder'ы).
    ///
    /// Tx-level advisory lock по `playlist_urn` сериализует параллельные
    /// refresh'ы одного плейлиста (Redis SETNX в cold_refresh защищает
    /// большинство случаев, но при cross-instance race с разными Redis-нодами
    /// окно остаётся). Lock освобождается на commit/rollback.
    pub async fn replace_tracks(
        &self,
        playlist_urn: &str,
        ordered_sc_track_ids: &[String],
    ) -> AppResult<()> {
        let mut tx = self.pg.begin().await?;
        let lock_key = format!("playlist_tracks:{playlist_urn}");
        sqlx::query_file!("queries/playlists/lock_tracks.sql", lock_key)
            .execute(&mut *tx)
            .await?;
        // Local-first гейт: pending локальная правка (desired_rev > synced_rev) —
        // SC-pull НЕ перетирает desired-state, наше побеждает. Для non-owned /
        // в-синке плейлистов (desired_rev == synced_rev) — обычный replace.
        let pending =
            sqlx::query_file_scalar!("queries/playlists/has_pending_intent.sql", playlist_urn)
                .fetch_optional(&mut *tx)
                .await?;
        if pending.unwrap_or(false) {
            tx.rollback().await?;
            return Ok(());
        }
        sqlx::query_file!("queries/playlists/delete_tracks.sql", playlist_urn)
            .execute(&mut *tx)
            .await?;
        if !ordered_sc_track_ids.is_empty() {
            let positions: Vec<i32> = (0..ordered_sc_track_ids.len() as i32).collect();
            let playlist_urns: Vec<&str> = (0..ordered_sc_track_ids.len())
                .map(|_| playlist_urn)
                .collect();
            sqlx::query_file!(
                "queries/playlists/insert_tracks.sql",
                &playlist_urns as &[&str],
                &positions,
                ordered_sc_track_ids
            )
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query_file!(
            "queries/playlists/set_track_count.sql",
            playlist_urn,
            ordered_sc_track_ids.len() as i32
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn page_track_ids(
        &self,
        playlist_urn: &str,
        offset: i64,
        limit: i64,
    ) -> AppResult<Vec<String>> {
        let rows = sqlx::query_file_scalar!(
            "queries/playlists/page_track_ids.sql",
            playlist_urn,
            offset,
            limit
        )
        .fetch_all(&self.pg)
        .await?;
        Ok(rows)
    }

    /// Все sc_track_id плейлиста по порядку (без пагинации) — для sync-экшена.
    pub async fn all_track_ids(&self, playlist_urn: &str) -> AppResult<Vec<String>> {
        let ids = sqlx::query_file_scalar!("queries/playlists/all_track_ids.sql", playlist_urn)
            .fetch_all(&self.pg)
            .await?;
        Ok(ids)
    }

    /// Owner-guard для write-операций. `me_bare` = extract_sc_id(session urn).
    /// Истина владения: user_owned_playlists (сидится /me/playlists reconcile)
    /// ИЛИ playlists.owner_sc_user_id (bare). Любой источник подтверждает.
    pub async fn assert_owner(&self, me_bare: &str, urn: &str) -> AppResult<()> {
        // user_owned_playlists.user_id на проде расщеплён URN/bare → матчим оба
        // ($3). owner_sc_user_id — bare entity-колонка, остаётся $1.
        let variants = crate::common::sc_ids::user_id_variants(me_bare);
        let owns = sqlx::query_file_scalar!(
            "queries/playlists/assert_owner.sql",
            me_bare,
            urn,
            &variants
        )
        .fetch_one(&self.pg)
        .await?;
        if !owns {
            return Err(AppError::not_found("Playlist not found"));
        }
        Ok(())
    }

    /// Применяет мутацию membership к DESIRED-состоянию в одной транзакции:
    /// advisory-lock (сериализует против SC-refresh) → читает текущий порядок →
    /// `transform` (server-computed delta) → DELETE+bulk INSERT нового порядка →
    /// desired_rev++ → `(new_desired_rev, new_ordered_ids)`. SC не трогает.
    async fn mutate_desired<F>(&self, urn: &str, transform: F) -> AppResult<(i64, Vec<String>)>
    where
        F: FnOnce(Vec<String>) -> Vec<String>,
    {
        let mut tx = self.pg.begin().await?;
        let lock_key = format!("playlist_tracks:{urn}");
        sqlx::query_file!("queries/playlists/lock_tracks.sql", lock_key)
            .execute(&mut *tx)
            .await?;
        let current = sqlx::query_file_scalar!("queries/playlists/all_track_ids.sql", urn)
            .fetch_all(&mut *tx)
            .await?;
        let next = transform(current);
        sqlx::query_file!("queries/playlists/delete_tracks.sql", urn)
            .execute(&mut *tx)
            .await?;
        if !next.is_empty() {
            let positions: Vec<i32> = (0..next.len() as i32).collect();
            let urns: Vec<&str> = std::iter::repeat(urn).take(next.len()).collect();
            sqlx::query_file!(
                "queries/playlists/insert_tracks.sql",
                &urns as &[&str],
                &positions,
                &next
            )
            .execute(&mut *tx)
            .await?;
        }
        let new_rev = sqlx::query_file_scalar!(
            "queries/playlists/bump_desired_rev.sql",
            urn,
            next.len() as i32
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((new_rev, next))
    }

    /// Добавить трек в конец (dedup — у SC нет дублей membership).
    pub async fn add_track(&self, urn: &str, sc_id: String) -> AppResult<(i64, Vec<String>)> {
        self.mutate_desired(urn, move |mut ids| {
            if !ids.iter().any(|t| t == &sc_id) {
                ids.push(sc_id);
            }
            ids
        })
        .await
    }

    pub async fn remove_track(&self, urn: &str, sc_id: String) -> AppResult<(i64, Vec<String>)> {
        self.mutate_desired(urn, move |ids| {
            ids.into_iter().filter(|t| t != &sc_id).collect()
        })
        .await
    }

    /// Переместить существующий трек на индекс `to` (clamp). No-op если отсутствует.
    pub async fn move_track(
        &self,
        urn: &str,
        sc_id: String,
        to: usize,
    ) -> AppResult<(i64, Vec<String>)> {
        self.mutate_desired(urn, move |mut ids| {
            if let Some(from) = ids.iter().position(|t| t == &sc_id) {
                let v = ids.remove(from);
                let to = to.min(ids.len());
                ids.insert(to, v);
            }
            ids
        })
        .await
    }

    /// Полная перестановка из свежей вью (dedup, сохраняя первое вхождение).
    pub async fn set_order(
        &self,
        urn: &str,
        ordered: Vec<String>,
    ) -> AppResult<(i64, Vec<String>)> {
        self.mutate_desired(urn, move |_old| dedup_keep_first(ordered))
            .await
    }

    /// merge: submitted-порядок (дедуп) вперёд, затем current-only id, которых
    /// клиент не знал — устаревшая клиентская вью не дропает треки.
    pub async fn merge_order(
        &self,
        urn: &str,
        submitted: Vec<String>,
    ) -> AppResult<(i64, Vec<String>)> {
        self.mutate_desired(urn, move |current| {
            // submitted вперёд, затем current — dedup сохраняет первое вхождение.
            dedup_keep_first(submitted.into_iter().chain(current).collect())
        })
        .await
    }

    /// Снапшот для sync-экшена: (desired_rev, ordered ids). None если строки нет.
    pub async fn desired_snapshot(&self, urn: &str) -> AppResult<Option<(i64, Vec<String>)>> {
        let rev = sqlx::query_file_scalar!("queries/playlists/desired_rev_by_urn.sql", urn)
            .fetch_optional(&self.pg)
            .await?;
        let Some(rev) = rev else {
            return Ok(None);
        };
        let ids = self.all_track_ids(urn).await?;
        Ok(Some((rev, ids)))
    }

    /// На SC-ack: synced_rev = pushed_rev ТОЛЬКО если desired_rev не ушёл вперёд
    /// под нами. `true` = зафиксировали (конкурентной правки не было).
    pub async fn mark_synced_if_unchanged(&self, urn: &str, pushed_rev: i64) -> AppResult<bool> {
        let r = sqlx::query_file!(
            "queries/playlists/mark_synced_if_unchanged.sql",
            urn,
            pushed_rev
        )
        .execute(&self.pg)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Есть ли pending локальная правка (desired_rev > synced_rev)?
    pub async fn has_pending_intent(&self, urn: &str) -> AppResult<bool> {
        let pending = sqlx::query_file_scalar!("queries/playlists/has_pending_intent.sql", urn)
            .fetch_optional(&self.pg)
            .await?;
        Ok(pending.unwrap_or(false))
    }

    /// URN-ы плейлистов юзера с pending-intent — reconcile их пропускает.
    pub async fn pending_owned_urns(&self, owner_bare: &str) -> AppResult<Vec<String>> {
        let urns = sqlx::query_file_scalar!("queries/playlists/pending_owned_urns.sql", owner_bare)
            .fetch_all(&self.pg)
            .await?;
        Ok(urns)
    }
}

fn dedup_keep_first(items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

struct ScPlaylistFields {
    urn: String,
    sc_playlist_id: String,
    title: String,
    title_normalized: String,
    description: Option<String>,
    genre: Option<String>,
    tags: Vec<String>,
    artwork_url: Option<String>,
    permalink_url: Option<String>,
    owner_sc_user_id: Option<String>,
    owner_urn: Option<String>,
    owner_username: Option<String>,
    track_count: i32,
    duration_ms: Option<i64>,
    playlist_type: Option<String>,
    kind: Option<String>,
    sharing: String,
    release_year: Option<i16>,
    release_date: Option<NaiveDate>,
    label_name: Option<String>,
    likes_count_sc: Option<i64>,
    reposts_count_sc: Option<i64>,
    sc_created_at: Option<DateTime<Utc>>,
    sc_last_modified: Option<DateTime<Utc>>,
}

impl ScPlaylistFields {
    fn from_sc(payload: &Value) -> Option<Self> {
        let urn = payload.get("urn").and_then(|v| v.as_str())?.to_string();
        if urn.is_empty() {
            return None;
        }
        let sc_playlist_id = crate::common::sc_ids::extract_sc_id(&urn).to_string();
        let title = payload
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if title.is_empty() {
            return None;
        }
        let title_normalized = normalize_title(&title);

        let description = string_field(payload, "description");
        let genre = string_field(payload, "genre");
        let tag_list = payload
            .get("tag_list")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let tags = tag_list
            .split_whitespace()
            .map(String::from)
            .filter(|s| !s.is_empty())
            .collect();

        let artwork_url = string_field(payload, "artwork_url");
        let permalink_url = string_field(payload, "permalink_url");

        let owner = payload.get("user");
        let owner_urn = owner
            .and_then(|u| u.get("urn"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let owner_sc_user_id = owner_urn
            .as_deref()
            .map(|u| crate::common::sc_ids::extract_sc_id(u).to_string())
            .or_else(|| {
                owner
                    .and_then(|u| u.get("id"))
                    .and_then(|v| v.as_i64())
                    .map(|i| i.to_string())
            });
        let owner_username = owner
            .and_then(|u| u.get("username"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let track_count = payload
            .get("track_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let duration_ms = payload.get("duration").and_then(|v| v.as_i64());
        let playlist_type = string_field(payload, "playlist_type");
        let kind = string_field(payload, "kind");
        let sharing = string_field(payload, "sharing").unwrap_or_else(|| "public".into());
        let label_name = string_field(payload, "label_name");
        let likes_count_sc = payload.get("likes_count").and_then(|v| v.as_i64());
        let reposts_count_sc = payload
            .get("reposts_count")
            .or_else(|| payload.get("repost_count"))
            .and_then(|v| v.as_i64());

        let (release_year, release_date) = release_date::extract(payload);
        let sc_created_at = parse_dt(payload.get("created_at"));
        let sc_last_modified = parse_dt(payload.get("last_modified"));

        Some(Self {
            urn,
            sc_playlist_id,
            title,
            title_normalized,
            description,
            genre,
            tags,
            artwork_url,
            permalink_url,
            owner_sc_user_id,
            owner_urn,
            owner_username,
            track_count,
            duration_ms,
            playlist_type,
            kind,
            sharing,
            release_year,
            release_date,
            label_name,
            likes_count_sc,
            reposts_count_sc,
            sc_created_at,
            sc_last_modified,
        })
    }
}

/// Проекция в SC-shape v1 playlist payload. owner — опциональный объект user
/// (заполняется JOIN'ом users; иначе из денорма колонок).
pub fn project_to_sc_shape(row: &PlaylistRow, owner: Option<&Value>) -> Value {
    let mut obj = Map::new();
    obj.insert(
        "kind".into(),
        Value::String(row.kind.clone().unwrap_or_else(|| "playlist".into())),
    );
    obj.insert("urn".into(), Value::String(row.urn.clone()));
    obj.insert("id".into(), parse_id_or_string(&row.sc_playlist_id));
    obj.insert("title".into(), Value::String(row.title.clone()));
    if let Some(d) = &row.description {
        obj.insert("description".into(), Value::String(d.clone()));
    }
    if let Some(g) = &row.genre {
        obj.insert("genre".into(), Value::String(g.clone()));
    }
    obj.insert("tag_list".into(), Value::String(row.tags.join(" ")));
    if let Some(a) = &row.artwork_url {
        obj.insert("artwork_url".into(), Value::String(a.clone()));
    }
    if let Some(p) = &row.permalink_url {
        obj.insert("permalink_url".into(), Value::String(p.clone()));
    }
    obj.insert("track_count".into(), json!(row.track_count));
    if let Some(d) = row.duration_ms {
        obj.insert("duration".into(), json!(d));
    }
    obj.insert(
        "likes_count".into(),
        row.likes_count_sc.map(|v| json!(v)).unwrap_or(Value::Null),
    );
    obj.insert(
        "reposts_count".into(),
        row.reposts_count_sc
            .map(|v| json!(v))
            .unwrap_or(Value::Null),
    );
    if let Some(p) = &row.playlist_type {
        obj.insert("playlist_type".into(), Value::String(p.clone()));
    }
    obj.insert("sharing".into(), Value::String(row.sharing.clone()));
    if let Some(y) = row.release_year {
        obj.insert("release_year".into(), json!(y));
    }
    if let Some(d) = row.release_date {
        obj.insert("release_date".into(), Value::String(d.to_string()));
    }
    if let Some(l) = &row.label_name {
        obj.insert("label_name".into(), Value::String(l.clone()));
    }
    if let Some(t) = row.sc_created_at {
        obj.insert("created_at".into(), Value::String(t.to_rfc3339()));
    }
    if let Some(t) = row.sc_last_modified {
        obj.insert("last_modified".into(), Value::String(t.to_rfc3339()));
    }

    let owner_val = owner.cloned().unwrap_or_else(|| {
        let mut u = Map::new();
        u.insert("kind".into(), Value::String("user".into()));
        if let Some(id) = &row.owner_sc_user_id {
            u.insert("id".into(), parse_id_or_string(id));
        }
        if let Some(urn) = &row.owner_urn {
            u.insert("urn".into(), Value::String(urn.clone()));
        }
        if let Some(n) = &row.owner_username {
            u.insert("username".into(), Value::String(n.clone()));
        }
        Value::Object(u)
    });
    obj.insert("user".into(), owner_val);

    Value::Object(obj)
}
