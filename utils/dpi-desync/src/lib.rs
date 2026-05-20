mod probe;
mod socks;
mod state;
mod tamper;

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;

pub use tamper::Strategy;

pub struct Desync {
    addr: SocketAddr,
    state: Arc<state::State>,
}

impl Desync {
    pub async fn spawn(enabled: bool) -> std::io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
        let addr = listener.local_addr()?;
        let st = state::State::new(enabled);
        let st2 = st.clone();
        tokio::spawn(async move { socks::run(listener, st2).await });
        Ok(Self { addr, state: st })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn proxy_url(&self) -> String {
        format!("socks5h://{}", self.addr)
    }

    pub fn set_enabled(&self, v: bool) {
        self.state.set_enabled(v);
    }

    pub fn is_enabled(&self) -> bool {
        self.state.is_enabled()
    }

    pub fn strategy(&self) -> Strategy {
        self.state.strategy()
    }

    pub fn set_strategy(&self, s: Strategy) {
        self.state.set_strategy(s);
    }

    pub async fn probe(&self, probe_url: &str) -> Strategy {
        probe::run(&self.state, &self.proxy_url(), probe_url).await
    }
}
