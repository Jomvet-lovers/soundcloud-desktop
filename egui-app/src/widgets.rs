//! 共通ウィジェット: TrackCard / TrackRow / SectionHeader。
//! egui 標準部品に無いもの (ホバー再生オーバーレイ等) は自作描画する。
//! 各ページは見た目をここに寄せる (個別 CSS 的調整はしない)。

use crate::backend::models::Track;
use crate::images::Images;

/// 再生/停止グリフを自作描画する (フォント依存を避けるため)。
pub(crate) fn paint_play_glyph(painter: &egui::Painter, rect: egui::Rect, playing: bool) {
    paint_play_glyph_alpha(painter, rect, playing, 1.0);
}

/// フェード付きの再生/停止グリフ (ホバーでふわっと出す)。
pub(crate) fn paint_play_glyph_alpha(
    painter: &egui::Painter,
    rect: egui::Rect,
    playing: bool,
    alpha: f32,
) {
    let a = |c: egui::Color32| c.gamma_multiply(alpha);
    let center = rect.center();
    let r = rect.width().min(rect.height()) * 0.22;
    painter.circle_filled(center, r, a(egui::Color32::from_black_alpha(160)));
    if playing {
        let w = r * 0.45;
        let h = r * 0.8;
        painter.rect_filled(
            egui::Rect::from_center_size(
                center + egui::Vec2::new(-w * 0.7, 0.0),
                egui::Vec2::new(w * 0.55, h),
            ),
            1.0,
            a(egui::Color32::WHITE),
        );
        painter.rect_filled(
            egui::Rect::from_center_size(
                center + egui::Vec2::new(w * 0.7, 0.0),
                egui::Vec2::new(w * 0.55, h),
            ),
            1.0,
            a(egui::Color32::WHITE),
        );
    } else {
        let p1 = center + egui::Vec2::new(-r * 0.35, -r * 0.55);
        let p2 = center + egui::Vec2::new(-r * 0.35, r * 0.55);
        let p3 = center + egui::Vec2::new(r * 0.55, 0.0);
        painter.add(egui::Shape::convex_polygon(
            vec![p1, p2, p3],
            a(egui::Color32::WHITE),
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
    let text_color = contrast_color(accent);
    ui.add(egui::Button::new(egui::RichText::new(text).color(text_color)).fill(accent))
}

/// ヒーロー用の円形再生ボタン (Tauri: RoomHero/PlaylistHero の 68px 円)。
/// 枠線 + 白グリフのアウトライン円。
pub fn hero_play_button(
    ui: &mut egui::Ui,
    playing: bool,
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
    let stroke_color = if !enabled {
        egui::Color32::from_white_alpha(40)
    } else if resp.hovered() {
        egui::Color32::from_white_alpha(110)
    } else {
        egui::Color32::from_white_alpha(46)
    };
    painter.circle_stroke(
        rect.center(),
        size * 0.5 - 1.0,
        egui::Stroke::new(1.0, stroke_color),
    );
    let color = if enabled {
        egui::Color32::from_white_alpha(230)
    } else {
        egui::Color32::from_white_alpha(60)
    };
    let icon = if playing {
        TransportIcon::Pause
    } else {
        TransportIcon::Play
    };
    paint_transport_glyph(painter, rect, icon, color, size * 0.42);
    resp
}

/// トランスポート用アイコン (フォント非依存で自作描画)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TransportIcon {
    Play,
    Pause,
    Prev,
    Next,
}

/// サイドバー/タイトルバー用の簡易アイコン (フォント非依存の自作描画)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UiIcon {
    Home,
    Search,
    Library,
    History,
    Offline,
    Settings,
    Collapse,
    ChevronLeft,
    ChevronRight,
    Minimize,
    Maximize,
    Close,
    Cloud,
    Heart,
    Bookmark,
    Users,
    Refresh,
    Shuffle,
    Repeat,
    Sliders,
    Queue,
    Volume,
    Download,
    Trash,
    AudioLines,
    Plus,
    Link,
    ListPlus,
    MapPin,
    Power,
}

/// アイコンを描いて応答を返す (クリックは呼出側で付ける)。
pub fn ui_icon(
    ui: &mut egui::Ui,
    icon: UiIcon,
    size: f32,
    color: egui::Color32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::hover());
    paint_ui_icon(ui.painter(), rect, icon, color);
    resp
}

/// アイコン本体 (直線/円/多角形の組み合わせ)。
pub fn paint_ui_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    icon: UiIcon,
    color: egui::Color32,
) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let stroke = egui::Stroke::new(1.4, color);
    match icon {
        UiIcon::Home => {
            let w = s * 0.34;
            painter.add(egui::Shape::line(
                vec![
                    c + egui::Vec2::new(-w, 0.0),
                    c + egui::Vec2::new(0.0, -w * 1.05),
                    c + egui::Vec2::new(w, 0.0),
                ],
                stroke,
            ));
            painter.rect_stroke(
                egui::Rect::from_min_max(
                    egui::Pos2::new(c.x - w * 0.72, c.y),
                    egui::Pos2::new(c.x + w * 0.72, c.y + w * 1.05),
                ),
                1.5,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        UiIcon::Search => {
            painter.circle_stroke(
                c + egui::Vec2::new(-s * 0.06, -s * 0.06),
                s * 0.24,
                stroke,
            );
            painter.line_segment(
                [
                    c + egui::Vec2::new(s * 0.12, s * 0.12),
                    c + egui::Vec2::new(s * 0.30, s * 0.30),
                ],
                stroke,
            );
        }
        UiIcon::Library => {
            for (i, h) in [0.5f32, 0.34, 0.44].iter().enumerate() {
                let x = c.x - s * 0.24 + i as f32 * s * 0.24;
                painter.line_segment(
                    [
                        egui::Pos2::new(x, c.y - s * h * 0.5),
                        egui::Pos2::new(x, c.y + s * h * 0.5),
                    ],
                    egui::Stroke::new(1.6, color),
                );
            }
        }
        UiIcon::History => {
            painter.circle_stroke(c, s * 0.28, stroke);
            painter.line_segment([c, c + egui::Vec2::new(0.0, -s * 0.16)], stroke);
            painter.line_segment([c, c + egui::Vec2::new(s * 0.13, 0.0)], stroke);
        }
        UiIcon::Offline => {
            painter.line_segment(
                [
                    c + egui::Vec2::new(0.0, -s * 0.30),
                    c + egui::Vec2::new(0.0, s * 0.10),
                ],
                stroke,
            );
            painter.add(egui::Shape::line(
                vec![
                    c + egui::Vec2::new(-s * 0.15, -s * 0.06),
                    c + egui::Vec2::new(0.0, s * 0.10),
                    c + egui::Vec2::new(s * 0.15, -s * 0.06),
                ],
                stroke,
            ));
            painter.line_segment(
                [
                    c + egui::Vec2::new(-s * 0.24, s * 0.28),
                    c + egui::Vec2::new(s * 0.24, s * 0.28),
                ],
                stroke,
            );
        }
        UiIcon::Settings => {
            painter.circle_stroke(c, s * 0.16, stroke);
            for k in 0..6 {
                let a = std::f32::consts::TAU * k as f32 / 6.0;
                let dir = egui::Vec2::new(a.cos(), a.sin());
                painter.line_segment(
                    [c + dir * s * 0.26, c + dir * s * 0.36],
                    egui::Stroke::new(1.2, color),
                );
            }
        }
        UiIcon::Collapse => {
            painter.rect_stroke(
                egui::Rect::from_center_size(c, egui::Vec2::new(s * 0.62, s * 0.5)),
                2.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment(
                [
                    c + egui::Vec2::new(-s * 0.16, -s * 0.25),
                    c + egui::Vec2::new(-s * 0.16, s * 0.25),
                ],
                stroke,
            );
        }
        UiIcon::ChevronLeft | UiIcon::ChevronRight => {
            let dir = if icon == UiIcon::ChevronLeft { -1.0 } else { 1.0 };
            painter.add(egui::Shape::line(
                vec![
                    c + egui::Vec2::new(-s * 0.14 * dir, -s * 0.20),
                    c + egui::Vec2::new(s * 0.12 * dir, 0.0),
                    c + egui::Vec2::new(-s * 0.14 * dir, s * 0.20),
                ],
                egui::Stroke::new(1.6, color),
            ));
        }
        UiIcon::Minimize => {
            painter.line_segment(
                [
                    c + egui::Vec2::new(-s * 0.22, 0.0),
                    c + egui::Vec2::new(s * 0.22, 0.0),
                ],
                stroke,
            );
        }
        UiIcon::Maximize => {
            painter.rect_stroke(
                egui::Rect::from_center_size(c, egui::Vec2::splat(s * 0.42)),
                1.0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        UiIcon::Close => {
            painter.line_segment(
                [
                    c + egui::Vec2::new(-s * 0.20, -s * 0.20),
                    c + egui::Vec2::new(s * 0.20, s * 0.20),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    c + egui::Vec2::new(s * 0.20, -s * 0.20),
                    c + egui::Vec2::new(-s * 0.20, s * 0.20),
                ],
                stroke,
            );
        }
        UiIcon::Cloud => {
            // SoundCloud 風の雲 (円 3 つ + ベース)。
            painter.circle_filled(c + egui::Vec2::new(-s * 0.14, -s * 0.02), s * 0.17, color);
            painter.circle_filled(c + egui::Vec2::new(s * 0.08, -s * 0.10), s * 0.20, color);
            painter.circle_filled(c + egui::Vec2::new(s * 0.22, s * 0.02), s * 0.14, color);
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::Pos2::new(c.x - s * 0.30, c.y - s * 0.02),
                    egui::Pos2::new(c.x + s * 0.28, c.y + s * 0.18),
                ),
                s * 0.08,
                color,
            );
        }
        UiIcon::Heart => {
            let r = s * 0.16;
            painter.circle_filled(c + egui::Vec2::new(-r * 0.85, -r * 0.55), r, color);
            painter.circle_filled(c + egui::Vec2::new(r * 0.85, -r * 0.55), r, color);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::Vec2::new(-r * 1.80, -r * 0.10),
                    c + egui::Vec2::new(r * 1.80, -r * 0.10),
                    c + egui::Vec2::new(0.0, r * 1.55),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        UiIcon::Bookmark => {
            let w = s * 0.24;
            let h = s * 0.34;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::Vec2::new(-w, -h),
                    c + egui::Vec2::new(w, -h),
                    c + egui::Vec2::new(w, h),
                    c + egui::Vec2::new(0.0, h * 0.45),
                    c + egui::Vec2::new(-w, h),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        UiIcon::Users => {
            let r = s * 0.14;
            painter.circle_filled(c + egui::Vec2::new(-r * 0.7, -r * 0.9), r, color);
            painter.circle_filled(c + egui::Vec2::new(r * 1.1, -r * 0.6), r * 0.8, color);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::Vec2::new(-r * 2.0, r * 1.5),
                    c + egui::Vec2::new(-r * 2.0, r * 0.5),
                    c + egui::Vec2::new(0.0, r * 0.2),
                    c + egui::Vec2::new(r * 0.6, r * 0.6),
                    c + egui::Vec2::new(r * 0.6, r * 1.5),
                ],
                color,
                egui::Stroke::NONE,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::Vec2::new(r * 0.8, r * 1.5),
                    c + egui::Vec2::new(r * 0.8, r * 0.8),
                    c + egui::Vec2::new(r * 2.0, r * 0.5),
                    c + egui::Vec2::new(r * 2.4, r * 1.5),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        UiIcon::Refresh => {
            // 円弧 + 矢印 (簡易)。
            let r = s * 0.26;
            let mut pts = Vec::new();
            for k in 0..24 {
                let a = -std::f32::consts::FRAC_PI_2
                    + std::f32::consts::TAU * k as f32 / 24.0 * 0.8;
                pts.push(c + egui::Vec2::new(a.cos() * r, a.sin() * r));
            }
            painter.add(egui::Shape::line(pts, egui::Stroke::new(1.4, color)));
            let tip = c + egui::Vec2::new(
                (-std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * 0.8).cos() * r,
                (-std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * 0.8).sin() * r,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    tip + egui::Vec2::new(-s * 0.10, -s * 0.02),
                    tip + egui::Vec2::new(s * 0.04, -s * 0.10),
                    tip + egui::Vec2::new(s * 0.02, s * 0.06),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        UiIcon::Shuffle => {
            // 交差する2本の矢印。
            let w = s * 0.30;
            let h = s * 0.22;
            painter.add(egui::Shape::line(
                vec![
                    c + egui::Vec2::new(-w, -h),
                    c + egui::Vec2::new(-w * 0.15, -h),
                    c + egui::Vec2::new(w * 0.15, h),
                    c + egui::Vec2::new(w * 0.62, h),
                ],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![
                    c + egui::Vec2::new(-w, h),
                    c + egui::Vec2::new(-w * 0.35, h),
                    c + egui::Vec2::new(w * 0.0, -h * 0.2),
                ],
                stroke,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::Vec2::new(w * 0.62, h * 0.45),
                    c + egui::Vec2::new(w, h),
                    c + egui::Vec2::new(w * 0.62, h * 1.55),
                ],
                color,
                egui::Stroke::NONE,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::Vec2::new(w * 0.35, -h * 1.55),
                    c + egui::Vec2::new(w * 0.75, -h),
                    c + egui::Vec2::new(w * 0.35, -h * 0.45),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        UiIcon::Repeat => {
            // 角丸ループ + 矢印。
            let r = s * 0.27;
            painter.rect_stroke(
                egui::Rect::from_center_size(c, egui::Vec2::new(r * 2.2, r * 1.5)),
                r * 0.6,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::Vec2::new(r * 0.45, -r * 1.15),
                    c + egui::Vec2::new(r * 1.05, -r * 0.75),
                    c + egui::Vec2::new(r * 0.45, -r * 0.35),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        UiIcon::Sliders => {
            // 横スライダー3本 (つまみ位置違い)。
            for (i, x) in [-0.26f32, 0.0, 0.26].iter().enumerate() {
                let y = c.y + (i as f32 - 1.0) * s * 0.24;
                painter.line_segment(
                    [
                        egui::Pos2::new(c.x - s * 0.34, y),
                        egui::Pos2::new(c.x + s * 0.34, y),
                    ],
                    stroke,
                );
                let knob = c.x + s * if i == 0 { 0.14 } else if i == 1 { -0.12 } else { 0.04 };
                painter.circle_filled(egui::Pos2::new(knob, y), s * 0.07, color);
                let _ = x;
            }
        }
        UiIcon::Queue => {
            // リスト (3本の線 + 音符)。
            for i in 0..3 {
                let y = c.y + (i as f32 - 1.0) * s * 0.24;
                painter.line_segment(
                    [
                        egui::Pos2::new(c.x - s * 0.30, y),
                        egui::Pos2::new(c.x + s * 0.10, y),
                    ],
                    stroke,
                );
            }
            painter.circle_filled(c + egui::Vec2::new(s * 0.22, s * 0.12), s * 0.08, color);
            painter.line_segment(
                [
                    c + egui::Vec2::new(s * 0.29, s * 0.12),
                    c + egui::Vec2::new(s * 0.29, -s * 0.24),
                ],
                stroke,
            );
        }
        UiIcon::Volume => {
            // スピーカー + 波。
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + egui::Vec2::new(-s * 0.30, -s * 0.10),
                    c + egui::Vec2::new(-s * 0.12, -s * 0.10),
                    c + egui::Vec2::new(s * 0.04, -s * 0.28),
                    c + egui::Vec2::new(s * 0.04, s * 0.28),
                    c + egui::Vec2::new(-s * 0.12, s * 0.10),
                    c + egui::Vec2::new(-s * 0.30, s * 0.10),
                ],
                color,
                egui::Stroke::NONE,
            ));
            for (r, w) in [(0.18f32, 1.2f32), (0.30, 1.2)] {
                let mut pts = Vec::new();
                for k in 0..10 {
                    let a = -0.9 + 1.8 * k as f32 / 9.0;
                    pts.push(c + egui::Vec2::new(s * r * a.cos(), s * r * a.sin()));
                }
                painter.add(egui::Shape::line(pts, egui::Stroke::new(w, color)));
            }
        }
        UiIcon::Download => {
            // lucide download: 下向き矢印 + 受け皿。
            painter.line_segment(
                [
                    c + egui::Vec2::new(0.0, -s * 0.32),
                    c + egui::Vec2::new(0.0, s * 0.06),
                ],
                stroke,
            );
            painter.add(egui::Shape::line(
                vec![
                    c + egui::Vec2::new(-s * 0.16, -s * 0.10),
                    c + egui::Vec2::new(0.0, s * 0.06),
                    c + egui::Vec2::new(s * 0.16, -s * 0.10),
                ],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![
                    c + egui::Vec2::new(-s * 0.28, s * 0.16),
                    c + egui::Vec2::new(-s * 0.28, s * 0.30),
                    c + egui::Vec2::new(s * 0.28, s * 0.30),
                    c + egui::Vec2::new(s * 0.28, s * 0.16),
                ],
                stroke,
            ));
        }
        UiIcon::Trash => {
            // lucide trash-2: 蓋 + 取っ手 + 缶 + 縦線 2 本。
            painter.line_segment(
                [
                    c + egui::Vec2::new(-s * 0.30, -s * 0.20),
                    c + egui::Vec2::new(s * 0.30, -s * 0.20),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    c + egui::Vec2::new(-s * 0.10, -s * 0.20),
                    c + egui::Vec2::new(-s * 0.10, -s * 0.32),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    c + egui::Vec2::new(s * 0.10, -s * 0.20),
                    c + egui::Vec2::new(s * 0.10, -s * 0.32),
                ],
                stroke,
            );
            painter.add(egui::Shape::line(
                vec![
                    c + egui::Vec2::new(-s * 0.22, -s * 0.20),
                    c + egui::Vec2::new(-s * 0.18, s * 0.30),
                    c + egui::Vec2::new(s * 0.18, s * 0.30),
                    c + egui::Vec2::new(s * 0.22, -s * 0.20),
                ],
                stroke,
            ));
            painter.line_segment(
                [
                    c + egui::Vec2::new(-s * 0.07, -s * 0.08),
                    c + egui::Vec2::new(-s * 0.05, s * 0.18),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    c + egui::Vec2::new(s * 0.07, -s * 0.08),
                    c + egui::Vec2::new(s * 0.05, s * 0.18),
                ],
                stroke,
            );
        }
        UiIcon::AudioLines => {
            // lucide audio-lines: 高さの違う縦バー 5 本 (波形)。
            for (i, h) in [0.30f32, 0.50, 0.36, 0.56, 0.32].iter().enumerate() {
                let x = c.x + (i as f32 - 2.0) * s * 0.12;
                painter.line_segment(
                    [
                        egui::Pos2::new(x, c.y - s * h * 0.5),
                        egui::Pos2::new(x, c.y + s * h * 0.5),
                    ],
                    egui::Stroke::new(1.6, color),
                );
            }
        }
        UiIcon::Plus => {
            painter.line_segment(
                [
                    c + egui::Vec2::new(-s * 0.28, 0.0),
                    c + egui::Vec2::new(s * 0.28, 0.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    c + egui::Vec2::new(0.0, -s * 0.28),
                    c + egui::Vec2::new(0.0, s * 0.28),
                ],
                stroke,
            );
        }
        UiIcon::Link => {
            // 斜めの鎖 2 本 (lucide link の簡略形)。
            painter.line_segment(
                [
                    c + egui::Vec2::new(-0.24 * s, 0.10 * s),
                    c + egui::Vec2::new(0.10 * s, 0.24 * s),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    c + egui::Vec2::new(-0.10 * s, -0.24 * s),
                    c + egui::Vec2::new(0.24 * s, -0.10 * s),
                ],
                stroke,
            );
        }
        UiIcon::ListPlus => {
            // リスト 3 本 + 右下の +。
            for i in 0..3 {
                let y = c.y + (i as f32 - 1.0) * s * 0.24 - s * 0.06;
                painter.line_segment(
                    [
                        egui::Pos2::new(c.x - s * 0.30, y),
                        egui::Pos2::new(c.x + s * 0.10, y),
                    ],
                    stroke,
                );
            }
            let px = c.x + s * 0.22;
            let py = c.y + s * 0.22;
            painter.line_segment(
                [
                    egui::Pos2::new(px - s * 0.10, py),
                    egui::Pos2::new(px + s * 0.10, py),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::Pos2::new(px, py - s * 0.10),
                    egui::Pos2::new(px, py + s * 0.10),
                ],
                stroke,
            );
        }
        UiIcon::MapPin => {
            painter.circle_stroke(c + egui::Vec2::new(0.0, -s * 0.12), s * 0.16, stroke);
            painter.add(egui::Shape::line(
                vec![
                    c + egui::Vec2::new(-s * 0.12, -s * 0.02),
                    c + egui::Vec2::new(0.0, s * 0.30),
                    c + egui::Vec2::new(s * 0.12, -s * 0.02),
                ],
                stroke,
            ));
        }
        UiIcon::Power => {
            // 電源: 上に縦線 + 開いた円弧。
            painter.line_segment(
                [
                    c + egui::Vec2::new(0.0, -s * 0.30),
                    c + egui::Vec2::new(0.0, -s * 0.02),
                ],
                stroke,
            );
            let r = s * 0.26;
            let mut pts = Vec::new();
            for k in 0..=20 {
                let a = -std::f32::consts::FRAC_PI_2
                    + std::f32::consts::TAU * (k as f32 / 20.0) * 0.86
                    + std::f32::consts::TAU * 0.07;
                pts.push(c + egui::Vec2::new(a.cos() * r, a.sin() * r));
            }
            painter.add(egui::Shape::line(pts, stroke));
        }
    }
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
    paint_transport_glyph(painter, rect, icon, color, size * 0.5);
    resp
}

/// 白丸 + 黒グリフの主要トランスポート (Play/Pause。Tauri 版の白丸ボタン)。
pub fn transport_primary_button(
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
    let bg = if !enabled {
        egui::Color32::from_white_alpha(60)
    } else if resp.hovered() {
        egui::Color32::from_white_alpha(235)
    } else {
        egui::Color32::WHITE
    };
    painter.circle_filled(rect.center(), size * 0.5, bg);
    let color = if enabled {
        egui::Color32::from_black_alpha(230)
    } else {
        egui::Color32::from_black_alpha(120)
    };
    paint_transport_glyph(painter, rect, icon, color, size * 0.5);
    resp
}

/// トランスポートのグリフ本体 (再生/一時停止/前へ/次へ)。
pub(crate) fn paint_transport_glyph(
    painter: &egui::Painter,
    rect: egui::Rect,
    icon: TransportIcon,
    color: egui::Color32,
    s: f32,
) {
    let c = rect.center();
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
}

/// カードの操作結果 (アート=再生 / タイトル=Track / アーティスト=User /
/// 右クリック=メニュー)。ホバーの like / playlist / queue / share は
/// `CardAction` 経由で shell が実行する。
pub struct CardHit {
    pub play: egui::Response,
    pub title: egui::Response,
    pub artist: egui::Response,
    pub menu_clicked: bool,
}

impl CardHit {
    pub fn play_clicked(&self) -> bool {
        self.play.clicked()
    }
    pub fn title_clicked(&self) -> bool {
        self.title.clicked()
    }
    pub fn artist_clicked(&self) -> bool {
        self.artist.clicked()
    }
    pub fn menu_clicked(&self) -> bool {
        self.menu_clicked
    }
}

/// カードのホバー操作のうち shell 側の状態が要るもの (like トグル等)。
/// ウィジェットが積み、shell が毎フレーム回収して実行する。
pub enum CardAction {
    ToggleLike { urn: String, next: bool },
    AddToQueue(Track),
    AddToPlaylist(Track),
    CopyLink(Track),
}

static CARD_ACTIONS: std::sync::Mutex<Vec<CardAction>> = std::sync::Mutex::new(Vec::new());

pub fn request_card_action(a: CardAction) {
    if let Ok(mut q) = CARD_ACTIONS.lock() {
        q.push(a);
    }
}

pub fn take_card_actions() -> Vec<CardAction> {
    CARD_ACTIONS
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

/// like の楽観上書き (ビューをまたいだ即時反映。Tauri の likes ストア相当)。
static LIKED_OVERRIDES: std::sync::Mutex<Vec<(String, bool)>> =
    std::sync::Mutex::new(Vec::new());

pub fn liked_state(urn: &str, fallback: bool) -> bool {
    LIKED_OVERRIDES
        .lock()
        .ok()
        .and_then(|v| v.iter().find(|(u, _)| u == urn).map(|(_, l)| *l))
        .unwrap_or(fallback)
}

pub fn set_liked_override(urn: &str, liked: bool) {
    if let Ok(mut v) = LIKED_OVERRIDES.lock() {
        if let Some(slot) = v.iter_mut().find(|(u, _)| u == urn) {
            slot.1 = liked;
        } else {
            v.push((urn.to_string(), liked));
        }
    }
}

/// カードのアートワーク (Tauri: aspect-square rounded bg-white/[0.03] +
/// hover で画像ズーム)。未取得時は 32px の再生アイコン (白 20%)。
pub fn card_art(
    ui: &mut egui::Ui,
    rt: &tokio::runtime::Handle,
    images: &mut Images,
    url: Option<&str>,
    size: f32,
    corner: egui::CornerRadius,
    zoom: f32,
) -> egui::Response {
    let tex = url.and_then(|u| images.texture(ui, rt, Some(u)));
    let (rect, resp) =
        ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, corner, egui::Color32::from_white_alpha(8));
    match tex {
        Some(tex) => {
            let draw =
                egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(size * zoom));
            let shape =
                egui::epaint::RectShape::filled(draw, corner, egui::Color32::WHITE)
                    .with_texture(
                        tex,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    );
            painter.add(egui::Shape::Rect(shape));
        }
        None => {
            paint_transport_glyph(
                painter,
                egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(32.0)),
                TransportIcon::Play,
                egui::Color32::from_white_alpha(51),
                32.0,
            );
        }
    }
    resp
}

/// カードのホバー/再生オーバーレイ (Tauri: bg-black/30 + 白丸 40px +
/// 黒グリフ。ホバーで scale 75→100)。
pub fn card_overlay(
    painter: &egui::Painter,
    rect: egui::Rect,
    corner: f32,
    t: f32,
    icon: TransportIcon,
) {
    if t <= 0.001 {
        return;
    }
    painter.rect_filled(rect, corner, egui::Color32::from_black_alpha((77.0 * t) as u8));
    let s = 0.75 + 0.25 * t;
    painter.circle_filled(
        rect.center(),
        20.0 * s,
        egui::Color32::from_white_alpha((230.0 * t) as u8),
    );
    let mut c = rect.center();
    if matches!(icon, TransportIcon::Play) {
        c.x += 1.0;
    }
    paint_transport_glyph(
        painter,
        egui::Rect::from_center_size(c, egui::Vec2::splat(20.0 * s)),
        icon,
        egui::Color32::from_black_alpha((230.0 * t) as u8),
        20.0 * s,
    );
}

/// カードのタイトル/アーティスト行 (truncate + hover 色。Tauri の <p> 相当)。
pub fn card_text(
    ui: &mut egui::Ui,
    text: &str,
    font: egui::FontId,
    idle: egui::Color32,
    hover: egui::Color32,
    height: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    let color = if resp.hovered() { hover } else { idle };
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    job.wrap = egui::text::TextWrapping {
        max_width: rect.width().max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = ui.painter().layout_job(job);
    ui.painter()
        .galley(egui::pos2(rect.left(), rect.top()), galley, color);
    resp
}

/// Tauri TrackCard 相当のカード 1 枚 (Home/Search/Tag/Library)。
/// `size` = セル幅。カード幅を固定して列をそろえる。
#[allow(clippy::too_many_arguments)]
fn paint_track_card(
    ui: &mut egui::Ui,
    rt: &tokio::runtime::Handle,
    images: &mut Images,
    track: &Track,
    size: f32,
    playing: bool,
    accent: egui::Color32,
    show_stats: bool,
) -> CardHit {
    ui.vertical(|ui| {
        ui.set_width(size);
        ui.spacing_mut().item_spacing.y = 2.0;
        // ホバー判定は配置前に予測する (ズームとオーバーレイに使う)。
        let next =
            egui::Rect::from_min_size(ui.next_widget_position(), egui::Vec2::splat(size));
        let hovered = ui.rect_contains_pointer(next);
        let hover_t = ui.ctx().animate_bool_with_time(
            egui::Id::new(("card-hover", &track.urn)),
            hovered || playing,
            0.15,
        );
        let reveal = ui.ctx().animate_bool_with_time(
            egui::Id::new(("card-reveal", &track.urn)),
            hovered,
            0.2,
        );
        let art = track.artwork("t300x300");
        let play = card_art(
            ui,
            rt,
            images,
            art.as_deref(),
            size,
            egui::CornerRadius::same(16),
            1.0 + 0.04 * hover_t,
        );
        // ring (Tauri: ring-1 ring-white/[0.06] → hover ring-white/[0.12])。
        ui.painter().rect_stroke(
            play.rect,
            16.0,
            egui::Stroke::new(
                1.0,
                egui::Color32::from_white_alpha((15.0 + 16.0 * hover_t) as u8),
            ),
            egui::StrokeKind::Inside,
        );
        // 再生/ホバーオーバーレイ (Tauri: bg-black/30 + 白丸 40px + 黒グリフ。
        // 再生中は常時、ホバーで scale 75→100)。
        let t = if playing { 1.0 } else { hover_t };
        card_overlay(
            ui.painter(),
            play.rect,
            16.0,
            t,
            if playing {
                TransportIcon::Pause
            } else {
                TransportIcon::Play
            },
        );
        // showStats: 右下の plays + 時間チップ (Tauri と同じ)。
        if show_stats {
            let chip_font = crate::theme::medium(10.0);
            let chip_text = egui::Color32::from_white_alpha(204);
            let mut chips: Vec<String> = Vec::new();
            if let Some(plays) = track.playback_count {
                chips.push(format!("{} plays", fmt_count(plays)));
            }
            chips.push(fmt_ms_short(track.duration));
            let pad = egui::vec2(8.0, 2.0);
            let mut chip_x = play.rect.right() - 8.0;
            let chip_bottom = play.rect.bottom() - 8.0;
            for label in chips.iter().rev() {
                let galley = ui
                    .painter()
                    .layout_no_wrap(label.clone(), chip_font.clone(), chip_text);
                let w = galley.size().x + pad.x * 2.0;
                let h = galley.size().y + pad.y * 2.0;
                let chip = egui::Rect::from_min_size(
                    egui::Pos2::new(chip_x - w, chip_bottom - h),
                    egui::vec2(w, h),
                );
                ui.painter()
                    .rect_filled(chip, h * 0.5, egui::Color32::from_black_alpha(89));
                ui.painter().galley(chip.min + pad, galley, chip_text);
                chip_x -= w + 4.0;
            }
        }
        let chip_c = |c: egui::Color32, a: f32| {
            egui::Color32::from_rgba_unmultiplied(
                c.r(),
                c.g(),
                c.b(),
                (c.a() as f32 * a) as u8,
            )
        };
        // like チップ (左上、hover でフェードイン。Tauri: LikeButton chip)。
        let liked = liked_state(&track.urn, track.user_favorite.unwrap_or(false));
        {
            let center = egui::pos2(play.rect.left() + 20.0, play.rect.top() + 20.0);
            let rect = egui::Rect::from_center_size(center, egui::Vec2::splat(24.0));
            let resp = ui.interact(
                rect,
                egui::Id::new(("card-like", &track.urn)),
                egui::Sense::click(),
            );
            if liked {
                ui.painter().circle_filled(
                    center,
                    12.0,
                    chip_c(
                        egui::Color32::from_rgba_unmultiplied(
                            accent.r(),
                            accent.g(),
                            accent.b(),
                            204,
                        ),
                        reveal,
                    ),
                );
            } else {
                let a = if resp.hovered() { 179 } else { 128 };
                ui.painter().circle_filled(
                    center,
                    12.0,
                    chip_c(egui::Color32::from_black_alpha(a), reveal),
                );
            }
            let color = if liked {
                chip_c(contrast_color(accent), reveal)
            } else if resp.hovered() {
                chip_c(egui::Color32::WHITE, reveal)
            } else {
                chip_c(egui::Color32::from_white_alpha(204), reveal)
            };
            paint_heart(
                ui.painter(),
                egui::Rect::from_center_size(center, egui::Vec2::splat(12.0)),
                color,
                liked,
            );
            if resp
                .on_hover_text(if liked { "Liked" } else { "Like" })
                .clicked()
            {
                request_card_action(CardAction::ToggleLike {
                    urn: track.urn.clone(),
                    next: !liked,
                });
            }
        }
        // 右上の 3 チップ: playlist / queue / share (Tauri: 24px 円 + 12px
        // アイコン、share はカード幅 120px 未満では非表示)。
        let trio: [(&str, UiIcon, bool); 3] = [
            ("playlist", UiIcon::ListPlus, true),
            ("queue", UiIcon::Queue, true),
            ("share", UiIcon::Link, size >= 120.0),
        ];
        let mut x = play.rect.right() - 20.0;
        for (id, icon, visible) in trio {
            if !visible {
                continue;
            }
            let center = egui::pos2(x, play.rect.top() + 20.0);
            let rect = egui::Rect::from_center_size(center, egui::Vec2::splat(24.0));
            let resp = ui.interact(
                rect,
                egui::Id::new(("card-chip", &track.urn, id)),
                egui::Sense::click(),
            );
            let a = if resp.hovered() { 179 } else { 128 };
            ui.painter().circle_filled(
                center,
                12.0,
                chip_c(egui::Color32::from_black_alpha(a), reveal),
            );
            let color = if resp.hovered() {
                chip_c(egui::Color32::WHITE, reveal)
            } else {
                chip_c(egui::Color32::from_white_alpha(204), reveal)
            };
            paint_ui_icon(
                ui.painter(),
                egui::Rect::from_center_size(center, egui::Vec2::splat(12.0)),
                icon,
                color,
            );
            let tip = match id {
                "playlist" => "Add to playlist",
                "queue" => "Add to Queue",
                _ => "Copy link",
            };
            let resp = resp.on_hover_text(tip);
            if resp.clicked() {
                match id {
                    "playlist" => {
                        request_card_action(CardAction::AddToPlaylist(track.clone()))
                    }
                    "queue" => request_card_action(CardAction::AddToQueue(track.clone())),
                    _ => request_card_action(CardAction::CopyLink(track.clone())),
                }
            }
            x -= 26.0;
        }
        // 情報 (Tauri: mt-3 / タイトル 13 medium white/90 hover:white /
        // アーティスト 11 white/35 hover:white/60)。
        ui.add_space(10.0);
        let title = card_text(
            ui,
            track.display_title(),
            crate::theme::medium(13.0),
            egui::Color32::from_white_alpha(230),
            egui::Color32::WHITE,
            17.0,
        );
        let artist = card_text(
            ui,
            track.artist_name(),
            egui::FontId::proportional(11.0),
            egui::Color32::from_white_alpha(89),
            egui::Color32::from_white_alpha(153),
            15.0,
        );
        if artist.secondary_clicked() {
            request_user_menu(ui, track, artist.rect);
        }
        let menu_clicked = play.secondary_clicked()
            || title.secondary_clicked()
            || artist.secondary_clicked();
        CardHit {
            play,
            title,
            artist,
            menu_clicked,
        }
    })
    .inner
}

/// グリッド用カード (Home の棚・Library レール・Tag)。
pub fn track_card(
    ui: &mut egui::Ui,
    rt: &tokio::runtime::Handle,
    images: &mut Images,
    track: &Track,
    size: f32,
    playing: bool,
    accent: egui::Color32,
) -> CardHit {
    paint_track_card(ui, rt, images, track, size, playing, accent, false)
}

/// 行の操作結果 (アート=再生 / タイトル=Track / アーティスト=User / 右クリック=メニュー)。
pub struct RowParts {
    pub art: egui::Response,
    pub title: egui::Response,
    pub artist: egui::Response,
}

impl RowParts {
    pub fn play_clicked(&self) -> bool {
        self.art.clicked()
    }
    pub fn title_clicked(&self) -> bool {
        self.title.clicked()
    }
    pub fn artist_clicked(&self) -> bool {
        self.artist.clicked()
    }
    pub fn menu_clicked(&self) -> bool {
        self.art.secondary_clicked()
            || self.title.secondary_clicked()
            || self.artist.secondary_clicked()
    }
}

/// 一覧行 (40px アート + タイトル/アーティスト + 時間)。ホバーで淡い背景 (Tauri と同じ)。
pub fn track_row(
    ui: &mut egui::Ui,
    rt: &tokio::runtime::Handle,
    images: &mut Images,
    track: &Track,
    playing: bool,
    accent: egui::Color32,
    duration_text: Option<&str>,
) -> RowParts {
    let art = track.artwork("t200x200");
    // 背景を後から差し込むためのプレースホルダ (コンテンツの下に描かれる)。
    let bg_idx = ui.painter().add(egui::Shape::Noop);
    let inner = ui.horizontal(|ui| {
        // 再生中は行頭にイコライザー (Spotify 風、背景ハイライトは使わない)。
        if playing {
            playing_bars(ui, true, accent);
        } else {
            ui.add_space(13.0);
        }
        let r = images.show(ui, rt, art.as_deref(), 40.0);
        let title = if playing {
            egui::RichText::new(track.display_title()).color(accent)
        } else {
            egui::RichText::new(track.display_title())
        };
        let (t, a) = ui
            .vertical(|ui| {
                let t = ui.add(
                    egui::Label::new(title)
                        .truncate()
                        .wrap_mode(egui::TextWrapMode::Truncate)
                        .sense(egui::Sense::click()),
                );
                let a = ui.add(
                    egui::Label::new(egui::RichText::new(track.artist_name()).weak())
                        .truncate()
                        .wrap_mode(egui::TextWrapMode::Truncate)
                        .sense(egui::Sense::click()),
                );
                if a.secondary_clicked() {
                    request_user_menu(ui, track, a.rect);
                }
                (t, a)
            })
            .inner;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(d) = duration_text {
                ui.label(d);
            }
        });
        (r, t, a)
    });
    let (r, t, a) = inner.inner;
    // ホバー背景 (フェード付き)。
    let hover_t = ui.ctx().animate_bool_with_time(
        egui::Id::new(("row-hover", &track.urn)),
        inner.response.hovered(),
        0.12,
    );
    if hover_t > 0.001 {
        let rect = inner.response.rect.expand2(egui::vec2(6.0, 3.0));
        ui.painter().set(
            bg_idx,
            egui::Shape::rect_filled(
                rect,
                6.0,
                egui::Color32::from_white_alpha((16.0 * hover_t) as u8),
            ),
        );
    }
    RowParts {
        art: r,
        title: t,
        artist: a,
    }
}

/// ジャンルの固定色 (search/utils.ts の GENRES) + ハッシュ由来の HSL 色。
pub fn genre_color(name: &str) -> egui::Color32 {
    const FIXED: [(&str, [u8; 3]); 12] = [
        ("lofi", [0x8b, 0x9d, 0xc3]),
        ("house", [0xff, 0x7a, 0x59]),
        ("phonk", [0xc0, 0x26, 0xd3]),
        ("ambient", [0x5e, 0xea, 0xd4]),
        ("rnb", [0xf0, 0xab, 0xfc]),
        ("trap", [0xfb, 0x71, 0x85]),
        ("jazz", [0xfb, 0xbf, 0x24]),
        ("techno", [0x60, 0xa5, 0xfa]),
        ("indie", [0xa3, 0xe6, 0x35]),
        ("soul", [0xfc, 0xa5, 0xa5]),
        ("dnb", [0x34, 0xd3, 0x99]),
        ("hyperpop", [0xe8, 0x79, 0xf9]),
    ];
    let lower = name.to_lowercase();
    if let Some((_, [r, g, b])) = FIXED.iter().find(|(k, _)| *k == lower) {
        return egui::Color32::from_rgb(*r, *g, *b);
    }
    let mut hash: i32 = 0;
    for ch in name.chars() {
        hash = hash.wrapping_mul(31).wrapping_add(ch as i32);
    }
    let hue = (hash.abs() % 360) as f32 / 360.0;
    let hsva = egui::ecolor::Hsva::new(hue, 0.70, 0.62, 1.0);
    let [r, g, b, _] = hsva.to_srgba_unmultiplied();
    egui::Color32::from_rgb(r, g, b)
}

/// created_at からの経過表示 (Tauri: FreshDrops の age / RoomHero の 2y)。
pub fn age_text(created_at: Option<&str>) -> String {
    let Some(s) = created_at else {
        return String::new();
    };
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) else {
        return String::new();
    };
    let days = (chrono::Utc::now() - dt.with_timezone(&chrono::Utc)).num_days();
    if days < 7 {
        format!("{}d", days.max(0))
    } else if days < 30 {
        format!("{}w", days / 7)
    } else if days < 365 {
        format!("{}mo", days / 30)
    } else {
        format!("{}y", days / 365)
    }
}

/// タブ用のニュートラルなピル (選択 = white/10。Tauri のタブ)。
pub fn tab_button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let text = egui::RichText::new(label).size(12.5);
    let text = if selected {
        text.color(egui::Color32::from_white_alpha(230))
    } else {
        text.weak()
    };
    ui.add(egui::Button::new(text).frame(selected))
}

/// 数値の省略表示 (833.4K / 1.2M)。対応: formatters.ts の `fc`。
pub fn fmt_count(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// ミリ秒 → `m:ss` (カードの統計チップ / Offline 行用)。
pub fn fmt_ms_short(ms: i64) -> String {
    let s = (ms.max(0) / 1000) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// 検索結果用カード (Tauri: TrackCard showStats)。
pub fn track_card_stats(
    ui: &mut egui::Ui,
    rt: &tokio::runtime::Handle,
    images: &mut Images,
    track: &Track,
    size: f32,
    playing: bool,
    accent: egui::Color32,
) -> CardHit {
    paint_track_card(ui, rt, images, track, size, playing, accent, true)
}

/// 検索のユーザ/プレイリスト/アルバム行 (アート + 名前。行全体クリック + ホバー背景)。
pub fn search_row(
    ui: &mut egui::Ui,
    rt: &tokio::runtime::Handle,
    images: &mut Images,
    art_url: Option<&str>,
    name: &str,
    round: bool,
) -> egui::Response {
    let bg_idx = ui.painter().add(egui::Shape::Noop);
    let inner = ui.horizontal(|ui| {
        let corner = if round {
            egui::CornerRadius::same(18)
        } else {
            egui::CornerRadius::same(6)
        };
        let img = images.show_rounded(ui, rt, art_url, 36.0, corner);
        let label = ui.add(
            egui::Label::new(egui::RichText::new(name).size(13.0))
                .sense(egui::Sense::click()),
        );
        img.union(label)
    });
    let hover_t = ui.ctx().animate_bool_with_time(
        egui::Id::new(("search-row", name)),
        inner.response.hovered(),
        0.12,
    );
    if hover_t > 0.001 {
        let rect = inner.response.rect.expand2(egui::vec2(6.0, 4.0));
        ui.painter().set(
            bg_idx,
            egui::Shape::rect_filled(
                rect,
                8.0,
                egui::Color32::from_white_alpha((14.0 * hover_t) as u8),
            ),
        );
    }
    inner.inner
}

/// サイドバー/設定ナビの項目 (アイコン + ラベル。選択は淡いグレー)。
pub fn nav_item(
    ui: &mut egui::Ui,
    icon: UiIcon,
    label: &str,
    selected: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::Vec2::new(ui.available_width(), 30.0),
        egui::Sense::click(),
    );
    if selected {
        ui.painter()
            .rect_filled(rect, 6.0, egui::Color32::from_white_alpha(20));
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, egui::Color32::from_white_alpha(10));
    }
    let icon_color = if selected {
        egui::Color32::from_white_alpha(235)
    } else {
        egui::Color32::from_white_alpha(150)
    };
    let icon_rect = egui::Rect::from_center_size(
        egui::Pos2::new(rect.left() + 15.0, rect.center().y),
        egui::Vec2::splat(16.0),
    );
    paint_ui_icon(ui.painter(), icon_rect, icon, icon_color);
    ui.painter().text(
        egui::Pos2::new(rect.left() + 32.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        if selected {
            egui::Color32::from_white_alpha(240)
        } else {
            egui::Color32::from_white_alpha(190)
        },
    );
    resp
}

/// Tauri の CSS グリッド相当: 列数は **ビューポート幅**のブレークポイントで
/// 決まる (Tailwind sm 640 / md 768 / lg 1024 / xl 1280)。
pub fn grid_cols(
    ctx: &egui::Context,
    xs: usize,
    sm: usize,
    md: usize,
    lg: usize,
    xl: usize,
) -> usize {
    let vw = ctx.content_rect().width();
    if vw < 640.0 {
        xs
    } else if vw < 768.0 {
        sm
    } else if vw < 1024.0 {
        md
    } else if vw < 1280.0 {
        lg
    } else {
        xl
    }
}

/// セル幅 (CSS の `1fr` 相当)。`avail` はコンテンツ幅。
pub fn grid_cell(avail: f32, cols: usize, gap: f32) -> f32 {
    ((avail - gap * (cols.saturating_sub(1)) as f32) / cols.max(1) as f32).max(60.0)
}

/// セクション見出し (Tauri: text-[16px] font-semibold white/90)。
pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(
        egui::RichText::new(text)
            .size(16.0)
            .color(egui::Color32::from_white_alpha(230)),
    ));
}

/// 「See all >」リンク (Tauri: text-[12px] font-semibold white/45 + chevron)。
pub fn see_all(ui: &mut egui::Ui) -> bool {
    link_button(ui, "See all")
}

/// テキストリンク + chevron (Tauri の "See all" / "Show less" 等)。
pub fn link_button(ui: &mut egui::Ui, text: &str) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        // RTL では先に追加した方が右端。chevron を先に置いて [text][›] にする。
        let chev = ui.add(
            egui::Label::new(
                egui::RichText::new("›")
                    .size(14.0)
                    .color(egui::Color32::from_white_alpha(115)),
            )
            .sense(egui::Sense::click()),
        );
        let label = ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .size(12.0)
                    .color(egui::Color32::from_white_alpha(115)),
            )
            .sense(egui::Sense::click()),
        );
        clicked = label.clicked() || chev.clicked();
    });
    clicked
}

/// セクション見出し (タイトル + 件数)。
pub fn section_header(ui: &mut egui::Ui, title: &str, count: Option<usize>) {
    ui.horizontal(|ui| {
        section_title(ui, title);
        if let Some(n) = count {
            ui.label(egui::RichText::new(n.to_string()).size(11.0).weak());
        }
    });
}

/// 読み込み中表示 (スピナー + 控えめラベル)。
pub fn loading_text(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(egui::RichText::new(text).weak());
    });
}

/// 読み込み中表示 ("Loading...")。
pub fn loading(ui: &mut egui::Ui) {
    loading_text(ui, "Loading...");
}

/// 空状態 (控えめな中央寄せテキスト)。
pub fn empty_note(ui: &mut egui::Ui, text: &str) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(text).weak());
    });
    ui.add_space(24.0);
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

/// アクセント色に対するコントラスト色 (黒 or 白。Tauri の `accent-contrast`)。
pub fn contrast_color(accent: egui::Color32) -> egui::Color32 {
    let lum =
        accent.r() as u32 * 299 + accent.g() as u32 * 587 + accent.b() as u32 * 114;
    if lum > 150_000 {
        egui::Color32::BLACK
    } else {
        egui::Color32::WHITE
    }
}

/// ハートのグリフ。`filled` = 塗り (lucide Heart の fill=currentColor 相当)、
/// それ以外は線画。カードのチップ / Like ボタン / バーで共用する。
pub fn paint_heart(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    filled: bool,
) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    if filled {
        let r = s * 0.16;
        painter.circle_filled(c + egui::Vec2::new(-r * 0.85, -r * 0.55), r, color);
        painter.circle_filled(c + egui::Vec2::new(r * 0.85, -r * 0.55), r, color);
        painter.add(egui::Shape::convex_polygon(
            vec![
                c + egui::Vec2::new(-r * 1.80, -r * 0.10),
                c + egui::Vec2::new(r * 1.80, -r * 0.10),
                c + egui::Vec2::new(0.0, r * 1.55),
            ],
            color,
            egui::Stroke::NONE,
        ));
        return;
    }
    // ハート曲線 (16sin³t / 13cos t − 5cos 2t − 2cos 3t − cos 4t) を線で描く。
    let mut pts: Vec<egui::Pos2> = Vec::with_capacity(33);
    for k in 0..=32 {
        let t = std::f32::consts::TAU * k as f32 / 32.0;
        let x = 16.0 * t.sin().powi(3);
        let y = 13.0 * t.cos()
            - 5.0 * (2.0 * t).cos()
            - 2.0 * (3.0 * t).cos()
            - (4.0 * t).cos();
        pts.push(egui::Pos2::new(x, -y));
    }
    let (mut minx, mut maxx) = (f32::MAX, f32::MIN);
    let (mut miny, mut maxy) = (f32::MAX, f32::MIN);
    for p in &pts {
        minx = minx.min(p.x);
        maxx = maxx.max(p.x);
        miny = miny.min(p.y);
        maxy = maxy.max(p.y);
    }
    let span = (maxx - minx).max(maxy - miny).max(1e-3);
    let fit = (s * 0.72) / span;
    let mid = egui::pos2((minx + maxx) * 0.5, (miny + maxy) * 0.5);
    let pts: Vec<egui::Pos2> = pts
        .into_iter()
        .map(|p| c + (p - mid) * fit)
        .collect();
    painter.add(egui::Shape::closed_line(
        pts,
        egui::Stroke::new(1.4, color),
    ));
}

/// Tauri `usePulseHeart` のスケール計算 (heart, pill)。
/// `t` = 経過 0..1 (560ms)、None = 静止。
pub fn pulse_scales(t: Option<f32>) -> (f32, f32) {
    let Some(t) = t else {
        return (1.0, 1.0);
    };
    let s = pulse_swell(t);
    (1.0 - (1.0 - 0.3) * s, 1.0 - 0.03 * s)
}

fn pulse_swell(t: f32) -> f32 {
    const OUT: f32 = 0.4;
    const OVERSHOOT: f32 = 1.7;
    if t <= 0.0 {
        0.0
    } else if t < OUT {
        1.0 - (1.0 - t / OUT).powi(3)
    } else {
        1.0 - pulse_back((t - OUT) / (1.0 - OUT), OVERSHOOT)
    }
}

fn pulse_back(k: f32, c: f32) -> f32 {
    let u = k - 1.0;
    1.0 + (c + 1.0) * u.powi(3) + c * u.powi(2)
}

/// Like ボタンの見た目 (Tauri の LikeButton バリアント)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LikeStyle {
    /// トラックページの `LikeBtn`: 枠線付きのハート + 件数 (h-10 rounded-full)。
    Chip,
    /// プレイリストページの `PlaylistLikeBtn`: 枠なしのハート + 件数
    /// (h-10 rounded-md、hover で白 6% 背景)。
    Ghost,
}

fn paint_like_button(
    ui: &mut egui::Ui,
    liked: bool,
    count: i64,
    accent: egui::Color32,
    style: LikeStyle,
    heart_scale: f32,
    pill_scale: f32,
) -> egui::Response {
    let h = 40.0;
    let pad_x = if style == LikeStyle::Chip { 14.0 } else { 12.0 };
    let heart = 15.0;
    let gap = 6.0;
    let font = crate::theme::medium(12.5);
    let text = fmt_count(count);
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), font.clone(), egui::Color32::WHITE);
    let w = pad_x * 2.0 + heart + gap + galley.size().x;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let hovered = resp.hovered();
    let color = if liked {
        accent
    } else if hovered {
        egui::Color32::WHITE
    } else if style == LikeStyle::Chip {
        egui::Color32::from_white_alpha(166)
    } else {
        egui::Color32::from_white_alpha(153)
    };
    let painter = ui.painter();
    let scale_pt = |p: egui::Pos2| rect.center() + (p - rect.center()) * pill_scale;
    let pill_rect = egui::Rect::from_min_max(
        scale_pt(rect.min),
        scale_pt(rect.max),
    );
    match style {
        LikeStyle::Chip => {
            let stroke_color = if liked {
                egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 115)
            } else if hovered {
                egui::Color32::from_white_alpha(82)
            } else {
                egui::Color32::from_white_alpha(36)
            };
            painter.rect_stroke(
                pill_rect,
                20.0,
                egui::Stroke::new(1.0, stroke_color),
                egui::StrokeKind::Inside,
            );
        }
        LikeStyle::Ghost => {
            if !liked && hovered {
                painter.rect_filled(pill_rect, 6.0, egui::Color32::from_white_alpha(15));
            }
        }
    }
    let heart_center = scale_pt(egui::pos2(
        rect.left() + pad_x + heart * 0.5,
        rect.center().y,
    ));
    paint_heart(
        painter,
        egui::Rect::from_center_size(
            heart_center,
            egui::Vec2::splat(heart * heart_scale * pill_scale),
        ),
        color,
        liked,
    );
    let galley = painter.layout_no_wrap(text, font, color);
    let text_pos = scale_pt(egui::pos2(
        rect.left() + pad_x + heart + gap,
        rect.center().y - galley.size().y * 0.5,
    ));
    painter.galley(text_pos, galley, color);
    resp
}

/// Tauri `LikeBtn` (トラックページ): ハート + 件数のアウトラインチップ。
pub fn like_chip(
    ui: &mut egui::Ui,
    liked: bool,
    count: i64,
    accent: egui::Color32,
    heart_scale: f32,
    pill_scale: f32,
) -> egui::Response {
    paint_like_button(ui, liked, count, accent, LikeStyle::Chip, heart_scale, pill_scale)
}

/// Tauri `PlaylistLikeBtn` (プレイリストページ): ハート + 件数 (枠なし)。
pub fn like_ghost(
    ui: &mut egui::Ui,
    liked: bool,
    count: i64,
    accent: egui::Color32,
    heart_scale: f32,
    pill_scale: f32,
) -> egui::Response {
    paint_like_button(ui, liked, count, accent, LikeStyle::Ghost, heart_scale, pill_scale)
}

// ─────────────────────── Tauri 準拠のスライダー ───────────────────────

/// Tauri の `input[type=range]` (accent-color) 相当の横スライダー。
/// 4px トラック (白 50% 枠 + 白 20% コア) + アクセント充填 + 15px 円サム。
/// 無効時は 40% 不透明で操作不可。
pub fn range_slider(
    ui: &mut egui::Ui,
    value: &mut f64,
    min: f64,
    max: f64,
    width: f32,
    enabled: bool,
    accent: egui::Color32,
) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(
        egui::vec2(width, 16.0),
        if enabled {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::hover()
        },
    );
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let thumb_r = 7.5;
    let usable = (rect.width() - thumb_r * 2.0).max(1.0);
    let span = (max - min).max(f64::EPSILON);
    if enabled && (resp.dragged() || resp.clicked()) {
        if let Some(pos) = resp.interact_pointer_pos() {
            let frac = ((pos.x - (rect.left() + thumb_r)) / usable).clamp(0.0, 1.0) as f64;
            *value = (min + frac * span).clamp(min, max);
            resp.mark_changed();
        }
    }
    let dim = if enabled { 1.0 } else { 0.4 };
    let white = |a: u8| egui::Color32::from_white_alpha((a as f32 * dim) as u8);
    let accent = egui::Color32::from_rgba_unmultiplied(
        accent.r(),
        accent.g(),
        accent.b(),
        (255.0 * dim) as u8,
    );
    let painter = ui.painter();
    let track = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 4.0));
    let frac = ((*value - min) / span).clamp(0.0, 1.0) as f32;
    let thumb_x = rect.left() + thumb_r + usable * frac;
    painter.rect_filled(track, 2.0, white(51));
    let fill = egui::Rect::from_min_max(track.min, egui::pos2(thumb_x, track.max.y));
    painter.rect_filled(fill, 2.0, accent);
    painter.rect_stroke(
        track,
        2.0,
        egui::Stroke::new(1.0, white(128)),
        egui::StrokeKind::Inside,
    );
    painter.circle_filled(egui::pos2(thumb_x, rect.center().y), thumb_r, accent);
    resp
}

/// Tauri 設定の `RangeSlider` 相当: 4px 白 10% トラック + 16px 白サム。
/// `step` 指定時はその刻みに丸める。
pub fn plain_range_slider(
    ui: &mut egui::Ui,
    value: &mut f64,
    min: f64,
    max: f64,
    width: f32,
    step: Option<f64>,
) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(
        egui::vec2(width, 18.0),
        egui::Sense::click_and_drag(),
    );
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let thumb_r = 8.0;
    let usable = (rect.width() - thumb_r * 2.0).max(1.0);
    let span = (max - min).max(f64::EPSILON);
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let frac = ((pos.x - (rect.left() + thumb_r)) / usable).clamp(0.0, 1.0) as f64;
            let mut v = min + frac * span;
            if let Some(s) = step {
                if s > 0.0 {
                    v = (v / s).round() * s;
                }
            }
            *value = v.clamp(min, max);
            resp.mark_changed();
        }
    }
    let painter = ui.painter();
    let track = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 4.0));
    painter.rect_filled(track, 2.0, egui::Color32::from_white_alpha(26));
    let frac = ((*value - min) / span).clamp(0.0, 1.0) as f32;
    let thumb_x = rect.left() + thumb_r + usable * frac;
    painter.circle_filled(
        egui::pos2(thumb_x, rect.center().y),
        thumb_r,
        egui::Color32::WHITE,
    );
    resp
}

/// Tauri `BandSlider` 相当の縦スライダー (EQ)。140px・3px レール +
/// 中心線 + 上下の充填 (正 = 緑 / 負 = 青) + 16px 円サム。0.5dB 刻み。
/// 戻り値は値が変化したか。
pub fn eq_band_slider(ui: &mut egui::Ui, gain: &mut f64, min: f64, max: f64) -> bool {
    let (rect, mut resp) = ui.allocate_exact_size(
        egui::vec2(28.0, 140.0),
        egui::Sense::click_and_drag(),
    );
    let mut changed = false;
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let pct = 1.0 - ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0) as f64;
            let v = (((pct * (max - min) + min) * 2.0).round()) / 2.0;
            if (v - *gain).abs() > f64::EPSILON {
                *gain = v;
                changed = true;
                resp.mark_changed();
            }
        }
    }
    if !ui.is_rect_visible(rect) {
        return changed;
    }
    let painter = ui.painter();
    let cx = rect.center().x;
    let rail = egui::Rect::from_center_size(rect.center(), egui::vec2(3.0, rect.height()));
    painter.rect_filled(rail, 1.5, egui::Color32::from_white_alpha(26));
    painter.rect_filled(
        egui::Rect::from_center_size(rect.center(), egui::vec2(8.0, 1.0)),
        0.0,
        egui::Color32::from_white_alpha(36),
    );
    let pct = ((*gain - min) / (max - min)).clamp(0.0, 1.0) as f32;
    let (fill_color, thumb_color) = if *gain > 0.0 {
        (
            egui::Color32::from_rgba_unmultiplied(52, 211, 153, 140),
            egui::Color32::from_rgb(52, 211, 153),
        )
    } else if *gain < 0.0 {
        (
            egui::Color32::from_rgba_unmultiplied(96, 165, 250, 140),
            egui::Color32::from_rgb(96, 165, 250),
        )
    } else {
        (
            egui::Color32::TRANSPARENT,
            egui::Color32::from_white_alpha(191),
        )
    };
    let thumb_y = rect.bottom() - pct * rect.height();
    if *gain > 0.0 {
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(cx - 1.5, thumb_y),
                egui::pos2(cx + 1.5, rect.center().y),
            ),
            1.5,
            fill_color,
        );
    } else if *gain < 0.0 {
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(cx - 1.5, rect.center().y),
                egui::pos2(cx + 1.5, thumb_y),
            ),
            1.5,
            fill_color,
        );
    }
    painter.circle_filled(egui::pos2(cx, thumb_y), 8.0, thumb_color);
    changed
}

// ─────────────────────── 右クリックメニュー ───────────────────────

/// 右クリックメニューの要求。ウィジェットが積み、shell が毎フレーム回収して
/// 開く (ビューの Action 列挙を増やさないための小さな受け渡し)。
pub enum MenuRequest {
    User {
        urn: String,
        permalink: Option<String>,
        pos: egui::Pos2,
    },
}

static MENU_REQUESTS: std::sync::Mutex<Vec<MenuRequest>> = std::sync::Mutex::new(Vec::new());

pub fn request_menu(req: MenuRequest) {
    if let Ok(mut q) = MENU_REQUESTS.lock() {
        q.push(req);
    }
}

pub fn take_menu_requests() -> Vec<MenuRequest> {
    MENU_REQUESTS
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

/// メニューのパネル (Tauri TrackContextMenu: 236px / 角丸 12 / #141417 /
/// 白 10% 枠 / 内側 6px)。
pub fn menu_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(egui::Color32::from_rgb(20, 20, 23))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(26)))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin {
            left: 6,
            right: 6,
            top: 6,
            bottom: 6,
        })
}

fn request_user_menu(ui: &egui::Ui, track: &Track, fallback: egui::Rect) {
    if let Some(user) = track.user.as_ref() {
        if !user.urn.is_empty() {
            request_menu(MenuRequest::User {
                urn: user.urn.clone(),
                permalink: user.permalink_url.clone(),
                pos: ui
                    .ctx()
                    .pointer_interact_pos()
                    .unwrap_or(fallback.center()),
            });
        }
    }
}

fn paint_menu_row(
    ui: &mut egui::Ui,
    label: &str,
    icon: Option<(UiIcon, egui::Color32)>,
    heart: Option<(bool, egui::Color32)>,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(224.0, 40.0), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let hovered = resp.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 8.0, egui::Color32::from_white_alpha(15));
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 12.0 + 7.5, rect.center().y),
        egui::Vec2::splat(15.0),
    );
    if let Some((icon, color)) = icon {
        paint_ui_icon(ui.painter(), icon_rect, icon, color);
    }
    if let Some((liked, color)) = heart {
        paint_heart(ui.painter(), icon_rect, color, liked);
    }
    let color = if hovered {
        egui::Color32::WHITE
    } else {
        egui::Color32::from_white_alpha(204)
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), crate::theme::medium(13.0), color);
    ui.painter().galley(
        egui::pos2(
            rect.left() + 12.0 + 15.0 + 12.0,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    resp
}

/// メニュー項目 (Tauri MenuItem: 高さ 40 / 角丸 8 / 13px medium /
/// アイコン 15px 白 50% / hover で白 6% 背景 + 白文字)。
pub fn menu_item(ui: &mut egui::Ui, icon: UiIcon, label: &str) -> egui::Response {
    paint_menu_row(
        ui,
        label,
        Some((icon, egui::Color32::from_white_alpha(128))),
        None,
    )
}

/// メニュー項目 (ハート。未いいねは線画 / いいね済みは塗り)。
pub fn menu_item_heart(ui: &mut egui::Ui, liked: bool, label: &str) -> egui::Response {
    paint_menu_row(
        ui,
        label,
        None,
        Some((liked, egui::Color32::from_white_alpha(128))),
    )
}

/// メニュー項目 (アイコンなし。Tauri に無い追加項目用)。
pub fn menu_item_plain(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(224.0, 40.0), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let hovered = resp.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 8.0, egui::Color32::from_white_alpha(15));
    }
    let color = if hovered {
        egui::Color32::WHITE
    } else {
        egui::Color32::from_white_alpha(204)
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), crate::theme::medium(13.0), color);
    ui.painter().galley(
        egui::pos2(
            rect.left() + 12.0,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    resp
}

/// メニューの区切り線 (Tauri: my-1 h-px 白 6%)。
pub fn menu_separator(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(224.0, 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, egui::Color32::from_white_alpha(15));
    ui.add_space(4.0);
}
