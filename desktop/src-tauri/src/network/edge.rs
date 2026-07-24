//! Транспортные тиры до наших доменов.
//!
//! `direct` → `relay` (`*.temp.scdinternal.site`, хост-в-хост, гоняет и gRPC)
//! → `worker` (Cloudflare, контракт `X-Target: base64(url)`).
//!
//! Тир липнет к origin'у: как только прямой путь лёг у конкретного юзера, он не
//! пытается ходить туда на каждом запросе. Раз в `REVALIDATE` прямой путь
//! пробуется снова, чтобы разбан подхватился сам. Состояние переживает перезапуск
//! (`edge_state.json`), иначе каждый старт стоил бы таймаута на первом запросе.
//!
//! Байты storage/s3 намеренно НЕ проксируются: relay их не вывезет по трафику.

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};

const STATE_FILE: &str = "edge_state.json";
const REVALIDATE: Duration = Duration::from_secs(600);
/// Cloudflare рубит воркер по своему минутному/суточному лимиту (1015/1027).
const WORKER_BAN_RATELIMIT: Duration = Duration::from_secs(3600);
const WORKER_BAN_ERROR: Duration = Duration::from_secs(60);

/// origin → relay-хост. Relay всегда https/443, порт origin'а отбрасывается
/// (call ходит на :444 напрямую, через relay — на обычный 443).
const RELAYS: &[(&str, &str)] = &[
    ("api.scdinternal.site", "api.temp.scdinternal.site"),
    ("api-star.scdinternal.site", "api-star.temp.scdinternal.site"),
    ("stream.scdinternal.site", "stream.temp.scdinternal.site"),
    (
        "stream-star.scdinternal.site",
        "stream-star.temp.scdinternal.site",
    ),
    ("images.scdinternal.site", "images.temp.scdinternal.site"),
    ("storage.scdinternal.site", "storage.temp.scdinternal.site"),
    ("pay.scdinternal.site", "pay.temp.scdinternal.site"),
    ("call.scdinternal.site", "call.temp.scdinternal.site"),
];

/// gRPC через X-Target-воркер не пролезает — у call тира воркеров нет.
const NO_WORKER: &[&str] = &["call.scdinternal.site"];

/// origin → сосед, чей вердикт наследуем пока своего нет. storage-байты тянет
/// только ядро (read-only), сам вердикт не наберёт быстро; но storage и stream
/// на одном main-host и банятся вместе, а stream активно щупает фронт — так
/// storage переезжает на relay сразу, без холодного таймаута.
const INHERIT: &[(&str, &str)] = &[("storage.scdinternal.site", "stream.scdinternal.site")];

const WORKERS: &[&str] = &[
    "https://sc.w942oonlso.workers.dev",
    "https://sc-prx.bmfniafx.workers.dev",
    "https://soundcloud.ziwpsorg.workers.dev",
    "https://sc.ylqkepqg.workers.dev",
    "https://sc.azsawydu.workers.dev",
    "https://sc.nlafehzy.workers.dev",
    "https://sc-v2.8138glynnis.workers.dev",
    "https://sc.loli-cf-zxc.workers.dev",
    "https://sc-proxy-v2.zxcghoul.workers.dev",
    "https://soundcloud-desktop-proxy-v2.sexy-loli.workers.dev",
    "https://soundcloud-proxy-v2.loli-hard.workers.dev",
    "https://broad-sea-0aef.majors-ketones-2a.workers.dev",
    "https://throbbing-star-8f63.beaches-yard45.workers.dev",
    "https://round-lake-a57b.yantra-atria-3z.workers.dev",
    "https://frosty-smoke-e5c9.tryout-bream1j.workers.dev",
    "https://cold-mountain-45c2.rams-36-balmier.workers.dev",
    "https://floral-snow-9fb2.digraph-duals-1c.workers.dev",
    "https://flat-pond-83a0.netbook-gerbils-4t.workers.dev",
    "https://young-base-b255.kin-fenders0p.workers.dev",
    "https://damp-breeze-da1e.corneas-absence5m.workers.dev",
    "https://withered-bush-bbba.snugger-armful-5d.workers.dev",
    "https://fancy-moon-2be0.cocos-dearer-2j.workers.dev",
    "https://late-mud-9d0e.tipper-oxidant-0v.workers.dev",
    "https://muddy-boat-1b39.lite-tend-2m.workers.dev",
    "https://holy-paper-fb1c.imager-abode3q.workers.dev",
];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Direct,
    Relay,
    Worker,
}

/// Одна попытка: куда бить.
#[derive(Clone, Debug)]
pub struct Hop {
    pub url: String,
    pub tier: Tier,
    pub origin: String,
}

impl Hop {
    pub fn note(&self, ok: bool) {
        note(&self.origin, self.tier, ok);
    }

    pub fn tier_label(&self) -> &'static str {
        match self.tier {
            Tier::Direct => "direct",
            Tier::Relay => "relay",
            Tier::Worker => "worker",
        }
    }

    /// Заголовок `X-Target` для воркер-тира (base64 исходного URL); direct/relay
    /// его не требуют — они уже бьют по нужному хосту.
    pub fn x_target_for(&self, target_url: &str) -> Option<String> {
        if self.tier == Tier::Worker {
            Some(BASE64.encode(target_url.as_bytes()))
        } else {
            None
        }
    }
}

struct OriginState {
    tier: Tier,
    revalidate_at: Instant,
    /// Одиночный сетевой чих не должен уводить здорового юзера на relay.
    direct_fails: u8,
}

/// Сколько подряд провалов прямого пути до переезда на relay.
const DIRECT_FAIL_THRESHOLD: u8 = 2;

struct Inner {
    origins: HashMap<String, OriginState>,
    /// Порядок свой у каждой установки — иначе тысяча клиентов долбит первый воркер.
    workers: Vec<String>,
    worker_ban: HashMap<String, Instant>,
    dir: Option<PathBuf>,
}

static STATE: OnceLock<Mutex<Inner>> = OnceLock::new();

fn state() -> &'static Mutex<Inner> {
    STATE.get_or_init(|| {
        Mutex::new(Inner {
            origins: HashMap::new(),
            workers: shuffled_workers(),
            worker_ban: HashMap::new(),
            dir: None,
        })
    })
}

fn shuffled_workers() -> Vec<String> {
    let mut list: Vec<String> = WORKERS.iter().map(|s| s.to_string()).collect();
    let mut seed = RandomState::new().build_hasher().finish() | 1;
    for i in (1..list.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        list.swap(i, (seed % (i as u64 + 1)) as usize);
    }
    list
}

// ─── Персист ────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default)]
struct Persisted {
    tiers: HashMap<String, Tier>,
}

pub fn init(data_dir: PathBuf) {
    let path = data_dir.join(STATE_FILE);
    let loaded: Persisted = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();

    let mut inner = match state().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    inner.dir = Some(data_dir);
    let now = Instant::now();
    for (host, tier) in loaded.tiers {
        if tier == Tier::Direct {
            continue;
        }
        inner.origins.insert(
            host,
            OriginState {
                tier,
                revalidate_at: now + REVALIDATE,
                direct_fails: DIRECT_FAIL_THRESHOLD,
            },
        );
    }
}

fn persist(inner: &Inner) {
    let Some(dir) = inner.dir.clone() else { return };
    let tiers: HashMap<String, Tier> = inner
        .origins
        .iter()
        .filter(|(_, s)| s.tier != Tier::Direct)
        .map(|(h, s)| (h.clone(), s.tier))
        .collect();
    let Ok(bytes) = serde_json::to_vec(&Persisted { tiers }) else {
        return;
    };
    std::thread::spawn(move || {
        let path = dir.join(STATE_FILE);
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, &bytes).is_ok() && std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    });
}

// ─── Разбор URL ─────────────────────────────────────────────

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()?
        .host_str()
        .map(|h| h.to_ascii_lowercase())
}

fn relay_host(origin: &str) -> Option<&'static str> {
    RELAYS.iter().find(|(o, _)| *o == origin).map(|(_, r)| *r)
}

/// Вердикт origin'а, а если своего нет — унаследованный от соседа (см. `INHERIT`).
fn resolved_state<'a>(inner: &'a Inner, origin: &str) -> Option<&'a OriginState> {
    if let Some(s) = inner.origins.get(origin) {
        return Some(s);
    }
    let src = INHERIT.iter().find(|(o, _)| *o == origin).map(|(_, s)| *s)?;
    inner.origins.get(src)
}

fn worker_capable(origin: &str) -> bool {
    !NO_WORKER.contains(&origin)
}

fn swap_host(url: &str, host: &str) -> Option<String> {
    let mut u = url::Url::parse(url).ok()?;
    u.set_scheme("https").ok()?;
    u.set_host(Some(host)).ok()?;
    u.set_port(None).ok()?;
    Some(u.to_string())
}

// ─── Планирование ───────────────────────────────────────────

/// Тиры для запроса. Воркеру подставляется только база — цель (`X-Target`)
/// проставляет вызывающий: у прокси картинок она своя, у остальных — исходный URL.
pub fn plan(url: &str) -> Vec<Hop> {
    let Some(origin) = host_of(url) else {
        return vec![];
    };
    let Some(relay) = relay_host(&origin) else {
        return vec![];
    };

    let mut inner = match state().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    let now = Instant::now();
    let entry = resolved_state(&inner, &origin);
    let tier = entry.map(|s| s.tier).unwrap_or(Tier::Direct);
    // Пока не пришло время ревалидации — тиры ниже залипшего не трогаем,
    // иначе каждый запрос платит таймаутом за заведомо закрытый путь.
    let from = if entry.map(|s| now >= s.revalidate_at).unwrap_or(true) {
        Tier::Direct
    } else {
        tier
    };

    let workers = if worker_capable(&origin) {
        live_workers(&mut inner, now)
    } else {
        vec![]
    };

    let mut hops: Vec<Hop> = Vec::new();
    let mut push = |t: Tier, url: Option<String>| {
        if let Some(u) = url {
            hops.push(Hop {
                url: u,
                tier: t,
                origin: origin.clone(),
            });
        }
    };

    if from <= Tier::Direct {
        push(Tier::Direct, Some(url.to_string()));
    }
    if from <= Tier::Relay {
        push(Tier::Relay, swap_host(url, relay));
    }
    for w in workers {
        push(Tier::Worker, Some(w));
    }
    hops
}

fn live_workers(inner: &mut Inner, now: Instant) -> Vec<String> {
    inner.worker_ban.retain(|_, until| *until > now);
    inner
        .workers
        .iter()
        .filter(|w| !inner.worker_ban.contains_key(*w))
        .cloned()
        .collect()
}

/// Исход одной попытки. Зовётся на каждый пройденный hop.
pub fn note(origin: &str, tier: Tier, ok: bool) {
    if origin.is_empty() {
        return;
    }
    let mut inner = match state().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    let now = Instant::now();

    if ok {
        let prev = inner.origins.get(origin);
        let changed = prev.map(|s| s.tier) != Some(tier);
        // Успех на залипшем тире ничего не меняет — часы ревалидации тикают.
        if !changed && tier != Tier::Direct {
            return;
        }
        inner.origins.insert(
            origin.to_string(),
            OriginState {
                tier,
                revalidate_at: now + REVALIDATE,
                direct_fails: if tier == Tier::Direct {
                    0
                } else {
                    DIRECT_FAIL_THRESHOLD
                },
            },
        );
        if changed {
            persist(&inner);
        }
        return;
    }

    if tier != Tier::Direct {
        return;
    }
    let entry = inner.origins.entry(origin.to_string()).or_insert(OriginState {
        tier: Tier::Direct,
        revalidate_at: now,
        direct_fails: 0,
    });
    entry.direct_fails = entry.direct_fails.saturating_add(1);
    entry.revalidate_at = now + REVALIDATE;
    if entry.tier == Tier::Direct && entry.direct_fails >= DIRECT_FAIL_THRESHOLD {
        entry.tier = Tier::Relay;
        persist(&inner);
    }
}

/// Ответ пришёл — но виноват ли транспорт? Relay отдаёт свои 502/503/504/421,
/// воркер — 429 от самого Cloudflare. И то и другое лечится следующим хопом,
/// а вот ответ origin'а (401/404/500 приложения) — уже валидный результат.
/// `false` = ротируем дальше, исход уже записан.
pub fn hop_ok(hop: &Hop, resp: &reqwest::Response) -> bool {
    let status = resp.status().as_u16();
    match hop.tier {
        Tier::Direct => true,
        Tier::Relay => {
            let bad = matches!(status, 421 | 502 | 503 | 504);
            if bad {
                hop.note(false);
            }
            !bad
        }
        Tier::Worker => {
            if status == 429 && cloudflare_edge_error(resp) {
                ban_worker(&hop.url, true);
                return false;
            }
            if status >= 500 {
                ban_worker(&hop.url, false);
                return false;
            }
            true
        }
    }
}

fn cloudflare_edge_error(resp: &reqwest::Response) -> bool {
    let h = resp.headers();
    let server = h
        .get(reqwest::header::SERVER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let ct = h
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    server.contains("cloudflare") && ct.contains("text/plain")
}

/// Воркер сдох сам (429 от Cloudflare / 5xx) — в бан, ротация на следующий.
pub fn ban_worker(url: &str, rate_limited: bool) {
    let base = url.split('/').take(3).collect::<Vec<_>>().join("/");
    let ttl = if rate_limited {
        WORKER_BAN_RATELIMIT
    } else {
        WORKER_BAN_ERROR
    };
    let mut inner = match state().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    inner.worker_ban.insert(base, Instant::now() + ttl);
}

// ─── Использование из Rust ──────────────────────────────────

/// Подменяет список апстримов проксей картинок на полный веер тиров.
/// `direct` (локальная выкачка своим UA) остаётся как есть.
pub fn expand_upstreams(upstreams: &[String]) -> Vec<Hop> {
    let mut out = Vec::new();
    for u in upstreams {
        if u == "direct" {
            out.push(Hop {
                url: u.clone(),
                tier: Tier::Direct,
                origin: String::new(),
            });
            continue;
        }
        let hops = plan(u);
        if hops.is_empty() {
            out.push(Hop {
                url: u.clone(),
                tier: Tier::Direct,
                origin: host_of(u).unwrap_or_default(),
            });
        } else {
            out.extend(hops);
        }
    }
    out
}

/// URL под текущий вердикт origin'а, ровно один. Для путей, которые гоняют свои
/// URL'ы гонкой (аудио-кеш) — веер тиров там превратился бы в залп по всем
/// воркерам сразу. Воркеры тут не годятся и по сути: качать через них аудио
/// нельзя (лимиты CF), а сам вердикт эти пути только читают — учат его
/// API-слой, прокся картинок и call.
pub fn best_url(url: &str) -> String {
    let Some(origin) = host_of(url) else {
        return url.to_string();
    };
    let Some(relay) = relay_host(&origin) else {
        return url.to_string();
    };
    let inner = match state().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    match resolved_state(&inner, &origin).map(|s| s.tier) {
        None | Some(Tier::Direct) => url.to_string(),
        Some(_) => swap_host(url, relay).unwrap_or_else(|| url.to_string()),
    }
}

/// Текущий (с учётом наследования) тир origin'а URL. `Direct`, если домен не наш
/// или вердикт ещё не выучен — по нему решают, стоит ли пробовать прямой хост.
pub fn current_tier(url: &str) -> Tier {
    let Some(origin) = host_of(url) else {
        return Tier::Direct;
    };
    let inner = match state().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    resolved_state(&inner, &origin)
        .map(|s| s.tier)
        .unwrap_or(Tier::Direct)
}

/// Домен на прямом тире? (прямой хост ещё жив / вердикта нет)
pub fn is_direct(url: &str) -> bool {
    current_tier(url) == Tier::Direct
}

/// Эндпоинты call-релея по тирам (у gRPC воркеров нет).
pub fn call_endpoints(endpoint: &str) -> Vec<Hop> {
    let hops = plan(endpoint);
    if hops.is_empty() {
        return vec![Hop {
            url: endpoint.to_string(),
            tier: Tier::Direct,
            origin: host_of(endpoint).unwrap_or_default(),
        }];
    }
    hops
}

// ─── Мост во фронт ──────────────────────────────────────────

#[derive(Serialize)]
pub struct EdgeConfig {
    /// origin → relay-хост.
    relays: Vec<(String, String)>,
    /// Воркеры в порядке этой установки.
    workers: Vec<String>,
    /// Origin'ы без воркер-тира.
    no_worker: Vec<String>,
    /// Уже известный тир — фронт стартует с него, не платя таймаутом.
    hints: HashMap<String, Tier>,
    revalidate_ms: u64,
}

#[tauri::command]
pub fn edge_config() -> EdgeConfig {
    let inner = match state().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    EdgeConfig {
        relays: RELAYS
            .iter()
            .map(|(o, r)| (o.to_string(), r.to_string()))
            .collect(),
        workers: inner.workers.clone(),
        no_worker: NO_WORKER.iter().map(|s| s.to_string()).collect(),
        hints: inner
            .origins
            .iter()
            .map(|(h, s)| (h.clone(), s.tier))
            .collect(),
        revalidate_ms: REVALIDATE.as_millis() as u64,
    }
}

#[tauri::command]
pub fn edge_note(origin: String, tier: Tier, ok: bool) {
    note(&origin, tier, ok);
}

#[tauri::command]
pub fn edge_ban_worker(url: String, rate_limited: bool) {
    ban_worker(&url, rate_limited);
}
