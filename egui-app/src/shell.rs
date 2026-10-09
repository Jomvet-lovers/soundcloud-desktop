//! Phase 2 シェル: Sidebar + 中央プレースホルダ + NowPlayingBar。
//! 対応: `desktop/src/components/layout/{AppShell,Sidebar,NowPlayingBar}.tsx`。
//! トランスポート (再生/停止/シーク/音量) とファイル読込を backend に直結する。

use super::state::{AppState, LoadState, RepeatMode, Route};
use crate::backend::audio::engine;
use crate::backend::track_cache::ExportFormat;
use crate::views::{
    album::AlbumAction, artist::ArtistAction, collection::CollectionAction, home::HomeAction,
    library::LibraryAction, login::LoginAction, offline::OfflineAction, playlist::PlaylistAction,
    search::SearchAction, settings::SettingsAction, tag::TagAction, track::TrackAction,
    user::UserAction,
};

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// 端リサイズ (OS 装飾なしのため自前で。6px のヒットゾーン)。
fn resize_edges(ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let t = 6.0;
    egui::Area::new(egui::Id::new("scd_resize_edges"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let zones: [(egui::Rect, egui::ResizeDirection, egui::CursorIcon); 8] = [
                (
                    egui::Rect::from_min_max(
                        egui::pos2(screen.left(), screen.top()),
                        egui::pos2(screen.left() + t, screen.top() + t),
                    ),
                    egui::ResizeDirection::NorthWest,
                    egui::CursorIcon::ResizeNwSe,
                ),
                (
                    egui::Rect::from_min_max(
                        egui::pos2(screen.center().x - t, screen.top()),
                        egui::pos2(screen.center().x + t, screen.top() + t),
                    ),
                    egui::ResizeDirection::North,
                    egui::CursorIcon::ResizeVertical,
                ),
                (
                    egui::Rect::from_min_max(
                        egui::pos2(screen.right() - t, screen.top()),
                        egui::pos2(screen.right(), screen.top() + t),
                    ),
                    egui::ResizeDirection::NorthEast,
                    egui::CursorIcon::ResizeNeSw,
                ),
                (
                    egui::Rect::from_min_max(
                        egui::pos2(screen.left(), screen.center().y - t),
                        egui::pos2(screen.left() + t, screen.center().y + t),
                    ),
                    egui::ResizeDirection::West,
                    egui::CursorIcon::ResizeHorizontal,
                ),
                (
                    egui::Rect::from_min_max(
                        egui::pos2(screen.right() - t, screen.center().y - t),
                        egui::pos2(screen.right(), screen.center().y + t),
                    ),
                    egui::ResizeDirection::East,
                    egui::CursorIcon::ResizeHorizontal,
                ),
                (
                    egui::Rect::from_min_max(
                        egui::pos2(screen.left(), screen.bottom() - t),
                        egui::pos2(screen.left() + t, screen.bottom()),
                    ),
                    egui::ResizeDirection::SouthWest,
                    egui::CursorIcon::ResizeNeSw,
                ),
                (
                    egui::Rect::from_min_max(
                        egui::pos2(screen.center().x - t, screen.bottom() - t),
                        egui::pos2(screen.center().x + t, screen.bottom()),
                    ),
                    egui::ResizeDirection::South,
                    egui::CursorIcon::ResizeVertical,
                ),
                (
                    egui::Rect::from_min_max(
                        egui::pos2(screen.right() - t, screen.bottom() - t),
                        egui::pos2(screen.right(), screen.bottom()),
                    ),
                    egui::ResizeDirection::SouthEast,
                    egui::CursorIcon::ResizeNwSe,
                ),
            ];
            for (i, (rect, dir, cursor)) in zones.into_iter().enumerate() {
                let resp = ui.interact(
                    rect,
                    egui::Id::new(("scd_resize_zone", i)),
                    egui::Sense::drag(),
                );
                if resp.hovered() || resp.dragged() {
                    ui.ctx().set_cursor_icon(cursor);
                }
                if resp.drag_started() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
                }
            }
        });
}

/// トランスポートのアイコントグル (Shuffle/Repeat)。ON はアクセント色。
fn icon_toggle(ui: &mut egui::Ui, icon: crate::widgets::UiIcon, on: bool) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::Vec2::new(30.0, 28.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, egui::Color32::from_white_alpha(14));
    }
    let color = if on {
        ui.visuals().hyperlink_color
    } else {
        egui::Color32::from_white_alpha(160)
    };
    crate::widgets::paint_ui_icon(ui.painter(), rect, icon, color);
    resp
}

/// タイトルバーの戻る/進む/ホーム等の小さなアイコンボタン。
fn nav_icon_button(
    ui: &mut egui::Ui,
    icon: crate::widgets::UiIcon,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::new(30.0, 26.0), sense);
    if resp.hovered() && enabled {
        ui.painter()
            .rect_filled(rect, 5.0, egui::Color32::from_white_alpha(14));
    }
    let color = if enabled {
        egui::Color32::from_white_alpha(210)
    } else {
        egui::Color32::from_white_alpha(60)
    };
    crate::widgets::paint_ui_icon(ui.painter(), rect, icon, color);
    resp
}

/// タイトルバーのウィンドウ操作ボタン (最小化/最大化/閉じる)。
fn window_button(
    ui: &mut egui::Ui,
    icon: crate::widgets::UiIcon,
    active: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::new(44.0, 28.0), egui::Sense::click());
    let is_close = icon == crate::widgets::UiIcon::Close;
    if resp.hovered() {
        let bg = if is_close {
            egui::Color32::from_rgb(196, 43, 28)
        } else {
            egui::Color32::from_white_alpha(18)
        };
        ui.painter().rect_filled(rect, 4.0, bg);
    }
    let color = if active {
        ui.visuals().hyperlink_color
    } else if resp.hovered() && is_close {
        egui::Color32::WHITE
    } else {
        egui::Color32::from_white_alpha(190)
    };
    crate::widgets::paint_ui_icon(ui.painter(), rect, icon, color);
    resp
}

pub fn show_shell(state: &mut AppState, ui: &mut egui::Ui) {
    crate::theme::ensure_applied(ui.ctx(), &state.settings, &mut state.theme_applied);
    handle_shortcuts(state, ui.ctx());
    state.poll_tray(ui.ctx());
    state.drain_events();
    state.drain_backend();
    state.sync_discord();
    state.poll_continuation();
    state.poll_source_fetch();
    state.poll_likes_full_fetch();
    state.poll_discover_play();
    state.poll_load();
    // NowPlaying バーの like 状態を回収 (最新のトラックで上書き)。
    if state.now_like.poll() {
        if let Some(st) = state.now_like.data {
            let fav = state.now_liked.unwrap_or(false);
            state.now_liked = Some(st.liked || fav);
        }
    }

    let has_audio = state.audio().is_some();
    let playing = state.player.is_playing;
    let loading = matches!(state.load, LoadState::Loading { .. });
    if playing || loading {
        ui.ctx().request_repaint();
    }

    // サイドバー用の自分のプロフィール (署名済みのみ)。
    if state.api.as_ref().and_then(|a| a.session_token()).is_some()
        && !state.sidebar_me.requested()
    {
        if let Some(api) = state.api.clone() {
            let rt = state.runtime().handle().clone();
            state.sidebar_me.request(&rt, async move {
                api.get_json("/me/cold")
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }
    }
    state.sidebar_me.poll();

    // プレイリスト追加の結果 (失敗はサイドバーに表示、成功はキャッシュ破棄)。
    if let Some(rx) = state.playlist_add_rx.as_mut() {
        match rx.try_recv() {
            Ok(result) => {
                state.playlist_add_rx = None;
                match result {
                    Ok(()) => {
                        state.playlist_add_error = None;
                        state.playlist.invalidate_tracks();
                        state.library.invalidate_playlists();
                    }
                    Err(e) => state.playlist_add_error = Some(e),
                }
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            Err(_) => state.playlist_add_rx = None,
        }
    }

    // ── タイトルバー (Tauri: Titlebar.tsx 相当、OS 装飾なし) ──
    let accent = crate::widgets::accent_color(&state.settings);
    egui::Panel::top("titlebar")
        .exact_size(42.0)
        .frame(
            egui::Frame::NONE
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::symmetric(10, 6)),
        )
        .show(ui, |ui| {
            let bar_rect = ui.max_rect();
            let drag = ui.interact(
                bar_rect,
                egui::Id::new("titlebar_drag"),
                egui::Sense::click_and_drag(),
            );
            // 手動ドラッグ (StartDrag より滑らか): 移動量を OuterPosition に反映する。
            if drag.dragged() {
                let delta = drag.drag_delta();
                if delta != egui::Vec2::ZERO
                    && let Some(outer) = ui.ctx().input(|i| i.viewport().outer_rect)
                {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                            outer.min + delta,
                        ));
                }
            }
            if drag.double_clicked() {
                state.maximized = !state.maximized;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Maximized(state.maximized));
            }
            ui.horizontal_centered(|ui| {
                // サイドバーが閉じているときは開くボタンを出す。
                if !state.sidebar_open
                    && nav_icon_button(ui, crate::widgets::UiIcon::Collapse, true).clicked()
                {
                    state.sidebar_open = true;
                }
                let (logo_rect, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(24.0), egui::Sense::hover());
                ui.painter().rect_filled(logo_rect, 6.0, accent);
                crate::widgets::paint_ui_icon(
                    ui.painter(),
                    logo_rect,
                    crate::widgets::UiIcon::Cloud,
                    egui::Color32::WHITE,
                );
                ui.add_space(6.0);
                ui.label(egui::RichText::new("SoundCloud").font(crate::theme::semibold(15.0)));
                ui.add_space(12.0);
                if nav_icon_button(ui, crate::widgets::UiIcon::ChevronLeft, state.can_nav_back())
                    .clicked()
                {
                    state.nav_back();
                }
                if nav_icon_button(
                    ui,
                    crate::widgets::UiIcon::ChevronRight,
                    state.can_nav_forward(),
                )
                .clicked()
                {
                    state.nav_forward();
                }
                if nav_icon_button(ui, crate::widgets::UiIcon::Home, true).clicked() {
                    state.navigate(Route::Home, None);
                }
                // 中央: グローバル検索 (Enter で Search ページへ)。
                let avail = ui.available_width();
                let search_w = 420.0_f32.min((avail - 170.0).max(160.0));
                ui.add_space(((avail - search_w) / 2.0 - 60.0).max(0.0));
                let resp = ui.add_sized(
                    [search_w, 26.0],
                    egui::TextEdit::singleline(&mut state.global_search)
                        .hint_text("Search")
                        .desired_width(search_w),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let q = state.global_search.trim().to_string();
                    if !q.is_empty() {
                        state.navigate(Route::Search, Some(q));
                        state.global_search.clear();
                    }
                }
                // 右端: ウィンドウ操作。
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if window_button(ui, crate::widgets::UiIcon::Close, false).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if window_button(ui, crate::widgets::UiIcon::Maximize, state.maximized)
                        .clicked()
                    {
                        state.maximized = !state.maximized;
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Maximized(state.maximized));
                    }
                    if window_button(ui, crate::widgets::UiIcon::Minimize, false).clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                });
            });
        });

    let mut account_action: Option<LoginAction> = None;
    if state.sidebar_open {
        egui::Panel::left("sidebar")
            .resizable(false)
            .exact_size(200.0)
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(8, 8)),
            )
            .show(ui, |ui| {
                // ナビゲーション (Tauri: Sidebar.tsx 相当。アイコン付き)。
                if crate::widgets::nav_item(
                    ui,
                    crate::widgets::UiIcon::Home,
                    "Home",
                    state.route == Route::Home,
                )
                .clicked()
                {
                    state.navigate(Route::Home, None);
                }
                if crate::widgets::nav_item(
                    ui,
                    crate::widgets::UiIcon::Search,
                    "Search",
                    state.route == Route::Search,
                )
                .clicked()
                {
                    state.navigate(Route::Search, None);
                }
                if crate::widgets::nav_item(
                    ui,
                    crate::widgets::UiIcon::Library,
                    "Library",
                    state.route == Route::Library,
                )
                .clicked()
                {
                    state.navigate(Route::Library, None);
                }
                if crate::widgets::nav_item(
                    ui,
                    crate::widgets::UiIcon::History,
                    "History",
                    state.route == Route::LibraryCollection
                        && state.nav_param.as_deref() == Some("history"),
                )
                .clicked()
                {
                    state.navigate(Route::LibraryCollection, Some("history".to_string()));
                }
                if crate::widgets::nav_item(
                    ui,
                    crate::widgets::UiIcon::Offline,
                    "Offline",
                    state.route == Route::Offline,
                )
                .clicked()
                {
                    state.navigate(Route::Offline, None);
                }
                if !state.settings.pinned_playlists.is_empty() {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Quick access").size(10.0).weak(),
                    );
                    ui.add_space(2.0);
                    let pins: Vec<(String, String)> = state
                        .settings
                        .pinned_playlists
                        .iter()
                        .map(|p| (p.urn.clone(), p.title.clone()))
                        .collect();
                    for (urn, title) in pins {
                        let label = egui::RichText::new(title).size(12.5);
                        if ui
                            .add_sized(
                                [ui.available_width(), 24.0],
                                egui::Button::selectable(false, label),
                            )
                            .clicked()
                        {
                            state.navigate(Route::Playlist, Some(urn));
                        }
                    }
                }
                // 下部: Collapse / Settings / ユーザ / 診断。
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    let signed_in = state.api.as_ref().and_then(|a| a.session_token()).is_some();
                    if signed_in {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Sign out").size(11.5).weak(),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            account_action = Some(LoginAction::Logout);
                        }
                    } else if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Sign in").size(11.5).weak(),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        account_action = Some(LoginAction::OpenLogin);
                    }
                    if let Some(me) = state.sidebar_me.data.clone() {
                        ui.horizontal(|ui| {
                            let rt = state.runtime().handle().clone();
                            state
                                .images
                                .show(ui, &rt, me.avatar_url.as_deref(), 24.0);
                            if ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(&me.username).size(12.5),
                                    )
                                    .sense(egui::Sense::click()),
                                )
                                .clicked()
                            {
                                state.navigate(Route::User, Some(me.urn.clone()));
                            }
                        });
                    }
                    if crate::widgets::nav_item(
                        ui,
                        crate::widgets::UiIcon::Settings,
                        "Settings",
                        state.route == Route::Settings,
                    )
                    .clicked()
                    {
                        state.navigate(Route::Settings, None);
                    }
                    if crate::widgets::nav_item(
                        ui,
                        crate::widgets::UiIcon::Collapse,
                        "Collapse",
                        false,
                    )
                    .clicked()
                    {
                        state.sidebar_open = false;
                    }
                    if let Some(backend) = &state.backend {
                        let s = &backend.servers;
                        egui::CollapsingHeader::new(
                            egui::RichText::new("Diagnostics").size(10.0).weak(),
                        )
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "api :{}  static :{}  proxy :{}",
                                    s.api_port, s.static_port, s.proxy_port
                                ))
                                .small()
                                .weak(),
                            );
                            if backend.audio.is_none() {
                                ui.label(egui::RichText::new("audio: unavailable").small());
                            }
                        });
                    }
                });
                if let Some(e) = &state.boot_error {
                    ui.colored_label(egui::Color32::RED, format!("boot: {e}"));
                }
                if let Some(e) = &state.backend.as_ref().and_then(|b| b.audio_error.clone()) {
                    ui.colored_label(egui::Color32::RED, format!("audio: {e}"));
                }
                if let Some(e) = &state.last_sync_error {
                    ui.colored_label(egui::Color32::YELLOW, format!("sync: {e}"));
                }
                if let Some(e) = &state.playlist_add_error {
                    ui.colored_label(egui::Color32::YELLOW, format!("playlist: {e}"));
                }
            });
    }
    if let Some(action) = account_action {
        apply_login_action(state, action);
    }

    if state.queue_open {
        egui::Panel::right("queue").exact_size(320.0).show(ui, |ui| {
            ui.heading("Queue");
            let accent = crate::widgets::accent_color(&state.settings);
            let rt = state.runtime().handle().clone();
            let mut jump: Option<usize> = None;
            let mut remove: Option<usize> = None;
            let mut reorder: Option<(usize, usize)> = None;
            let current = state.player.queue_index;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, t) in state.player.queue.iter().enumerate() {
                    let row_id = egui::Id::new(("queue-row", i));
                    let (_inner, dropped) = ui.dnd_drop_zone::<usize, _>(egui::Frame::NONE, |ui| {
                        ui.horizontal(|ui| {
                            let src = ui.dnd_drag_source(row_id, i, |ui| {
                                ui.horizontal(|ui| {
                                    let art = t.artwork("t200x200");
                                    state.images.show(ui, &rt, art.as_deref(), 28.0);
                                    let label = egui::RichText::new(format!(
                                        "{} — {}",
                                        t.display_title(),
                                        t.artist_name()
                                    ));
                                    let label = if Some(i) == current {
                                        label.color(accent)
                                    } else {
                                        label
                                    };
                                    ui.add(
                                        egui::Label::new(label)
                                            .truncate()
                                            .wrap_mode(egui::TextWrapMode::Truncate),
                                    );
                                });
                            });
                            if src.response.clicked() {
                                jump = Some(i);
                            }
                            if ui.small_button("x").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                    if let Some(from) = dropped {
                        reorder = Some((*from, i));
                    }
                }
            });
            if let Some((from, to)) = reorder {
                state.player.move_queue_item(from, to);
            }
            if let Some(i) = remove {
                if i < state.player.queue.len() {
                    state.player.queue.remove(i);
                }
                match state.player.queue_index {
                    Some(_) if state.player.queue.is_empty() => {
                        state.player.queue_index = None;
                    }
                    Some(c) if c == i => {
                        state.player.queue_index =
                            Some(i.min(state.player.queue.len().saturating_sub(1)));
                    }
                    Some(c) if c > i => {
                        state.player.queue_index = Some(c - 1);
                    }
                    _ => {}
                }
            }
            if let Some(i) = jump {
                state.play_queue_index(i);
            }
            ui.horizontal(|ui| {
                if ui.button("Clear").clicked() {
                    state.player.clear_queue();
                }
                ui.label(format!("{} tracks", state.player.queue.len()));
            });
        });
    }

    egui::Panel::bottom("now_playing").show(ui, |ui| {
        let accent = crate::widgets::accent_color(&state.settings);
        // 再生中トラック (Tauri 版 NowPlayingBar: アートワーク + タイトル/アーティスト)。
        let current = state
            .player
            .queue_index
            .and_then(|i| state.player.queue.get(i))
            .cloned();
        // ── 3 分割バー (Tauri 版 NowPlayingBar: 左メタ / 中央 / 右操作) ──
        let audio = state.audio().cloned();
        let (pos, dur) = match &audio {
            Some(a) => (engine::get_position(a), state.player.duration_secs),
            None => (0.0, None),
        };
        let total_w = ui.available_width();
        let right_w = (total_w * 0.26).clamp(240.0, 340.0);
        let center_w = (total_w * 0.42).clamp(280.0, 640.0);
        let left_w = (total_w - center_w - right_w).max(220.0);
        ui.horizontal(|ui| {
            // 左: アートワーク + メタ + like。
            let _ = ui.allocate_ui_with_layout(
                egui::vec2(left_w, 56.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(left_w - 12.0);
                    match &current {
                    Some(track) => {
                        let is_sc = track.urn.starts_with("soundcloud:");
                        let rt = state.runtime().handle().clone();
                        let art = track.artwork("t200x200");
                        state.images.show(ui, &rt, art.as_deref(), 48.0);
                        ui.vertical(|ui| {
                            ui.set_max_width((left_w - 120.0).max(120.0));
                            let link_color = ui.visuals().hyperlink_color;
                            let title = egui::RichText::new(track.display_title());
                            let title = if is_sc { title.color(link_color) } else { title };
                            if ui
                                .add(
                                    egui::Label::new(title)
                                        .truncate()
                                        .wrap_mode(egui::TextWrapMode::Truncate)
                                        .sense(egui::Sense::click()),
                                )
                                .clicked()
                                && is_sc
                            {
                                state.navigate(Route::Track, Some(track.urn.clone()));
                            }
                            if let Some(user) = track.user.as_ref() {
                                let artist = egui::RichText::new(&user.username).small().weak();
                                let artist = if is_sc {
                                    artist.color(link_color)
                                } else {
                                    artist
                                };
                                if ui
                                    .add(
                                        egui::Label::new(artist)
                                            .truncate()
                                            .wrap_mode(egui::TextWrapMode::Truncate)
                                            .sense(egui::Sense::click()),
                                    )
                                    .clicked()
                                    && is_sc
                                {
                                    state.navigate(Route::User, Some(user.urn.clone()));
                                }
                            }
                        });
                        if is_sc {
                            let liked = state.now_liked.unwrap_or(false);
                            if crate::widgets::like_button(ui, liked, accent) {
                                let next = !liked;
                                state.now_liked = Some(next);
                                if let Some(api) = state.api.clone() {
                                    let rt = state.runtime().handle().clone();
                                    let path = format!(
                                        "/likes/tracks/{}",
                                        urlencoding::encode(&track.urn)
                                    );
                                    rt.spawn(async move {
                                        let method = if next { "POST" } else { "DELETE" };
                                        let _ = api.request_json(method, &path, None).await;
                                    });
                                }
                            }
                        }
                    }
                    None => {
                        ui.label(egui::RichText::new("Not playing").weak());
                    }
                    }
                },
            );
            // 中央: トランスポート + 進行。
            let _ = ui.allocate_ui_with_layout(
                egui::vec2(center_w, 56.0),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.set_min_width(center_w - 12.0);
                    ui.horizontal(|ui| {
                        let sh = icon_toggle(
                            ui,
                            crate::widgets::UiIcon::Shuffle,
                            state.player.shuffle,
                        );
                        if sh.on_hover_text("Shuffle").clicked() {
                            state.player.toggle_shuffle();
                        }
                        if crate::widgets::transport_button(
                            ui,
                            crate::widgets::TransportIcon::Prev,
                            has_audio,
                            28.0,
                        )
                        .clicked()
                        {
                            state.prev_track(pos);
                        }
                        let icon = if playing {
                            crate::widgets::TransportIcon::Pause
                        } else {
                            crate::widgets::TransportIcon::Play
                        };
                        if crate::widgets::transport_primary_button(ui, icon, has_audio, 36.0)
                            .clicked()
                        {
                            if let Some(audio) = &audio {
                                if playing {
                                    engine::pause(audio);
                                    state.player.is_playing = false;
                                } else {
                                    engine::play(audio);
                                    state.player.is_playing = true;
                                }
                            }
                        }
                        if crate::widgets::transport_button(
                            ui,
                            crate::widgets::TransportIcon::Next,
                            has_audio,
                            28.0,
                        )
                        .clicked()
                        {
                            state.next_track();
                        }
                        let repeat_on = state.player.repeat != RepeatMode::Off;
                        let rp = icon_toggle(
                            ui,
                            crate::widgets::UiIcon::Repeat,
                            repeat_on,
                        );
                        let tip = match state.player.repeat {
                            RepeatMode::Off => "Repeat",
                            RepeatMode::All => "Repeat: All",
                            RepeatMode::One => "Repeat: One",
                        };
                        if rp.on_hover_text(tip).clicked() {
                            state.player.cycle_repeat();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(fmt_time(pos)).monospace().small().weak());
                        ui.spacing_mut().slider_width = (center_w - 110.0).max(120.0);
                        match dur {
                            Some(d) if d > 0.0 => {
                                let mut p = pos.min(d);
                                if ui
                                    .add(
                                        egui::Slider::new(&mut p, 0.0..=d)
                                            .show_value(false)
                                            .trailing_fill(true),
                                    )
                                    .changed()
                                {
                                    if let Some(a) = &audio {
                                        let _ = engine::seek(p, a);
                                    }
                                }
                                ui.label(
                                    egui::RichText::new(fmt_time(d)).monospace().small().weak(),
                                );
                            }
                            _ => {
                                let mut p = 0.0_f64;
                                ui.add_enabled(
                                    false,
                                    egui::Slider::new(&mut p, 0.0..=1.0).show_value(false),
                                );
                            }
                        }
                    });
                },
            );
            // 右: 音量・ミュート・キュー・EQ・Tune・A-B・停止。
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut vol = state.player.volume;
                let _ = ui.allocate_ui_with_layout(
                    egui::vec2(120.0, 24.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().slider_width = 112.0;
                        let resp = ui.add(
                            egui::Slider::new(&mut vol, 0.0..=100.0)
                                .show_value(false)
                                .trailing_fill(true),
                        );
                        if resp.changed() {
                            if vol > 0.0 {
                                state.player.volume_before_mute = vol;
                            }
                            state.set_volume(vol);
                        }
                        if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                            if let Err(e) = crate::backend::prefs::save(&state.settings) {
                                eprintln!("[prefs] save failed: {e}");
                            }
                        }
                    },
                );
                let muted = state.player.volume <= 0.0;
                if ui
                    .small_button(if muted { "Unmute" } else { "Mute" })
                    .clicked()
                {
                    state.toggle_mute();
                }
                let queue_label = format!("Queue ({})", state.player.queue.len());
                if ui
                    .selectable_label(state.queue_open, queue_label)
                    .clicked()
                {
                    state.queue_open = !state.queue_open;
                }
                if ui.selectable_label(state.show_eq, "EQ").clicked() {
                    state.show_eq = !state.show_eq;
                }
                if ui.selectable_label(state.show_tuning, "Tune").clicked() {
                    state.show_tuning = !state.show_tuning;
                }
                let ab_label = match (state.ab_a, state.ab_b) {
                    (Some(_), Some(_)) => "A-B: on",
                    (Some(_), None) => "A-B: A",
                    _ => "A-B",
                };
                if ui
                    .selectable_label(state.ab_b.is_some(), ab_label)
                    .clicked()
                {
                    cycle_ab_loop(state);
                }
                if ui
                    .add_enabled(has_audio, egui::Button::new("Stop"))
                    .clicked()
                {
                    if let Some(audio) = &audio {
                        engine::stop(audio);
                        state.player.is_playing = false;
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("File:");
            ui.text_edit_singleline(&mut state.file_path_input);
            if ui
                .add_enabled(has_audio && !loading, egui::Button::new("Load"))
                .clicked()
            {
                state.start_file_load();
            }
            if loading {
                ui.spinner();
            }
            if let Some(e) = state.load_error.clone() {
                ui.colored_label(egui::Color32::RED, e);
            }
        });
    });

    egui::CentralPanel::default().show(ui, |ui| {
        // ページ遷移のソフトイン (Tauri: animate-soft-in)。
        let fade = ui.ctx().animate_bool_with_time(
            egui::Id::new((
                "page-fade",
                state.route as u8,
                state.nav_param.clone(),
                state.nav_index,
            )),
            true,
            0.25,
        );
        ui.multiply_opacity(fade);
        egui::ScrollArea::vertical().show(ui, |ui| {
            let rt = state.runtime().handle().clone();
            let audio = state.audio().cloned();
            let audio_ref = audio.as_ref();
            state.sync_api_session();
            let api = state.api.clone();
            let api_ref = api.as_ref();
            let param = state.nav_param.clone();
            let param_ref = param.as_deref();
            let accent = crate::widgets::accent_color(&state.settings);
            // `backend` と各ビューは disjoint field のため同時借用できる。
            let cache = state.backend.as_ref().map(|b| &b.track_cache);
            let images = &mut state.images;
            let player = &mut state.player;

            match state.route {
                Route::Home => match state.home.show(
                    api_ref,
                    &rt,
                    images,
                    player,
                    audio_ref,
                    &state.settings,
                    ui,
                ) {
                    HomeAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    HomeAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    HomeAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
                    }
                    HomeAction::StartDiscover(item) => {
                        state.start_discover(item);
                    }
                    HomeAction::Navigate(route, param) => {
                        state.navigate(route, param);
                    }
                    HomeAction::None => {}
                },
                Route::Search => match state.search.show(
                    api_ref,
                    &rt,
                    images,
                    player,
                    audio_ref,
                    param_ref,
                    cache,
                    accent,
                    &mut state.settings,
                    ui,
                ) {
                    SearchAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    SearchAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    SearchAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
                    }
                    SearchAction::Navigate(route, param) => {
                        state.navigate(route, param);
                    }
                    SearchAction::None => {}
                },
                Route::Tag => match state.tag.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    TagAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    TagAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    TagAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
                    }
                    TagAction::Navigate(route, param) => {
                        state.navigate(route, param);
                    }
                    TagAction::None => {}
                },
                Route::Library => match state.library.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    LibraryAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    LibraryAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    LibraryAction::PlayLikes(tracks, i) => {
                        state.play_list(tracks, i);
                        state.arm_likes_continuation();
                    }
                    LibraryAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
                    }
                    LibraryAction::ShuffleLikes(tracks) => {
                        state.shuffle_likes(tracks);
                    }
                    LibraryAction::Navigate(route, param) => {
                        state.navigate(route, param);
                    }
                    LibraryAction::None => {}
                },
                Route::LibraryCollection => match state.collection.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    CollectionAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    CollectionAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    CollectionAction::PlayLikes(tracks, i) => {
                        state.play_list(tracks, i);
                        state.arm_likes_continuation();
                    }
                    CollectionAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
                    }
                    CollectionAction::Navigate(route, param) => {
                        state.navigate(route, param);
                    }
                    CollectionAction::None => {}
                },
                Route::Track => match state.track.show(
                    api_ref,
                    &rt,
                    images,
                    player,
                    audio_ref,
                    param_ref,
                    cache,
                    accent,
                    state.settings.floating_comments,
                    ui,
                ) {
                    TrackAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    TrackAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    TrackAction::AddNextUp(track) => {
                        state.player.insert_next(vec![track]);
                    }
                    TrackAction::AddToPlaylist(track) => {
                        state.open_add_to_playlist(track);
                    }
                    TrackAction::OpenDownload(track) => {
                        state.open_download(track);
                    }
                    TrackAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
                    }
                    TrackAction::Seek(frac) => {
                        if let (Some(audio), Some(dur)) =
                            (audio.as_ref(), state.player.duration_secs)
                        {
                            if dur > 0.0 {
                                let _ =
                                    engine::seek((f64::from(frac) * dur).clamp(0.0, dur), audio);
                            }
                        }
                    }
                    TrackAction::Navigate(route, param) => {
                        state.navigate(route, param);
                    }
                    TrackAction::None => {}
                },
                Route::Playlist => match state.playlist.show(
                    api_ref,
                    &rt,
                    images,
                    player,
                    audio_ref,
                    param_ref,
                    cache,
                    accent,
                    &state.settings,
                    ui,
                ) {
                    PlaylistAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    PlaylistAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                        if let Some(urn) = state.playlist.urn().map(str::to_string) {
                            state.arm_playlist_continuation(urn);
                        }
                    }
                    PlaylistAction::ShufflePlay(tracks) => {
                        state.shuffle_play_playlist(tracks);
                    }
                    PlaylistAction::TogglePin(urn, title) => {
                        state.toggle_pin_playlist(urn, title);
                    }
                    PlaylistAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
                    }
                    PlaylistAction::Navigate(route, param) => {
                        state.navigate(route, param);
                    }
                    PlaylistAction::None => {}
                },
                Route::Album => match state.album.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    AlbumAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    AlbumAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    AlbumAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
                    }
                    AlbumAction::Navigate(route, param) => {
                        state.navigate(route, param);
                    }
                    AlbumAction::None => {}
                },
                Route::User => match state.user.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    UserAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    UserAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    UserAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
                    }
                    UserAction::Navigate(route, param) => {
                        state.navigate(route, param);
                    }
                    UserAction::None => {}
                },
                Route::Artist => match state.artist.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    ArtistAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    ArtistAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    ArtistAction::OpenMenu(track) => {
                        open_menu(state, ui, track);
                    }
                    ArtistAction::Navigate(route, param) => {
                        state.navigate(route, param);
                    }
                    ArtistAction::None => {}
                },
                Route::Settings => {
                    let settings = &mut state.settings;
                    let view = &mut state.settings_view;
                    match view.show(
                        api_ref, &rt, images, player, audio_ref, param_ref, cache, settings, ui,
                    ) {
                        SettingsAction::None => {}
                    }
                }
                Route::Offline => match state.offline.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    OfflineAction::PlayFile(path) => {
                        state.play_file(path);
                    }
                    OfflineAction::Navigate(route, param) => {
                        state.navigate(route, param);
                    }
                    OfflineAction::None => {}
                },
                Route::Login => {
                    let action = state.login.show(
                        api_ref, &rt, images, player, audio_ref, param_ref, cache, ui,
                    );
                    apply_login_action(state, action);
                }
            }
        });
    });

    if state.show_shortcuts {
        let mut open = true;
        egui::Window::new("Keyboard Shortcuts")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                egui::Grid::new("shortcuts")
                    .num_columns(2)
                    .spacing([28.0, 6.0])
                    .show(ui, |ui| {
                        for (key, label) in SHORTCUTS {
                            ui.strong(*key);
                            ui.label(*label);
                            ui.end_row();
                        }
                    });
            });
        if !open {
            state.show_shortcuts = false;
        }
    }
    if state.show_eq {
        show_eq_window(state, ui.ctx());
    }
    if state.show_tuning {
        show_tuning_window(state, ui.ctx());
    }
    if state.add_to_playlist.is_some() {
        show_add_to_playlist(state, ui.ctx());
    }
    if state.track_menu.is_some() {
        show_track_menu(state, ui.ctx());
    }
    if state.download_track.is_some() {
        show_download_window(state, ui.ctx());
    }
    // 端リサイズ (最大化中は無効)。
    if !state.maximized {
        resize_edges(ui.ctx());
    }
}

/// ログイン系アクションの共通処理 (Login 画面とサイドバーの両方から使う)。
fn apply_login_action(state: &mut AppState, action: LoginAction) {
    let rt = state.runtime().handle().clone();
    match action {
        LoginAction::OpenLogin => {
            if let Some(b) = &state.backend {
                crate::backend::weblogin::open_login_window(
                    &rt,
                    b.direct.clone(),
                    b.session.clone(),
                    b.bus.clone(),
                );
            }
        }
        LoginAction::SetToken(token) => {
            if let Some(b) = &state.backend {
                let session = b.session.clone();
                let bus = b.bus.clone();
                rt.spawn(async move {
                    let _ = session.set_token(&bus, token).await;
                });
            }
            state.reset_views();
        }
        LoginAction::Logout => {
            if let (Some(b), Some(api)) = (&state.backend, &state.api) {
                let session = b.session.clone();
                let bus = b.bus.clone();
                let base = api.base().to_string();
                rt.spawn(async move {
                    let _ = crate::backend::auth::auth_logout(base, &bus, &session).await;
                });
            }
            state.reset_views();
            // ログアウト後はログイン画面に戻す。
            state.navigate(Route::Login, None);
        }
        LoginAction::None => {}
    }
}

pub(crate) const FOCUS_SEARCH_ID: &str = "scd_focus_search";

const SHORTCUTS: &[(&str, &str)] = &[
    ("Space", "Play / Pause"),
    ("← / →", "Seek back / forward 5s"),
    ("N / P", "Next / Previous track"),
    ("S / R", "Toggle shuffle / repeat"),
    ("B", "Cycle A-B loop point"),
    ("↑ / ↓", "Volume up / down"),
    ("M", "Mute / Unmute"),
    ("/ or Ctrl+K", "Search"),
    ("Q", "Toggle queue"),
    ("[", "Toggle sidebar"),
    ("F11", "Toggle fullscreen"),
    ("Esc", "Close panel"),
    ("Ctrl+/", "Show shortcuts"),
];

#[derive(Default)]
struct ShortcutHits {
    space: bool,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    next: bool,
    prev: bool,
    shuffle: bool,
    repeat: bool,
    ab: bool,
    mute: bool,
    queue: bool,
    sidebar: bool,
    fullscreen: bool,
    escape: bool,
    search: bool,
    search_ctrl: bool,
    shorts: bool,
}

/// キーボードショートカット (Tauri 版 AppShell 相当)。
fn handle_shortcuts(state: &mut AppState, ctx: &egui::Context) {
    use egui::{Key, Modifiers};

    let typing = ctx.egui_wants_keyboard_input();
    let mut hits = ShortcutHits::default();
    ctx.input_mut(|i| {
        hits.shorts = i.consume_key(Modifiers::COMMAND, Key::Slash);
        hits.search_ctrl = i.consume_key(Modifiers::COMMAND, Key::K);
        if typing {
            return;
        }
        hits.space = i.consume_key(Modifiers::NONE, Key::Space);
        hits.left = i.consume_key(Modifiers::NONE, Key::ArrowLeft);
        hits.right = i.consume_key(Modifiers::NONE, Key::ArrowRight);
        hits.up = i.consume_key(Modifiers::NONE, Key::ArrowUp);
        hits.down = i.consume_key(Modifiers::NONE, Key::ArrowDown);
        hits.next = i.consume_key(Modifiers::NONE, Key::N);
        hits.prev = i.consume_key(Modifiers::NONE, Key::P);
        hits.shuffle = i.consume_key(Modifiers::NONE, Key::S);
        hits.repeat = i.consume_key(Modifiers::NONE, Key::R);
        hits.ab = i.consume_key(Modifiers::NONE, Key::B);
        hits.mute = i.consume_key(Modifiers::NONE, Key::M);
        hits.queue = i.consume_key(Modifiers::NONE, Key::Q);
        hits.sidebar = i.consume_key(Modifiers::NONE, Key::OpenBracket);
        hits.fullscreen = i.consume_key(Modifiers::NONE, Key::F11);
        hits.escape = i.consume_key(Modifiers::NONE, Key::Escape);
        hits.search = i.consume_key(Modifiers::NONE, Key::Slash);
    });

    if hits.shorts {
        state.show_shortcuts = !state.show_shortcuts;
    }
    if hits.search || hits.search_ctrl {
        state.navigate(Route::Search, None);
        ctx.memory_mut(|m| {
            m.data.insert_temp(egui::Id::new(FOCUS_SEARCH_ID), true);
        });
    }
    if hits.space {
        if let Some(audio) = state.audio().cloned() {
            if state.player.is_playing {
                engine::pause(&audio);
                state.player.is_playing = false;
            } else {
                engine::play(&audio);
                state.player.is_playing = true;
            }
        }
    }
    if hits.left || hits.right {
        if let (Some(audio), Some(dur)) = (state.audio().cloned(), state.player.duration_secs) {
            let pos = engine::get_position(&audio);
            let delta = if hits.right { 5.0 } else { -5.0 };
            let _ = engine::seek((pos + delta).clamp(0.0, dur.max(0.0)), &audio);
        }
    }
    if hits.next {
        state.next_track();
    }
    if hits.prev {
        if let Some(audio) = state.audio().cloned() {
            let pos = engine::get_position(&audio);
            state.prev_track(pos);
        }
    }
    if hits.shuffle {
        state.player.toggle_shuffle();
    }
    if hits.repeat {
        state.player.cycle_repeat();
    }
    if hits.ab {
        cycle_ab_loop(state);
    }
    if hits.up || hits.down {
        let delta = if hits.up { 5.0 } else { -5.0 };
        let vol = (state.player.volume + delta).clamp(0.0, 100.0);
        if vol > 0.0 {
            state.player.volume_before_mute = vol;
        }
        state.set_volume(vol);
        if let Err(e) = crate::backend::prefs::save(&state.settings) {
            eprintln!("[prefs] save failed: {e}");
        }
    }
    if hits.mute {
        state.toggle_mute();
    }
    if hits.queue {
        state.queue_open = !state.queue_open;
    }
    if hits.sidebar {
        state.sidebar_open = !state.sidebar_open;
    }
    if hits.fullscreen {
        state.fullscreen = !state.fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(state.fullscreen));
    }
    if hits.escape {
        if state.track_menu.is_some() {
            state.track_menu = None;
        } else if state.queue_open {
            state.queue_open = false;
        } else if state.show_shortcuts {
            state.show_shortcuts = false;
        }
    }
}

/// A-B ループの循環操作: A 設定 → B 設定 → 解除 (Tauri 版 B キー相当)。
fn cycle_ab_loop(state: &mut AppState) {
    let Some(audio) = state.audio().cloned() else {
        return;
    };
    let pos = engine::get_position(&audio);
    if state.ab_a.is_none() {
        state.ab_a = Some(pos);
    } else if state.ab_b.is_none() {
        let a = state.ab_a.unwrap_or(0.0);
        if pos > a + 0.5 {
            state.ab_b = Some(pos);
        } else {
            // B は A より後ろのみ有効。短すぎる場合はやり直し。
            state.ab_a = None;
            state.ab_b = None;
        }
    } else {
        state.ab_a = None;
        state.ab_b = None;
    }
    crate::backend::audio::engine::set_ab_loop(state.ab_a, state.ab_b, &audio);
}

const EQ_LABELS: [&str; 10] = [
    "32", "64", "125", "250", "500", "1K", "2K", "4K", "8K", "16K",
];

/// Tauri 版 `equalizer.ts` のプリセット。
const EQ_PRESETS: &[(&str, [f64; 10])] = &[
    ("Flat", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    (
        "Bass Boost",
        [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    ),
    (
        "Bass Destroyer",
        [12.0, 12.0, 10.0, 7.0, 3.0, 0.0, -2.0, -4.0, -4.0, -5.0],
    ),
    (
        "Treble Boost",
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 5.0, 6.0],
    ),
    (
        "Vocal",
        [-2.0, -1.0, 0.0, 2.0, 4.0, 4.0, 3.0, 1.0, 0.0, -1.0],
    ),
    ("Rock", [4.0, 3.0, 1.0, 0.0, -1.0, 0.0, 2.0, 3.0, 4.0, 4.0]),
    (
        "Electronic",
        [5.0, 4.0, 2.0, 0.0, -1.0, 0.0, 1.0, 3.0, 4.0, 5.0],
    ),
    (
        "Classical",
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -2.0, -3.0, -3.0, -4.0],
    ),
    (
        "Loudness",
        [5.0, 4.0, 1.0, 0.0, -1.0, 0.0, -1.0, 0.0, 3.0, 4.0],
    ),
    (
        "V-Shape",
        [5.0, 3.0, 1.0, -1.0, -3.0, -3.0, -1.0, 1.0, 3.0, 5.0],
    ),
    (
        "Night",
        [-3.0, -2.0, 0.0, 2.0, 3.0, 3.0, 2.0, 0.0, -2.0, -4.0],
    ),
];

/// イコライザー窓 (Tauri 版 `EqualizerPanel` 相当)。
fn show_eq_window(state: &mut AppState, ctx: &egui::Context) {
    let mut open = true;
    let mut changed = false;
    egui::Window::new("Equalizer")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            changed |= ui
                .checkbox(&mut state.settings.eq_enabled, "Enable")
                .changed();
            ui.horizontal(|ui| {
                egui::ComboBox::from_label("Preset")
                    .selected_text("Custom")
                    .show_ui(ui, |ui| {
                        for (name, gains) in EQ_PRESETS {
                            if ui.selectable_label(false, *name).clicked() {
                                state.settings.eq_gains = gains.to_vec();
                                changed = true;
                            }
                        }
                    });
                if ui.button("Flat").clicked() {
                    state.settings.eq_gains = vec![0.0; 10];
                    changed = true;
                }
            });
            let mut gains = state.settings.eq_gains_or_default();
            let mut sliders_changed = false;
            ui.horizontal(|ui| {
                for (i, g) in gains.iter_mut().enumerate() {
                    ui.vertical(|ui| {
                        sliders_changed |= ui
                            .add(egui::Slider::new(g, -12.0..=12.0).vertical())
                            .changed();
                        ui.small(EQ_LABELS[i]);
                    });
                }
            });
            if sliders_changed {
                state.settings.eq_gains = gains;
                changed = true;
            }
        });
    state.show_eq = open;
    if changed {
        state.apply_eq();
    }
}

/// サウンドチューニング窓 (Tauri 版 `SoundTuningPopover` 相当)。
fn show_tuning_window(state: &mut AppState, ctx: &egui::Context) {
    let mut open = true;
    let mut changed = false;
    egui::Window::new("Sound tuning")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Speed");
                changed |= ui
                    .add(
                        egui::Slider::new(&mut state.settings.playback_rate, 0.5..=2.0)
                            .fixed_decimals(2)
                            .suffix("x"),
                    )
                    .changed();
            });
            let mut manual = !state.settings.pitch_auto;
            if ui.checkbox(&mut manual, "Manual pitch control").changed() {
                state.settings.pitch_auto = !manual;
                changed = true;
            }
            ui.add_enabled_ui(manual, |ui| {
                changed |= ui
                    .add(
                        egui::Slider::new(&mut state.settings.pitch_semitones, -12.0..=12.0)
                            .step_by(0.5)
                            .suffix(" st"),
                    )
                    .changed();
            });
            if ui.button("Reset").clicked() {
                state.settings.playback_rate = 1.0;
                state.settings.pitch_semitones = 0.0;
                state.settings.pitch_auto = true;
                changed = true;
            }
            ui.label(format!(
                "Effective rate: {:.3}x",
                state.settings.effective_rate()
            ));
        });
    state.show_tuning = open;
    if changed {
        state.apply_rate();
    }
}

/// 「プレイリストに追加」ダイアログ (Tauri 版 `AddToPlaylistDialog` 相当)。
fn show_add_to_playlist(state: &mut AppState, ctx: &egui::Context) {
    let Some(track) = state.add_to_playlist.clone() else {
        return;
    };
    let api = state.api.clone();
    if !state.dialog_playlists.requested() {
        if let Some(api) = api.clone() {
            let rt = state.runtime().handle().clone();
            state.dialog_playlists.request(&rt, async move {
                let v = api.get_json("/me/playlists?limit=50&page=0").await?;
                serde_json::from_value(v).map_err(|e| e.to_string())
            });
        }
    }
    state.dialog_playlists.poll();

    let mut open = true;
    let mut add_to: Option<String> = None;
    let mut create = false;
    egui::Window::new("Add to playlist")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(format!("Track: {}", track.display_title()));
            ui.separator();
            if state.dialog_playlists.loading && state.dialog_playlists.data.is_none() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading playlists...");
                });
            }
            if let Some(err) = state.dialog_playlists.error.clone() {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 150, 150),
                    format!("Playlists unavailable: {err}"),
                );
            }
            if let Some(page) = state.dialog_playlists.data.as_ref() {
                if page.collection.is_empty() {
                    ui.label("No playlists yet");
                }
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for p in &page.collection {
                            ui.horizontal(|ui| {
                                ui.label(&p.title);
                                if ui.small_button("Add").clicked() {
                                    add_to = Some(p.urn.clone());
                                }
                            });
                        }
                    });
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("New playlist:");
                ui.text_edit_singleline(&mut state.new_playlist_title);
            });
            let can_create = !state.new_playlist_title.trim().is_empty();
            if ui
                .add_enabled(can_create, egui::Button::new("Create & add"))
                .clicked()
            {
                create = true;
            }
        });

    if let Some(urn) = add_to {
        if let Some(api) = api {
            let rt = state.runtime().handle().clone();
            let path = format!("/playlists/{}/tracks", urlencoding::encode(&urn));
            let body = serde_json::json!({ "add": track.urn });
            let (tx, rx) = tokio::sync::oneshot::channel();
            rt.spawn(async move {
                let result = api
                    .request_json("POST", &path, Some(&body))
                    .await
                    .map(|_| ());
                let _ = tx.send(result);
            });
            state.playlist_add_rx = Some(rx);
        }
        state.add_to_playlist = None;
        return;
    }
    if create {
        let title = state.new_playlist_title.trim().to_string();
        if let Some(api) = api {
            let rt = state.runtime().handle().clone();
            let body = serde_json::json!({
                "playlist": {
                    "title": title,
                    "sharing": "private",
                    "tracks": [ { "urn": track.urn } ],
                }
            });
            let (tx, rx) = tokio::sync::oneshot::channel();
            rt.spawn(async move {
                let result = api
                    .request_json("POST", "/playlists", Some(&body))
                    .await
                    .map(|_| ());
                let _ = tx.send(result);
            });
            state.playlist_add_rx = Some(rx);
        }
        state.add_to_playlist = None;
        return;
    }
    if !open {
        state.add_to_playlist = None;
    }
}

/// 右クリック位置にトラックメニューを開く。
fn open_menu(state: &mut AppState, ui: &egui::Ui, track: crate::backend::models::Track) {
    let pos = ui
        .ctx()
        .pointer_interact_pos()
        .unwrap_or_else(|| egui::pos2(240.0, 240.0));
    state.open_track_menu(track, pos);
}

/// トラックの右クリックメニュー (Tauri 版 `TrackContextMenu` 相当)。
fn show_track_menu(state: &mut AppState, ctx: &egui::Context) {
    let Some(menu) = state.track_menu.clone() else {
        return;
    };
    state.menu_like.poll();
    state.menu_dislike.poll();

    let track = menu.track.clone();
    let liked = state.menu_like.data.map(|f| f.liked).unwrap_or(false);
    let disliked = state.menu_dislike.data.map(|f| f.disliked).unwrap_or(false);

    enum Act {
        SetLike(bool),
        SetDislike(bool),
        PlayNext,
        AddToPlaylist,
        Download,
        CopyLink,
        GoTrack,
        GoArtist,
    }
    let mut act: Option<Act> = None;

    let area = egui::Area::new(egui::Id::new("scd-track-menu"))
        .fixed_pos(menu.pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(210.0);
                let like_label = if liked {
                    "Remove from library"
                } else {
                    "Add to library"
                };
                if ui.button(like_label).clicked() {
                    act = Some(Act::SetLike(!liked));
                }
                let dis_label = if disliked {
                    "Remove dislike"
                } else {
                    "Not interested"
                };
                if ui.button(dis_label).clicked() {
                    act = Some(Act::SetDislike(!disliked));
                }
                if ui.button("Play next").clicked() {
                    act = Some(Act::PlayNext);
                }
                if ui.button("Add to playlist").clicked() {
                    act = Some(Act::AddToPlaylist);
                }
                if ui.button("Download...").clicked() {
                    act = Some(Act::Download);
                }
                ui.separator();
                if ui.button("Copy link").clicked() {
                    act = Some(Act::CopyLink);
                }
                if ui.button("Go to track").clicked() {
                    act = Some(Act::GoTrack);
                }
                if track.user.is_some() && ui.button("Go to artist").clicked() {
                    act = Some(Act::GoArtist);
                }
            });
        })
        .response;

    // メニュー外クリックで閉じる (Esc は handle_shortcuts 側)。
    if ctx.input(|i| i.pointer.any_click()) {
        let inside = ctx
            .input(|i| i.pointer.interact_pos())
            .map(|p| area.rect.contains(p))
            .unwrap_or(false);
        if !inside {
            state.track_menu = None;
            return;
        }
    }

    let Some(act) = act else {
        return;
    };
    match act {
        Act::SetLike(next) => {
            state.toggle_track_like(&track.urn, next);
            if state.player.current_queued().map(|t| t.urn.as_str()) == Some(track.urn.as_str()) {
                state.now_liked = Some(next);
            }
        }
        Act::SetDislike(next) => {
            state.toggle_track_dislike(&track.urn, next);
        }
        Act::PlayNext => {
            state.player.insert_next(vec![track.clone()]);
        }
        Act::AddToPlaylist => {
            state.open_add_to_playlist(track.clone());
        }
        Act::Download => {
            state.open_download(track.clone());
        }
        Act::CopyLink => {
            if let Some(url) = track.permalink_url.clone() {
                ctx.copy_text(url);
            } else if let Some(api) = state.api.clone() {
                let rt = state.runtime().handle().clone();
                let ctx2 = ctx.clone();
                let path = format!("/tracks/{}", urlencoding::encode(&track.urn));
                rt.spawn(async move {
                    if let Ok(v) = api.get_json(&path).await {
                        if let Some(u) = v.get("permalink_url").and_then(|x| x.as_str()) {
                            ctx2.copy_text(u.to_string());
                        }
                    }
                });
            }
        }
        Act::GoTrack => {
            state.navigate(Route::Track, Some(track.urn.clone()));
        }
        Act::GoArtist => {
            if let Some(user) = track.user.as_ref() {
                state.navigate(Route::User, Some(user.urn.clone()));
            }
        }
    }
    state.track_menu = None;
}

/// 単曲ダウンロード (保存先はネイティブダイアログ、形式は 4 種)。
fn show_download_window(state: &mut AppState, ctx: &egui::Context) {
    let Some(track) = state.download_track.clone() else {
        return;
    };
    state.download_status.poll();

    let mut open = true;
    let mut choose = false;
    egui::Window::new("Download")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(format!("Track: {}", track.display_title()));
            ui.horizontal(|ui| {
                ui.label("Format:");
                for (fmt, label) in [
                    (ExportFormat::M4a, "M4A (AAC)"),
                    (ExportFormat::Mp3, "MP3 320"),
                    (ExportFormat::Flac, "FLAC"),
                    (ExportFormat::Wav, "WAV"),
                ] {
                    if ui
                        .selectable_label(state.download_format == fmt, label)
                        .clicked()
                    {
                        state.download_format = fmt;
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Save as...").clicked() {
                    choose = true;
                }
                if state.download_status.loading {
                    ui.spinner();
                    ui.label("Exporting...");
                } else if let Some(status) = state.download_status.data.as_ref() {
                    ui.label(status.as_str());
                } else if let Some(err) = state.download_status.error.as_ref() {
                    ui.colored_label(egui::Color32::from_rgb(255, 150, 150), err.as_str());
                }
            });
        });

    if choose {
        let fmt = state.download_format;
        let ext = match fmt {
            ExportFormat::M4a => "m4a",
            ExportFormat::Mp3 => "mp3",
            ExportFormat::Flac => "flac",
            ExportFormat::Wav => "wav",
        };
        let safe = |s: &str| {
            s.chars()
                .map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c })
                .collect::<String>()
        };
        let default_name = format!(
            "{} - {}.{ext}",
            safe(track.artist_name()),
            safe(track.display_title())
        );
        let picked = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter(ext.to_uppercase(), &[ext])
            .save_file();
        if let Some(path) = picked {
            let dest = path.to_string_lossy().into_owned();
            if let Some(cache) = state.backend.as_ref().map(|b| b.track_cache.clone()) {
                let session = state
                    .api
                    .as_ref()
                    .and_then(|a| a.session_token().map(str::to_string));
                let hq = state.settings.hq_streaming;
                let cover = track.artwork("t500x500");
                let urn = track.urn.clone();
                let expected = (track.duration > 0).then_some(track.duration as u64);
                let rt = state.runtime().handle().clone();
                state.download_status = crate::query::Query::default();
                state.download_status.request(&rt, async move {
                    let req = crate::backend::track_cache::CacheRequest {
                        urn: &urn,
                        urls: &[],
                        download_urls: &[],
                        storage_urls: &[],
                        session_id: session.as_deref(),
                        hq,
                        liked: false,
                        expected_duration_ms: expected,
                    };
                    cache
                        .export_track(req, dest, cover, fmt)
                        .await
                        .map(|p| format!("Saved: {p}"))
                });
            }
        }
    }
    if !open {
        state.download_track = None;
    }
}
