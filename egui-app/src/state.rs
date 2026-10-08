//! AppState: egui 版の中央状態。
//! 現行 `desktop/src/stores/*` (zustand) + `desktop/src/lib/hooks.ts`
//! (react-query) の代替。Phase 2 で backend 起動 (`boot::boot`) と
//! ファイル再生の状態機械を追加した。

use std::sync::Arc;

use tokio::sync::{
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    oneshot,
};

use crate::backend::api::ApiClient;
use crate::backend::audio::state::AudioState;
use crate::backend::boot::{self, BootHandle};
use crate::backend::events::EventBus;
use crate::backend::models::Track;
use crate::images::Images;
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
        }
    }
}

/// `desktop/src/stores/settings.ts` 対応 (Phase 0 は subset)。
/// 既定アクセントは SoundCloud オレンジ `#ff5500`。
#[derive(Debug)]
pub struct SettingsState {
    pub accent: [u8; 3],
    pub theme_preset: ThemePreset,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
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
    pub api: Option<ApiClient>,
    pub home: HomeView,
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
        // 現行はダーク専用のため egui もダーク既定。テーマ切替は Phase 3。
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let (backend_tx, backend_rx) = mpsc::unbounded_channel();
        let (backend, boot_error) = match boot::boot(&runtime, EventBus::new(backend_tx)) {
            Ok(handle) => (Some(handle), None),
            Err(e) => (None, Some(e)),
        };
        let api = backend.as_ref().map(|b| {
            let mut api = ApiClient::new(b.servers.api_port);
            api.set_session(b.session.token());
            api
        });
        Self {
            route: Route::Home,
            player: PlayerState::default(),
            settings: SettingsState::default(),
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
            api,
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

    /// SoundCloud トラックのストリーム再生を開始する。
    pub fn play_stream(&mut self, track: &Track, url: String) {
        let title = track.display_title().to_string();
        let artist = track.artist_name().to_string();
        self.player.current_title = Some(title.clone());
        self.player.current_artist = Some(artist.clone());
        // メタの duration を仮置きし、読込完了で確定値に更新する。
        self.player.duration_secs = Some(track.duration_secs());
        self.player.is_playing = true;
        self.load_error = None;
        let Some(audio) = self.audio().cloned() else {
            self.load_error = Some("no audio device".to_string());
            self.player.is_playing = false;
            return;
        };
        let session = self
            .api
            .as_ref()
            .and_then(|a| a.session_token().map(str::to_string));
        let (tx, rx) = oneshot::channel();
        self.runtime.spawn(async move {
            let result =
                crate::backend::audio::engine::load_url(url, session, None, None, None, false, &audio)
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
        self.file_path_input = path;
        self.start_file_load();
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
        if let Some(rx) = self.backend_rx.as_mut() {
            while let Ok((event, payload)) = rx.try_recv() {
                match event.as_str() {
                    "audio:ended" => ended = true,
                    "direct:sync-error" => {
                        let s = payload.to_string();
                        sync_error = Some(s.chars().take(200).collect());
                    }
                    _ => {}
                }
            }
        }
        if ended {
            self.player.is_playing = false;
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
                    self.player.duration_secs = duration;
                    self.player.is_playing = true;
                    self.load_error = None;
                }
                Err(e) => {
                    self.load_error = Some(e);
                    self.player.is_playing = false;
                }
            }
        }
    }
}
