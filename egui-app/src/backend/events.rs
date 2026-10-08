//! Tauri の `AppHandle` + `Emitter` の代替。
//!
//! 移植前の `app.emit("event", &payload)` / `handle.emit(..).ok()` は
//! `bus.emit("event", &payload)` に置換する。送信先が無い場合 (null) は
//! 捨てられる。UI 側は受信端を持ち、既知イベントを `AppState` に反映する
//! (配線は Phase 2)。

use serde::Serialize;

/// UI スレッドへの `(event, payload)` 送信端。`Clone` して各所に配る。
#[derive(Clone)]
pub struct EventBus {
    tx: Option<tokio::sync::mpsc::UnboundedSender<(String, serde_json::Value)>>,
    wake: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus")
            .field("tx", &self.tx.is_some())
            .field("wake", &self.wake.is_some())
            .finish()
    }
}

impl EventBus {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<(String, serde_json::Value)>) -> Self {
        Self {
            tx: Some(tx),
            wake: None,
        }
    }

    /// 送信先なし (テスト・ヘッドレス用)。`emit` は no-op。
    pub fn null() -> Self {
        Self {
            tx: None,
            wake: None,
        }
    }

    pub fn is_null(&self) -> bool {
        self.tx.is_none()
    }

    /// UI を起こすコールバックを設定する (egui の `request_repaint`)。
    /// eframe は入力があるまでフレームを回さないため、バックエンド起点の
    /// 変化 (ログイン完了・再生終了など) を即時反映するには明示的な起床が要る。
    pub fn set_wake(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        self.wake = Some(std::sync::Arc::new(wake));
    }

    pub fn emit(&self, event: &str, payload: impl Serialize) {
        if let Some(tx) = &self.tx {
            let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
            let _ = tx.send((event.to_string(), value));
        }
        if let Some(wake) = &self.wake {
            wake();
        }
    }
}
