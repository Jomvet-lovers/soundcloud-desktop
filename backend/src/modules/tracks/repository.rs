//! CRUD над `tracks` + проекция в SC-shape JSON для read-path.
//!
//! Этот слой ничего не знает про NATS / transcode / qdrant — он только пишет
//! и читает Postgres. Кикинг пайплайнов на новый трек живёт в
//! [`crate::modules::indexing::IndexingService::ingest_track_from_sc`], который
//! композирует репозиторий и шину.

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Map, Value};
use sqlx::types::Json;
use sqlx::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use crate::common::sc_payload::parse_id_or_string;
use crate::error::AppResult;
use crate::modules::tracks::normalize::ScTrackFields;

/// Шкала pickup-приоритетов для индексации и storage-аплоада.
/// Меньше — раньше; синхронизирована с `tracks.{index_priority,storage_priority}`.
/// `Played`/`FreshDrop` пока не используются callers'ами, но зарезервированы
/// под events/discovery — оставлены для семантической полноты схемы.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum TrackPriority {
    Like = 1,
    Playlist = 2,
    Played = 3,
    FreshDrop = 4,
    Discovery = 5,
}

impl TrackPriority {
    pub fn as_i16(self) -> i16 {
        self as i16
    }
}

/// Результат UPSERT'а: id строки + true если строка только что создана
/// (через PostgreSQL idiom `xmax = 0` в RETURNING).
#[allow(dead_code)]
pub struct IngestResult {
    pub id: Uuid,
    pub was_new: bool,
}

/// Полная строка `tracks` в виде, удобном для read-path и воркеров.
/// Все Option-поля — те, которые SC может не отдать или которые заполняются
/// нашими пайплайнами уже после первого ingest'а.
#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct TrackRow {
    pub id: Uuid,
    pub sc_track_id: String,
    pub urn: String,

    pub title: String,
    pub title_normalized: String,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub tags: Vec<String>,
    pub duration_ms: i32,
    pub artwork_url: Option<String>,
    pub permalink_url: Option<String>,
    pub waveform_url: Option<String>,
    pub language: Option<String>,
    pub language_confidence: Option<f32>,
    pub isrc: Option<String>,
    pub metadata_artist: Option<String>,
    pub sharing: String,
    pub sc_created_at: Option<DateTime<Utc>>,
    pub sc_last_modified: Option<DateTime<Utc>>,
    pub release_year: Option<i16>,
    pub release_date: Option<NaiveDate>,

    pub uploader_sc_user_id: Option<String>,
    pub uploader_urn: Option<String>,
    pub uploader_username: Option<String>,
    pub uploader_avatar_url: Option<String>,

    pub primary_artist_id: Option<Uuid>,
    pub album_id: Option<Uuid>,
    pub album_position: Option<i16>,
    pub canonical_track_id: Option<Uuid>,
    pub cover_of_artist_id: Option<Uuid>,
    pub upload_kind: String,

    pub audio_fingerprint: Option<String>,
    pub quality_score: Option<f32>,
    pub play_count_sc: Option<i64>,
    pub likes_count_sc: Option<i64>,
    pub reposts_count_sc: Option<i64>,
    pub comments_count_sc: Option<i64>,

    pub enrich_state: String,
    pub enrich_attempts: i16,
    pub enrich_source: Option<String>,
    pub enrich_confidence: Option<f32>,
    pub enriched_at: Option<DateTime<Utc>>,

    pub index_state: String,
    pub index_priority: i16,
    pub index_attempts: i16,
    pub indexed_at: Option<DateTime<Utc>>,

    pub storage_state: String,
    pub storage_priority: i16,
    pub storage_quality: Option<String>,
    pub storage_attempts: i16,
    pub s3_verified_at: Option<DateTime<Utc>>,
    pub s3_missing_at: Option<DateTime<Utc>>,
    pub hq_upgrade_pending: bool,
    pub hq_upgrade_attempts: i16,
    pub hq_upgrade_last_at: Option<DateTime<Utc>>,

    pub needs_duration_resolve: bool,

    pub sc_synced_at: DateTime<Utc>,
    pub last_read_at: Option<DateTime<Utc>>,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct TrackRepository {
    pg: PgPool,
}

impl TrackRepository {
    pub fn new(pg: PgPool) -> Self {
        Self { pg }
    }

    /// UPSERT из SC payload. Сохраняет owned-поля (primary_artist_id, album_id,
    /// canonical_track_id, audio_fingerprint, *_state, *_at, *_priority) —
    /// они находятся под управлением enrich/indexing/storage пайплайнов и
    /// не должны затираться при каждом cold-refresh'е.
    ///
    /// Возвращает [`IngestResult`] с флагом `was_new`. true — это значит
    /// строка только что создана и каллер должен kick-нуть пайплайны.
    pub async fn upsert_from_sc(
        &self,
        fields: &ScTrackFields,
        new_index_priority: TrackPriority,
        new_storage_priority: TrackPriority,
    ) -> AppResult<IngestResult> {
        let row: (Uuid, bool) = sqlx::query_as(
            "INSERT INTO tracks (
                sc_track_id, urn, title, title_normalized, description, genre, tags,
                duration_ms, artwork_url, permalink_url, waveform_url, language, isrc,
                metadata_artist, sharing, sc_created_at, sc_last_modified, release_year, release_date,
                uploader_sc_user_id, uploader_urn, uploader_username, uploader_avatar_url,
                play_count_sc, likes_count_sc, reposts_count_sc, comments_count_sc,
                needs_duration_resolve, index_priority, storage_priority, sc_synced_at
             ) VALUES (
                $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,
                $20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30, now()
             )
             ON CONFLICT (sc_track_id) DO UPDATE SET
                urn = EXCLUDED.urn,
                title = EXCLUDED.title,
                title_normalized = EXCLUDED.title_normalized,
                description = EXCLUDED.description,
                genre = EXCLUDED.genre,
                tags = EXCLUDED.tags,
                duration_ms = CASE
                    WHEN EXCLUDED.duration_ms > 0 THEN EXCLUDED.duration_ms
                    ELSE tracks.duration_ms
                END,
                artwork_url = EXCLUDED.artwork_url,
                permalink_url = EXCLUDED.permalink_url,
                waveform_url = EXCLUDED.waveform_url,
                language = COALESCE(EXCLUDED.language, tracks.language),
                isrc = COALESCE(EXCLUDED.isrc, tracks.isrc),
                metadata_artist = COALESCE(EXCLUDED.metadata_artist, tracks.metadata_artist),
                sharing = EXCLUDED.sharing,
                sc_created_at = COALESCE(EXCLUDED.sc_created_at, tracks.sc_created_at),
                sc_last_modified = COALESCE(EXCLUDED.sc_last_modified, tracks.sc_last_modified),
                release_year = COALESCE(EXCLUDED.release_year, tracks.release_year),
                release_date = COALESCE(EXCLUDED.release_date, tracks.release_date),
                uploader_sc_user_id = COALESCE(EXCLUDED.uploader_sc_user_id, tracks.uploader_sc_user_id),
                uploader_urn = COALESCE(EXCLUDED.uploader_urn, tracks.uploader_urn),
                uploader_username = COALESCE(EXCLUDED.uploader_username, tracks.uploader_username),
                uploader_avatar_url = COALESCE(EXCLUDED.uploader_avatar_url, tracks.uploader_avatar_url),
                play_count_sc = COALESCE(EXCLUDED.play_count_sc, tracks.play_count_sc),
                likes_count_sc = COALESCE(EXCLUDED.likes_count_sc, tracks.likes_count_sc),
                reposts_count_sc = COALESCE(EXCLUDED.reposts_count_sc, tracks.reposts_count_sc),
                comments_count_sc = COALESCE(EXCLUDED.comments_count_sc, tracks.comments_count_sc),
                needs_duration_resolve = EXCLUDED.needs_duration_resolve,
                index_priority = LEAST(tracks.index_priority, EXCLUDED.index_priority),
                storage_priority = LEAST(tracks.storage_priority, EXCLUDED.storage_priority),
                sc_synced_at = now(),
                updated_at = now()
             RETURNING id, (xmax = 0) AS was_new",
        )
        .bind(&fields.sc_track_id)
        .bind(&fields.urn)
        .bind(&fields.title)
        .bind(&fields.title_normalized)
        .bind(&fields.description)
        .bind(&fields.genre)
        .bind(&fields.tags)
        .bind(fields.duration_ms)
        .bind(&fields.artwork_url)
        .bind(&fields.permalink_url)
        .bind(&fields.waveform_url)
        .bind(&fields.language)
        .bind(&fields.isrc)
        .bind(&fields.metadata_artist)
        .bind(&fields.sharing)
        .bind(fields.sc_created_at)
        .bind(fields.sc_last_modified)
        .bind(fields.release_year)
        .bind(fields.release_date)
        .bind(&fields.uploader_sc_user_id)
        .bind(&fields.uploader_urn)
        .bind(&fields.uploader_username)
        .bind(&fields.uploader_avatar_url)
        .bind(fields.play_count_sc)
        .bind(fields.likes_count_sc)
        .bind(fields.reposts_count_sc)
        .bind(fields.comments_count_sc)
        .bind(fields.needs_duration_resolve)
        .bind(new_index_priority.as_i16())
        .bind(new_storage_priority.as_i16())
        .fetch_one(&self.pg)
        .await?;

        Ok(IngestResult {
            id: row.0,
            was_new: row.1,
        })
    }

    pub async fn find_by_sc_track_id(&self, sc_track_id: &str) -> AppResult<Option<TrackRow>> {
        let row: Option<TrackRow> = sqlx::query_as("SELECT * FROM tracks WHERE sc_track_id = $1")
            .bind(sc_track_id)
            .fetch_optional(&self.pg)
            .await?;
        Ok(row)
    }

    /// Terminal `too_long`: excluded from storage/index/transcribe pickup queues.
    pub async fn mark_too_long(&self, sc_track_id: &str) -> AppResult<()> {
        sqlx::query(
            "UPDATE tracks SET \
                 storage_state = 'too_long', \
                 index_state = 'too_long', \
                 transcribe_state = 'disabled', \
                 updated_at = now() \
             WHERE sc_track_id = $1 \
               AND (storage_state <> 'too_long' OR index_state <> 'too_long')",
        )
            .bind(sc_track_id)
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    /// S3-аплоад завершён. `quality` ∈ {`'sq'`,`'hq'`} — кладём в
    /// `storage_quality`. `storage_state` всегда `'ok'`. Если приземлился `sq` —
    /// взводим `hq_upgrade_pending`, чтобы стриминговый cron позже перекачал
    /// в hq. На приземление `hq` — флаг сбрасываем.
    pub async fn mark_storage_done(&self, sc_track_id: &str, quality: &str) -> AppResult<()> {
        sqlx::query(
            "UPDATE tracks SET \
                 storage_state = 'ok', \
                 storage_quality = $2, \
                 s3_verified_at = now(), \
                 s3_missing_at = NULL, \
                 storage_attempts = 0, \
                 hq_upgrade_pending = CASE \
                     WHEN $2 = 'hq' THEN false \
                     WHEN $2 = 'sq' AND tracks.storage_quality IS DISTINCT FROM 'hq' THEN true \
                     ELSE hq_upgrade_pending \
                 END, \
                 updated_at = now() \
             WHERE sc_track_id = $1",
        )
        .bind(sc_track_id)
        .bind(quality)
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    /// Pickup треков, которые SC отдал с подозрительной длительностью
    /// (sentinel 30000ms без full_duration). Cron перечитывает через apiv2
    /// и фиксит duration_ms.
    pub async fn pick_duration_resolve(&self, limit: i64) -> AppResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM tracks \
             WHERE needs_duration_resolve = true \
             ORDER BY sc_synced_at \
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pg)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    pub async fn apply_resolved_duration(
        &self,
        sc_track_id: &str,
        duration_ms: i32,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE tracks SET duration_ms = $2, needs_duration_resolve = false, \
                 updated_at = now() \
             WHERE sc_track_id = $1",
        )
        .bind(sc_track_id)
        .bind(duration_ms)
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    pub async fn clear_duration_resolve(&self, sc_track_id: &str) -> AppResult<()> {
        sqlx::query(
            "UPDATE tracks SET needs_duration_resolve = false, updated_at = now() \
             WHERE sc_track_id = $1",
        )
        .bind(sc_track_id)
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    /// Воркер qdrant'а сообщает: indexing завершён. Снимает pending → indexed.
    pub async fn mark_indexed(&self, sc_track_id: &str) -> AppResult<()> {
        sqlx::query(
            "UPDATE tracks SET \
                 index_state = 'indexed', \
                 indexed_at = now(), \
                 index_attempts = 0, \
                 updated_at = now() \
             WHERE sc_track_id = $1 AND index_state <> 'indexed'",
        )
        .bind(sc_track_id)
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    /// Помечаем fingerprint, ищем близкого соседа по prefix и сшиваем
    /// `canonical_track_id`. Возвращает id канонического трека (если есть).
    pub async fn apply_fingerprint(
        &self,
        sc_track_id: &str,
        fingerprint: &str,
    ) -> AppResult<Option<Uuid>> {
        let row: Option<(Uuid, Option<Uuid>)> =
            sqlx::query_as("SELECT id, canonical_track_id FROM tracks WHERE sc_track_id = $1")
                .bind(sc_track_id)
                .fetch_optional(&self.pg)
                .await?;
        let Some((track_id, current_canonical)) = row else {
            return Ok(None);
        };

        sqlx::query("UPDATE tracks SET audio_fingerprint = $2, updated_at = now() WHERE id = $1")
            .bind(track_id)
            .bind(fingerprint)
            .execute(&self.pg)
            .await?;

        let prefix: String = fingerprint.chars().take(64).collect();
        let neighbour: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
            "SELECT id, canonical_track_id FROM tracks \
             WHERE substr(audio_fingerprint, 1, 64) = $1 AND id <> $2 LIMIT 1",
        )
        .bind(&prefix)
        .bind(track_id)
        .fetch_optional(&self.pg)
        .await?;
        let Some((other_id, other_canonical)) = neighbour else {
            return Ok(current_canonical);
        };

        let canonical_id = current_canonical
            .or(other_canonical)
            .unwrap_or_else(Uuid::new_v4);
        sqlx::query(
            "UPDATE tracks SET canonical_track_id = $1, updated_at = now() \
             WHERE id IN ($2, $3) AND (canonical_track_id IS NULL OR canonical_track_id <> $1)",
        )
        .bind(canonical_id)
        .bind(track_id)
        .bind(other_id)
        .execute(&self.pg)
        .await?;
        Ok(Some(canonical_id))
    }
}

/// Собрать SC-shape v1 payload из строки tracks + опционального уже-известного
/// uploader-карты (если read-path сделал JOIN на `users`). Без uploader-карты
/// используется денорм минимум (uploader_username/uploader_avatar_url из самого
/// трек-row).
///
/// Этот вид payload'а потребляют существующие клиентские поля (UI / desktop).
/// Не воссоздаём поля, которые SC отдаёт но мы не используем (label, monetization,
/// publisher_metadata.*, media.transcodings — берётся в живую через стриминг).
pub fn project_to_sc_shape(row: &TrackRow, uploader_user: Option<&Value>) -> Value {
    let mut obj = Map::new();
    obj.insert("kind".into(), Value::String("track".into()));
    obj.insert("id".into(), parse_id_or_string(&row.sc_track_id));
    obj.insert("urn".into(), Value::String(row.urn.clone()));
    obj.insert("title".into(), Value::String(row.title.clone()));
    if let Some(d) = &row.description {
        obj.insert("description".into(), Value::String(d.clone()));
    }
    if let Some(g) = &row.genre {
        obj.insert("genre".into(), Value::String(g.clone()));
    }
    obj.insert("tag_list".into(), Value::String(row.tags.join(" ")));
    obj.insert("duration".into(), json!(row.duration_ms));
    obj.insert("full_duration".into(), json!(row.duration_ms));
    if let Some(a) = &row.artwork_url {
        obj.insert("artwork_url".into(), Value::String(a.clone()));
    }
    if let Some(p) = &row.permalink_url {
        obj.insert("permalink_url".into(), Value::String(p.clone()));
    }
    if let Some(w) = &row.waveform_url {
        obj.insert("waveform_url".into(), Value::String(w.clone()));
    }
    obj.insert("sharing".into(), Value::String(row.sharing.clone()));
    if let Some(t) = row.sc_created_at {
        obj.insert("created_at".into(), Value::String(t.to_rfc3339()));
    }
    if let Some(t) = row.sc_last_modified {
        obj.insert("last_modified".into(), Value::String(t.to_rfc3339()));
    }
    if let Some(y) = row.release_year {
        obj.insert("release_year".into(), json!(y));
    }
    if let Some(d) = row.release_date {
        obj.insert("release_date".into(), Value::String(d.to_string()));
    }
    if let Some(l) = &row.language {
        obj.insert("language".into(), Value::String(l.clone()));
    }
    if let Some(isrc) = &row.isrc {
        let mut pm = Map::new();
        pm.insert("isrc".into(), Value::String(isrc.clone()));
        obj.insert("publisher_metadata".into(), Value::Object(pm));
    }
    obj.insert(
        "playback_count".into(),
        row.play_count_sc.map(|v| json!(v)).unwrap_or(Value::Null),
    );
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
    obj.insert(
        "comment_count".into(),
        row.comments_count_sc
            .map(|v| json!(v))
            .unwrap_or(Value::Null),
    );

    let user = uploader_user.cloned().unwrap_or_else(|| {
        let mut u = Map::new();
        if let Some(id) = &row.uploader_sc_user_id {
            u.insert("id".into(), parse_id_or_string(id));
        }
        if let Some(urn) = &row.uploader_urn {
            u.insert("urn".into(), Value::String(urn.clone()));
        }
        if let Some(n) = &row.uploader_username {
            u.insert("username".into(), Value::String(n.clone()));
        }
        if let Some(a) = &row.uploader_avatar_url {
            u.insert("avatar_url".into(), Value::String(a.clone()));
        }
        u.insert("kind".into(), Value::String("user".into()));
        Value::Object(u)
    });
    obj.insert("user".into(), user);

    // Мета для UI-бейджей: позволяет показывать "в кэше" / "анализ идёт" /
    // "проиндексирован" без отдельных запросов.
    let mut meta = Map::new();
    meta.insert(
        "storage_state".into(),
        Value::String(row.storage_state.clone()),
    );
    if let Some(q) = &row.storage_quality {
        meta.insert("storage_quality".into(), Value::String(q.clone()));
    }
    meta.insert("index_state".into(), Value::String(row.index_state.clone()));
    meta.insert(
        "enrich_state".into(),
        Value::String(row.enrich_state.clone()),
    );
    obj.insert("_scd_meta".into(), Value::Object(meta));

    Value::Object(obj)
}

/// Bulk-load с проекцией. Возвращает упорядоченный по входному порядку
/// массив. Отсутствующие в БД sc_track_id заменяются Value::Null (caller
/// решает что с ними делать — обычно фильтрует).
///
/// Видит ВСЕ строки, включая `sharing='private'` — звать только когда видимость
/// уже установлена caller'ом (`/me/*`, single-track после owner-guard, internal
/// replay). Для discovery/чужих профилей — [`project_many_public`].
pub async fn project_many(pg: &PgPool, sc_track_ids: &[String]) -> AppResult<Vec<Option<Value>>> {
    project_many_filtered(pg, sc_track_ids, false).await
}

/// То же, но отдаёт только `sharing='public'`. Приватные строки выпадают в
/// `None` (caller'ы их `flatten`'ят). Default для всех публичных read-path'ов.
pub async fn project_many_public(
    pg: &PgPool,
    sc_track_ids: &[String],
) -> AppResult<Vec<Option<Value>>> {
    project_many_filtered(pg, sc_track_ids, true).await
}

async fn project_many_filtered(
    pg: &PgPool,
    sc_track_ids: &[String],
    public_only: bool,
) -> AppResult<Vec<Option<Value>>> {
    if sc_track_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = if public_only {
        "SELECT * FROM tracks WHERE sc_track_id = ANY($1) AND sharing = 'public'"
    } else {
        "SELECT * FROM tracks WHERE sc_track_id = ANY($1)"
    };
    let rows: Vec<TrackRow> = sqlx::query_as(sql)
        .bind(sc_track_ids)
        .fetch_all(pg)
        .await?;
    let by_id: std::collections::HashMap<String, TrackRow> = rows
        .into_iter()
        .map(|r| (r.sc_track_id.clone(), r))
        .collect();

    // Сразу подмешиваем uploader из users (если есть) — один доп. запрос
    // взамен N JOIN'ов.
    let uploader_ids: Vec<String> = by_id
        .values()
        .filter_map(|r| r.uploader_sc_user_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let users: std::collections::HashMap<String, Value> = if uploader_ids.is_empty() {
        Default::default()
    } else {
        let rows: Vec<(String, Json<Value>)> = sqlx::query_as(
            "SELECT sc_user_id, jsonb_build_object(
                 'kind','user',
                 'id', sc_user_id,
                 'urn', urn,
                 'username', username,
                 'avatar_url', avatar_url,
                 'permalink_url', permalink_url,
                 'verified', verified,
                 'country_code', country,
                 'city', city,
                 'description', description,
                 'followers_count', followers_count,
                 'followings_count', followings_count,
                 'track_count', tracks_count
             ) AS u
             FROM users WHERE sc_user_id = ANY($1)",
        )
        .bind(&uploader_ids)
        .fetch_all(pg)
        .await?;
        rows.into_iter().map(|(k, j)| (k, j.0)).collect()
    };

    Ok(sc_track_ids
        .iter()
        .map(|id| {
            by_id.get(id).map(|row| {
                let uploader = row
                    .uploader_sc_user_id
                    .as_deref()
                    .and_then(|uid| users.get(uid));
                project_to_sc_shape(row, uploader)
            })
        })
        .collect())
}
