//! GET /download/:track_urn — собирает кандидатов SoundCloud для прямого
//! скачивания клиентом. Сервер только резолвит URL'ы и (для encrypted)
//! делает handshake через `decrypt::Engine` — сам трек не качает.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use base64::Engine as _;
use bytes::Bytes;
use futures::future::BoxFuture;
use reqwest::Client;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, warn};

use super::handler::{check_is_premium, extract_session_id, StreamQuery};
use super::proxy::{fetch_get_bytes, fetch_get_json, fetch_get_text};
use super::restricted::{build_transcoding_target, Transcoding};
use crate::error::AppError;
use crate::AppState;

#[derive(Serialize)]
pub struct DownloadResponse {
    pub track_urn: String,
    pub candidates: Vec<Candidate>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Candidate {
    Progressive {
        quality: String,
        preset: String,
        mime: String,
        url: String,
    },
    Hls {
        quality: String,
        preset: String,
        mime: String,
        manifest_url: String,
    },
    EncryptedHls {
        quality: String,
        preset: String,
        mime: String,
        content_type: String,
        init_base64: String,
        segments: Vec<String>,
        key_base64: String,
    },
}

/// Транскодинг + контекст под который надо резолвить
/// (откуда мы его взяли — anon-сессия или cookies-сессия).
struct Entry {
    t: Transcoding,
    client_id: String,
    track_auth: Option<String>,
    headers: HashMap<String, String>,
}

#[derive(serde::Deserialize)]
struct ResolveResp {
    url: String,
    #[serde(rename = "licenseAuthToken")]
    auth_token: Option<String>,
}

pub async fn download(
    State(state): State<AppState>,
    Path(track_urn): Path<String>,
    headers: HeaderMap,
    Query(query): Query<StreamQuery>,
) -> Result<Json<DownloadResponse>, AppError> {
    let session_id = extract_session_id(&headers, &query)?;
    let session = state
        .pg
        .get_session(&session_id)
        .await?
        .ok_or(AppError::Unauthorized)?;

    let is_premium = check_is_premium(&state, &session).await;

    if state.config.premium_only && !is_premium {
        return Err(AppError::Forbidden);
    }

    let entries = collect_entries(&state, &track_urn, is_premium).await;
    if entries.is_empty() {
        warn!("[download] {track_urn} → no transcodings");
        return Err(AppError::NotFound);
    }

    let futures = entries
        .into_iter()
        .map(|e| resolve_entry(&state, e))
        .collect::<Vec<_>>();
    let candidates: Vec<Candidate> = futures::future::join_all(futures)
        .await
        .into_iter()
        .flatten()
        .collect();

    if candidates.is_empty() {
        warn!("[download] {track_urn} → no usable candidates");
        return Err(AppError::NoStream);
    }

    Ok(Json(DownloadResponse {
        track_urn,
        candidates,
    }))
}

async fn collect_entries(state: &AppState, track_urn: &str, is_premium: bool) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    match state.anon.fetch_track_meta(track_urn).await {
        Ok((tcs, track_auth, cid)) => {
            for t in tcs {
                if seen.insert(t.url.clone()) {
                    entries.push(Entry {
                        t,
                        client_id: cid.clone(),
                        track_auth: track_auth.clone(),
                        headers: HashMap::new(),
                    });
                }
            }
        }
        Err(e) => debug!("[download] anon meta failed for {track_urn}: {e}"),
    }

    if is_premium {
        if let Some(cookies) = state.cookies.as_ref() {
            match cookies.fetch_track_meta(track_urn).await {
                Ok((tcs, track_auth, cid)) => {
                    let h = cookies.cookie_auth_headers();
                    for t in tcs {
                        if seen.insert(t.url.clone()) {
                            entries.push(Entry {
                                t,
                                client_id: cid.clone(),
                                track_auth: track_auth.clone(),
                                headers: h.clone(),
                            });
                        }
                    }
                }
                Err(e) => debug!("[download] cookies meta failed for {track_urn}: {e}"),
            }
        }
    }

    entries
}

async fn resolve_entry(state: &AppState, entry: Entry) -> Option<Candidate> {
    let protocol = entry
        .t
        .format
        .as_ref()
        .and_then(|f| f.protocol.as_deref())
        .unwrap_or("");
    let mime = entry
        .t
        .format
        .as_ref()
        .and_then(|f| f.mime_type.as_deref())
        .unwrap_or("audio/mpeg")
        .to_string();
    let quality = entry
        .t
        .quality
        .clone()
        .unwrap_or_else(|| "sq".to_string());
    let preset = entry
        .t
        .preset
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    if entry.t.snipped.unwrap_or(false) || entry.t.url.contains("/preview") {
        return None;
    }

    let target = build_transcoding_target(
        &entry.t.url,
        &entry.client_id,
        entry.track_auth.as_deref(),
    );

    let resp: ResolveResp = match fetch_get_json(
        &state.http_client,
        &state.config.sc_proxy_url,
        &target,
        entry.headers.clone(),
        false,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            log_resolve_failure(&e.to_string(), "resolve", &preset, protocol);
            return None;
        }
    };

    match protocol {
        "progressive" => Some(Candidate::Progressive {
            quality,
            preset,
            mime,
            url: resp.url,
        }),
        "hls" => Some(Candidate::Hls {
            quality,
            preset,
            mime,
            manifest_url: resp.url,
        }),
        "ctr-encrypted-hls" => {
            let token = resp.auth_token?;
            prepare_encrypted(
                state,
                quality,
                preset,
                mime,
                resp.url,
                token,
                entry.headers,
            )
            .await
        }
        _ => None,
    }
}

async fn prepare_encrypted(
    state: &AppState,
    quality: String,
    preset: String,
    mime: String,
    manifest_url: String,
    token: String,
    headers: HashMap<String, String>,
) -> Option<Candidate> {
    let engine = state.decryptor.as_ref()?;

    let (manifest, _) = match fetch_get_text(
        &state.http_client,
        &state.config.sc_proxy_url,
        &manifest_url,
        headers.clone(),
        false,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            log_resolve_failure(&e.to_string(), "manifest", &preset, "ctr-encrypted-hls");
            return None;
        }
    };

    let fetcher: Arc<dyn decrypt::Fetcher> = Arc::new(SegmentFetcher {
        client: state.http_client.clone(),
        proxy_url: state.config.sc_proxy_url.clone(),
        headers,
    });

    let prep = match engine.prepare_for_client(&manifest, &token, fetcher).await {
        Ok(p) => p,
        Err(e) => {
            if !e.is_disabled() {
                warn!("[download] prepare encrypted failed: {e}");
            }
            return None;
        }
    };

    let b64 = base64::engine::general_purpose::STANDARD;
    Some(Candidate::EncryptedHls {
        quality,
        preset,
        mime,
        content_type: prep.content_type,
        init_base64: b64.encode(&prep.init),
        segments: prep.segment_urls,
        key_base64: b64.encode(prep.key),
    })
}

/// `decrypt::Fetcher`, который тянет init/license через ту же прокси/релей
/// инфраструктуру что и весь остальной SC-трафик, с указанным набором headers.
struct SegmentFetcher {
    client: Client,
    proxy_url: String,
    headers: HashMap<String, String>,
}

impl decrypt::Fetcher for SegmentFetcher {
    fn get(
        &self,
        url: String,
        extra: Vec<(String, String)>,
    ) -> BoxFuture<'static, Result<Bytes, decrypt::Error>> {
        let client = self.client.clone();
        let proxy_url = self.proxy_url.clone();
        let mut merged = self.headers.clone();
        for (k, v) in extra {
            merged.insert(k, v);
        }
        Box::pin(async move {
            let (b, _) = fetch_get_bytes(&client, &proxy_url, &url, merged, false)
                .await
                .map_err(|e| decrypt::Error::Fetch(e.to_string()))?;
            Ok(b)
        })
    }

    fn post(
        &self,
        url: String,
        extra: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> BoxFuture<'static, Result<Bytes, decrypt::Error>> {
        let client = self.client.clone();
        let proxy_url = self.proxy_url.clone();
        let mut merged = self.headers.clone();
        for (k, v) in extra {
            merged.insert(k, v);
        }
        Box::pin(async move {
            let (b, _) = super::proxy::fetch_post_bytes(&client, &proxy_url, &url, merged, body)
                .await
                .map_err(|e| decrypt::Error::Fetch(e.to_string()))?;
            Ok(b)
        })
    }
}

/// Парсит итоговый HTTP-статус из строки ошибки прокси-слоя.
/// Формат строк фиксирован в `proxy.rs`: `"status NNN"` для direct/proxy,
/// `"relay status NNN"` для relay. Транспорт-/parse-ошибки возвращают `None`.
fn classify_status(msg: &str) -> Option<u16> {
    for pat in ["relay status ", "status "] {
        if let Some(i) = msg.find(pat) {
            let tail = &msg[i + pat.len()..];
            let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<u16>() {
                return Some(n);
            }
        }
    }
    None
}

/// 404/410 — точно нет такой дорожки (ожидаемо для DRM-only треков с
/// «фейковыми» plain-транскодингами). Всё остальное (rate-limit прокси,
/// 5xx, transport) — warn'ом, потому что трек может быть и рабочим.
fn log_resolve_failure(msg: &str, stage: &str, preset: &str, protocol: &str) {
    match classify_status(msg) {
        Some(404) | Some(410) => debug!("[download] {stage} {preset}/{protocol} gone: {msg}"),
        Some(s) => warn!("[download] {stage} {preset}/{protocol} status {s}: {msg}"),
        None => warn!("[download] {stage} {preset}/{protocol} failed: {msg}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_status_parses_known_formats() {
        assert_eq!(classify_status("status 404"), Some(404));
        assert_eq!(classify_status("status 502"), Some(502));
        assert_eq!(classify_status("relay status 429"), Some(429));
        // Через враппер, как реально приходит:
        assert_eq!(classify_status("fetch: status 410"), Some(410));
        assert_eq!(classify_status("send: connection reset"), None);
        assert_eq!(classify_status("timeout"), None);
        assert_eq!(classify_status("parse: expected `,`"), None);
    }
}
