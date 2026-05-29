use std::net::IpAddr;
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
    /// Peers allowed to dictate the client addr via PROXY v1 (haproxy). Others' PROXY headers are ignored.
    pub proxy_trusted_cidrs: Vec<IpCidr>,
    /// Hostnames resolved (and periodically re-resolved) to trusted peer IPs —
    /// e.g. `haproxy`, so the allowlist survives container recreation.
    pub proxy_trusted_hosts: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct IpCidr {
    net: IpAddr,
    prefix: u8,
}

impl IpCidr {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (ip_s, prefix) = match s.split_once('/') {
            Some((a, b)) => (a, b.trim().parse::<u8>().ok()?),
            None => (s, if s.contains(':') { 128 } else { 32 }),
        };
        let net: IpAddr = ip_s.trim().parse().ok()?;
        let max = if net.is_ipv6() { 128 } else { 32 };
        if prefix > max {
            return None;
        }
        Some(Self { net, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.net, ip) {
            (IpAddr::V4(n), IpAddr::V4(i)) => prefix_match(&n.octets(), &i.octets(), self.prefix),
            (IpAddr::V6(n), IpAddr::V6(i)) => prefix_match(&n.octets(), &i.octets(), self.prefix),
            _ => false,
        }
    }
}

fn prefix_match(a: &[u8], b: &[u8], prefix: u8) -> bool {
    let full = (prefix / 8) as usize;
    if a[..full] != b[..full] {
        return false;
    }
    let rem = prefix % 8;
    if rem == 0 {
        return true;
    }
    let mask = 0xFFu8 << (8 - rem);
    (a[full] & mask) == (b[full] & mask)
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
            proxy_protocol: env_bool("TLS_PROXY_PROTOCOL", false),
            proxy_trusted_cidrs: parse_csv(
                &std::env::var("TLS_PROXY_TRUSTED_CIDRS").unwrap_or_default(),
            )
                .iter()
                .filter_map(|s| IpCidr::parse(s))
                .collect(),
            proxy_trusted_hosts: parse_csv(
                &std::env::var("TLS_PROXY_TRUSTED_HOSTS").unwrap_or_default(),
            ),
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
