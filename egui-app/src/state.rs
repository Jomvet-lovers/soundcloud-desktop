//! AppState: egui 版の中央状態。
//! 現行 `desktop/src/stores/*` (zustand) + `desktop/src/lib/hooks.ts`
//! (react-query) の代替。Phase 2 で backend 起動 (`boot::boot`) と
//! ファイル再生の状態機械を追加した。

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    oneshot,
};

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::boot::{self, BootHandle};
use crate::backend::events::EventBus;
use crate::backend::models::{LikedFlag, Track};
use crate::images::Images;
use crate::query::Query;
use crate::views::home::HomeView;
use crate::views::{
    album::AlbumView, artist::ArtistView, collection::CollectionView, library::LibraryView,
    login::LoginView, offline::OfflineView, playlist::PlaylistView, search::SearchView,
    settings::SettingsView, tag::TagView, track::TrackView, user::UserView,
};

/// 現行 react-router の 13 ルートに対応するナビゲーション先。
/// 対応: `desktop/src/App.tsx` の Route 定義。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Route {
    #[default]
    Home,
    Search,
    Tag,
    Library,
    LibraryCollection,
    Offline,
    Track,
    Playlist,
    User,
    Artist,
    Album,
    Settings,
    Login,
}

impl Route {
    /// サイドバーに並べる主要ルート (パラメータ付きは除外)。
    pub const ALL: &[Route] = &[
        Route::Home,
        Route::Search,
        Route::Library,
        Route::Offline,
        Route::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Route::Home => "Home",
            Route::Search => "Search",
            Route::Tag => "Tag",
            Route::Library => "Library",
            Route::LibraryCollection => "Collection",
            Route::Offline => "Offline",
            Route::Track => "Track",
            Route::Playlist => "Playlist",
            Route::User => "User",
            Route::Artist => "Artist",
            Route::Album => "Album",
            Route::Settings => "Settings",
            Route::Login => "Login",
        }
    }
}

/// `desktop/src/stores/player.ts` 対応 (Phase 2 は subset)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

#[derive(Debug)]
pub struct PlayerState {
    pub current_title: Option<String>,
    pub current_artist: Option<String>,
    pub is_playing: bool,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub duration_secs: Option<f64>,
    pub queue: Vec<Track>,
    pub queue_index: Option<usize>,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            current_title: None,
            current_artist: None,
            is_playing: false,
            volume: 80.0,
            shuffle: false,
            repeat: RepeatMode::Off,
            duration_secs: None,
            queue: Vec::new(),
            queue_index: None,
        }
    }
}

impl PlayerState {
    pub fn current_queued(&self) -> Option<&Track> {
        self.queue_index.and_then(|i| self.queue.get(i))
    }

    /// キューを置き換えてメタを反映する。読込自体は呼出側が行う。
    pub fn set_queue(&mut self, tracks: Vec<Track>, index: usize) {
        let current = tracks.get(index).cloned();
        self.queue = tracks;
        self.queue_index = current.as_ref().map(|_| index);
        match current {
            Some(t) => {
                self.current_title = Some(t.display_title().to_string());
                self.current_artist = Some(t.artist_name().to_string());
                self.duration_secs = Some(t.duration_secs());
                self.is_playing = true;
            }
            None => {
                self.queue_index = None;
                self.is_playing = false;
            }
        }
    }

    pub fn clear_queue(&mut self) {
        self.queue.clear();
        self.queue_index = None;
    }

    /// 手動送り・自動送り共通の次 index。repeat-one は呼出側で処理する。
    pub fn next_index(&self) -> Option<usize> {
        let current = self.queue_index?;
        let len = self.queue.len();
        if len == 0 {
            return None;
        }
        if self.shuffle && len > 1 {
            return Some(shuffled_next(len, current));
        }
        if current + 1 < len {
            Some(current + 1)
        } else if self.repeat == RepeatMode::All {
            Some(0)
        } else {
            None
        }
    }

    pub fn prev_index(&self) -> Option<usize> {
        let current = self.queue_index?;
        if self.queue.is_empty() {
            return None;
        }
        if self.shuffle && self.queue.len() > 1 {
            return Some(shuffled_next(self.queue.len(), current));
        }
        if current > 0 {
            Some(current - 1)
        } else if self.repeat == RepeatMode::All {
            Some(self.queue.len() - 1)
        } else {
            None
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
    }
}

fn shuffled_next(len: usize, current: usize) -> usize {
    if len <= 1 {
        return current;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    let mut next = (current + 1 + nanos) % len;
    if next == current {
        next = (next + 1) % len;
    }
    next
}

/// `desktop/src/stores/settings.ts` 対応 (Phase 0 は subset)。
/// 既定アクセントは SoundCloud オレンジ `#ff5500`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsState {
    pub accent: [u8; 3],
    pub theme_preset: ThemePreset,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ThemePreset {
    #[default]
    SoundCloud,
    Dark,
    Neon,
    Forest,
    Crimson,
    Custom,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            accent: [0xff, 0x55, 0x00],
            theme_preset: ThemePreset::SoundCloud,
        }
    }
}

/// バックエンド→UI の通知。Tauri の `listen("audio:ended")` 等の代替。
/// Phase 1 で `audio/tick.rs`・`track_cache` の emit 送信側をここに繋ぐ。
#[derive(Debug)]
pub enum BackendEvent {
    AudioEnded,
    CacheProgress { done: u64, total: u64 },
    SyncError(String),
}

/// ファイル読込の非同期状態。デコード中も UI を固めない。
pub enum LoadState {
    Idle,
    Loading {
        label: String,
        /// Some の場合は完了時にこのタイトルを使う (ストリーム再生)。
        /// None の場合は label (ファイルパス) から推定する。
        title: Option<(String, Option<String>)>,
        rx: oneshot::Receiver<Result<Option<f64>, String>>,
    },
}

pub struct AppState {
    pub route: Route,
    pub player: PlayerState,
    pub settings: SettingsState,
    runtime: tokio::runtime::Runtime,
    events_rx: UnboundedReceiver<BackendEvent>,
    pub events_tx: UnboundedSender<BackendEvent>,
    pub backend: Option<BootHandle>,
    pub boot_error: Option<String>,
    backend_rx: Option<UnboundedReceiver<(String, serde_json::Value)>>,
    pub load: LoadState,
    pub load_error: Option<String>,
    pub file_path_input: String,
    pub last_sync_error: Option<String>,
    /// 再生中トラックの like 状態 (NowPlaying バー用)。
    pub now_liked: Option<bool>,
    pub now_like: Query<LikedFlag>,
    pub api: Option<ApiClient>,
    pub queue_open: bool,
    pub theme_applied: Option<(ThemePreset, [u8; 3])>,    pub home: HomeView,
    pub search: SearchView,
    pub tag: TagView,
    pub library: LibraryView,
    pub collection: CollectionView,
    pub track: TrackView,
    pub playlist: PlaylistView,
    pub album: AlbumView,
    pub user: UserView,
    pub artist: ArtistView,
    pub settings_view: SettingsView,
    pub offline: OfflineView,
    pub login: LoginView,
    pub images: Images,
    /// パラメータ付きルート (track/:urn 等) の選択値。`Navigate` で設定される。
    pub nav_param: Option<String>,
}

impl AppState {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::install_fonts(&cc.egui_ctx);
        crate::theme::install_text_styles(&cc.egui_ctx);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let (backend_tx, backend_rx) = mpsc::unbounded_channel();
        let mut bus = EventBus::new(backend_tx);
        bus.set_wake({
            let ctx = cc.egui_ctx.clone();
            move || ctx.request_repaint()
        });
        let (backend, boot_error) = match boot::boot(&runtime, bus) {
            Ok(handle) => (Some(handle), None),
            Err(e) => (None, Some(e)),
        };
        let api = backend.as_ref().map(|b| {
            let mut api = ApiClient::new(b.servers.api_port);
            api.set_session(b.session.token());
            api
        });
        // 起動時に未ログインならログイン画面を出す。
        let signed_in = api.as_ref().and_then(|a| a.session_token()).is_some();
        Self {
            route: if signed_in { Route::Home } else { Route::Login },
            player: PlayerState::default(),
            settings: crate::backend::prefs::load().unwrap_or_default(),
            runtime,
            events_rx,
            events_tx,
            backend,
            boot_error,
            backend_rx: Some(backend_rx),
            load: LoadState::Idle,
            load_error: None,
            file_path_input: String::new(),
            last_sync_error: None,
            now_liked: None,
            now_like: Query::default(),
            api,
            queue_open: false,
            theme_applied: None,
            home: HomeView::default(),
            search: SearchView::default(),
            tag: TagView::default(),
            library: LibraryView::default(),
            collection: CollectionView::default(),
            track: TrackView::default(),
            playlist: PlaylistView::default(),
            album: AlbumView::default(),
            user: UserView::default(),
            artist: ArtistView::default(),
            settings_view: SettingsView::default(),
            offline: OfflineView::default(),
            login: LoginView::default(),
            images: Images::new(std::sync::Arc::new(wreq::Client::new())),
            nav_param: None,
        }
    }

    pub fn runtime(&self) -> &tokio::runtime::Runtime {
        &self.runtime
    }

    pub fn audio(&self) -> Option<&Arc<AudioState>> {
        self.backend.as_ref().and_then(|b| b.audio.as_ref())
    }

    /// ファイル読込を開始する (非同期。完了は `poll_load` で回収)。
    pub fn start_file_load(&mut self) {
        if !matches!(self.load, LoadState::Idle) {
            return;
        }
        let path = self.file_path_input.trim().to_string();
        if path.is_empty() {
            return;
        }
        let Some(audio) = self.audio().cloned() else {
            self.load_error = Some("no audio device".to_string());
            return;
        };
        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let result =
                crate::backend::audio::engine::load_file(path, None, None, false, &audio)
                    .await
                    .map(|out| out.duration_secs);
            let _ = tx.send(result);
        });
        self.load = LoadState::Loading {
            label: self.file_path_input.trim().to_string(),
            title: None,
            rx,
        };
        self.load_error = None;
    }

    /// SoundCloud トラックのストリーム再生を開始する (単曲=1件キュー)。
    /// `url` は旧 Tauri 呼び出しとの互換用で未使用 (direct-mode は cache 経由)。
    pub fn play_stream(&mut self, track: &Track, _url: String) {
        let queue = vec![track.clone()];
        self.player.set_queue(queue, 0);
        self.load_error = None;
        self.load_stream(track);
    }

    /// キュー内 index のトラックを再生する。
    pub fn play_queue_index(&mut self, index: usize) {
        let track = match self.player.queue.get(index).cloned() {
            Some(t) => t,
            None => return,
        };
        let queue = std::mem::take(&mut self.player.queue);
        self.player.set_queue(queue, index);
        self.load_error = None;
        self.load_stream(&track);
    }

    /// メタ反映済みを前提にストリーム読込だけ行う。
    /// Tauri 版 `loadTrack` と同じく cache (`ensure_playable`) → ローカルファイル再生。
    fn load_stream(&mut self, track: &Track) {
        let title = track.display_title().to_string();
        let artist = track.artist_name().to_string();
        let Some(audio) = self.audio().cloned() else {
            self.load_error = Some("no audio device".to_string());
            self.player.is_playing = false;
            return;
        };
        let Some(cache) = self.backend.as_ref().map(|b| b.track_cache.clone()) else {
            self.load_error = Some("backend not running".to_string());
            self.player.is_playing = false;
            return;
        };
        let session = self
            .api
            .as_ref()
            .and_then(|a| a.session_token().map(str::to_string));
        let urn = track.urn.clone();
        let expected_ms = (track.duration > 0).then_some(track.duration as u64);
        // NowPlaying バー用の like 状態を取得し直す (ローカルストア + user_favorite)。
        self.now_liked = track.user_favorite;
        self.now_like = Query::default();
        if let Some(api) = self.api.clone() {
            let rt = self.runtime.handle().clone();
            let path = format!("/likes/tracks/{}", urlencoding::encode(&urn));
            self.now_like.request(&rt, async move {
                api.get_json(&path)
                    .await
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
            });
        }
        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let entry = match cache.get_cache_entry(&urn) {
                Some(entry) => entry,
                None => match cache
                    .ensure_playable(&urn, session.as_deref(), false, expected_ms)
                    .await
                {
                    Ok(entry) => entry,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                },
            };
            let result = crate::backend::audio::engine::load_file(
                entry.path, None, None, false, &audio,
            )
            .await
            .map(|out| out.duration_secs);
            let _ = tx.send(result);
        });
        self.load = LoadState::Loading {
            label: title.clone(),
            title: Some((title, Some(artist))),
            rx,
        };
    }

    /// キャッシュ済みファイルの再生 (Offline ページ用)。
    pub fn play_file(&mut self, path: String) {
        let stem = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        self.player.set_queue(
            vec![Track {
                urn: format!("file:{path}"),
                title: stem,
                ..Default::default()
            }],
            0,
        );
        self.file_path_input = path;
        self.start_file_load();
    }

    pub fn next_track(&mut self) {
        match self.player.next_index() {
            Some(i) => self.play_queue_index(i),
            None => self.stop_playback(),
        }
    }

    /// 対応: React の `handlePrev`。3秒超えは先頭に戻す。
    pub fn prev_track(&mut self, pos_secs: f64) {
        if pos_secs > 3.0 {
            if let Some(audio) = self.audio().cloned() {
                let _ = crate::backend::audio::engine::seek(0.0, &audio);
            }
            return;
        }
        if let Some(i) = self.player.prev_index() {
            self.play_queue_index(i);
        }
    }

    pub fn stop_playback(&mut self) {
        if let Some(audio) = self.audio() {
            crate::backend::audio::engine::stop(audio);
        }
        self.player.is_playing = false;
    }

    /// 再生履歴を記録する (fire-and-forget)。repeat-one のループは除く。
    /// 対応: `desktop/src/lib/audio.ts` の `afterLoad`。
    fn record_history(&self) {
        if self.player.repeat == RepeatMode::One {
            return;
        }
        let (Some(api), Some(index)) = (self.api.clone(), self.player.queue_index) else {
            return;
        };
        let track = match self.player.queue.get(index).cloned() {
            Some(t) if !t.urn.starts_with("file:") => t,
            _ => return,
        };
        let rt = self.runtime.handle().clone();
        rt.spawn(async move {
            let body = serde_json::json!({
                "scTrackId": track.urn,
                "title": track.display_title(),
                "artistName": track.artist_name(),
                "artistUrn": track.user.as_ref().map(|u| &u.urn),
                "artworkUrl": track.artwork_url,
                "duration": track.duration,
            });
            let _ = api.request_json("POST", "/history", Some(&body)).await;
        });
    }

    /// ログイン/ログアウト後の再取得のため全ビューの取得状態を捨てる。
    pub fn reset_views(&mut self) {
        self.home = HomeView::default();
        self.search = SearchView::default();
        self.tag = TagView::default();
        self.library = LibraryView::default();
        self.collection = CollectionView::default();
        self.track = TrackView::default();
        self.playlist = PlaylistView::default();
        self.album = AlbumView::default();
        self.user = UserView::default();
        self.artist = ArtistView::default();
        self.settings_view = SettingsView::default();
        self.offline = OfflineView::default();
        self.login = LoginView::default();
    }

    /// ログイン/ログアウトによるトークン変化を ApiClient に反映する。
    pub fn sync_api_session(&mut self) {
        let token = self.backend.as_ref().and_then(|b| b.session.token());
        if let Some(api) = self.api.as_mut() {
            if api.session_token().map(str::to_string) != token {
                api.set_session(token);
            }
        }
    }

    /// フレーム先頭でイベントを捌く。送信側は `ctx.request_repaint()` で起こす。
    pub fn drain_events(&mut self) {
        while let Ok(event) = self.events_rx.try_recv() {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: BackendEvent) {
        match event {
            BackendEvent::AudioEnded => {
                self.player.is_playing = false;
            }
            BackendEvent::CacheProgress { .. } => {
                // Phase 1: 進捗表示に反映。
            }
            BackendEvent::SyncError(_) => {
                // Phase 4: `direct:sync-error` 相当の通知に反映。
            }
        }
    }

    /// backend (tick/ended/sync-error) イベントを捌く。
    pub fn drain_backend(&mut self) {
        let mut ended = false;
        let mut sync_error = None;
        let mut auth_changed = false;
        if let Some(rx) = self.backend_rx.as_mut() {
            while let Ok((event, payload)) = rx.try_recv() {
                match event.as_str() {
                    "audio:ended" => ended = true,
                    "direct:sync-error" => {
                        let s = payload.to_string();
                        sync_error = Some(s.chars().take(200).collect());
                    }
                    "auth:changed" => auth_changed = true,
                    _ => {}
                }
            }
        }
        if auth_changed {
            // ログイン/ログアウト後は ApiClient を追随させ、取得済みの
            // ビュー状態 (トークン無しでエラーになった分) を捨てて再取得する。
            self.sync_api_session();
            self.reset_views();
        }
        if ended {
            if self.player.repeat == RepeatMode::One {
                if let Some(audio) = self.audio().cloned() {
                    let _ = crate::backend::audio::engine::seek(0.0, &audio);
                    crate::backend::audio::engine::play(&audio);
                }
                self.player.is_playing = true;
            } else if let Some(i) = self.player.next_index() {
                self.play_queue_index(i);
            } else {
                self.player.is_playing = false;
            }
        }
        if sync_error.is_some() {
            self.last_sync_error = sync_error;
        }
    }

    /// 非同期ファイル読込の完了を回収する。
    pub fn poll_load(&mut self) {
        let result = match &mut self.load {
            LoadState::Loading { rx, .. } => rx.try_recv().ok(),
            _ => None,
        };
        if let Some(result) = result {
            let (label, title) = match &self.load {
                LoadState::Loading { label, title, .. } => (label.clone(), title.clone()),
                _ => (String::new(), None),
            };
            self.load = LoadState::Idle;
            match result {
                Ok(duration) => {
                    match title {
                        Some((t, a)) => {
                            self.player.current_title = Some(t);
                            self.player.current_artist = a;
                        }
                        None => {
                            let t = std::path::Path::new(&label)
                                .file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or(label);
                            self.player.current_title = Some(t);
                            self.player.current_artist = None;
                        }
                    }
                    if duration.is_some() {
                        self.player.duration_secs = duration;
                    }
                    self.player.is_playing = true;
                    self.load_error = None;
                    self.record_history();
                }
                Err(e) => {
                    self.load_error = Some(e);
                    self.player.is_playing = false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(urn: &str) -> Track {
        Track {
            urn: urn.to_string(),
            title: format!("t-{urn}"),
            duration: 180_000,
            ..Default::default()
        }
    }

    fn queued(n: usize) -> PlayerState {
        let mut s = PlayerState::default();
        let tracks: Vec<Track> = (0..n).map(|i| track(&format!("urn-{i}"))).collect();
        s.set_queue(tracks, 0);
        s
    }

    #[test]
    fn set_queue_reflects_meta_and_index() {
        let mut s = PlayerState::default();
        s.set_queue(vec![track("a"), track("b"), track("c")], 1);
        assert_eq!(s.queue.len(), 3);
        assert_eq!(s.queue_index, Some(1));
        assert_eq!(s.current_title.as_deref(), Some("t-b"));
        assert_eq!(s.current_artist.as_deref(), Some("Unknown artist"));
        assert_eq!(s.duration_secs, Some(180.0));
        assert!(s.is_playing);
    }

    #[test]
    fn next_linear_and_terminal_none_when_off() {
        let mut s = queued(3);
        assert_eq!(s.repeat, RepeatMode::Off);
        assert_eq!(s.queue_index, Some(0));
        assert_eq!(s.next_index(), Some(1));
        s.queue_index = Some(1);
        assert_eq!(s.next_index(), Some(2));
        s.queue_index = Some(2);
        assert_eq!(s.next_index(), None);
    }

    #[test]
    fn next_wraps_when_all() {
        let mut s = queued(3);
        s.repeat = RepeatMode::All;
        s.queue_index = Some(2);
        assert_eq!(s.next_index(), Some(0));
    }

    #[test]
    fn prev_steps_back_and_head_none_when_off() {
        let mut s = queued(3);
        s.queue_index = Some(2);
        assert_eq!(s.prev_index(), Some(1));
        s.queue_index = Some(1);
        assert_eq!(s.prev_index(), Some(0));
        s.queue_index = Some(0);
        assert_eq!(s.prev_index(), None);
    }

    #[test]
    fn prev_wraps_to_last_when_all() {
        let mut s = queued(3);
        s.repeat = RepeatMode::All;
        s.queue_index = Some(0);
        assert_eq!(s.prev_index(), Some(2));
    }

    #[test]
    fn cycle_repeat_follows_off_all_one_off() {
        let mut s = PlayerState::default();
        assert_eq!(s.repeat, RepeatMode::Off);
        s.cycle_repeat();
        assert_eq!(s.repeat, RepeatMode::All);
        s.cycle_repeat();
        assert_eq!(s.repeat, RepeatMode::One);
        s.cycle_repeat();
        assert_eq!(s.repeat, RepeatMode::Off);
    }

    #[test]
    fn toggle_shuffle_flips() {
        let mut s = PlayerState::default();
        assert!(!s.shuffle);
        s.toggle_shuffle();
        assert!(s.shuffle);
        s.toggle_shuffle();
        assert!(!s.shuffle);
    }

    #[test]
    fn shuffle_next_stays_in_range() {
        let mut s = queued(5);
        s.shuffle = true;
        for _ in 0..50 {
            let next = s.next_index().expect("shuffled next");
            assert!(next < 5);
        }
    }
}
