use std::time::Duration;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub port: u16,

    pub soundcloud: SoundcloudCfg,
    pub database: DatabaseCfg,
    pub streaming: StreamingCfg,
    pub admin: AdminCfg,
    pub redis: RedisCfg,
    pub nats: NatsCfg,
    pub qdrant: QdrantCfg,
    pub storage: StorageCfg,
    pub internal: InternalCfg,
    pub subscriptions: SubscriptionsCfg,
    pub soundwave: SoundwaveCfg,
    pub collab: CollabCfg,
    pub lyrics: LyricsCfg,
    pub netease: NeteaseCfg,
    pub mxm: MxmCfg,
    pub genius: GeniusCfg,
    pub enrich: EnrichCfg,
    pub enrich_crawl: EnrichCrawlCfg,
    pub discovery: DiscoveryCfg,
    pub cold: ColdCfg,
    pub max_track_duration_ms: i32,
    /// Нода-резерв для премиума: фоновые пайплайны off, ендпоинты только премиум.
    pub premium_reserve: bool,
}

/// TTL'и для cold-cache. Если sc_synced_at старше TTL — на чтении спавним
/// фоновый refresh (с Redis SETNX-дедупом).
#[derive(Clone, Debug)]
pub struct ColdCfg {
    pub track_ttl_sec: u64,
    pub user_ttl_sec: u64,
    pub playlist_ttl_sec: u64,
    pub liked_tracks_ttl_sec: u64,
    pub liked_playlists_ttl_sec: u64,
    pub followings_ttl_sec: u64,
    pub owned_ttl_sec: u64,
    #[allow(dead_code)]
    pub evict_after_sec: u64,
    pub refresh_concurrency: usize,
    pub refresh_lock_ttl_sec: u64,
}

#[derive(Clone, Debug)]
pub struct GeniusCfg {
    pub access_token: String,
    pub max_concurrent_scrapes: usize,
}

#[derive(Clone, Debug)]
pub struct SoundcloudCfg {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub proxy_url: String,
    pub proxy_fallback: bool,
}

#[derive(Clone, Debug)]
pub struct DatabaseCfg {
    pub url: String,
    pub pool_max: u32,
    pub acquire_timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct StreamingCfg {
    pub service_url: String,
}

#[derive(Clone, Debug)]
pub struct AdminCfg {
    pub token: String,
}

#[derive(Clone, Debug)]
pub struct RedisCfg {
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct NatsCfg {
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct QdrantCfg {
    pub url: String,
    pub api_key: String,
}

#[derive(Clone, Debug)]
pub struct StorageCfg {
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct InternalCfg {
    pub token: String,
}

#[derive(Clone, Debug)]
pub struct SubscriptionsCfg {
    pub snapshot_dir: String,
    pub always_premium: bool,
}

#[derive(Clone, Debug)]
pub struct SoundwaveCfg {
    /// Бонус к score за популярность трека (log(playback_count) * boost).
    /// Применяется в `enrich_and_boost` для search.
    pub popularity_boost: f64,
    /// Сколько треков одного артиста максимум помещается в выдачу (anti-spam).
    pub artist_cap: usize,
}

#[derive(Clone, Debug)]
pub struct CollabCfg {
    pub auto_train: bool,
    pub trigger_events: u32,
    pub trigger_cooldown_ms: u64,
    pub dim: u32,
    pub min_count: u32,
    pub min_sessions: u32,
}

#[derive(Clone, Debug)]
pub struct LyricsCfg {
    pub indexing_concurrency: usize,
}

#[derive(Clone, Debug)]
pub struct NeteaseCfg {
    pub api_base: String,
}

#[derive(Clone, Debug)]
pub struct MxmCfg {
    pub api_base: String,
}

#[derive(Clone, Debug)]
pub struct EnrichCfg {
    pub enabled: bool,
    pub mb_user_agent: String,
    pub mb_rate_limit_ms: u64,
    pub max_attempts: u32,
    pub ai_enabled: bool,
    pub ai_timeout_ms: u64,
    pub ai_daily_budget: u64,
    pub consumer_concurrency: usize,
}

#[derive(Clone, Debug)]
pub struct EnrichCrawlCfg {
    pub interval_sec: u64,
}

/// Catalog discovery (crawl every artist on Genius/MB) + wanted-track resolve,
/// all on the work::Scheduler substrate. Separate Genius (proxy-parallel) and MB
/// (serialized) lanes; no confidence floor, no lifetime cap — every artist with
/// an external id is reachable on a freshness cadence.
#[derive(Clone, Debug)]
pub struct DiscoveryCfg {
    pub enabled: bool,
    pub genius_concurrency: usize,
    pub mb_concurrency: usize,
    pub batch: i64,
    pub recrawl_days: i64,
    pub max_fails: i16,
    pub interest_interval_sec: u64,
    pub account_concurrency: usize,
    pub account_walk_days: i64,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(url) if !url.is_empty() => url,
            _ => {
                let host = env_str("DATABASE_HOST", "localhost");
                let port = env_u16("DATABASE_PORT", 5432);
                let user = env_str("DATABASE_USERNAME", "soundcloud");
                let pass = env_str("DATABASE_PASSWORD", "soundcloud");
                let name = env_str("DATABASE_NAME", "soundcloud_desktop");
                format!("postgres://{user}:{pass}@{host}:{port}/{name}")
            }
        };

        Self {
            port: env_u16("PORT", 3000),

            soundcloud: SoundcloudCfg {
                client_id: env_str("SOUNDCLOUD_CLIENT_ID", ""),
                client_secret: env_str("SOUNDCLOUD_CLIENT_SECRET", ""),
                redirect_uri: env_str(
                    "SOUNDCLOUD_REDIRECT_URI",
                    "http://localhost:3000/auth/callback",
                ),
                proxy_url: env_str("SC_PROXY_URL", ""),
                proxy_fallback: env_str("SC_PROXY_FALLBACK", "") == "true",
            },

            database: DatabaseCfg {
                url: database_url,
                pool_max: env_u32("PG_POOL_MAX", 20),
                acquire_timeout: Duration::from_secs(env_u64("PG_ACQUIRE_TIMEOUT_SECS", 10)),
            },

            streaming: StreamingCfg {
                service_url: env_str("STREAMING_SERVICE_URL", "http://localhost:8080"),
            },

            admin: AdminCfg {
                token: env_str("ADMIN_TOKEN", ""),
            },

            redis: RedisCfg {
                url: env_str("REDIS_URL", "redis://localhost:6379"),
            },

            nats: NatsCfg {
                url: env_str("NATS_URL", "nats://localhost:4222"),
            },

            qdrant: QdrantCfg {
                url: env_str("QDRANT_URL", "http://localhost:6333"),
                api_key: env_str("QDRANT_API_KEY", ""),
            },

            storage: StorageCfg {
                url: env_str("STORAGE_URL", "https://storage.scdinternal.site"),
            },

            internal: InternalCfg {
                token: env_str("INTERNAL_TOKEN", ""),
            },

            subscriptions: SubscriptionsCfg {
                snapshot_dir: env_str("SUBSCRIPTIONS_SNAPSHOT_DIR", "/snapshots"),
                always_premium: env_str("SUBSCRIPTIONS_ALWAYS_PREMIUM", "false") == "true",
            },

            soundwave: SoundwaveCfg {
                popularity_boost: env_f64("SOUNDWAVE_POPULARITY_BOOST", 0.0),
                artist_cap: env_usize("SOUNDWAVE_ARTIST_CAP", 2),
            },

            collab: CollabCfg {
                auto_train: env_str("COLLAB_AUTO_TRAIN", "true") != "false",
                trigger_events: env_u32("COLLAB_TRIGGER_EVENTS", 100),
                trigger_cooldown_ms: env_u64("COLLAB_TRIGGER_COOLDOWN_MS", 600_000),
                dim: env_u32("COLLAB_DIM", 128),
                min_count: env_u32("COLLAB_MIN_COUNT", 3),
                min_sessions: env_u32("COLLAB_MIN_SESSIONS", 20),
            },

            lyrics: LyricsCfg {
                indexing_concurrency: env_usize("LYRICS_INDEXING_CONCURRENCY", 3),
            },

            netease: NeteaseCfg {
                api_base: env_str("NETEASE_API_BASE", "https://ncm.nekohasegawa.com"),
            },

            mxm: MxmCfg {
                api_base: env_str(
                    "MUSIXMATCH_API_BASE",
                    "https://apic-desktop.musixmatch.com/ws/1.1",
                ),
            },

            genius: GeniusCfg {
                access_token: env_str("GENIUS_ACCESS_TOKEN", ""),
                max_concurrent_scrapes: env_usize("GENIUS_MAX_CONCURRENT_SCRAPES", 150),
            },

            enrich: EnrichCfg {
                enabled: env_str("ENRICH_ENABLED", "true") != "false",
                mb_user_agent: env_str(
                    "ENRICH_MB_USER_AGENT",
                    "scd-backend/0.1 ( https://scdinternal.site )",
                ),
                mb_rate_limit_ms: env_u64("ENRICH_MB_RATE_LIMIT_MS", 1100),
                max_attempts: env_u32("ENRICH_MAX_ATTEMPTS", 5),
                ai_enabled: env_str("ENRICH_AI_ENABLED", "true") != "false",
                ai_timeout_ms: env_u64("ENRICH_AI_TIMEOUT_MS", 20_000),
                ai_daily_budget: env_u64("ENRICH_AI_DAILY_BUDGET", 5000),
                consumer_concurrency: env_usize("ENRICH_CONSUMER_CONCURRENCY", 32),
            },

            enrich_crawl: EnrichCrawlCfg {
                interval_sec: env_u64("ENRICH_CRAWL_INTERVAL_SEC", 3600),
            },

            discovery: DiscoveryCfg {
                enabled: env_str("DISCOVERY_ENABLED", "true") != "false",
                genius_concurrency: env_usize("DISCOVERY_GENIUS_CONCURRENCY", 8),
                mb_concurrency: env_usize("DISCOVERY_MB_CONCURRENCY", 1),
                batch: env_u64("DISCOVERY_BATCH", 64) as i64,
                recrawl_days: env_u64("DISCOVERY_RECRAWL_DAYS", 14) as i64,
                max_fails: env_u32("DISCOVERY_MAX_FAILS", 8) as i16,
                interest_interval_sec: env_u64("DISCOVERY_INTEREST_INTERVAL_SEC", 3600),
                account_concurrency: env_usize("DISCOVERY_ACCOUNT_CONCURRENCY", 6),
                account_walk_days: env_u64("DISCOVERY_ACCOUNT_WALK_DAYS", 2) as i64,
            },

            cold: ColdCfg {
                track_ttl_sec: env_u64("COLD_TTL_TRACK_SEC", 21600),
                user_ttl_sec: env_u64("COLD_TTL_USER_SEC", 21600),
                playlist_ttl_sec: env_u64("COLD_TTL_PLAYLIST_SEC", 3600),
                liked_tracks_ttl_sec: env_u64("COLD_TTL_LIKED_TRACKS_SEC", 1800),
                liked_playlists_ttl_sec: env_u64("COLD_TTL_LIKED_PLAYLISTS_SEC", 1800),
                followings_ttl_sec: env_u64("COLD_TTL_FOLLOWINGS_SEC", 1800),
                owned_ttl_sec: env_u64("COLD_TTL_OWNED_SEC", 300),
                evict_after_sec: env_u64("COLD_EVICT_AFTER_SEC", 2_592_000),
                refresh_concurrency: env_usize("COLD_REFRESH_CONCURRENCY", 32),
                refresh_lock_ttl_sec: env_u64("COLD_REFRESH_LOCK_TTL_SEC", 60),
            },

            max_track_duration_ms: (env_u64("MAX_TRACK_DURATION_SEC", 420) * 1000) as i32,
            premium_reserve: env_str("PREMIUM_RESERVE", "false") == "true",
        }
    }
}

fn env_str(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn env_u16(key: &str, default: u16) -> u16 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_f64(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
