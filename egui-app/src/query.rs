//! Phase 3: react-query の代替。非同期取得 + ポーリング回収。
//! stale-while-revalidate: 再取得中も古い `data` を保持する。

use tokio::sync::oneshot;

pub struct Query<T> {
    rx: Option<oneshot::Receiver<Result<T, String>>>,
    pub data: Option<T>,
    pub error: Option<String>,
    pub loading: bool,
}

impl<T> Default for Query<T> {
    fn default() -> Self {
        Self {
            rx: None,
            data: None,
            error: None,
            loading: false,
        }
    }
}

impl<T: Send + 'static> Query<T> {
    /// 取得開始。既存 `data` は消さない (stale 表示)。
    pub fn request(
        &mut self,
        rt: &tokio::runtime::Handle,
        fut: impl std::future::Future<Output = Result<T, String>> + Send + 'static,
    ) {
        let (tx, rx) = oneshot::channel();
        rt.spawn(async move {
            let _ = tx.send(fut.await);
        });
        self.rx = Some(rx);
        self.loading = true;
        self.error = None;
    }

    pub fn requested(&self) -> bool {
        self.rx.is_some() || self.data.is_some() || self.error.is_some()
    }

    /// フレーム毎に呼ぶ。完了していれば `data`/`error` に反映する。
    /// 戻り値は再描画が必要かどうか。
    pub fn poll(&mut self) -> bool {
        if let Some(rx) = self.rx.as_mut() {
            match rx.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(v) => {
                            self.data = Some(v);
                            self.error = None;
                        }
                        Err(e) => {
                            self.error = Some(e);
                        }
                    }
                    self.rx = None;
                    self.loading = false;
                    return true;
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.rx = None;
                    self.loading = false;
                    return true;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
        }
        false
    }
}
