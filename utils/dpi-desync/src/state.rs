use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use crate::tamper::Strategy;

pub struct State {
    enabled: AtomicBool,
    strategy: AtomicU8,
}

impl State {
    pub fn new(enabled: bool) -> Arc<Self> {
        Arc::new(Self {
            enabled: AtomicBool::new(enabled),
            strategy: AtomicU8::new(Strategy::None as u8),
        })
    }

    pub fn set_enabled(&self, v: bool) {
        self.enabled.store(v, Ordering::Release);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub fn set_strategy(&self, s: Strategy) {
        self.strategy.store(s as u8, Ordering::Release);
    }

    pub fn strategy(&self) -> Strategy {
        match self.strategy.load(Ordering::Acquire) {
            1 => Strategy::TlsRec,
            2 => Strategy::Split,
            3 => Strategy::TlsRecSplit,
            4 => Strategy::MultiSplit,
            _ => Strategy::None,
        }
    }
}

