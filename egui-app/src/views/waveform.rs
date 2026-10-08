//! Phase 3b: 波形シークバー。SC の `_m.json` を取得し自前描画する。
//! 対応: `desktop/src/components/music/soundwave/waveform.tsx` +
//! `desktop/src/lib/waveform.ts`。コメントレーンは未対応。

use crate::query::Query;

/// `_m.png` → `_m.json`。対応: waveform.ts の `normalizeWaveformUrl`。
pub fn normalize_url(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    let url = raw
        .replace(".png", ".json")
        .replace("http://", "https://");
    if url.ends_with(".json") || url.contains(".json?") {
        Some(url)
    } else {
        None
    }
}

/// 正規化済みサンプル (0..1) を取得する。
pub async fn fetch_samples(raw_url: &str) -> Result<Vec<f32>, String> {
    let url = normalize_url(raw_url).ok_or_else(|| "bad waveform url".to_string())?;
    let v: serde_json::Value = wreq::Client::new()
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let arr = v
        .get("samples")
        .and_then(|s| s.as_array())
        .ok_or_else(|| "no samples".to_string())?;
    let mut out: Vec<f32> = arr
        .iter()
        .filter_map(|x| x.as_f64().map(|f| f as f32))
        .collect();
    if out.is_empty() {
        return Err("empty samples".to_string());
    }
    let max = out.iter().cloned().fold(0f32, f32::max).max(1.0);
    for x in &mut out {
        *x /= max;
    }
    Ok(out)
}

/// 波形を描く。戻り値はクリックされたシーク位置 (0..1)。
/// `query` の取得・poll は呼出側 (Track ページ) が行う。
pub fn show(
    ui: &mut egui::Ui,
    query: &Query<Vec<f32>>,
    progress: f32,
    height: f32,
    accent: egui::Color32,
) -> Option<f32> {
    if query.loading {
        ui.spinner();
        ui.ctx().request_repaint();
        return None;
    }
    let Some(samples) = query.data.as_ref() else {
        return None;
    };
    if samples.is_empty() {
        return None;
    }
    let width = ui.available_width().max(60.0);
    let n = ((width / 4.0) as usize).clamp(40, 300);
    let (rect, resp) =
        ui.allocate_exact_size(egui::Vec2::new(width, height), egui::Sense::click());
    let painter = ui.painter();
    let bar_w = width / n as f32;
    let dim = egui::Color32::from_white_alpha(45);
    for i in 0..n {
        let lo = i * samples.len() / n;
        let hi = ((i + 1) * samples.len() / n).max(lo + 1);
        let h = samples[lo..hi.min(samples.len())]
            .iter()
            .cloned()
            .fold(0f32, f32::max);
        let bh = (h * (height - 10.0)).max(2.0);
        let x0 = rect.left() + i as f32 * bar_w;
        let y0 = rect.center().y - bh / 2.0;
        let played = (i as f32 + 1.0) / n as f32 <= progress;
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::Pos2::new(x0, y0),
                egui::Vec2::new((bar_w - 1.0).max(1.0), bh),
            ),
            1.0,
            if played { accent } else { dim },
        );
    }
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            return Some(((pos.x - rect.left()) / width).clamp(0.0, 1.0));
        }
    }
    None
}
