//! Общий `list_page` для SC GET-листинг эндпоинтов.
//!
//! Был трижды продублирован в `tracks/users/playlists::service::list_page_with_kind`
//! (идентичные реализации, только с разными `scope`/`session_id`). Вынесли сюда,
//! чтобы изменения в SC pagination/token rotation шли в одном месте.

use serde_json::Value;
use std::sync::Arc;

use crate::cache::cache_service::CacheScope;
use crate::cache::{FetchChunkResult, GetPageOptions, ListCacheService, ListPageResult};
use crate::error::{AppError, AppResult};
use crate::modules::auth::{try_with_chain, TokenKind, TokenProvider};
use crate::sc::{ScClient, ScReadService, SearchType};

/// Текстовый запрос для apiv2-поиска: `Some(q)`, если задан непустой `q` и НЕТ
/// id-batch / фасетных параметров (`ids`/`genres`/`tags`) — у них нет token-free
/// apiv2-маппинга, они остаются на apiv1.
pub fn plain_query(extra: &[(String, String)]) -> Option<String> {
    let mut q = None;
    for (k, v) in extra {
        match k.as_str() {
            "q" => q = Some(v.clone()),
            "ids" | "genres" | "tags" => return None,
            _ => {}
        }
    }
    q.filter(|s| !s.is_empty())
}

/// Параметры apiv2 search-листинга (через `ScReadService`: relay/Lua hedged с
/// apiv2-proxy). Курсор остаётся в apiv2-пространстве — apiv1 не подмешивается.
pub struct ScSearchArgs<'a> {
    pub list_cache: &'a ListCacheService,
    pub read: &'a ScReadService,
    pub ty: SearchType,
    pub q: String,
    pub cache_key: &'a str,
    pub ttl: u64,
    pub page: i64,
    pub limit: i64,
}

/// Страница типизированного SC-поиска через apiv2 (token-free). Кэшируется как
/// shared-листинг; пагинация следует apiv2 `next_href`.
pub async fn search_page(args: ScSearchArgs<'_>) -> AppResult<ListPageResult<Value>> {
    let read = args.read;
    let q = Arc::new(args.q);
    let ty = args.ty;
    args.list_cache
        .get_page::<Value, _, _>(
            GetPageOptions {
                key: args.cache_key,
                scope: CacheScope::Shared,
                session_id: None,
                ttl_sec: args.ttl,
                page: args.page,
                limit: args.limit,
                chunk_size: None,
            },
            |next_href, chunk_size| {
                let q = Arc::clone(&q);
                async move {
                    let page = read
                        .search_page(ty, &q, next_href.as_deref(), chunk_size as i64)
                        .await?;
                    Ok::<_, AppError>(FetchChunkResult {
                        items: page.items,
                        next_href: page.next_href,
                    })
                }
            },
        )
        .await
}

/// Параметры одного chunk-fetch'а через SC pagination.
pub struct ScListPageArgs<'a> {
    pub list_cache: &'a ListCacheService,
    pub sc: &'a ScClient,
    pub tokens: &'a TokenProvider,
    pub kind: TokenKind,
    pub cache_key: &'a str,
    pub ttl: u64,
    pub scope: CacheScope,
    pub session_id: Option<&'a str>,
    pub page: i64,
    pub limit: i64,
    pub path: String,
    pub extra_params: Vec<(String, String)>,
}

/// Вытащить страницу из SC-списка. Берёт chain один раз через
/// `TokenProvider::chain(kind)` и ротирует через [`try_with_chain`] на каждый
/// chunk-fetch до первого Ok или истощения chain'а.
pub async fn list_page(args: ScListPageArgs<'_>) -> AppResult<ListPageResult<Value>> {
    let chain = args.tokens.chain(args.kind).await?;
    let sc = args.sc.clone();
    let chain = Arc::new(chain);
    let path = Arc::new(args.path);
    let extra = Arc::new(args.extra_params);
    args.list_cache
        .get_page::<Value, _, _>(
            GetPageOptions {
                key: args.cache_key,
                scope: args.scope,
                session_id: args.session_id,
                ttl_sec: args.ttl,
                page: args.page,
                limit: args.limit,
                chunk_size: None,
            },
            |next_href, chunk_size| {
                let sc = sc.clone();
                let chain = Arc::clone(&chain);
                let path = Arc::clone(&path);
                let extra = Arc::clone(&extra);
                async move {
                    let resp: Value = try_with_chain(&chain, |tok| {
                        let sc = sc.clone();
                        let path = Arc::clone(&path);
                        let extra = Arc::clone(&extra);
                        let next_href = next_href.clone();
                        async move {
                            match next_href {
                                Some(href) => sc.api_get_absolute_value(&href, &tok).await,
                                None => {
                                    let mut params: Vec<(String, String)> = (*extra).clone();
                                    params.push(("limit".into(), chunk_size.to_string()));
                                    params.push(("linked_partitioning".into(), "true".into()));
                                    sc.api_get_value(&path, &tok, Some(&params)).await
                                }
                            }
                        }
                    })
                    .await?;
                    let items: Vec<Value> = resp
                        .get("collection")
                        .and_then(|v| v.as_array().cloned())
                        .unwrap_or_default();
                    let next_href = resp
                        .get("next_href")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .filter(|s| !s.is_empty());
                    Ok::<_, AppError>(FetchChunkResult { items, next_href })
                }
            },
        )
        .await
}
