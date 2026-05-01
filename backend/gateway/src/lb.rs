use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use rand::Rng;

pub const STATE_DOWN: u8 = 0;
pub const STATE_UP: u8 = 1;

#[derive(Debug)]
pub struct Backend {
    pub id: usize,
    pub socket_path: PathBuf,
    pub inflight: AtomicU32,
    pub state: AtomicU8,
}

impl Backend {
    pub fn is_up(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_UP
    }

    pub fn set_state(&self, new: u8) -> bool {
        let prev = self.state.swap(new, Ordering::AcqRel);
        prev != new
    }
}

#[derive(Clone)]
pub struct BackendPool {
    pub backends: Arc<Vec<Arc<Backend>>>,
}

impl BackendPool {
    pub fn new(count: usize, socket_dir: &Path) -> Self {
        let backends = (0..count)
            .map(|i| {
                Arc::new(Backend {
                    id: i,
                    socket_path: socket_dir.join(format!("backend-{i}.sock")),
                    inflight: AtomicU32::new(0),
                    state: AtomicU8::new(STATE_DOWN),
                })
            })
            .collect::<Vec<_>>();
        Self {
            backends: Arc::new(backends),
        }
    }

    pub fn pick(&self) -> Option<InflightHandle> {
        let alive: Vec<&Arc<Backend>> = self
            .backends
            .iter()
            .filter(|b| b.is_up())
            .collect();
        let chosen = match alive.len() {
            0 => return None,
            1 => alive[0].clone(),
            n => {
                let mut rng = rand::thread_rng();
                let i = rng.gen_range(0..n);
                let mut j = rng.gen_range(0..n);
                if j == i {
                    j = (j + 1) % n;
                }
                let a = alive[i];
                let b = alive[j];
                if a.inflight.load(Ordering::Relaxed) <= b.inflight.load(Ordering::Relaxed) {
                    a.clone()
                } else {
                    b.clone()
                }
            }
        };
        chosen.inflight.fetch_add(1, Ordering::AcqRel);
        Some(InflightHandle { backend: chosen })
    }
}

pub struct InflightHandle {
    pub backend: Arc<Backend>,
}

impl Drop for InflightHandle {
    fn drop(&mut self) {
        self.backend.inflight.fetch_sub(1, Ordering::AcqRel);
    }
}
