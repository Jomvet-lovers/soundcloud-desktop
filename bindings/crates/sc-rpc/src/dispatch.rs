//! Диспатч rpc-метода по имени: имена = методы ScRuntime, контракт аргументов
//! и ответов — Core/data (@sc/data client.ts). Меняешь здесь — меняй и там.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sc_core::{CoreError, ScRuntime};
use sc_domain::Urn;
use serde_json::{Value, json};

pub enum RpcError {
    UnknownMethod(String),
    BadArgs(String),
    Core(String),
}

impl IntoResponse for RpcError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            RpcError::UnknownMethod(m) => (StatusCode::NOT_FOUND, format!("неизвестный метод: {m}")),
            RpcError::BadArgs(e) => (StatusCode::BAD_REQUEST, e),
            RpcError::Core(e) => (StatusCode::BAD_GATEWAY, e),
        };
        (status, axum::Json(json!({ "error": msg }))).into_response()
    }
}

impl From<CoreError> for RpcError {
    fn from(e: CoreError) -> Self {
        RpcError::Core(e.to_string())
    }
}

fn ok<T: serde::Serialize>(v: T) -> Result<Value, RpcError> {
    serde_json::to_value(v).map_err(|e| RpcError::Core(e.to_string()))
}

// --- извлечение именованных аргументов ---

fn req_str(a: &Value, k: &str) -> Result<String, RpcError> {
    a.get(k)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| RpcError::BadArgs(format!("нужен строковый аргумент `{k}`")))
}

fn opt_str(a: &Value, k: &str) -> Option<String> {
    a.get(k).and_then(Value::as_str).map(str::to_owned)
}

fn req_urn(a: &Value, k: &str) -> Result<Urn, RpcError> {
    req_str(a, k).map(Urn::from)
}

fn req_u32(a: &Value, k: &str) -> Result<u32, RpcError> {
    a.get(k)
        .and_then(Value::as_u64)
        .map(|v| v as u32)
        .ok_or_else(|| RpcError::BadArgs(format!("нужен числовой аргумент `{k}`")))
}

fn req_f64(a: &Value, k: &str) -> Result<f64, RpcError> {
    a.get(k)
        .and_then(Value::as_f64)
        .ok_or_else(|| RpcError::BadArgs(format!("нужен числовой аргумент `{k}`")))
}

fn opt_f64(a: &Value, k: &str) -> Option<f64> {
    a.get(k).and_then(Value::as_f64)
}

fn req_bool(a: &Value, k: &str) -> Result<bool, RpcError> {
    a.get(k)
        .and_then(Value::as_bool)
        .ok_or_else(|| RpcError::BadArgs(format!("нужен булев аргумент `{k}`")))
}

fn bool_or(a: &Value, k: &str, default: bool) -> bool {
    a.get(k).and_then(Value::as_bool).unwrap_or(default)
}

fn str_vec(a: &Value, k: &str) -> Vec<String> {
    a.get(k)
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

fn f64_vec(a: &Value, k: &str) -> Vec<f64> {
    a.get(k)
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default()
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if "/\\?%*:|\"<>".contains(c) { '_' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Системный диалог сохранения (`rfd`, кроссплатформа) — платформенный слой Desktop.
/// `None` — отмена.
async fn download_to_file(rt: &ScRuntime, a: &Value) -> Result<Option<String>, RpcError> {
    let urn = req_urn(a, "urn")?;
    let cover = opt_str(a, "cover_url");
    let name = format!("{}.m4a", sanitize_filename(&format!("{} - {}", req_str(a, "artist")?, req_str(a, "title")?)));
    let Some(handle) = rfd::AsyncFileDialog::new()
        .set_file_name(name)
        .add_filter("Audio", &["m4a"])
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let dest = handle.path().to_string_lossy().into_owned();
    rt.cache_export(&urn, &dest, cover.as_deref()).await?;
    Ok(Some(dest))
}

pub async fn dispatch(rt: &ScRuntime, method: &str, args: Value) -> Result<Value, RpcError> {
    let a = &args;
    match method {
        // --- auth ---
        "auth_status" => ok(rt.auth_status().await),
        "set_session" => ok(rt.set_session(opt_str(a, "token"))?),
        "start_login" => ok(rt.start_login().await?),
        "poll_login" => ok(rt.poll_login(&req_str(a, "login_request_id")?).await?),
        "logout" => ok(rt.logout().await?),
        "auth_link_create" => ok(rt.auth_link_create(&req_str(a, "mode")?).await?),
        "auth_link_status" => ok(rt.auth_link_status(&req_str(a, "link_request_id")?).await?),
        "auth_link_claim" => ok(rt.auth_link_claim(&req_str(a, "payload")?).await?),

        // --- home / рекомендации ---
        "home_clusters" => ok(rt
            .home_clusters(req_u32(a, "limit")?, &str_vec(a, "languages"), bool_or(a, "hide_listened", false))
            .await?),
        "wave" => ok(rt
            .wave(
                req_u32(a, "limit")?,
                opt_str(a, "cursor").as_deref(),
                &str_vec(a, "languages"),
                bool_or(a, "hide_listened", false),
            )
            .await?),
        "wave_feedback" => ok(rt
            .wave_feedback(&req_str(a, "cursor")?, req_u32(a, "negatives")?, req_u32(a, "positives")?)
            .await?),
        "recommendations_feedback" => {
            ok(rt.recommendations_feedback(&req_str(a, "cluster_id")?, &req_str(a, "kind")?).await?)
        }
        "recommendations_similar" => {
            ok(rt.recommendations_similar(&req_str(a, "track_id")?, req_u32(a, "limit")?).await?)
        }
        "recommendations_artist" => {
            ok(rt.recommendations_artist(&req_str(a, "artist_id")?, req_u32(a, "limit")?).await?)
        }

        // --- поиск ---
        "search" => ok(rt
            .search(&req_str(a, "query")?, req_u32(a, "limit")?, req_u32(a, "offset")?)
            .await?),
        "search_sc_tracks" => ok(rt.search_sc_tracks(&req_str(a, "query")?, req_u32(a, "limit")?).await?),
        "search_vibe" => ok(rt.search_vibe(&req_str(a, "query")?, req_u32(a, "limit")?).await?),

        // --- треки ---
        "resolve" => ok(rt.resolve(&req_urn(a, "urn")?).await?),
        "resolve_tracks" => ok(rt.resolve_tracks(&str_vec(a, "urns")).await?),
        "resolve_url" => ok(rt.resolve_url(&req_str(a, "url")?).await?),
        "track_related" => ok(rt.track_related(&req_urn(a, "urn")?, req_u32(a, "limit")?).await?),
        "track_streams" => ok(rt.track_streams(&req_urn(a, "urn")?).await?),
        "track_waveform" => ok(rt.track_waveform(&req_str(a, "waveform_url")?).await?),
        "like_track" => ok(rt.like_track(&req_urn(a, "urn")?).await?),
        "unlike_track" => ok(rt.unlike_track(&req_urn(a, "urn")?).await?),
        "dislike_track" => ok(rt.dislike_track(&req_str(a, "sc_track_id")?).await?),
        "undislike_track" => ok(rt.undislike_track(&req_str(a, "sc_track_id")?).await?),
        "dislike_status" => ok(rt.dislike_status(&req_str(a, "sc_track_id")?).await?),
        "dislike_ids" => ok(rt.dislike_ids().await?),
        "track_favoriters" => ok(rt.track_favoriters(&req_urn(a, "urn")?, req_u32(a, "limit")?).await?),
        "track_reposters" => ok(rt.track_reposters(&req_urn(a, "urn")?, req_u32(a, "limit")?).await?),
        "track_comments" => ok(rt
            .track_comments(&req_urn(a, "urn")?, req_u32(a, "limit")?, req_u32(a, "offset")?)
            .await?),
        "post_comment" => ok(rt
            .post_comment(
                &req_urn(a, "urn")?,
                &req_str(a, "body")?,
                a.get("timestamp_ms").and_then(|v| v.as_i64()),
            )
            .await?),
        "lyrics" => ok(rt.lyrics(&req_str(a, "sc_track_id")?).await?),

        // --- библиотека / профиль ---
        "library_likes_tracks" => {
            ok(rt.library_likes_tracks(req_u32(a, "limit")?, req_u32(a, "offset")?).await?)
        }
        "library_playlists" => {
            ok(rt.library_playlists(req_u32(a, "limit")?, req_u32(a, "offset")?).await?)
        }
        "playlist_add_track" => {
            ok(rt.playlist_add_track(&req_urn(a, "playlist_urn")?, &req_urn(a, "track_urn")?).await?)
        }
        "create_playlist" => {
            ok(rt.create_playlist(&req_str(a, "title")?, &str_vec(a, "track_urns")).await?)
        }
        "clear_history" => ok(rt.clear_history().await?),
        "me" => ok(rt.me().await?),
        "me_subscription" => ok(rt.me_subscription().await?),
        "user" => ok(rt.user(&req_urn(a, "urn")?).await?),
        "follow_user" => ok(rt.follow_user(&req_urn(a, "urn")?).await?),
        "unfollow_user" => ok(rt.unfollow_user(&req_urn(a, "urn")?).await?),

        // --- плеер ---
        "play_track" => ok(rt.play_track(&req_urn(a, "urn")?).await?),
        "pause" => ok(rt.pause()),
        "resume" => ok(rt.resume()),
        "stop" => ok(rt.stop()),
        "seek" => ok(rt.seek(req_f64(a, "position_secs")?)?),
        "set_volume" => ok(rt.set_volume(req_f64(a, "volume")?)),
        "set_speed" => ok(rt.set_speed(req_f64(a, "speed")?)),
        "set_pitch" => ok(rt.set_pitch(opt_f64(a, "semitones"))),
        "set_eq" => ok(rt.set_eq(req_bool(a, "enabled")?, &f64_vec(a, "gains"))),
        "set_ab_loop" => ok(rt.set_ab_loop(opt_f64(a, "a"), opt_f64(a, "b"))),
        "is_playing" => ok(rt.is_playing()),
        "position_secs" => ok(rt.position_secs()),
        "audio_output_devices" => ok(rt.audio_output_devices()),
        "set_audio_output" => ok(rt.set_audio_output(opt_str(a, "name"))?),
        "preview_play" => ok(rt.preview_play(&req_urn(a, "urn")?, req_f64(a, "volume")?).await?),
        "preview_stop" => ok(rt.preview_stop(req_u32(a, "fade_ms")? as u64)),

        // --- оффлайн-кэш ---
        "cache_inventory" => ok(rt.cache_inventory()?),
        "cache_total_bytes" => ok(rt.cache_total_bytes()?),
        "cache_liked_bytes" => ok(rt.cache_liked_bytes()?),
        "cache_is_cached" => ok(rt.cache_is_cached(&req_urn(a, "urn")?)),
        "cache_ensure" => ok(rt.cache_ensure(&req_urn(a, "urn")?).await?),
        "cache_remove" => ok(rt.cache_remove(&req_urn(a, "urn")?).await?),
        "preload_track" => ok(rt.preload_track(req_urn(a, "urn")?)),
        "download_track" => {
            ok(download_to_file(rt, a).await?)
        }
        // bulk-прогрев длится минуты — не держим HTTP-ответ, прогресс идёт
        // каналом likes_progress
        "cache_likes" => {
            let urns: Vec<Urn> = str_vec(a, "urns").into_iter().map(Urn::from).collect();
            let rt = rt.clone();
            tokio::spawn(async move {
                let _ = rt.cache_likes(urns).await;
            });
            ok(())
        }
        "cache_likes_running" => ok(rt.cache_likes_running()),
        "cancel_cache_likes" => ok(rt.cancel_cache_likes()),
        "cache_clear" => ok(rt.cache_clear()?),
        "cache_clear_liked" => ok(rt.cache_clear_liked()?),
        "cache_enforce_limit" => ok(rt.cache_enforce_limit(req_u32(a, "limit_mb")? as u64)),
        "export_track" => ok(rt.export_track(&req_urn(a, "urn")?, &req_str(a, "dest")?).await?),

        // --- конфиги (генерик KV-персист) ---
        "config_get" => ok(rt.config_get(&req_str(a, "key")?).await?),
        "config_set" => {
            ok(rt.config_set(&req_str(a, "key")?, a.get("value").cloned().unwrap_or(Value::Null)).await?)
        }
        "config_delete" => ok(rt.config_delete(&req_str(a, "key")?).await?),

        // --- хосты ---
        "host_status" => ok(rt.host_status()),
        "request_host_recheck" => ok(rt.request_host_recheck()),

        other => Err(RpcError::UnknownMethod(other.to_owned())),
    }
}
