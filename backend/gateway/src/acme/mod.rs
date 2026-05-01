pub mod issuer;

use std::path::Path;
use std::time::Duration;

use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::tls::TlsState;

const RENEW_BEFORE_DAYS: i64 = 7;
const RENEWAL_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);

pub async fn ensure_initial_cert(cfg: &Config) -> std::io::Result<()> {
    tokio::fs::create_dir_all(&cfg.tls.cache_dir).await?;
    if let Ok(days) = remaining_days(&cfg.cert_path()).await {
        if days > RENEW_BEFORE_DAYS {
            info!("cert valid for {days}d — skipping issue");
            return Ok(());
        }
        warn!("cert expires in {days}d — re-issuing");
    } else {
        info!("no cert in cache — issuing fresh");
    }
    let (cert_pem, key_pem) = issuer::issue(cfg, None)
        .await
        .map_err(std::io::Error::other)?;
    write_pem(&cfg.cert_path(), cert_pem.as_bytes()).await?;
    write_pem(&cfg.key_path(), key_pem.as_bytes()).await?;
    Ok(())
}

pub fn spawn_renew_loop(cfg: Config, tls: TlsState) -> JoinHandle<()> {
    tokio::spawn(async move {
        let cert_path = cfg.cert_path();
        let key_path = cfg.key_path();
        loop {
            tokio::time::sleep(RENEWAL_CHECK_INTERVAL).await;
            let days = match remaining_days(&cert_path).await {
                Ok(d) => d,
                Err(e) => {
                    warn!("[renew] cert read: {e}");
                    continue;
                }
            };
            if days > RENEW_BEFORE_DAYS {
                info!("[renew] {days}d remaining");
                continue;
            }
            warn!("[renew] {days}d remaining → re-issuing");
            let maps = (tls.http_challenges.clone(), tls.alpn_challenges.clone());
            match issuer::issue(&cfg, Some(maps)).await {
                Ok((cert_pem, key_pem)) => {
                    if let Err(e) = write_pem(&cert_path, cert_pem.as_bytes()).await {
                        error!("[renew] write cert: {e}");
                        continue;
                    }
                    if let Err(e) = write_pem(&key_path, key_pem.as_bytes()).await {
                        error!("[renew] write key: {e}");
                        continue;
                    }
                    if let Err(e) = tls.reload(&cert_path, &key_path).await {
                        error!("[renew] tls reload: {e}");
                        continue;
                    }
                    info!("[renew] cert reloaded");
                }
                Err(e) => error!("[renew] issue failed: {e}"),
            }
        }
    })
}

async fn remaining_days(cert_path: &Path) -> std::io::Result<i64> {
    let pem_bytes = tokio::fs::read(cert_path).await?;
    let (_, parsed) = x509_parser::pem::parse_x509_pem(&pem_bytes)
        .map_err(|e| std::io::Error::other(format!("pem: {e}")))?;
    let (_, cert) = x509_parser::parse_x509_certificate(&parsed.contents)
        .map_err(|e| std::io::Error::other(format!("x509: {e}")))?;
    let not_after = cert.validity().not_after.timestamp();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok((not_after - now) / 86400)
}

async fn write_pem(path: &Path, content: &[u8]) -> std::io::Result<()> {
    tokio::fs::write(path, content).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = tokio::fs::metadata(path).await?.permissions();
        perm.set_mode(0o600);
        tokio::fs::set_permissions(path, perm).await?;
    }
    Ok(())
}
