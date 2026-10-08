//! 共通ウィジェット: TrackCard / TrackRow / SectionHeader。
//! egui 標準部品に無いもの (ホバー再生オーバーレイ等) は自作描画する。
//! 各ページは見た目をここに寄せる (個別 CSS 的調整はしない)。

use crate::backend::models::Track;
use crate::images::Images;

/// 再生/停止グリフを自作描画する (フォント依存を避けるため)。
fn paint_play_glyph(painter: &egui::Painter, rect: egui::Rect, playing: bool) {
    let center = rect.center();
    let r = rect.width().min(rect.height()) * 0.22;
    painter.circle_filled(center, r, egui::Color32::from_black_alpha(160));
    if playing {
        let w = r * 0.45;
        let h = r * 0.8;
        painter.rect_filled(
            egui::Rect::from_center_size(center + egui::Vec2::new(-w * 0.7, 0.0), egui::Vec2::new(w * 0.55, h)),
            1.0,
            egui::Color32::WHITE,
        );
        painter.rect_filled(
            egui::Rect::from_center_size(center + egui::Vec2::new(w * 0.7, 0.0), egui::Vec2::new(w * 0.55, h)),
            1.0,
            egui::Color32::WHITE,
        );
    } else {
        let p1 = center + egui::Vec2::new(-r * 0.35, -r * 0.55);
        let p2 = center + egui::Vec2::new(-r * 0.35, r * 0.55);
        let p3 = center + egui::Vec2::new(r * 0.55, 0.0);
        painter.add(egui::Shape::convex_polygon(
            vec![p1, p2, p3],
            egui::Color32::WHITE,
            egui::Stroke::NONE,
        ));
    }
}

/// グリッド用カード (Home の棚・Search 結果)。戻り値はクリック応答。
pub fn track_card(
    ui: &mut egui::Ui,
    rt: &tokio::runtime::Handle,
    images: &mut Images,
    track: &Track,
    size: f32,
    playing: bool,
    accent: egui::Color32,
) -> egui::Response {
    ui.vertical(|ui| {
        ui.set_max_width(size + 8.0);
        let art = track.artwork("t300x300");
        let resp = images.show(ui, rt, art.as_deref(), size);
        if resp.hovered() || playing {
            paint_play_glyph(ui.painter(), resp.rect, playing);
        }
        let title = if playing {
            egui::RichText::new(track.display_title()).color(accent)
        } else {
            egui::RichText::new(track.display_title())
        };
        ui.add(
            egui::Label::new(title)
                .truncate()
                .wrap_mode(egui::TextWrapMode::Truncate),
        );
        ui.add(
            egui::Label::new(track.artist_name())
                .truncate()
                .wrap_mode(egui::TextWrapMode::Truncate),
        );
        resp
    })
    .inner
}

/// 一覧行 (40px アート + タイトル/アーティスト + 時間)。
/// 戻り値はアートワークのクリック応答 (再生トリガ)。
pub fn track_row(
    ui: &mut egui::Ui,
    rt: &tokio::runtime::Handle,
    images: &mut Images,
    track: &Track,
    playing: bool,
    accent: egui::Color32,
    duration_text: Option<&str>,
) -> egui::Response {
    let art = track.artwork("t100x100");
    let mut art_resp: Option<egui::Response> = None;
    ui.horizontal(|ui| {
        let r = images.show(ui, rt, art.as_deref(), 40.0);
        if r.hovered() || playing {
            paint_play_glyph(ui.painter(), r.rect, playing);
        }
        art_resp = Some(r);
        ui.vertical(|ui| {
            let title = if playing {
                egui::RichText::new(track.display_title()).color(accent)
            } else {
                egui::RichText::new(track.display_title())
            };
            ui.add(
                egui::Label::new(title)
                    .truncate()
                    .wrap_mode(egui::TextWrapMode::Truncate),
            );
            ui.label(track.artist_name());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(d) = duration_text {
                ui.label(d);
            }
        });
    });
    art_resp.expect("artwork response")
}

/// セクション見出し (タイトル + 件数)。
pub fn section_header(ui: &mut egui::Ui, title: &str, count: Option<usize>) {
    ui.horizontal(|ui| {
        ui.heading(title);
        if let Some(n) = count {
            ui.label(format!("{n}"));
        }
    });
}

/// 現在のアクセント色。
pub fn accent_color(settings: &crate::state::SettingsState) -> egui::Color32 {
    egui::Color32::from_rgb(settings.accent[0], settings.accent[1], settings.accent[2])
}

/// 再生中トラックかどうか (タイトル一致の簡易判定)。
pub fn is_currently_playing(player: &crate::state::PlayerState, track: &Track) -> bool {
    player.is_playing
        && player
            .current_title
            .as_deref()
            .map(|t| t == track.display_title())
            .unwrap_or(false)
}

/// Like トグルボタン。フォント依存の ♥/♡ を使わず、塗り + 色で状態を示す。
/// 戻り値はクリックされたか (状態更新は呼出側)。
pub fn like_button(ui: &mut egui::Ui, liked: bool, accent: egui::Color32) -> bool {
    let button = if liked {
        egui::Button::new(egui::RichText::new("Liked").color(egui::Color32::WHITE))
            .fill(accent.gamma_multiply(0.45))
    } else {
        egui::Button::new("Like")
    };
    ui.add(button).clicked()
}
