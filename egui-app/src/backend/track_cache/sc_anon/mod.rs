//! Direct anon download from the SoundCloud public API v2.
//!
//! Mirrors the streaming server's `anon` flow but performs every request
//! straight from the user's machine — no proxy hops. Used as a fallback
//! between local storage and the streaming server: if the user can reach
//! SoundCloud directly, we save a round trip to our infra.

pub(super) mod hls;

use bytes::Bytes;
use wreq::Client;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use crate::backend::events::EventBus;
use tokio::sync::{Mutex, RwLock};

use hls::{download_hls_full, download_progressive};

const SC_BASE_URL: &str = "https://soundcloud.com";
const SC_API_V2: &str = "https://api-v2.soundcloud.com";
const SC_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

/// Preset preference, best quality first. The AAC HLS stream (`aac_160k`) is
/// both the best anon quality and the only one that remuxes into the m4a cache
/// without a re-encode — it must beat the progressive MP3, which is 128k and
/// used to get re-encoded into AAC 256k (audible on drums).
const PRESET_ORDER: &[&str] = &["mp3_1_0", "aac_160k", "opus_0_0", "abr_sq"];
/// High-quality preference: AAC first (160k), MP3 as a fallback.
const HQ_PRESET_ORDER: &[&str] = &["aac_160k", "abr_sq", "mp3_1_0", "opus_0_0"];

/// Circuit breaker: trip after this many consecutive network failures so users
/// behind a regulator that blocks SC don't pay 1.5s connect-timeout per track.
const FAIL_THRESHOLD: u8 = 3;
const COOLDOWN_SECS: u64 = 300;
const CLIENT_ID_MIN_REFRESH: Duration = Duration::from_secs(30);

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, serde::Deserialize)]
pub struct TranscodingFormat {
    pub protocol: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Transcoding {
    pub url: String,
    pub preset: Option<String>,
    pub snipped: Option<bool>,
    pub format: Option<TranscodingFormat>,
}

#[derive(Debug, serde::Deserialize)]
pub struct TrackMedia {
    pub transcodings: Option<Vec<Transcoding>>,
}

#[derive(Debug, serde::Deserialize)]
pub struct ResolvedTrack {
    pub media: Option<TrackMedia>,
    pub track_authorization: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct TranscodingResolveResponse {
    url: String,
}

/// Successful download from SC anon API.
pub struct AnonStreamResult {
    pub data: Bytes,
}

/// Caches a public `client_id` extracted from soundcloud.com homepage hydration.
/// Includes a circuit breaker so users with SC blocked don't eat connect-timeouts
/// on every track.
pub struct AnonClient {
    client: Client,
    client_id: Arc<RwLock<Option<String>>>,
    refresh_gate: Mutex<Option<Instant>>,
    fail_count: Arc<AtomicU8>,
    cooldown_until: Arc<AtomicU64>,
    app_handle: OnceLock<EventBus>,
}

impl AnonClient {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            client_id: Arc::new(RwLock::new(None)),
            refresh_gate: Mutex::new(None),
            fail_count: Arc::new(AtomicU8::new(0)),
            cooldown_until: Arc::new(AtomicU64::new(0)),
            app_handle: OnceLock::new(),
        }
    }

    pub fn set_app_handle(&self, handle: EventBus) {
        let _ = self.app_handle.set(handle);
    }

    fn log(&self, level: &str, msg: String) {
        let line = format!("[SCAnon] {msg}");
        match level {
            "ERROR" | "WARN" => eprintln!("{line}"),
            _ => println!("{line}"),
        }
        // TODO(egui-port): Tauri の `log_native` (AppHandle::path().app_log_dir()
        // への追記) に相当するファイルログは未配線。コンソール出力のみ行う。
        // `crate::backend::paths::app_log_dir()` への追記として Phase 2 で復活予定。
        let _ = self.app_handle.get();
    }

    fn in_cooldown(&self) -> bool {
        self.cooldown_until.load(Ordering::Relaxed) > now_secs()
    }

    fn note_success(&self) {
        self.fail_count.store(0, Ordering::Relaxed);
        self.cooldown_until.store(0, Ordering::Relaxed);
    }

    fn note_failure(&self) {
        let count = self.fail_count.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= FAIL_THRESHOLD {
            self.cooldown_until
                .store(now_secs() + COOLDOWN_SECS, Ordering::Relaxed);
            self.fail_count.store(0, Ordering::Relaxed);
            self.log(
                "WARN",
                format!("circuit open — skipping anon for {COOLDOWN_SECS}s"),
            );
        }
    }

    /// Fetch the full audio bytes for a track URN.
    /// Returns `Ok(None)` if SC has no usable transcoding (geo-blocked,
    /// preview-only, etc.) so the caller can fall through to the next source.
    /// `Err` is reserved for network failures and feeds the circuit breaker.
    pub async fn get_stream(
        &self,
        track_urn: &str,
        hq: bool,
    ) -> Result<Option<AnonStreamResult>, String> {
        if self.in_cooldown() {
            return Ok(None);
        }
        let result = self.do_get_stream(track_urn, hq).await;
        match &result {
            Ok(Some(_)) => self.note_success(),
            Err(_) => self.note_failure(),
            Ok(None) => {}
        }
        result
    }

    async fn do_get_stream(
        &self,
        track_urn: &str,
        hq: bool,
    ) -> Result<Option<AnonStreamResult>, String> {
        let track_id = track_urn.rsplit(':').next().unwrap_or(track_urn);

        let track = match self.get_track_by_id(track_id).await {
            Ok(t) => t,
            Err(e) => {
                self.log("WARN", format!("get track failed: {e}"));
                return Err(e);
            }
        };

        let mut track_auth = track.track_authorization.clone();
        let transcodings = track.media.as_ref().and_then(|m| m.transcodings.as_ref());

        // No transcodings? refresh client_id once and retry the lookup.
        let transcodings_owned;
        let transcodings: &[Transcoding] = match transcodings {
            Some(t) if !t.is_empty() => t.as_slice(),
            _ => {
                self.log(
                    "INFO",
                    format!("no transcodings for {track_id}, refreshing client_id"),
                );
                self.invalidate_and_refresh().await?;
                let retry_track = match self.get_track_by_id(track_id).await {
                    Ok(t) => t,
                    Err(e) => {
                        self.log("WARN", format!("retry get track failed: {e}"));
                        return Err(e);
                    }
                };
                track_auth = retry_track.track_authorization.clone();
                transcodings_owned = retry_track
                    .media
                    .and_then(|m| m.transcodings)
                    .unwrap_or_default();
                if transcodings_owned.is_empty() {
                    self.log(
                        "INFO",
                        format!("still no transcodings for {track_id} after refresh"),
                    );
                    return Ok(None);
                }
                transcodings_owned.as_slice()
            }
        };

        match self
            .stream_from_transcodings(transcodings, track_auth.as_deref(), hq)
            .await
        {
            Ok(Some(r)) => Ok(Some(r)),
            Ok(None) => Ok(None),
            Err(e) => {
                self.log(
                    "WARN",
                    format!("stream failed for {track_id}, refreshing client_id: {e}"),
                );
                self.invalidate_and_refresh().await?;
                let retry_track = match self.get_track_by_id(track_id).await {
                    Ok(t) => t,
                    Err(e2) => {
                        self.log("WARN", format!("retry get track failed: {e2}"));
                        return Err(e2);
                    }
                };
                let retry_auth = retry_track.track_authorization.clone();
                let retry_transcodings = retry_track
                    .media
                    .and_then(|m| m.transcodings)
                    .unwrap_or_default();
                if retry_transcodings.is_empty() {
                    return Ok(None);
                }
                self.stream_from_transcodings(&retry_transcodings, retry_auth.as_deref(), hq)
                    .await
            }
        }
    }

    async fn stream_from_transcodings(
        &self,
        transcodings: &[Transcoding],
        track_auth: Option<&str>,
        hq: bool,
    ) -> Result<Option<AnonStreamResult>, String> {
        let ranked = ranked_transcodings(transcodings, hq);
        if ranked.is_empty() {
            return Ok(None);
        }

        let mut last_err: Option<String> = None;
        // Собираем ВСЕ ошибки кандидатов: «ресурс исчез» — только когда 404
        // у каждого. Раньше флаг сбрасывался на любой не-404, из-за чего
        // единственный мёртвый пресет (`abr_sq` 404-ит у всех треков) маскировал
        // рабочие `mp3_1_0`/`aac_160k` под «трек недоступен».
        let mut errors: Vec<String> = Vec::new();
        for t in ranked {
            let is_progressive =
                t.format.as_ref().and_then(|f| f.protocol.as_deref()) == Some("progressive");

            let media_url = match self.resolve_transcoding_url(&t.url, None, track_auth).await {
                Ok(u) => u,
                Err(e) => {
                    last_err = Some(format!(
                        "resolve {} failed: {e}",
                        t.preset.as_deref().unwrap_or("?")
                    ));
                    errors.push(last_err.clone().unwrap_or_default());
                    continue;
                }
            };

            let result = if is_progressive {
                download_progressive(&self.client, &media_url).await
            } else {
                download_hls_full(&self.client, &media_url).await
            };

            match result {
                Ok(data) => return Ok(Some(AnonStreamResult { data })),
                Err(e) => {
                    last_err = Some(format!(
                        "{} ({}) failed: {e}",
                        t.preset.as_deref().unwrap_or("?"),
                        if is_progressive { "progressive" } else { "hls" },
                    ));
                    errors.push(last_err.clone().unwrap_or_default());
                }
            }
        }

        // 404 на всех кандидатах = у трека нет аудио. 401/403/429 — клиентский
        // (протухший client_id / бан): это Err, чтобы сработал refresh+retry
        // и НЕ открылся circuit breaker.
        if all_candidates_gone(&errors) && !errors.iter().any(|e| is_auth_or_rate_limited(e)) {
            return Ok(None);
        }
        Err(last_err.unwrap_or_else(|| "all anon transcodings failed".into()))
    }

    async fn get_client_id(&self) -> Result<String, String> {
        {
            let cached = self.client_id.read().await;
            if let Some(ref id) = *cached {
                return Ok(id.clone());
            }
        }
        self.coalesced_refresh().await
    }

    /// Принудительно сбросить кэш client_id и взять новый.
    ///
    /// Обычный `coalesced_refresh` уважает 30-секундный гейт и возвращает
    /// ТОТ ЖЕ протухший id — «обновление» после 401 снова била тем же id.
    /// Здесь гейт пропускается: id выбрасывается, и следующий запрос уходит с
    /// реально свежим.
    async fn invalidate_and_refresh(&self) -> Result<String, String> {
        *self.client_id.write().await = None;
        *self.refresh_gate.lock().await = None;
        self.coalesced_refresh().await
    }

    /// Гейт нужен, чтобы пачка параллельных треков не устроила N заходов на
    /// homepage. Кэш при этом НЕ трогаем — решает вызывающий.
    async fn coalesced_refresh(&self) -> Result<String, String> {
        {
            let gate = self.refresh_gate.lock().await;
            if let Some(last) = *gate
                && last.elapsed() < CLIENT_ID_MIN_REFRESH
                && let Some(id) = self.client_id.read().await.clone()
            {
                return Ok(id);
            }
        }

        // Гейт отпускаем ДО похода в сеть: иначе все ждущие конкуренты
        // блокируются на время фетча homepage вместо того, чтобы увидеть
        // уже готовый свежий id.
        let client_id = self.fetch_client_id().await?;

        *self.client_id.write().await = Some(client_id.clone());
        *self.refresh_gate.lock().await = Some(Instant::now());
        self.log("INFO", "refreshed public client_id".to_string());
        Ok(client_id)
    }

    async fn fetch_client_id(&self) -> Result<String, String> {
        let html = self
            .client
            .get(SC_BASE_URL)
            .header("User-Agent", SC_USER_AGENT)
            .send()
            .await
            .map_err(|e| format!("fetch sc home: {e}"))?
            .text()
            .await
            .map_err(|e| format!("read sc home body: {e}"))?;

        extract_client_id_from_hydration(&html)
            .ok_or_else(|| "Failed to extract SoundCloud client_id from page".to_string())
    }

    async fn get_track_by_id(&self, track_id: &str) -> Result<ResolvedTrack, String> {
        let client_id = self.get_client_id().await?;
        let target = format!("{SC_API_V2}/tracks/{track_id}?client_id={client_id}");

        match self.fetch_json::<ResolvedTrack>(&target).await {
            Ok(t) => Ok(t),
            // Refresh+retry — только когда смена client_id реально лечит
            // (401/403/429). 404/410 — окончательный ответ (удалён/приватный),
            // остальное (500/таймаут/decode) новый id не чинит: лишний
            // homepage-запрос жечь не будем, ошибка уходит наверх как есть.
            Err(e) if is_auth_or_rate_limited(&e) => {
                self.log(
                    "INFO",
                    format!("get track failed ({e}), refreshing client_id for {track_id}"),
                );
                let new_id = self.invalidate_and_refresh().await?;
                let retry = format!("{SC_API_V2}/tracks/{track_id}?client_id={new_id}");
                self.fetch_json(&retry).await
            }
            Err(e) => Err(e),
        }
    }

    async fn resolve_transcoding_url(
        &self,
        transcoding_url: &str,
        explicit_client_id: Option<&str>,
        track_authorization: Option<&str>,
    ) -> Result<String, String> {
        let client_id = match explicit_client_id {
            Some(id) => id.to_string(),
            None => self.get_client_id().await?,
        };
        let target = build_transcoding_target(transcoding_url, &client_id, track_authorization);

        match self.fetch_json::<TranscodingResolveResponse>(&target).await {
            Ok(r) => Ok(r.url),
            // Только 401/403/429 лечатся сменой client_id. 404 на пресете
            // (`abr_sq` 404-ит у большинства треков) — окончательный ответ,
            // остальное новый id не чинит: refresh здесь лишь сжигал
            // homepage-запрос на каждый мёртвый пресет, замедляя весь подбор.
            Err(e) if explicit_client_id.is_none() && is_auth_or_rate_limited(&e) => {
                let new_id = self.invalidate_and_refresh().await?;
                let retry = build_transcoding_target(transcoding_url, &new_id, track_authorization);
                self.fetch_json::<TranscodingResolveResponse>(&retry)
                    .await
                    .map(|r| r.url)
            }
            Err(e) => Err(e),
        }
    }

    async fn fetch_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        let resp = self
            .client
            .get(url)
            .header("User-Agent", SC_USER_AGENT)
            .send()
            .await
            .map_err(|e| format!("request: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }
        resp.json::<T>().await.map_err(|e| format!("decode: {e}"))
    }
}

/// Drop previews/snipped/restricted, then rank by preset quality first — a
/// 160k AAC HLS stream is both better and remuxable (no re-encode) — and only
/// inside the same preset prefer progressive (one GET, no chunk failures).
fn ranked_transcodings(transcodings: &[Transcoding], hq: bool) -> Vec<&Transcoding> {
    let order = if hq { HQ_PRESET_ORDER } else { PRESET_ORDER };
    let candidates: Vec<&Transcoding> = transcodings
        .iter()
        .filter(|t| {
            let encrypted = t
                .format
                .as_ref()
                .and_then(|f| f.protocol.as_deref())
                .unwrap_or("")
                .contains("encrypted");
            !encrypted && !t.snipped.unwrap_or(false) && !t.url.contains("/preview")
        })
        .collect();

    if candidates.is_empty() {
        return Vec::new();
    }

    let is_progressive = |t: &&Transcoding| {
        t.format.as_ref().and_then(|f| f.protocol.as_deref()) == Some("progressive")
    };

    let mut ordered: Vec<&Transcoding> = Vec::with_capacity(candidates.len());

    for preset in order {
        for progressive in [true, false] {
            if let Some(t) = candidates
                .iter()
                .find(|t| is_progressive(t) == progressive && t.preset.as_deref() == Some(preset))
            {
                ordered.push(t);
            }
        }
    }
    for t in &candidates {
        if !ordered.iter().any(|o| std::ptr::eq(*o, *t)) {
            ordered.push(t);
        }
    }
    ordered
}

/// Match on the `HTTP {status}` prefix our own `fetch_json` produces — never
/// on a bare digit run: reqwest error text can embed the request URL, and
/// SoundCloud signed URLs (`expires=…`, base64 `Policy`/`Signature`) routinely
/// contain runs like "404". A bare `.contains("404")` misclassified such
/// transient network failures as "track gone" and skipped all retries.
fn looks_like_resource_gone(err: &str) -> bool {
    err.contains("HTTP 404") || err.contains("HTTP 410")
}

/// 401/403/429 — проблема клиента (протухший client_id, бан), а не трека.
/// Такие ошибки обязаны дойти до вызывающего как `Err`: он обновит client_id и
/// повторит, а `Ok(None)` их обещал бы как «аудио нет».
fn is_auth_or_rate_limited(err: &str) -> bool {
    err.contains("HTTP 401") || err.contains("HTTP 403") || err.contains("HTTP 429")
}

/// 404 на ВСЕХ кандидатах — трек реально без аудио (geo-block, превью,
/// удалён). Один 404 — норма: `abr_sq` 404-ит у большинства треков, и раньше
/// он маскировал рабочие `mp3_1_0`/`aac_160k` под «трек недоступен».
fn all_candidates_gone(errors: &[String]) -> bool {
    !errors.is_empty() && errors.iter().all(|e| looks_like_resource_gone(e))
}

/// Pull `client_id` out of `window.__sc_hydration` on the SC homepage.
fn extract_client_id_from_hydration(html: &str) -> Option<String> {
    static PATTERN: &str =
        r#""hydratable"\s*:\s*"apiClient"\s*,\s*"data"\s*:\s*\{\s*"id"\s*:\s*"([^"]+)""#;
    let re = regex::Regex::new(PATTERN).ok()?;
    let caps = re.captures(html)?;
    caps.get(1).map(|m| m.as_str().to_string())
}

fn build_transcoding_target(
    transcoding_url: &str,
    client_id: &str,
    track_authorization: Option<&str>,
) -> String {
    let sep = if transcoding_url.contains('?') {
        "&"
    } else {
        "?"
    };
    let mut target = format!("{transcoding_url}{sep}client_id={client_id}");
    if let Some(auth) = track_authorization.filter(|a| !a.is_empty()) {
        target.push_str("&track_authorization=");
        target.push_str(auth);
    }
    target
}

#[cfg(test)]
mod tests {
    use super::{
        all_candidates_gone, build_transcoding_target, is_auth_or_rate_limited,
        looks_like_resource_gone,
    };

    /// Регрессия: `abr_sq` 404-ит у почти всех треков. Считать «трек без
    /// аудио» по ЕДИНСТВЕННОМУ 404 значило отбрасывать рабочие mp3/aac
    /// и ронять воспроизведение целиком (см. `dreamin chuchu <3`).
    #[test]
    fn one_404_does_not_condemn_the_track() {
        let errors = vec![
            "resolve mp3_1_0 failed: HTTP 404 Not Found".to_string(),
            "resolve aac_160k failed: HTTP 404 Not Found".to_string(),
        ];
        assert!(all_candidates_gone(&errors));

        let errors = vec![
            "abr_sq (hls) failed: HTTP 404 Not Found".to_string(),
            "mp3_1_0 (progressive) failed: segment download: HTTP 500".to_string(),
        ];
        assert!(!all_candidates_gone(&errors));
    }

    /// Пустой список ошибок — не «всё пропало»: значит кандидатов не было.
    #[test]
    fn no_errors_is_not_all_gone() {
        assert!(!all_candidates_gone(&[]));
    }

    /// Подписанные URL (`expires=…`, base64) содержат runs вроде "404":
    /// голый `.contains("404")` принимал их за статус и хоронил трек.
    /// Матчим только префикс `HTTP {status}` от собственного `fetch_json`.
    #[test]
    fn signed_url_digit_runs_are_not_statuses() {
        assert!(!looks_like_resource_gone(
            "segment download: request: error sending request for url \
             (https://example.com/x?expires=1791340451): operation timed out",
        ));
        assert!(looks_like_resource_gone(
            "abr_sq (hls) failed: HTTP 404 Not Found"
        ));
        assert!(looks_like_resource_gone(
            "resolve mp3_1_0 failed: HTTP 410 Gone"
        ));
        // NOTE: `fetch_json` formats real 3-digit statuses, so prefix matching
        // is exact in practice; there is no 4-digit HTTP status to collide with.
    }

    /// Протухший client_id обязан дойти до вызывающего как Err, иначе
    /// circuit breaker откроется и убьёт все треки разом.
    #[test]
    fn client_errors_are_not_resource_gone() {
        assert!(is_auth_or_rate_limited("resolve mp3_1_0 failed: HTTP 401"));
        assert!(is_auth_or_rate_limited("get track failed: HTTP 403"));
        assert!(is_auth_or_rate_limited("aac_160k (hls) failed: HTTP 429"));
        assert!(!is_auth_or_rate_limited("abr_sq (hls) failed: HTTP 404 Not Found"));
        assert!(!is_auth_or_rate_limited("segment download: request timed out"));
    }

    const PROGRESSIVE_URL: &str =
        "https://api-v2.soundcloud.com/media/soundcloud:tracks:2028682452/1dc4586b/stream/progressive";

    #[test]
    fn transcoding_target_includes_track_authorization() {
        assert_eq!(
            build_transcoding_target(PROGRESSIVE_URL, "CID", Some("AUTH")),
            format!("{PROGRESSIVE_URL}?client_id=CID&track_authorization=AUTH"),
        );
        assert_eq!(
            build_transcoding_target(PROGRESSIVE_URL, "CID", None),
            format!("{PROGRESSIVE_URL}?client_id=CID"),
        );
        assert_eq!(
            build_transcoding_target(PROGRESSIVE_URL, "CID", Some("")),
            build_transcoding_target(PROGRESSIVE_URL, "CID", None),
        );
        // Correct separator when the URL already has a query.
        assert_eq!(
            build_transcoding_target("https://x/stream/progressive?foo=1", "CID", Some("A")),
            "https://x/stream/progressive?foo=1&client_id=CID&track_authorization=A",
        );
    }
}
