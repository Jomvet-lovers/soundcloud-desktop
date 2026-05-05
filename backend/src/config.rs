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
    pub ltr: LtrCfg,
    pub lyrics: LyricsCfg,
    pub netease: NeteaseCfg,
    pub mxm: MxmCfg,
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
}

#[derive(Clone, Debug)]
pub struct SoundwaveCfg {
    pub collab_weight: f64,
    pub audio_weight: f64,
    pub clap_weight: f64,
    pub lyrics_weight: f64,
    pub popularity_boost: f64,
    pub artist_cap: usize,
    pub score_threshold: f64,
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
pub struct LtrCfg {
    pub auto_train: bool,
    pub rerank_enabled: bool,
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
            },

            soundwave: SoundwaveCfg {
                collab_weight: env_f64("SOUNDWAVE_COLLAB_WEIGHT", 0.55),
                audio_weight: env_f64("SOUNDWAVE_AUDIO_WEIGHT", 0.20),
                clap_weight: env_f64("SOUNDWAVE_CLAP_WEIGHT", 0.10),
                lyrics_weight: env_f64("SOUNDWAVE_LYRICS_WEIGHT", 0.15),
                popularity_boost: env_f64("SOUNDWAVE_POPULARITY_BOOST", 0.0),
                artist_cap: env_usize("SOUNDWAVE_ARTIST_CAP", 2),
                score_threshold: env_f64("SOUNDWAVE_SCORE_THRESHOLD", 0.05),
            },

            collab: CollabCfg {
                auto_train: env_str("COLLAB_AUTO_TRAIN", "true") != "false",
                trigger_events: env_u32("COLLAB_TRIGGER_EVENTS", 100),
                trigger_cooldown_ms: env_u64("COLLAB_TRIGGER_COOLDOWN_MS", 600_000),
                dim: env_u32("COLLAB_DIM", 128),
                min_count: env_u32("COLLAB_MIN_COUNT", 3),
                min_sessions: env_u32("COLLAB_MIN_SESSIONS", 20),
            },

            ltr: LtrCfg {
                auto_train: env_str("LTR_AUTO_TRAIN", "true") != "false",
                rerank_enabled: env_str("LTR_RERANK_ENABLED", "true") != "false",
            },

            lyrics: LyricsCfg {
                indexing_concurrency: env_usize("LYRICS_INDEXING_CONCURRENCY", 3),
            },

            netease: NeteaseCfg {
                api_base: env_str("NETEASE_API_BASE", "https://ncm.nekohasegawa.com"),
            },

            mxm: MxmCfg {
                api_base: env_str("MUSIXMATCH_API_BASE", "https://apic-desktop.musixmatch.com/ws/1.1"),
            },
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
