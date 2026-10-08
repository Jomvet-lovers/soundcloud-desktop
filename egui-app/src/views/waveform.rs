//! Phase 3b: 波形シークバー。SC の `_m.json` を取得し自前描画する。
//! 対応: `desktop/src/components/music/soundwave/waveform.tsx` +
//! `desktop/src/lib/waveform.ts`。コメント点 (WaveVoices) は `voices` で描く。
//! フローティングコメント (FloatingComments) は Track ページ側で描画する。

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

/// 波形上のコメント点 (Tauri: WaveVoices のドット)。
pub struct WaveVoice {
    pub timestamp_ms: f64,
    pub body: String,
}

/// 波形の操作結果。
pub struct WaveHit {
    /// 波形の描画矩形 (フローティングコメントの配置に使う)。
    pub rect: egui::Rect,
    /// 波形クリックによるシーク位置 (0..1)。
    pub seek: Option<f32>,
    /// コメント点クリックによるシーク (ms)。
    pub comment_seek_ms: Option<f64>,
}

/// 波形を描く (コメント点つき)。`voices` はタイムスタンプ付きコメント。
/// `query` の取得・poll は呼出側 (Track ページ) が行う。
pub fn show(
    ui: &mut egui::Ui,
    query: &Query<Vec<f32>>,
    progress: f32,
    height: f32,
    accent: egui::Color32,
    voices: &[WaveVoice],
    duration_ms: f64,
) -> WaveHit {
    let mut hit = WaveHit {
        rect: egui::Rect::NOTHING,
        seek: None,
        comment_seek_ms: None,
    };
    if query.loading {
        ui.spinner();
        ui.ctx().request_repaint();
        return hit;
    }
    let Some(samples) = query.data.as_ref() else {
        return hit;
    };
    if samples.is_empty() {
        return hit;
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
    hit.rect = rect;
    // コメント点 (各タイムスタンプに小さな点。クリックでそこへシーク)。
    let mut dots: Vec<(egui::Pos2, f64)> = Vec::new();
    if duration_ms > 0.0 {
        for v in voices {
            if v.body.is_empty() {
                continue;
            }
            let pct = (v.timestamp_ms / duration_ms).clamp(0.0, 1.0) as f32;
            let c = egui::Pos2::new(rect.left() + pct * width, rect.top() + 6.0);
            painter.circle_filled(c, 4.0, egui::Color32::from_white_alpha(160));
            painter.circle_stroke(c, 4.0, egui::Stroke::new(1.0, egui::Color32::from_black_alpha(120)));
            dots.push((c, v.timestamp_ms));
        }
    }
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            // 近傍 (8px) のコメント点を優先し、無ければ通常シーク。
            if let Some((_, ts)) = dots.iter().find(|(c, _)| (c.x - pos.x).abs() <= 8.0) {
                hit.comment_seek_ms = Some(*ts);
            } else {
                hit.seek = Some(((pos.x - rect.left()) / width).clamp(0.0, 1.0));
            }
        }
    }
    hit
}
