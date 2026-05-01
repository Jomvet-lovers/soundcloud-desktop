use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Config {
    pub backend_count: usize,
    pub backend_command: Vec<OsString>,
    pub socket_dir: PathBuf,
    pub http_port: u16,
    pub https_port: u16,
    pub redirect_http: bool,
    pub health_path: String,
    pub health_interval: Duration,
    pub health_timeout: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    pub kill_grace: Duration,
    pub tls: TlsConfig,
}

#[derive(Clone, Debug)]
pub struct TlsConfig {
    pub enabled: bool,
    pub domains: Vec<String>,
    pub email: String,
    pub cache_dir: PathBuf,
    pub staging: bool,
}

impl Config {
    pub fn from_env(argv: Vec<OsString>) -> Result<Self, String> {
        if argv.is_empty() {
            return Err("backend command not provided (argv[1..])".into());
        }

        let backend_count = parse_usize("BACKEND_WORKERS")
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1))
            .max(1);

        let socket_dir = PathBuf::from(
            env::var("BACKEND_SOCKET_DIR").unwrap_or_else(|_| "/run/scd".to_string()),
        );

        let http_port = parse_u16("TLS_HTTP_PORT").unwrap_or(80);
        let https_port = parse_u16("TLS_HTTPS_PORT").unwrap_or(443);

        let tls_enabled = parse_bool("TLS_ENABLED", false);
        let domains = parse_csv(&env::var("DOMAINS").unwrap_or_default());
        if tls_enabled && domains.is_empty() {
            return Err("TLS_ENABLED=true but DOMAINS is empty".into());
        }
        let email = env::var("ACME_EMAIL")
            .unwrap_or_else(|_| format!("admin@{}", domains.first().cloned().unwrap_or_default()));
        let cache_dir = PathBuf::from(
            env::var("ACME_CACHE_DIR").unwrap_or_else(|_| "/var/cache/acme".to_string()),
        );
        let staging = parse_bool("ACME_STAGING", false);

        Ok(Self {
            backend_count,
            backend_command: argv,
            socket_dir,
            http_port,
            https_port,
            redirect_http: parse_bool("TLS_HTTP_REDIRECT", true),
            health_path: env::var("HEALTH_PATH").unwrap_or_else(|_| "/health".to_string()),
            health_interval: Duration::from_millis(parse_u64("HEALTH_INTERVAL_MS").unwrap_or(2_000)),
            health_timeout: Duration::from_millis(parse_u64("HEALTH_TIMEOUT_MS").unwrap_or(1_500)),
            backoff_min: Duration::from_millis(parse_u64("BACKOFF_MIN_MS").unwrap_or(500)),
            backoff_max: Duration::from_millis(parse_u64("BACKOFF_MAX_MS").unwrap_or(30_000)),
            kill_grace: Duration::from_millis(parse_u64("KILL_GRACE_MS").unwrap_or(10_000)),
            tls: TlsConfig {
                enabled: tls_enabled,
                domains,
                email,
                cache_dir,
                staging,
            },
        })
    }

    pub fn cert_path(&self) -> PathBuf {
        self.tls.cache_dir.join("cert.pem")
    }

    pub fn key_path(&self) -> PathBuf {
        self.tls.cache_dir.join("key.pem")
    }
}

fn parse_bool(key: &str, default: bool) -> bool {
    env::var(key)
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

fn parse_u16(key: &str) -> Option<u16> {
    env::var(key).ok().and_then(|v| v.parse().ok())
}

fn parse_u64(key: &str) -> Option<u64> {
    env::var(key).ok().and_then(|v| v.parse().ok())
}

fn parse_usize(key: &str) -> Option<usize> {
    env::var(key).ok().and_then(|v| v.parse().ok())
}

fn parse_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}
