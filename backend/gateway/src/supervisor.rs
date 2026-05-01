use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::process::Command;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::lb::{Backend, BackendPool, STATE_DOWN};

pub struct Supervisor {
    handles: Vec<JoinHandle<()>>,
    shutdown_tx: watch::Sender<bool>,
    cfg: Config,
}

pub async fn start(cfg: Config, pool: BackendPool) -> std::io::Result<Supervisor> {
    tokio::fs::create_dir_all(&cfg.socket_dir).await?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut handles = Vec::with_capacity(pool.backends.len());
    for backend in pool.backends.iter().cloned() {
        let cfg = cfg.clone();
        let rx = shutdown_rx.clone();
        handles.push(tokio::spawn(async move {
            run_loop(cfg, backend, rx).await;
        }));
    }
    Ok(Supervisor {
        handles,
        shutdown_tx,
        cfg,
    })
}

impl Supervisor {
    pub async fn shutdown(self) {
        info!("[supervisor] broadcasting shutdown");
        let _ = self.shutdown_tx.send(true);
        let grace = self.cfg.kill_grace + Duration::from_secs(2);
        for h in self.handles {
            let _ = tokio::time::timeout(grace, h).await;
        }
    }
}

async fn run_loop(cfg: Config, backend: Arc<Backend>, mut shutdown_rx: watch::Receiver<bool>) {
    let mut backoff = cfg.backoff_min;
    let healthy_uptime = Duration::from_secs(60);

    loop {
        if *shutdown_rx.borrow() {
            return;
        }

        let _ = tokio::fs::remove_file(&backend.socket_path).await;

        let (program, args) = match cfg.backend_command.split_first() {
            Some(p) => p,
            None => {
                error!("[supervisor] empty backend command");
                return;
            }
        };
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.env("BACKEND_SOCKET", &backend.socket_path);
        cmd.env("BACKEND_INDEX", backend.id.to_string());
        cmd.kill_on_drop(true);

        info!(
            "[supervisor] spawning backend {} (sock={})",
            backend.id,
            backend.socket_path.display()
        );
        let started = Instant::now();
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                error!("[supervisor] spawn backend {}: {e}", backend.id);
                if wait_or_shutdown(&mut shutdown_rx, backoff).await {
                    return;
                }
                backoff = (backoff * 2).min(cfg.backoff_max);
                continue;
            }
        };

        let exit = tokio::select! {
            status = child.wait() => Exit::Natural(status),
            _ = shutdown_rx.changed() => Exit::Shutdown,
        };

        if backend.set_state(STATE_DOWN) {
            warn!("[supervisor] backend {} → DOWN", backend.id);
        }

        match exit {
            Exit::Shutdown => {
                let _ = child.start_kill();
                if tokio::time::timeout(cfg.kill_grace, child.wait()).await.is_err() {
                    warn!("[supervisor] backend {} hard-kill", backend.id);
                    let _ = child.kill().await;
                }
                return;
            }
            Exit::Natural(status) => {
                let uptime = started.elapsed();
                warn!(
                    "[supervisor] backend {} exited (uptime={:?}, status={:?})",
                    backend.id, uptime, status
                );
                backoff = if uptime >= healthy_uptime {
                    cfg.backoff_min
                } else {
                    (backoff * 2).min(cfg.backoff_max)
                };
                if wait_or_shutdown(&mut shutdown_rx, backoff).await {
                    return;
                }
            }
        }
    }
}

enum Exit {
    Natural(std::io::Result<std::process::ExitStatus>),
    Shutdown,
}

async fn wait_or_shutdown(rx: &mut watch::Receiver<bool>, dur: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(dur) => false,
        _ = rx.changed() => true,
    }
}
