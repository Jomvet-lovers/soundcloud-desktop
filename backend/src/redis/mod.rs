use deadpool_redis::{Config, Pool, Runtime};

use crate::config::AppConfig;

pub fn connect(cfg: &AppConfig) -> Result<Pool, deadpool_redis::CreatePoolError> {
    let rcfg = Config::from_url(&cfg.redis.url);
    rcfg.create_pool(Some(Runtime::Tokio1))
}
