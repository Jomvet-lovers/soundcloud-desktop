//! Loopback rpc-фасад ядра: web-бандл (Linux-шелл / дев-превью) говорит с
//! ScRuntime через 127.0.0.1 — POST /rpc/{method} + WS /events. Доступ закрыт
//! одноразовым токеном (печатается в stdout вместе с портом при старте).

mod dispatch;
mod events;
mod http;

use std::path::PathBuf;

use clap::Parser;
use rand::Rng;
use sc_core::{ScConfig, ScRuntime};

#[derive(Parser)]
struct Args {
    /// Каталог данных ядра (сессия и пр.)
    #[arg(long)]
    data_dir: PathBuf,
    /// Каталог кэша треков
    #[arg(long)]
    cache_dir: PathBuf,
    /// Порт на 127.0.0.1 (0 — случайный)
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Пробив DPI как fallback при блокировке
    #[arg(long)]
    dpi_bypass: bool,
}

fn new_token() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = ScConfig::new(&args.data_dir, &args.cache_dir).with_dpi_bypass(args.dpi_bypass);
    let rt = ScRuntime::new(config)
        .await
        .map_err(|e| anyhow::anyhow!("ядро не поднялось: {e}"))?;

    let token = new_token();
    let app = http::router(rt, token.clone());
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", args.port)).await?;
    let port = listener.local_addr()?.port();
    println!("{}", serde_json::json!({ "port": port, "token": token }));

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
