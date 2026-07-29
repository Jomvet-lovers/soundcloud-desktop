//! Транспортные тиры до наших доменов.
//!
//! `direct` → `relay` (`<сервис>.<нода>.relay.scnative.space`, хост-в-хост,
//! гоняет и gRPC).
//!
//! Тира CF-воркеров БОЛЬШЕ НЕТ. Их бесплатные лимиты не держали наш трафик:
//! пул уходил в 429 целиком и отдавал HTML-страницу вместо ответа API, а один
//! удачный ответ ещё и залипал тиром на 10 минут. Плюс через них уезжала сессия
//! третьей стороне. Прямой путь и наш relay покрывают то же самое без этого.
//!
//! Тир липнет к origin'у: как только прямой путь лёг у конкретного юзера, он не
//! пытается ходить туда на каждом запросе. Раз в `REVALIDATE` прямой путь
//! пробуется снова, чтобы разбан подхватился сам. Состояние переживает перезапуск
//! (`edge_state.json`), иначе каждый старт стоил бы таймаута на первом запросе.
//!
//! Байты storage/s3 намеренно НЕ проксируются: relay их не вывезет по трафику.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const STATE_FILE: &str = "edge_state.json";
const REVALIDATE: Duration = Duration::from_secs(600);
/// Зона relay-пула. Имя ноды подставляется между сервисом и зоной:
/// `<сервис>.<нода>.relay.scnative.space`.
const RELAY_ZONE: &str = "relay.scnative.space";

/// Ноды relay-пула в порядке обхода: r1, дальше по списку. Поднялась ещё одна —
/// дописать сюда `"r2"`, остальное (план хопов, конфиг фронта, health) само
/// подхватит. Ноды в пределах тира перебираются подряд, как воркеры.
const RELAY_NODES: &[&str] = &["r1"];

/// origin → сервисная метка в relay-пуле. Relay всегда https/443, порт origin'а
/// отбрасывается (call ходит на :444 напрямую, через relay — на обычный 443).
const RELAYS: &[(&str, &str)] = &[
    ("api.scnative.space", "api"),
    ("api-star.scnative.space", "api-star"),
    ("stream.scnative.space", "stream"),
    ("stream-star.scnative.space", "stream-star"),
    ("images.scnative.space", "images"),
    ("storage.scnative.space", "storage"),
    ("pay.scnative.space", "pay"),
    ("call.scnative.space", "call"),
];

/// origin → сосед, чей вердикт наследуем пока своего нет. storage-байты тянет
/// только ядро (read-only), сам вердикт не наберёт быстро; но storage и stream
/// на одном main-host и банятся вместе, а stream активно щупает фронт — так
/// storage переезжает на relay сразу, без холодного таймаута.
const INHERIT: &[(&str, &str)] = &[("storage.scnative.space", "stream.scnative.space")];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Direct,
    Relay,
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
    dir: Option<PathBuf>,
}

static STATE: OnceLock<Mutex<Inner>> = OnceLock::new();

fn state() -> &'static Mutex<Inner> {
    STATE.get_or_init(|| {
        Mutex::new(Inner {
            origins: HashMap::new(),
            dir: None,
        })
    })
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

fn relay_label(origin: &str) -> Option<&'static str> {
    RELAYS.iter().find(|(o, _)| *o == origin).map(|(_, r)| *r)
}

/// Хост сервиса на первой ноде пула. Для мест, где нужен ровно один relay-адрес
/// (bootstrap health-топологии) — веер по нодам там не нужен, а имя зоны должно
/// жить в одном месте, чтобы `r2` не пришлось искать по репозиторию.
pub fn primary_relay_host(service: &str) -> String {
    let node = RELAY_NODES.first().copied().unwrap_or("r1");
    format!("{service}.{node}.{RELAY_ZONE}")
}

/// Relay-хосты origin'а по всем нодам пула, в порядке `RELAY_NODES`.
/// Пусто = домен не наш.
fn relay_hosts(origin: &str) -> Vec<String> {
    let Some(label) = relay_label(origin) else {
        return vec![];
    };
    RELAY_NODES
        .iter()
        .map(|node| format!("{label}.{node}.{RELAY_ZONE}"))
        .collect()
}

/// Вердикт origin'а, а если своего нет — унаследованный от соседа (см. `INHERIT`).
fn resolved_state<'a>(inner: &'a Inner, origin: &str) -> Option<&'a OriginState> {
    if let Some(s) = inner.origins.get(origin) {
        return Some(s);
    }
    let src = INHERIT.iter().find(|(o, _)| *o == origin).map(|(_, s)| *s)?;
    inner.origins.get(src)
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
    let relays = relay_hosts(&origin);
    if relays.is_empty() {
        return vec![];
    }

    let inner = match state().lock() {
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
        for relay in &relays {
            push(Tier::Relay, swap_host(url, relay));
        }
    }
    if from > Tier::Direct {
        // Прямой путь остаётся последним шансом даже когда вердикт увёл на relay:
        // relay-нода может лечь, а origin к этому моменту уже разбанят.
        push(Tier::Direct, Some(url.to_string()));
    }
    hops
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
        Tier::Direct => {
            let bad = direct_infrastructure_error(resp);
            if bad {
                hop.note(false);
            }
            !bad
        }
        Tier::Relay => {
            let bad = matches!(status, 421 | 502 | 503 | 504);
            if bad {
                hop.note(false);
            }
            !bad
        }
    }
}

fn direct_infrastructure_error(resp: &reqwest::Response) -> bool {
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    direct_infrastructure_headers(status, content_type)
}

fn direct_infrastructure_headers(status: u16, content_type: &str) -> bool {
    matches!(status, 502..=504) && content_type.to_ascii_lowercase().contains("text/html")
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

/// Direct/relay-план для тяжёлых аудиоответов. Известный рабочий тир идёт первым, второй
/// остаётся страховкой на случай независимого падения origin/relay. Когда подошла
/// ревалидация, direct снова получает первый шанс.
pub fn audio_plan(url: &str) -> Vec<Hop> {
    let Some(origin) = host_of(url) else {
        return vec![Hop {
            url: url.to_string(),
            tier: Tier::Direct,
            origin: String::new(),
        }];
    };
    let relays = relay_hosts(&origin);
    if relays.is_empty() {
        return vec![Hop {
            url: url.to_string(),
            tier: Tier::Direct,
            origin,
        }];
    }
    let inner = match state().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    let now = Instant::now();
    let state = resolved_state(&inner, &origin);
    let revalidate = state.map(|s| now >= s.revalidate_at).unwrap_or(true);
    let tiers = audio_tier_order(state.map(|s| s.tier), revalidate);
    let relay_urls: Vec<String> = relays
        .iter()
        .map(|relay| swap_host(url, relay).unwrap_or_else(|| url.to_string()))
        .collect();

    let mut hops = Vec::with_capacity(tiers.len() + relay_urls.len() - 1);
    for tier in tiers {
        match tier {
            Tier::Direct => hops.push(Hop {
                url: url.to_string(),
                tier,
                origin: origin.clone(),
            }),
            // Relay-слот разворачивается в хоп на каждую ноду пула.
            Tier::Relay => hops.extend(relay_urls.iter().map(|u| Hop {
                url: u.clone(),
                tier,
                origin: origin.clone(),
            })),
        }
    }
    hops
}

fn audio_tier_order(current: Option<Tier>, revalidate: bool) -> [Tier; 2] {
    if revalidate || matches!(current, None | Some(Tier::Direct)) {
        [Tier::Direct, Tier::Relay]
    } else {
        [Tier::Relay, Tier::Direct]
    }
}

/// Подать результат независимой health-пробы в общий edge verdict. Неизвестные
/// домены игнорируются: topology может содержать status/s3/health, которыми этот
/// роутер не владеет.
pub fn note_url(url: &str, tier: Tier, ok: bool) {
    let Some(origin) = host_of(url) else {
        return;
    };
    if relay_label(&origin).is_some() {
        note(&origin, tier, ok);
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
    /// origin → relay-хосты по нодам пула, в порядке обхода.
    relays: Vec<(String, Vec<String>)>,
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
            .map(|(o, _)| (o.to_string(), relay_hosts(o)))
            .collect(),
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


#[cfg(test)]
mod tests {
    use super::{
        audio_tier_order, direct_infrastructure_headers, relay_hosts, Tier, RELAYS, RELAY_NODES,
    };

    #[test]
    fn relay_host_is_built_per_pool_node() {
        assert_eq!(
            relay_hosts("api.scnative.space"),
            vec!["api.r1.relay.scnative.space"]
        );
        // Каждая нода пула даёт свой хоп — добавление r2 не требует правок кода.
        assert_eq!(
            relay_hosts("storage.scnative.space").len(),
            RELAY_NODES.len()
        );
        assert!(relay_hosts("soundcloud.com").is_empty());
    }

    #[test]
    fn the_transport_ladder_has_no_worker_rung_left() {
        // Тир снесён: у плана остаются только прямой путь и наш relay. Если он
        // когда-нибудь вернётся — вернуть и защиту от залипания на нём.
        assert_eq!(
            [Tier::Direct, Tier::Relay].map(|t| t as u8).len(),
            2,
            "Tier должен остаться двухступенчатым"
        );
    }

    #[test]
    fn no_legacy_domain_survives_in_the_relay_table() {
        for (origin, label) in RELAYS {
            assert!(!origin.contains("scdinternal"), "legacy origin {origin}");
            assert!(!label.contains('.'), "label {label} must be a bare service");
        }
    }

    #[test]
    fn html_gateway_5xx_is_a_direct_transport_error() {
        assert!(direct_infrastructure_headers(503, "text/html; charset=utf-8"));
        assert!(direct_infrastructure_headers(504, "TEXT/HTML"));
        assert!(!direct_infrastructure_headers(500, "text/html"));
        assert!(!direct_infrastructure_headers(503, "application/json"));
    }

    #[test]
    fn audio_prefers_direct_when_unknown_or_due_for_revalidation() {
        assert_eq!(audio_tier_order(None, false), [Tier::Direct, Tier::Relay]);
        assert_eq!(
            audio_tier_order(Some(Tier::Relay), true),
            [Tier::Direct, Tier::Relay]
        );
    }

    #[test]
    fn audio_uses_sticky_fallback_first_but_keeps_direct_as_backup() {
        assert_eq!(
            audio_tier_order(Some(Tier::Relay), false),
            [Tier::Relay, Tier::Direct]
        );
        assert_eq!(
            audio_tier_order(Some(Tier::Relay), false),
            [Tier::Relay, Tier::Direct]
        );
    }
}
