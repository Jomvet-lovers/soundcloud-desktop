//! ページネーション (`{ collection, has_more }`) の共有ヘルパー。
//! `Pager` はページ単位の追加読込、`auto_load` は末尾 sentinel が
//! スクロール域に見えたら次ページを要求する (無限スクロール)。

use serde::Deserialize;

use crate::backend::api::ApiClient;
use crate::query::Query;

/// page エンベロープ (`common::page()` / `ListPage` 系) の応答。
#[derive(Clone, Debug, Deserialize)]
#[serde(bound(deserialize = "T: serde::de::Deserialize<'de>"))]
pub struct ListPage<T> {
    #[serde(default)]
    pub collection: Vec<T>,
    #[serde(default)]
    pub has_more: bool,
}

impl<T> Default for ListPage<T> {
    fn default() -> Self {
        Self {
            collection: Vec::new(),
            has_more: false,
        }
    }
}

/// Append-only pager for page-based endpoints.
pub struct Pager<T> {
    pub items: Vec<T>,
    pub next_page: usize,
    pub has_more: bool,
    pub started: bool,
    pub q: Query<ListPage<T>>,
}

impl<T> Default for Pager<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next_page: 0,
            has_more: false,
            started: false,
            q: Query::default(),
        }
    }
}

impl<T: for<'de> Deserialize<'de> + Send + 'static> Pager<T> {
    /// 初回のみページ 0 を取得する。
    pub fn ensure(&mut self, rt: &tokio::runtime::Handle, api: &ApiClient, base: &str) {
        self.ensure_page(rt, api, base, 30);
    }

    /// 初回のみページ 0 を取得する (limit 指定)。
    pub fn ensure_page(
        &mut self,
        rt: &tokio::runtime::Handle,
        api: &ApiClient,
        base: &str,
        limit: usize,
    ) {
        if self.started {
            return;
        }
        self.started = true;
        self.fetch_page(rt, api, base, limit);
    }

    /// 次ページを取得する (応答は `poll` で追記)。
    pub fn fetch(&mut self, rt: &tokio::runtime::Handle, api: &ApiClient, base: &str) {
        self.fetch_page(rt, api, base, 30);
    }

    pub fn fetch_page(
        &mut self,
        rt: &tokio::runtime::Handle,
        api: &ApiClient,
        base: &str,
        limit: usize,
    ) {
        let sep = if base.contains('?') { '&' } else { '?' };
        let url = format!("{base}{sep}limit={limit}&page={}", self.next_page);
        let api = api.clone();
        self.q.request(rt, async move {
            let v = api.get_json(&url).await?;
            serde_json::from_value(v).map_err(|e| e.to_string())
        });
    }

    /// Returns true when a repaint is needed (completed or in flight).
    pub fn poll(&mut self) -> bool {
        let changed = self.q.poll();
        if changed {
            if let Some(page) = self.q.data.take() {
                self.next_page += 1;
                self.has_more = page.has_more;
                self.items.extend(page.collection);
            }
        }
        changed || self.q.loading
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// 末尾 sentinel。スクロール域に見えていれば次ページ要求 (true)。
/// `loading` 中は false (二重要求を防ぐ)。
pub fn auto_load(ui: &mut egui::Ui, loading: bool, has_more: bool) -> bool {
    if !has_more {
        return false;
    }
    let resp = ui
        .horizontal(|ui| {
            if loading {
                ui.spinner();
                ui.label("Loading more...");
            } else {
                ui.label("· · ·");
            }
        })
        .response;
    !loading && ui.clip_rect().intersects(resp.rect)
}
