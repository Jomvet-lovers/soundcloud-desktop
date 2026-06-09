mod acceptor;
mod config;
mod proxy;
mod redirect;
mod serve;
mod shutdown;

pub use config::TlsConfig;
pub use serve::serve;
pub use shutdown::shutdown_signal;

/// Ставит процесс-дефолтный rustls CryptoProvider (ring). Вызывать ПЕРВЫМ в
/// `main()` — до любого TLS (NATS, Qdrant, reqwest, sqlx, ACME). Когда в дереве
/// есть и `ring`, и `aws-lc-rs`, rustls 0.23 не выбирает провайдер сам и паникует
/// на первом хендшейке; явная установка снимает неоднозначность. Идемпотентно.
pub fn init_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
