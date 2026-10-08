//! Phase 3: Settings — 外観/再生/起動/ストレージ/アカウントの簡易再現。
//! 対応: `desktop/src/pages/Settings.tsx` (+ `desktop/src/components/settings/`)。
//! 永続化なし・in-memory。テーマ変更は `settings.theme_preset` / `settings.accent`
//! の保存のみ (egui の visuals はダーク固定のため、将来配線で shell 側の
//! `set_visuals` 反映が必要)。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::images::Images;
use crate::state::{PlayerState, SettingsState, ThemePreset};

pub enum SettingsAction {
    None,
}

/// `desktop/src/components/settings/registry.tsx` のカテゴリ対応
/// (integrations=Discord 連携は Phase 4 のため見送り)。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum SettingsCategory {
    #[default]
    General,
    Appearance,
    Audio,
    Storage,
    Account,
}

impl SettingsCategory {
    const ALL: &[SettingsCategory] = &[
        SettingsCategory::General,
        SettingsCategory::Appearance,
        SettingsCategory::Audio,
        SettingsCategory::Storage,
        SettingsCategory::Account,
    ];

    fn title(self) -> &'static str {
        match self {
            SettingsCategory::General => "General",
            SettingsCategory::Appearance => "Appearance",
            SettingsCategory::Audio => "Audio",
            SettingsCategory::Storage => "Storage",
            SettingsCategory::Account => "Account",
        }
    }
}

/// `StartupCard` の PAGES 対応 (in-memory)。
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum StartupPage {
    #[default]
    Home,
    Search,
    Library,
    Settings,
}

impl StartupPage {
    const ALL: &[StartupPage] = &[
        StartupPage::Home,
        StartupPage::Search,
        StartupPage::Library,
        StartupPage::Settings,
    ];

    fn title(self) -> &'static str {
        match self {
            StartupPage::Home => "Home",
            StartupPage::Search => "Search",
            StartupPage::Library => "Library",
            StartupPage::Settings => "Settings",
        }
    }
}

#[derive(Default)]
pub struct SettingsView {
    active: SettingsCategory,
    startup_page: StartupPage,
    floating_comments: bool,
    normalize_volume: bool,
    bg_dim: f32,
    bg_opacity: f32,
    bg_blur: f32,
    cache_limit_mb: f32,
    save_status: Option<String>,
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes / 1024)
    }
}

impl SettingsView {
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        api: Option<&ApiClient>,
        rt: &tokio::runtime::Handle,
        images: &mut Images,
        player: &mut PlayerState,
        audio: Option<&Arc<AudioState>>,
        param: Option<&str>,
        cache: Option<&crate::backend::track_cache::TrackCacheState>,
        settings: &mut SettingsState,
        ui: &mut egui::Ui,
    ) -> SettingsAction {
        let _ = rt;
        let _ = images;
        let _ = player;
        let _ = audio;
        let _ = param;

        ui.heading("Settings");
        ui.separator();
        ui.horizontal(|ui| {
            for cat in SettingsCategory::ALL {
                if ui.selectable_label(self.active == *cat, cat.title()).clicked() {
                    self.active = *cat;
                }
            }
        });
        ui.separator();

        match self.active {
            SettingsCategory::General => self.show_general(ui),
            SettingsCategory::Appearance => self.show_appearance(settings, ui),
            SettingsCategory::Audio => self.show_audio(settings, ui),
            SettingsCategory::Storage => self.show_storage(cache, ui),
            SettingsCategory::Account => Self::show_account(api, ui),
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                match crate::backend::prefs::save(settings) {
                    Ok(()) => self.save_status = Some("Saved".to_string()),
                    Err(e) => self.save_status = Some(format!("Save failed: {e}")),
                }
            }
            if let Some(status) = self.save_status.as_deref() {
                ui.label(status);
            }
        });

        SettingsAction::None
    }

    /// `StartupCard` 対応 (起動時ページ選択。in-memory)。
    fn show_general(&mut self, ui: &mut egui::Ui) {
        ui.heading("Startup");
        ui.label("Choose which page opens when the app launches (in-memory only)");
        ui.horizontal_wrapped(|ui| {
            for page in StartupPage::ALL {
                if ui
                    .selectable_label(self.startup_page == *page, page.title())
                    .clicked()
                {
                    self.startup_page = *page;
                }
            }
        });
    }

    /// `ThemeCard` + `WallpaperCard` 対応 (簡易再現。壁紙ファイル管理は Phase 4)。
    fn show_appearance(&mut self, settings: &mut SettingsState, ui: &mut egui::Ui) {
        ui.heading("Appearance");
        ui.label("Theme preset (saved only — visuals stay dark until wired)");
        ui.horizontal_wrapped(|ui| {
            for (preset, label) in [
                (ThemePreset::SoundCloud, "SoundCloud"),
                (ThemePreset::Dark, "Dark"),
                (ThemePreset::Neon, "Neon"),
                (ThemePreset::Forest, "Forest"),
                (ThemePreset::Crimson, "Crimson"),
                (ThemePreset::Custom, "Custom"),
            ] {
                if ui.selectable_label(settings.theme_preset == preset, label).clicked() {
                    settings.theme_preset = preset;
                    // 将来配線メモ: egui visuals は現行ダーク固定
                    // (`AppState::new` の `set_visuals`)。反映には shell 側で
                    // `settings.theme_preset` に応じた Visuals 構築が必要。
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Accent");
            ui.color_edit_button_srgb(&mut settings.accent);
        });
        if ui.button("Reset to defaults").clicked() {
            *settings = SettingsState::default();
        }
        ui.separator();
        ui.heading("Background image");
        ui.label("Wallpaper files are Phase 4 (in-memory tuning only)");
        ui.add(egui::Slider::new(&mut self.bg_dim, 0.0..=0.85).text("Darkening"));
        ui.add(egui::Slider::new(&mut self.bg_opacity, 0.0..=0.7).text("Edge darkening"));
        ui.add(egui::Slider::new(&mut self.bg_blur, 0.0..=40.0).text("Blur"));
    }

    /// `PlaybackCard` 対応 (`hq_streaming` は永続化、他は in-memory。
    /// `AudioDeviceCard` の出力先切替は Phase 4)。
    fn show_audio(&mut self, settings: &mut SettingsState, ui: &mut egui::Ui) {
        ui.heading("Playback");
        ui.checkbox(&mut self.floating_comments, "Floating comments");
        ui.label("Show comments as floating pills during playback");
        ui.checkbox(&mut self.normalize_volume, "Volume normalization");
        ui.label("Balances quiet and loud tracks to a more even level");
        ui.checkbox(&mut settings.hq_streaming, "High quality streaming");
        ui.label("Prefer the highest available quality during playback");
    }

    /// `CacheCard` 対応 (サイズ表示 + クリア + 上限制御)。
    fn show_storage(
        &mut self,
        cache: Option<&crate::backend::track_cache::TrackCacheState>,
        ui: &mut egui::Ui,
    ) {
        ui.heading("Cache");
        let Some(cache) = cache else {
            ui.label("cache unavailable");
            return;
        };
        let audio_bytes = cache.cache_size();
        let liked_bytes = cache.liked_cache_size();
        let status = cache.transcode_status();
        ui.label(format!(
            "Audio: {} · Likes (protected): {} · Total: {}",
            format_bytes(audio_bytes),
            format_bytes(liked_bytes),
            format_bytes(audio_bytes + liked_bytes),
        ));
        ui.label(format!(
            "Transcoder: {} · incoming: {} · clean: {}",
            status.ffmpeg, status.incoming, status.clean,
        ));
        ui.horizontal(|ui| {
            if ui.button("Clear audio cache").clicked() {
                cache.clear_cache();
            }
            if ui.button("Clear likes cache").clicked() {
                cache.clear_liked_cache();
            }
        });
        ui.separator();
        ui.label("Audio cache limit (0 = unlimited, enforced on apply)");
        ui.add(egui::Slider::new(&mut self.cache_limit_mb, 0.0..=8192.0).text("Limit MB"));
        if ui.button("Apply limit").clicked() {
            cache.enforce_limit(self.cache_limit_mb as u64);
        }
        ui.label("Likes bulk-download is Phase 4.");
    }

    /// `AccountCard` 対応 (Sign Out 本体は Login ページ側。shell が処理する)。
    fn show_account(api: Option<&ApiClient>, ui: &mut egui::Ui) {
        ui.heading("Account");
        let signed_in = api.and_then(|a| a.session_token()).is_some();
        ui.label(if signed_in {
            "Signed in — use the Login page to sign out"
        } else {
            "Not signed in"
        });
    }
}
