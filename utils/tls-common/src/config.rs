use std::path::PathBuf;

pub struct TlsConfig {
    pub domains: Vec<String>,
    pub email: String,
    pub cache_dir: PathBuf,
    pub staging: bool,
    pub https_port: u16,
    pub http_port: u16,
    pub http_redirect: bool,
    pub proxy_protocol: bool,
}

impl TlsConfig {
    /// `Some` когда TLS_ENABLED=true; иначе `None`.
    /// Паника если TLS_ENABLED=true, а DOMAINS пустой — fail fast at boot.
    pub fn from_env() -> Option<Self> {
        if !env_bool("TLS_ENABLED", false) {
            return None;
        }

        let domains = parse_csv(&std::env::var("DOMAINS").unwrap_or_default());
        if domains.is_empty() {
            panic!("TLS_ENABLED=true but DOMAINS is empty (expected comma-separated domain list)");
        }

        let email = std::env::var("ACME_EMAIL").unwrap_or_else(|_| format!("admin@{}", domains[0]));
        let cache_dir = PathBuf::from(
            std::env::var("ACME_CACHE_DIR").unwrap_or_else(|_| "/var/cache/acme".to_string()),
        );

        Some(Self {
            domains,
            email,
            cache_dir,
            staging: env_bool("ACME_STAGING", false),
            https_port: env_u16("TLS_HTTPS_PORT", 443),
            http_port: env_u16("TLS_HTTP_PORT", 80),
            // HTTP→HTTPS 301 by default; off для смешанного режима.
            http_redirect: env_bool("TLS_HTTP_REDIRECT", true),
            // PROXY v1 (haproxy `send-proxy`) — читаем real client addr перед TLS.
            proxy_protocol: env_bool("TLS_PROXY_PROTOCOL", false),
        })
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

fn env_u16(key: &str, default: u16) -> u16 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn parse_csv(v: &str) -> Vec<String> {
    v.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
