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
            egui::Rect::from_center_size(
                center + egui::Vec2::new(-w * 0.7, 0.0),
                egui::Vec2::new(w * 0.55, h),
            ),
            1.0,
            egui::Color32::WHITE,
        );
        painter.rect_filled(
            egui::Rect::from_center_size(
                center + egui::Vec2::new(w * 0.7, 0.0),
                egui::Vec2::new(w * 0.55, h),
            ),
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

/// Spotify 風イコライザー (3本バー)。`playing` の間は時刻でアニメーションする。
pub fn playing_bars(ui: &mut egui::Ui, playing: bool, accent: egui::Color32) {
    let size = egui::Vec2::new(13.0, 12.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let t = ui.input(|i| i.time) as f32;
    let painter = ui.painter();
    let bar_w = 3.0;
    let gap = 2.0;
    for i in 0..3 {
        let phase = i as f32 * 0.9;
        let frac = if playing {
            let s = (t * 4.2 + phase).sin() * 0.5 + 0.5;
            0.35 + 0.65 * s
        } else {
            0.45
        };
        let h = size.y * frac;
        let x = rect.left() + i as f32 * (bar_w + gap);
        let r = egui::Rect::from_min_size(
            egui::Pos2::new(x, rect.bottom() - h),
            egui::Vec2::new(bar_w, h),
        );
        painter.rect_filled(r, 1.0, accent);
    }
}

/// アクセント塗りの主要ボタン (Play 等)。
pub fn primary_button(ui: &mut egui::Ui, text: &str, accent: egui::Color32) -> egui::Response {
    let lum =
        accent.r() as u32 * 299 + accent.g() as u32 * 587 + accent.b() as u32 * 114;
    let text_color = if lum > 150_000 {
        egui::Color32::BLACK
    } else {
        egui::Color32::WHITE
    };
    ui.add(egui::Button::new(egui::RichText::new(text).color(text_color)).fill(accent))
}

/// トランスポート用アイコン (フォント非依存で自作描画)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TransportIcon {
    Play,
    Pause,
    Prev,
    Next,
}

/// 円形のトランスポートボタン。
pub fn transport_button(
    ui: &mut egui::Ui,
    icon: TransportIcon,
    enabled: bool,
    size: f32,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(size), sense);
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let painter = ui.painter();
    if enabled && resp.hovered() {
        painter.circle_filled(
            rect.center(),
            size * 0.5,
            egui::Color32::from_white_alpha(18),
        );
    }
    let color = if enabled {
        egui::Color32::from_white_alpha(225)
    } else {
        egui::Color32::from_white_alpha(70)
    };
    let c = rect.center();
    let s = size * 0.5;
    match icon {
        TransportIcon::Play => {
            let p1 = c + egui::Vec2::new(-s * 0.28, -s * 0.42);
            let p2 = c + egui::Vec2::new(-s * 0.28, s * 0.42);
            let p3 = c + egui::Vec2::new(s * 0.45, 0.0);
            painter.add(egui::Shape::convex_polygon(
                vec![p1, p2, p3],
                color,
                egui::Stroke::NONE,
            ));
        }
        TransportIcon::Pause => {
            let w = s * 0.22;
            let h = s * 0.8;
            for dx in [-s * 0.18, s * 0.18] {
                painter.rect_filled(
                    egui::Rect::from_center_size(
                        c + egui::Vec2::new(dx, 0.0),
                        egui::Vec2::new(w, h),
                    ),
                    1.0,
                    color,
                );
            }
        }
        TransportIcon::Prev => {
            painter.rect_filled(
                egui::Rect::from_center_size(
                    c + egui::Vec2::new(-s * 0.38, 0.0),
                    egui::Vec2::new(s * 0.16, s * 0.7),
                ),
                1.0,
                color,
            );
            let p1 = c + egui::Vec2::new(s * 0.38, -s * 0.35);
            let p2 = c + egui::Vec2::new(s * 0.38, s * 0.35);
            let p3 = c + egui::Vec2::new(-s * 0.12, 0.0);
            painter.add(egui::Shape::convex_polygon(
                vec![p1, p2, p3],
                color,
                egui::Stroke::NONE,
            ));
        }
        TransportIcon::Next => {
            painter.rect_filled(
                egui::Rect::from_center_size(
                    c + egui::Vec2::new(s * 0.38, 0.0),
                    egui::Vec2::new(s * 0.16, s * 0.7),
                ),
                1.0,
                color,
            );
            let p1 = c + egui::Vec2::new(-s * 0.38, -s * 0.35);
            let p2 = c + egui::Vec2::new(-s * 0.38, s * 0.35);
            let p3 = c + egui::Vec2::new(s * 0.12, 0.0);
            painter.add(egui::Shape::convex_polygon(
                vec![p1, p2, p3],
                color,
                egui::Stroke::NONE,
            ));
        }
    }
    resp
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
            egui::Label::new(egui::RichText::new(track.artist_name()).weak())
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
        // 再生中は行頭にイコライザー (Spotify 風、背景ハイライトは使わない)。
        if playing {
            playing_bars(ui, true, accent);
        } else {
            ui.add_space(13.0);
        }
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
            ui.label(egui::RichText::new(track.artist_name()).weak());
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

/// セクション見出し + 右端の追加操作 (「See all」等)。
pub fn section_row(
    ui: &mut egui::Ui,
    title: &str,
    count: Option<usize>,
    extra: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.heading(title);
        if let Some(n) = count {
            ui.label(format!("{n}"));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            extra(ui);
        });
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

/// 行/カードの操作結果 (通常クリック / 右クリックメニュー)。
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum RowHit {
    #[default]
    None,
    Clicked,
    Menu,
}

/// `Response` から RowHit を判定する共通ヘルパー。
pub fn hit_of(resp: &egui::Response) -> RowHit {
    if resp.clicked() {
        RowHit::Clicked
    } else if resp.secondary_clicked() {
        RowHit::Menu
    } else {
        RowHit::None
    }
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
