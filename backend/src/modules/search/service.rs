//! Слой кеширования и оркестрации над `repository::search_*`.
//!
//! Каждая выдача оборачивается в Redis-кеш на короткий TTL — одинаковые
//! поисковые запросы от множества клиентов (toggle SCD ⇄ SC, ребиндинг на
//! debounce) идут в pg только при cache miss. Кеш-ключ выводится из всех
//! query-параметров через build_list_cache_key, чтобы пара (q, user_urn, page,
//! limit) разводилась.

use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::cache::cache_service::CacheScope;
use crate::cache::{build_list_cache_key, CacheService, ListPageResult};
use crate::error::AppResult;
use crate::modules::enrich::dto as enrich_dto;
use crate::modules::enrich::normalize::normalize_name;
use crate::modules::search::repository;

/// 60s — баланс между "помогает дешёвыми обновлениями FE" и "не запоминает
/// слишком долго свежеоткрытый трек, который только что попал в базу".
const TTL_SECONDS: u64 = 60;

/// Минимальная длина запроса. Под лимитом — поиск отказан (400) или возвращает
/// пустую выдачу, в зависимости от вкуса caller'а.
pub const MIN_QUERY_LEN: usize = 2;

/// Сколько символов из q вообще пропускаем дальше. Защита от копи-паста
/// мегатекста в инпут.
pub const MAX_QUERY_LEN: usize = 128;

/// Максимальная страница (включительно). 25 * limit_max = 1250 элементов —
/// дальше пагинировать DB-поиск бессмысленно, пусть юзер уточнит запрос.
pub const MAX_PAGE: i64 = 24;

/// Лимит элементов на страницу.
pub const MAX_LIMIT: i64 = 50;

pub struct SearchService {
    pg: PgPool,
    cache: Arc<CacheService>,
}

impl SearchService {
    pub fn new(pg: PgPool, cache: Arc<CacheService>) -> Arc<Self> {
        Arc::new(Self { pg, cache })
    }

    /// Нормализуем запрос к виду, под который заточены индексы. q_lower —
    /// единственное представление, что мы используем для substring match'а.
    fn normalize_query(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let truncated: String = trimmed.chars().take(MAX_QUERY_LEN).collect();
        // Используем тот же `normalize_name`, что и при записи: иначе кириллица
        // с диакритиками / кавычки разойдутся между писателем и читателем.
        let normalized = normalize_name(&truncated);
        if normalized.chars().count() < MIN_QUERY_LEN {
            return None;
        }
        Some(normalized)
    }

    fn clamp_page_limit(page: i64, limit: i64) -> (i64, i64) {
        let page = page.clamp(0, MAX_PAGE);
        let limit = limit.clamp(1, MAX_LIMIT);
        (page, limit)
    }

    /// Универсальный wrapper: cache hit отдаст готовый JSON, иначе compute(),
    /// сериализуем, складываем в Redis.
    async fn cached<F, Fut>(&self, cache_key: &str, compute: F) -> AppResult<Value>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = AppResult<Value>>,
    {
        if let Ok(Some(raw)) = self.cache.get_raw(cache_key).await {
            if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                return Ok(v);
            }
        }
        let value = compute().await?;
        if let Ok(json) = serde_json::to_string(&value) {
            let _ = self
                .cache
                .set_raw(
                    cache_key,
                    &json,
                    TTL_SECONDS,
                    None,
                    CacheScope::Shared,
                    None,
                )
                .await;
        }
        Ok(value)
    }

    pub async fn tracks(
        &self,
        q: &str,
        user_urn: Option<&str>,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let Some(q_norm) = Self::normalize_query(q) else {
            return Ok(empty_page(page, limit));
        };
        let (page, limit) = Self::clamp_page_limit(page, limit);

        // Разруливаем user_urn → sc_user_id один раз, кешируем в ключе уже
        // нормализованный id, чтобы запросы по разным формам того же URN не
        // плодили cache misses.
        let user_sc_id = match user_urn {
            Some(urn) if !urn.is_empty() => match repository::resolve_user_sc_id(&self.pg, urn).await? {
                Some(id) => Some(id),
                None => return Ok(empty_page(page, limit)),
            },
            _ => None,
        };

        let mut params: Vec<(&str, String)> = vec![
            ("q", q_norm.clone()),
            ("page", page.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(uid) = &user_sc_id {
            params.push(("u", uid.clone()));
        }
        let key = build_list_cache_key("search-db-tracks", &params);

        let value = self
            .cached(&key, || async {
                let (mut items, has_more) = repository::search_tracks(
                    &self.pg,
                    &q_norm,
                    user_sc_id.as_deref(),
                    page,
                    limit,
                )
                .await?;
                enrich_dto::apply_to_tracks(&self.pg, &mut items).await?;
                Ok(serde_json::to_value(PageEnvelope {
                    collection: items,
                    page,
                    page_size: limit,
                    has_more,
                })
                .unwrap_or(Value::Null))
            })
            .await?;
        Ok(decode_page(value, page, limit))
    }

    pub async fn playlists(
        &self,
        q: &str,
        user_urn: Option<&str>,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let Some(q_norm) = Self::normalize_query(q) else {
            return Ok(empty_page(page, limit));
        };
        let (page, limit) = Self::clamp_page_limit(page, limit);

        let user_sc_id = match user_urn {
            Some(urn) if !urn.is_empty() => match repository::resolve_user_sc_id(&self.pg, urn).await? {
                Some(id) => Some(id),
                None => return Ok(empty_page(page, limit)),
            },
            _ => None,
        };

        let mut params: Vec<(&str, String)> = vec![
            ("q", q_norm.clone()),
            ("page", page.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(uid) = &user_sc_id {
            params.push(("u", uid.clone()));
        }
        let key = build_list_cache_key("search-db-playlists", &params);

        let value = self
            .cached(&key, || async {
                let (items, has_more) = repository::search_playlists(
                    &self.pg,
                    &q_norm,
                    user_sc_id.as_deref(),
                    page,
                    limit,
                )
                .await?;
                Ok(serde_json::to_value(PageEnvelope {
                    collection: items,
                    page,
                    page_size: limit,
                    has_more,
                })
                .unwrap_or(Value::Null))
            })
            .await?;
        Ok(decode_page(value, page, limit))
    }

    pub async fn users(
        &self,
        q: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let Some(q_norm) = Self::normalize_query(q) else {
            return Ok(empty_page(page, limit));
        };
        let (page, limit) = Self::clamp_page_limit(page, limit);

        let params: Vec<(&str, String)> = vec![
            ("q", q_norm.clone()),
            ("page", page.to_string()),
            ("limit", limit.to_string()),
        ];
        let key = build_list_cache_key("search-db-users", &params);

        let value = self
            .cached(&key, || async {
                let (items, has_more) = repository::search_users(&self.pg, &q_norm, page, limit).await?;
                Ok(serde_json::to_value(PageEnvelope {
                    collection: items,
                    page,
                    page_size: limit,
                    has_more,
                })
                .unwrap_or(Value::Null))
            })
            .await?;
        Ok(decode_page(value, page, limit))
    }

    pub async fn artists(
        &self,
        q: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let Some(q_norm) = Self::normalize_query(q) else {
            return Ok(empty_page(page, limit));
        };
        let (page, limit) = Self::clamp_page_limit(page, limit);

        let params: Vec<(&str, String)> = vec![
            ("q", q_norm.clone()),
            ("page", page.to_string()),
            ("limit", limit.to_string()),
        ];
        let key = build_list_cache_key("search-db-artists", &params);

        let value = self
            .cached(&key, || async {
                let (rows, has_more) =
                    repository::search_artists(&self.pg, &q_norm, page, limit).await?;
                let items: Vec<Value> = rows.into_iter().map(artist_to_value).collect();
                Ok(serde_json::to_value(PageEnvelope {
                    collection: items,
                    page,
                    page_size: limit,
                    has_more,
                })
                .unwrap_or(Value::Null))
            })
            .await?;
        Ok(decode_page(value, page, limit))
    }

    pub async fn albums(
        &self,
        q: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let Some(q_norm) = Self::normalize_query(q) else {
            return Ok(empty_page(page, limit));
        };
        let (page, limit) = Self::clamp_page_limit(page, limit);

        let params: Vec<(&str, String)> = vec![
            ("q", q_norm.clone()),
            ("page", page.to_string()),
            ("limit", limit.to_string()),
        ];
        let key = build_list_cache_key("search-db-albums", &params);

        let value = self
            .cached(&key, || async {
                let (rows, has_more) =
                    repository::search_albums(&self.pg, &q_norm, page, limit).await?;
                let items: Vec<Value> = rows.into_iter().map(album_to_value).collect();
                Ok(serde_json::to_value(PageEnvelope {
                    collection: items,
                    page,
                    page_size: limit,
                    has_more,
                })
                .unwrap_or(Value::Null))
            })
            .await?;
        Ok(decode_page(value, page, limit))
    }
}

#[derive(Debug, Serialize)]
struct PageEnvelope {
    collection: Vec<Value>,
    page: i64,
    page_size: i64,
    has_more: bool,
}

fn decode_page(v: Value, fallback_page: i64, fallback_limit: i64) -> ListPageResult<Value> {
    serde_json::from_value::<ListPageResult<Value>>(v)
        .unwrap_or_else(|_| empty_page(fallback_page, fallback_limit))
}

fn empty_page(page: i64, limit: i64) -> ListPageResult<Value> {
    ListPageResult {
        collection: Vec::new(),
        page,
        page_size: limit,
        has_more: false,
    }
}

fn artist_to_value(r: repository::ArtistSearchRow) -> Value {
    json!({
        "id": r.id,
        "name": r.name,
        "country": r.country,
        "avatar_url": r.avatar_url,
        "confidence": r.confidence,
        "track_count_primary": r.track_count_primary,
        "track_count_featured": r.track_count_featured,
        "album_count": r.album_count_denorm,
        "monthly_listeners": r.monthly_listeners,
        "trending": r.trending_score,
        "tags": crate::modules::discover::tags::canonicalize_tags(r.tags),
        "star": r.is_star,
        "aura_id": if r.is_star { r.star_aura_id } else { None },
        "custom_hex": if r.is_star { r.star_custom_hex } else { None },
    })
}

fn album_to_value(r: repository::AlbumSearchRow) -> Value {
    let release_month = r
        .release_date
        .map(|d| d.format("%-m").to_string().parse::<i32>().unwrap_or(0));
    json!({
        "id": r.id,
        "title": r.title,
        "type": r.kind,
        "release_year": r.release_year,
        "release_month": release_month,
        "cover_url": r.cover_url,
        "confidence": r.confidence,
        "primary_artist": {
            "id": r.primary_artist_id.unwrap_or_else(uuid::Uuid::nil),
            "name": r.primary_artist_name.unwrap_or_default(),
            "avatar_url": r.primary_artist_avatar,
        },
        "track_count": r.track_count,
        "total_duration_ms": r.total_duration_ms,
        "popularity": r.popularity_score,
        "star": r.is_star_artist,
    })
}
