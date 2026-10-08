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

    fn key(self) -> &'static str {
        match self {
            StartupPage::Home => "home",
            StartupPage::Search => "search",
            StartupPage::Library => "library",
            StartupPage::Settings => "settings",
        }
    }
}

#[derive(Default)]
pub struct SettingsView {
    active: SettingsCategory,
    floating_comments: bool,
    bg_dim: f32,
    bg_opacity: f32,
    bg_blur: f32,
    save_status: Option<String>,
    /// 更新チェック結果 (tag, url)。
    update_check: crate::query::Query<(String, String)>,
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
        let _ = images;
        let _ = player;
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
            SettingsCategory::General => self.show_general(settings, rt, ui),
            SettingsCategory::Appearance => self.show_appearance(settings, ui),
            SettingsCategory::Audio => self.show_audio(settings, audio, ui),
            SettingsCategory::Storage => self.show_storage(cache, settings, ui),
            SettingsCategory::Account => Self::show_account(api, settings, ui),
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

    /// `StartupCard` 対応 (起動ページ。設定に永続化して実際に適用)。
    fn show_general(
        &mut self,
        settings: &mut SettingsState,
        rt: &tokio::runtime::Handle,
        ui: &mut egui::Ui,
    ) {
        ui.heading("Startup");
        ui.label("Choose which page opens when the app launches (signed-in only)");
        ui.horizontal_wrapped(|ui| {
            for page in StartupPage::ALL {
                if ui
                    .selectable_label(settings.startup_page == page.key(), page.title())
                    .clicked()
                {
                    settings.startup_page = page.key().to_string();
                    let _ = crate::backend::prefs::save(settings);
                }
            }
        });
        ui.separator();
        if ui
            .checkbox(&mut settings.close_to_tray, "Close to tray (keep running)")
            .changed()
        {
            let _ = crate::backend::prefs::save(settings);
        }
        ui.separator();
        ui.heading("Updates");
        if ui.button("Check for updates").clicked() {
            self.update_check = crate::query::Query::default();
            self.update_check.request(rt, async move {
                let client = wreq::Client::new();
                let resp = client
                    .get("https://api.github.com/repos/Jomvet-lovers/SCD-Direct/releases/latest")
                    .header("User-Agent", "scd-egui")
                    .send()
                    .await
                    .map_err(|e| e.to_string())?;
                let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
                let tag = v
                    .get("tag_name")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let url = v
                    .get("html_url")
                    .and_then(|t| t.as_str())
                    .unwrap_or("https://github.com/Jomvet-lovers/SCD-Direct/releases")
                    .to_string();
                Ok((tag, url))
            });
        }
        let _ = self.update_check.poll();
        if self.update_check.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking...");
            });
        } else if let Some((tag, url)) = self.update_check.data.clone() {
            ui.horizontal(|ui| {
                ui.label("Latest GitHub release:");
                ui.hyperlink_to(tag, url);
            });
        } else if let Some(err) = self.update_check.error.as_ref() {
            ui.colored_label(egui::Color32::from_rgb(255, 150, 150), err.as_str());
        }
        ui.label(format!("Current version: {}", env!("CARGO_PKG_VERSION")));
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

    /// `PlaybackCard` 対応 (EQ/速度/ピッチは NowPlaying バーの EQ / Tune 窓)。
    fn show_audio(
        &mut self,
        settings: &mut SettingsState,
        audio: Option<&Arc<AudioState>>,
        ui: &mut egui::Ui,
    ) {
        ui.heading("Playback");
        ui.checkbox(&mut self.floating_comments, "Floating comments");
        ui.label("Show comments as floating pills during playback");
        if ui
            .checkbox(&mut settings.normalize_volume, "Volume normalization")
            .changed()
        {
            if let Some(a) = audio {
                crate::backend::audio::engine::set_normalization(
                    settings.normalize_volume,
                    a,
                );
            }
            let _ = crate::backend::prefs::save(settings);
        }
        ui.label("Balances quiet and loud tracks to a more even level");
        ui.checkbox(&mut settings.hq_streaming, "High quality streaming");
        ui.label("Prefer the highest available quality during playback");
        if ui
            .checkbox(
                &mut settings.autopilot,
                "Autopilot (continue with related tracks)",
            )
            .changed()
        {
            let _ = crate::backend::prefs::save(settings);
        }
        ui.label("When the queue ends, keep playing related tracks");
        ui.separator();
        ui.heading("Output device");
        let follow = settings.follow_default_output;
        let mut follow_new = follow;
        ui.checkbox(&mut follow_new, "Follow system default");
        if follow_new != follow {
            settings.follow_default_output = follow_new;
            if let Some(a) = audio {
                crate::backend::audio::set_follow_default_output(a, follow_new);
                if follow_new {
                    let _ = crate::backend::audio::switch_device(a, None);
                } else if let Some(name) = settings.output_device.clone() {
                    let _ = crate::backend::audio::switch_device(a, Some(name));
                }
            }
            let _ = crate::backend::prefs::save(settings);
        }
        if !settings.follow_default_output {
            let devices = crate::backend::audio::list_devices();
            if devices.is_empty() {
                crate::widgets::empty_note(ui, "No output devices found");
            }
            egui::ScrollArea::vertical()
                .id_salt("audio-devices")
                .max_height(160.0)
                .show(ui, |ui| {
                    for d in devices {
                        let label = d
                            .interface
                            .clone()
                            .unwrap_or_else(|| d.description.clone());
                        let label = if d.is_default {
                            format!("{label} (default)")
                        } else {
                            label
                        };
                        let selected =
                            settings.output_device.as_deref() == Some(d.name.as_str());
                        if ui.selectable_label(selected, label).clicked() {
                            settings.output_device = Some(d.name.clone());
                            if let Some(a) = audio {
                                if let Err(e) =
                                    crate::backend::audio::switch_device(a, Some(d.name.clone()))
                                {
                                    eprintln!("[audio] device switch failed: {e}");
                                }
                            }
                            let _ = crate::backend::prefs::save(settings);
                        }
                    }
                });
        }
        ui.separator();
        ui.label("Equalizer and speed / pitch live in the now-playing bar (EQ / Tune).");
    }

    /// `CacheCard` 対応 (サイズ表示 + クリア + 上限制御)。
    fn show_storage(
        &mut self,
        cache: Option<&crate::backend::track_cache::TrackCacheState>,
        settings: &mut SettingsState,
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
        ui.add(egui::Slider::new(&mut settings.cache_limit_mb, 0..=8192).text("Limit MB"));
        if ui.button("Apply limit").clicked() {
            cache.enforce_limit(settings.cache_limit_mb);
            let _ = crate::backend::prefs::save(settings);
        }
        ui.label("Bulk-download your liked tracks from the Offline page.");
    }

    /// `AccountCard` 対応 (Sign Out 本体は Login ページ側。shell が処理する)。
    fn show_account(api: Option<&ApiClient>, settings: &mut SettingsState, ui: &mut egui::Ui) {
        ui.heading("Account");
        let signed_in = api.and_then(|a| a.session_token()).is_some();
        ui.label(if signed_in {
            "Signed in — use the Login page to sign out"
        } else {
            "Not signed in"
        });
        ui.separator();
        ui.heading("Discord");
        if ui
            .checkbox(&mut settings.discord_rpc, "Discord Rich Presence")
            .changed()
        {
            let _ = crate::backend::prefs::save(settings);
        }
        if settings.discord_rpc
            && ui
                .checkbox(&mut settings.discord_show_button, "Show GitHub button")
                .changed()
        {
            let _ = crate::backend::prefs::save(settings);
        }
        ui.label("Shows the track you are listening to on your Discord profile.");
    }
}
