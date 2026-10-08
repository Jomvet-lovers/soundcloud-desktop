//! SCD visual language: dark-first theme + bundled fonts.
//! `settings.accent` / `theme_preset` が egui の Visuals を駆動する。
//! CSS 由来の装飾 (blur/glow/アニメ) は持たない。足りない部品は自作する。

use crate::state::{SettingsState, ThemePreset};

pub const FONT_SANS: &str = "Inter-Regular";
pub const FONT_SANS_MEDIUM: &str = "Inter-Medium";
pub const FONT_SANS_SEMIBOLD: &str = "Inter-SemiBold";
pub const FONT_SANS_BOLD: &str = "Inter-Bold";
pub const FONT_MONO: &str = "JetBrainsMono-Regular";
/// CJK (日本語等) フォールバック。Inter/JetBrains に無いグリフを補う。
pub const FONT_JP: &str = "NotoSansJP";
/// 韓国語 (Hangul) フォールバック。
pub const FONT_KR: &str = "NotoSansKR";
/// 数学用英数字 (U+1D400 帯)・記号のフォールバック。
pub const FONT_MATH: &str = "NotoSansMath";

/// 起動時に1回だけ呼ぶ。Inter (400/500/600/700) + JetBrains Mono (400)。
/// `include_bytes!` はリテラルパスのみ受けるため直書きする。
pub fn install_fonts(ctx: &egui::Context) {
    use std::sync::Arc;

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        FONT_SANS.to_string(),
        Arc::new(egui::FontData::from_owned(
            include_bytes!("../assets/fonts/Inter-Regular.otf").to_vec(),
        )),
    );
    fonts.font_data.insert(
        FONT_SANS_MEDIUM.to_string(),
        Arc::new(egui::FontData::from_owned(
            include_bytes!("../assets/fonts/Inter-Medium.otf").to_vec(),
        )),
    );
    fonts.font_data.insert(
        FONT_SANS_SEMIBOLD.to_string(),
        Arc::new(egui::FontData::from_owned(
            include_bytes!("../assets/fonts/Inter-SemiBold.otf").to_vec(),
        )),
    );
    fonts.font_data.insert(
        FONT_SANS_BOLD.to_string(),
        Arc::new(egui::FontData::from_owned(
            include_bytes!("../assets/fonts/Inter-Bold.otf").to_vec(),
        )),
    );
    fonts.font_data.insert(
        FONT_MONO.to_string(),
        Arc::new(egui::FontData::from_owned(
            include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf").to_vec(),
        )),
    );
    fonts.font_data.insert(
        FONT_JP.to_string(),
        Arc::new(egui::FontData::from_owned(
            include_bytes!("../assets/fonts/NotoSansJP-Regular.otf").to_vec(),
        )),
    );
    fonts.font_data.insert(
        FONT_KR.to_string(),
        Arc::new(egui::FontData::from_owned(
            include_bytes!("../assets/fonts/NotoSansKR-Regular.otf").to_vec(),
        )),
    );
    fonts.font_data.insert(
        FONT_MATH.to_string(),
        Arc::new(egui::FontData::from_owned(
            include_bytes!("../assets/fonts/NotoSansMath-Regular.ttf").to_vec(),
        )),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, FONT_SANS.to_string());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, FONT_MONO.to_string());
    // CJK/数学フォールバック (Proportional/Monospace の末尾に追加)。
    for extra in [FONT_JP, FONT_KR, FONT_MATH] {
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push(extra.to_string());
    }
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push(FONT_JP.to_string());
    // 太字系は名前付きファミリとして登録し、見出し側で明示指定する。
    for name in [FONT_SANS_MEDIUM, FONT_SANS_SEMIBOLD, FONT_SANS_BOLD] {
        fonts.families.insert(
            egui::FontFamily::Name(name.into()),
            vec![
                name.to_string(),
                FONT_JP.to_string(),
                FONT_KR.to_string(),
                FONT_MATH.to_string(),
            ],
        );
    }
    ctx.set_fonts(fonts);
}

fn shade(rgb: [u8; 3], alpha: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], alpha)
}

/// プリセット毎の (window_fill, panel_fill, faint_text_alpha)。
fn preset_colors(preset: ThemePreset) -> ([u8; 3], [u8; 3]) {
    match preset {
        ThemePreset::SoundCloud => ([17, 17, 20], [8, 8, 10]),
        ThemePreset::Dark => ([16, 16, 22], [10, 10, 14]),
        ThemePreset::Neon => ([10, 15, 20], [6, 10, 14]),
        ThemePreset::Forest => ([12, 18, 14], [8, 12, 10]),
        ThemePreset::Crimson => ([20, 12, 14], [12, 8, 10]),
        ThemePreset::Custom => ([17, 17, 20], [8, 8, 10]),
    }
}

pub fn visuals(settings: &SettingsState) -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    let accent = egui::Color32::from_rgb(
        settings.accent[0],
        settings.accent[1],
        settings.accent[2],
    );
    let (window, panel) = preset_colors(settings.theme_preset);
    v.window_fill = egui::Color32::from_rgb(window[0], window[1], window[2]);
    v.panel_fill = egui::Color32::from_rgb(panel[0], panel[1], panel[2]);
    v.faint_bg_color = shade([255, 255, 255], 8);
    v.extreme_bg_color = shade([0, 0, 0], 200);
    v.selection.bg_fill = shade(
        [settings.accent[0], settings.accent[1], settings.accent[2]],
        70,
    );
    v.selection.stroke = egui::Stroke::new(1.0, accent);
    v.widgets.noninteractive.weak_bg_fill = shade([255, 255, 255], 10);
    v.widgets.inactive.weak_bg_fill = shade([255, 255, 255], 16);
    v.widgets.hovered.weak_bg_fill = shade([255, 255, 255], 28);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    v.widgets.active.weak_bg_fill = shade(
        [settings.accent[0], settings.accent[1], settings.accent[2]],
        60,
    );
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, accent);
    v
}

/// 起動時に1回だけ呼ぶ。文字サイズは Style 側 (Visuals には無い)。
pub fn install_text_styles(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    for (text_style, size) in [
        (egui::TextStyle::Small, 11.0),
        (egui::TextStyle::Body, 13.0),
        (egui::TextStyle::Button, 13.0),
        (egui::TextStyle::Heading, 20.0),
    ] {
        style.text_styles.insert(
            text_style,
            egui::FontId::new(size, egui::FontFamily::Proportional),
        );
    }
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(12.0, egui::FontFamily::Monospace),
    );
    ctx.set_style_of(egui::Theme::Dark, style);
}

/// 設定変化時のみ Visuals を作り直す (毎フレーム再構築を避ける)。
pub fn ensure_applied(
    ctx: &egui::Context,
    settings: &SettingsState,
    applied: &mut Option<(ThemePreset, [u8; 3])>,
) {
    let key = (settings.theme_preset, settings.accent);
    if *applied != Some(key) {
        ctx.set_visuals(visuals(settings));
        *applied = Some(key);
    }
}

/// 見出し用セミボールド。
pub fn semibold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(FONT_SANS_SEMIBOLD.into()))
}
