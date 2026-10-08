//! Phase 3: アートワークの texture キャッシュ。
//! 初回表示で fetch を spawn し、完了まではプレースホルダを描く。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::oneshot;

pub struct Images {
    textures: HashMap<String, egui::TextureHandle>,
    pending: HashMap<String, oneshot::Receiver<Result<egui::ColorImage, String>>>,
    failed: HashSet<String>,
    client: Arc<wreq::Client>,
}

impl Images {
    pub fn new(client: Arc<wreq::Client>) -> Self {
        Self {
            textures: HashMap::new(),
            pending: HashMap::new(),
            failed: HashSet::new(),
            client,
        }
    }

    /// アートワーク (正方形) またはプレースホルダを描き、クリック応答を返す。
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rt: &tokio::runtime::Handle,
        url: Option<&str>,
        size: f32,
    ) -> egui::Response {
        let Some(url) = url else {
            return Self::placeholder(ui, size);
        };
        if let Some(rx) = self.pending.get_mut(url) {
            if let Ok(result) = rx.try_recv() {
                self.pending.remove(url);
                match result {
                    Ok(img) => {
                        self.textures.insert(
                            url.to_string(),
                            ui.ctx().load_texture(url, img, Default::default()),
                        );
                    }
                    Err(_) => {
                        self.failed.insert(url.to_string());
                    }
                }
                ui.ctx().request_repaint();
            }
        }
        if let Some(tex) = self.textures.get(url) {
            return ui.add(
                egui::Image::new((tex.id(), egui::Vec2::splat(size)))
                    .corner_radius(egui::CornerRadius::same(4))
                    .sense(egui::Sense::click()),
            );
        }
        if self.failed.contains(url) {
            return Self::placeholder(ui, size);
        }
        if !self.pending.contains_key(url) {
            let (tx, rx) = oneshot::channel();
            let client = self.client.clone();
            let url_owned = url.to_string();
            let ctx = ui.ctx().clone();
            rt.spawn(async move {
                let result = fetch_image(&client, &url_owned).await;
                let _ = tx.send(result);
                // 完了を UI に知らせる (egui は入力まで描画しないため)。
                ctx.request_repaint();
            });
            self.pending.insert(url.to_string(), rx);
        }
        // 取得完了までフレームを回して `try_recv` を poll する。
        ui.ctx().request_repaint();
        Self::placeholder(ui, size)
    }

    fn placeholder(ui: &mut egui::Ui, size: f32) -> egui::Response {
        let (rect, resp) =
            ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::click());
        ui.painter().rect_filled(
            rect,
            4.0,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "♪",
            egui::FontId::proportional(size * 0.35),
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 60),
        );
        resp
    }
}

async fn fetch_image(
    client: &wreq::Client,
    url: &str,
) -> Result<egui::ColorImage, String> {
    let bytes = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [w, h],
        &rgba,
    ))
}
