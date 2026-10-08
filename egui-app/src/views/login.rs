//! Phase 3: Login — セッション状態表示 + 手動トークン入力 + ログアウト。
//! 対応: `desktop/src/pages/Login.tsx` (+ `desktop/src/components/auth/`)。
//! wry ログインウィンドウは Phase 4 のため見送り。トークン適用/破棄は
//! shell が `SetToken` / `Logout` を処理する。

use std::sync::Arc;

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::models::ScUser;
use crate::images::Images;
use crate::query::Query;
use crate::state::PlayerState;

pub enum LoginAction {
    None,
    OpenLogin,
    SetToken(String),
    Logout,
}

#[derive(Default)]
pub struct LoginView {
    me: Query<ScUser>,
    token_input: String,
    show_token: bool,
    error: Option<String>,
}

impl LoginView {
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
        ui: &mut egui::Ui,
    ) -> LoginAction {
        let _ = images;
        let _ = player;
        let _ = audio;
        let _ = param;
        let _ = cache;

        let mut action = LoginAction::None;
        let has_session = api.and_then(|a| a.session_token()).is_some();

        if !has_session {
            self.me.data = None;
            self.me.error = None;
        } else if let Some(api) = api {
            if !self.me.requested() {
                let api_owned = api.clone();
                self.me.request(rt, async move {
                    api_owned
                        .get_json("/me/cold")
                        .await
                        .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
                });
            }
        }
        if self.me.poll() || self.me.loading {
            ui.ctx().request_repaint();
        }

        ui.heading("Sign in");
        ui.label("Your music, your way");
        ui.separator();

        if has_session {
            ui.label("Signed in");
            if let Some(me) = self.me.data.as_ref() {
                if me.username.is_empty() {
                    ui.label("Profile loaded");
                } else {
                    ui.label(format!("Hello, {}", me.username));
                }
            } else if self.me.loading {
                crate::widgets::loading_text(ui, "Loading profile...");
            } else if let Some(err) = self.me.error.as_ref() {
                let label = if err.contains("401") {
                    "Session expired — sign in again".to_string()
                } else {
                    format!("Profile unavailable: {err}")
                };
                ui.colored_label(egui::Color32::from_rgb(255, 150, 150), label);
            }
            if ui.button("Sign out").clicked() {
                action = LoginAction::Logout;
            }
        } else {
            ui.label("Not signed in");
            if ui.button("Sign in with SoundCloud").clicked() {
                action = LoginAction::OpenLogin;
            }
            ui.label(
                "A browser window opens soundcloud.com. Manual token paste also works.",
            );
            ui.horizontal(|ui| {
                ui.label("Session token");
                if self.show_token {
                    ui.text_edit_singleline(&mut self.token_input);
                } else {
                    ui.add(egui::TextEdit::singleline(&mut self.token_input).password(true));
                }
                if ui
                    .button(if self.show_token { "Hide" } else { "Show" })
                    .clicked()
                {
                    self.show_token = !self.show_token;
                }
            });
            if let Some(err) = self.error.as_ref() {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 150, 150),
                    err.clone(),
                );
            }
            if ui.button("Save token").clicked() {
                let token = self.token_input.trim().to_string();
                if token.is_empty() {
                    self.error = Some("Token is empty".to_string());
                } else {
                    self.error = None;
                    self.token_input.clear();
                    action = LoginAction::SetToken(token);
                }
            }
            ui.separator();
            ui.label("The offline library stays available without signing in.");
        }

        action
    }
}
