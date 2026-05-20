use std::net::SocketAddr;
use std::time::Duration;

use tokio::sync::oneshot;
use tracing::{debug, info};

use crate::state::State;
use crate::tamper::{Strategy, LADDER};

const PROBE_TIMEOUT: Duration = Duration::from_secs(4);

pub async fn run(state: &State, _proxy_url: &str, probe_url: &str) -> Strategy {
    let mut handles = Vec::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Strategy>(LADDER.len());
    for s in LADDER {
        let listener =
            match tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await {
                Ok(l) => l,
                Err(e) => {
                    debug!(?e, "probe bind");
                    continue;
                }
            };
        let addr = match listener.local_addr() {
            Ok(a) => a,
            Err(_) => continue,
        };
        let st = State::new(true);
        st.set_strategy(s);
        let h = tokio::spawn(async move { crate::socks::run(listener, st).await });
        let url = probe_url.to_string();
        let proxy = format!("socks5h://{}", addr);
        let tx = tx.clone();
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let probe_h = tokio::spawn(async move {
            let client = match reqwest::Client::builder()
                .proxy(reqwest::Proxy::all(&proxy).unwrap())
                .timeout(PROBE_TIMEOUT)
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    debug!(?e, ?s, "probe client");
                    return;
                }
            };
            tokio::select! {
                _ = stop_rx => {},
                r = client.head(&url).send() => match r {
                    Ok(resp) if resp.status().is_success() || resp.status().is_redirection() => {
                        info!(?s, status = %resp.status(), "dpi-desync: strategy works");
                        let _ = tx.send(s).await;
                    }
                    Ok(resp) => debug!(?s, status = %resp.status(), "dpi-desync: bad status"),
                    Err(e) => debug!(?s, ?e, "dpi-desync: probe err"),
                }
            }
        });
        handles.push((h, probe_h, stop_tx));
    }
    drop(tx);

    let winner = tokio::select! {
        v = rx.recv() => v,
        _ = tokio::time::sleep(PROBE_TIMEOUT + Duration::from_millis(500)) => None,
    };

    for (h, ph, stop) in handles {
        let _ = stop.send(());
        ph.abort();
        h.abort();
    }

    let chosen = winner.unwrap_or(Strategy::None);
    state.set_strategy(chosen);
    chosen
}
