//! Phase 2 シェル: Sidebar + 中央プレースホルダ + NowPlayingBar。
//! 対応: `desktop/src/components/layout/{AppShell,Sidebar,NowPlayingBar}.tsx`。
//! トランスポート (再生/停止/シーク/音量) とファイル読込を backend に直結する。

use super::state::{AppState, LoadState, RepeatMode, Route};
use crate::backend::audio::engine;
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

pub fn show_shell(state: &mut AppState, ui: &mut egui::Ui) {
    crate::theme::ensure_applied(ui.ctx(), &state.settings, &mut state.theme_applied);
    handle_shortcuts(state, ui.ctx());
    state.drain_events();
    state.drain_backend();
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

    let mut account_action: Option<LoginAction> = None;
    if state.sidebar_open {
        egui::Panel::left("sidebar").resizable(false).show(ui, |ui| {
        ui.heading("SCD-Direct");
        let signed_in = state.api.as_ref().and_then(|a| a.session_token()).is_some();
        if signed_in {
            if ui.button("Sign out").clicked() {
                account_action = Some(LoginAction::Logout);
            }
        } else if ui.button("Sign in").clicked() {
            account_action = Some(LoginAction::OpenLogin);
        }
        ui.separator();
        for route in Route::ALL {
            ui.selectable_value(&mut state.route, *route, route.title());
        }
        ui.separator();
        if let Some(backend) = &state.backend {
            let s = &backend.servers;
            ui.label(format!(
                "api :{}  static :{}  proxy :{}",
                s.api_port, s.static_port, s.proxy_port
            ));
            if backend.audio.is_none() {
                ui.label("audio: unavailable");
            }
        }
        if let Some(e) = &state.boot_error {
            ui.colored_label(egui::Color32::RED, format!("boot: {e}"));
        }
        if let Some(e) = &state.backend.as_ref().and_then(|b| b.audio_error.clone()) {
            ui.colored_label(egui::Color32::RED, format!("audio: {e}"));
        }
        if let Some(e) = &state.last_sync_error {
            ui.colored_label(egui::Color32::YELLOW, format!("sync: {e}"));
        }
        });
    }
    if let Some(action) = account_action {
        apply_login_action(state, action);
    }

    if state.queue_open {
        egui::Panel::right("queue").show(ui, |ui| {
            ui.heading("Queue");
            let mut jump: Option<usize> = None;
            let mut remove: Option<usize> = None;
            let mut reorder: Option<(usize, usize)> = None;
            let current = state.player.queue_index;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, t) in state.player.queue.iter().enumerate() {
                    let label = format!("{} — {}", t.display_title(), t.artist_name());
                    let row_id = egui::Id::new(("queue-row", i));
                    let (_inner, dropped) =
                        ui.dnd_drop_zone::<usize, _>(egui::Frame::NONE, |ui| {
                            ui.horizontal(|ui| {
                                let src = ui.dnd_drag_source(row_id, i, |ui| {
                                    ui.selectable_label(Some(i) == current, label);
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
        // 再生中トラック (Tauri 版 NowPlayingBar: タイトル/アーティストは各ページへのリンク)。
        ui.horizontal(|ui| {
            let current = state
                .player
                .queue_index
                .and_then(|i| state.player.queue.get(i))
                .cloned();
            match current {
                Some(track) => {
                    let is_sc = track.urn.starts_with("soundcloud:");
                    if is_sc {
                        if ui.link(track.display_title()).clicked() {
                            state.route = Route::Track;
                            state.nav_param = Some(track.urn.clone());
                        }
                    } else {
                        ui.label(track.display_title());
                    }
                    if let Some(user) = track.user.as_ref() {
                        ui.label("—");
                        if is_sc {
                            if ui.link(&user.username).clicked() {
                                state.route = Route::User;
                                state.nav_param = Some(user.urn.clone());
                            }
                        } else {
                            ui.label(&user.username);
                        }
                    }
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
                    ui.label("Nothing playing");
                }
            }
        });
        ui.horizontal(|ui| {
            let audio = state.audio().cloned();
            let label = if playing { "Pause" } else { "Play" };
            if ui
                .add_enabled(has_audio, egui::Button::new(label))
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
            if ui
                .add_enabled(has_audio, egui::Button::new("Stop"))
                .clicked()
            {
                if let Some(audio) = &audio {
                    engine::stop(audio);
                    state.player.is_playing = false;
                }
            }
            let pos0 = audio.as_ref().map(|a| engine::get_position(a)).unwrap_or(0.0);
            if ui
                .add_enabled(has_audio, egui::Button::new("Prev"))
                .clicked()
            {
                state.prev_track(pos0);
            }
            if ui
                .add_enabled(has_audio, egui::Button::new("Next"))
                .clicked()
            {
                state.next_track();
            }
            if ui
                .selectable_label(state.player.shuffle, "Shuffle")
                .clicked()
            {
                state.player.toggle_shuffle();
            }
            let repeat_label = match state.player.repeat {
                RepeatMode::Off => "Repeat: Off",
                RepeatMode::All => "Repeat: All",
                RepeatMode::One => "Repeat: One",
            };
            if ui
                .selectable_label(state.player.repeat != RepeatMode::Off, repeat_label)
                .clicked()
            {
                state.player.cycle_repeat();
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
            match (state.ab_a, state.ab_b) {
                (Some(a), Some(b)) => {
                    ui.label(format!("{}–{}", fmt_time(a), fmt_time(b)));
                }
                (Some(a), None) => {
                    ui.label(format!("A {}", fmt_time(a)));
                }
                _ => {}
            }
            let queue_label = format!("Queue ({})", state.player.queue.len());
            if ui.button(queue_label).clicked() {
                state.queue_open = !state.queue_open;
            }

            let (pos, dur) = match &audio {
                Some(a) => (engine::get_position(a), state.player.duration_secs),
                None => (0.0, None),
            };
            match dur {
                Some(d) if d > 0.0 => {
                    let mut p = pos.min(d);
                    if ui
                        .add(egui::Slider::new(&mut p, 0.0..=d).text("Position"))
                        .changed()
                    {
                        if let Some(a) = &audio {
                            let _ = engine::seek(p, a);
                        }
                    }
                    ui.label(format!("{} / {}", fmt_time(p), fmt_time(d)));
                }
                _ => {
                    ui.label(fmt_time(pos));
                }
            }

            let mut vol = state.player.volume;
            let resp = ui.add(egui::Slider::new(&mut vol, 0.0..=100.0).text("Volume"));
            if resp.changed() {
                if vol > 0.0 {
                    state.player.volume_before_mute = vol;
                }
                state.set_volume(vol);
            }
            let muted = state.player.volume <= 0.0;
            if ui
                .button(if muted { "Unmute" } else { "Mute" })
                .clicked()
            {
                state.toggle_mute();
            }
            // ドラッグ終了 (またはクリック等の単発変更) で永続化する。
            if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                if let Err(e) = crate::backend::prefs::save(&state.settings) {
                    eprintln!("[prefs] save failed: {e}");
                }
            }
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
                    HomeAction::None => {}
                },
                Route::Search => match state.search.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    SearchAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    SearchAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    SearchAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
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
                    TagAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
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
                    LibraryAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
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
                    CollectionAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
                    }
                    CollectionAction::None => {}
                },
                Route::Track => match state.track.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
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
                    TrackAction::Seek(frac) => {
                        if let (Some(audio), Some(dur)) =
                            (audio.as_ref(), state.player.duration_secs)
                        {
                            if dur > 0.0 {
                                let _ = engine::seek(
                                    (f64::from(frac) * dur).clamp(0.0, dur),
                                    audio,
                                );
                            }
                        }
                    }
                    TrackAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
                    }
                    TrackAction::None => {}
                },
                Route::Playlist => match state.playlist.show(
                    api_ref, &rt, images, player, audio_ref, param_ref, cache, accent, ui,
                ) {
                    PlaylistAction::PlayTrack(track) => {
                        state.play_list(vec![track], 0);
                    }
                    PlaylistAction::PlayList(tracks, i) => {
                        state.play_list(tracks, i);
                    }
                    PlaylistAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
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
                    AlbumAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
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
                    UserAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
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
                    ArtistAction::Navigate(route, param) => {
                        state.route = route;
                        state.nav_param = param;
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
                        state.route = route;
                        state.nav_param = param;
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
            state.route = Route::Login;
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
        state.route = Route::Search;
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
        if state.queue_open {
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
