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
use crate::backend::models::{DislikedFlag, LikedFlag, Playlist, Track, tracks_from_value};
use crate::backend::track_cache::ExportFormat;
use crate::images::Images;
use crate::pager::ListPage;
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
    /// ミュート解除用の退避音量 (ミュートボタン / M キー)。
    pub volume_before_mute: f32,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub duration_secs: Option<f64>,
    pub queue: Vec<Track>,
    pub queue_index: Option<usize>,
    /// shuffle ON 時の元の並び (OFF で復元)。Tauri 版 `originalQueue` 相当。
    pub original_queue: Option<Vec<Track>>,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            current_title: None,
            current_artist: None,
            is_playing: false,
            volume: 80.0,
            volume_before_mute: 80.0,
            shuffle: false,
            repeat: RepeatMode::Off,
            duration_secs: None,
            queue: Vec::new(),
            queue_index: None,
            original_queue: None,
        }
    }
}

impl PlayerState {
    pub fn current_queued(&self) -> Option<&Track> {
        self.queue_index.and_then(|i| self.queue.get(i))
    }

    /// コンテキスト再生のキュー設定 (Tauri 版 `play(track, queue)` 相当)。
    /// shuffle 中は選択トラックを先頭に、残りをシャッフルする。
    pub fn set_queue(&mut self, tracks: Vec<Track>, index: usize) {
        if tracks.is_empty() {
            self.queue.clear();
            self.queue_index = None;
            self.is_playing = false;
            return;
        }
        let index = index.min(tracks.len() - 1);
        if self.shuffle && tracks.len() > 1 {
            let chosen = tracks[index].clone();
            self.original_queue = Some(tracks.clone());
            let mut rest: Vec<Track> = tracks
                .into_iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .map(|(_, t)| t)
                .collect();
            shuffle_tracks(&mut rest);
            rest.insert(0, chosen);
            self.apply_queue(rest, 0);
        } else {
            self.original_queue = None;
            self.apply_queue(tracks, index);
        }
    }

    /// キューをそのまま差し替えて index を選択する (shuffle 変換なし)。
    fn apply_queue(&mut self, tracks: Vec<Track>, index: usize) {
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

    /// キュー内 index を選択してメタを更新する (キューは変更なし)。
    /// Tauri 版 `playFromQueue` 相当。
    pub fn select_index(&mut self, index: usize) {
        let Some(t) = self.queue.get(index).cloned() else {
            return;
        };
        self.queue_index = Some(index);
        self.current_title = Some(t.display_title().to_string());
        self.current_artist = Some(t.artist_name().to_string());
        self.duration_secs = Some(t.duration_secs());
        self.is_playing = true;
    }

    pub fn clear_queue(&mut self) {
        self.queue.clear();
        self.queue_index = None;
        self.original_queue = None;
    }

    /// 現在の次の位置 (再生中が無ければ末尾) に差し込む
    /// (Tauri 版 `addToQueueNext` 相当)。
    pub fn insert_next(&mut self, tracks: Vec<Track>) {
        if tracks.is_empty() {
            return;
        }
        match self.queue_index {
            Some(i) => {
                let at = (i + 1).min(self.queue.len());
                for (offset, t) in tracks.iter().enumerate() {
                    self.queue.insert(at + offset, t.clone());
                }
            }
            None => self.queue.extend(tracks.iter().cloned()),
        }
        if let Some(oq) = self.original_queue.as_mut() {
            oq.extend(tracks);
        }
    }

    /// キュー末尾へ追加 (Tauri 版 `addToQueue` 相当)。
    /// shuffle 中は現在位置より後ろのランダムな位置へ差し込む。
    pub fn append_queue(&mut self, tracks: Vec<Track>) {
        if tracks.is_empty() {
            return;
        }
        if self.shuffle && self.queue_index.is_some() && !self.queue.is_empty() {
            let cur = self.queue_index.unwrap();
            for t in tracks.iter() {
                let span = self.queue.len() - cur;
                let pos = cur + 1 + rand_below(span);
                self.queue.insert(pos.min(self.queue.len()), t.clone());
            }
        } else {
            self.queue.extend(tracks.iter().cloned());
        }
        if let Some(oq) = self.original_queue.as_mut() {
            oq.extend(tracks);
        }
    }

    /// キュー内の並替 (D&D)。現在位置の追従も行う。
    pub fn move_queue_item(&mut self, from: usize, to: usize) {
        if from == to || from >= self.queue.len() || to >= self.queue.len() {
            return;
        }
        let item = self.queue.remove(from);
        self.queue.insert(to, item);
        self.queue_index = match self.queue_index {
            Some(cur) if cur == from => Some(to),
            Some(cur) if from < cur && to >= cur => Some(cur - 1),
            Some(cur) if from > cur && to <= cur => Some(cur + 1),
            other => other,
        };
        if let Some(cur) = self.queue_index {
            if let Some(t) = self.queue.get(cur) {
                self.current_title = Some(t.display_title().to_string());
                self.current_artist = Some(t.artist_name().to_string());
            }
        }
    }

    /// 手動送り・自動送り共通の次 index。repeat-one は呼出側で処理する。
    /// shuffle は並び自体を並替える方式なので、ここは常に順送り。
    pub fn next_index(&self) -> Option<usize> {
        let current = self.queue_index?;
        let len = self.queue.len();
        if len == 0 {
            return None;
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
        if current > 0 {
            Some(current - 1)
        } else if self.repeat == RepeatMode::All {
            Some(self.queue.len() - 1)
        } else {
            None
        }
    }

    /// シャッフル切替 (Tauri 版 `toggleShuffle` 相当)。
    /// ON: 現在位置より後ろをシャッフルして元の並びを退避。
    /// OFF: 元の並びを復元して現在トラックの位置へ戻す。
    pub fn toggle_shuffle(&mut self) {
        if !self.shuffle {
            let after = match self.queue_index {
                Some(i) => (i + 1).min(self.queue.len()),
                None => 0,
            };
            self.original_queue = Some(self.queue.clone());
            let mut tail: Vec<Track> = self.queue.split_off(after);
            shuffle_tracks(&mut tail);
            self.queue.extend(tail);
            self.shuffle = true;
        } else {
            if let Some(original) = self.original_queue.take() {
                let urn = self.current_queued().map(|t| t.urn.clone());
                self.queue = original;
                if let Some(urn) = urn {
                    if let Some(i) = self.queue.iter().position(|t| t.urn == urn) {
                        self.queue_index = Some(i);
                    }
                }
            }
            self.shuffle = false;
        }
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
    }
}

/// xorshift の簡易乱数 (0..n)。
fn rand_below(n: usize) -> usize {
    if n <= 1 {
        return 0;
    }
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
        | 1;
    seed ^= seed << 13;
    seed ^= seed >> 7;
    seed ^= seed << 17;
    (seed as usize) % n
}

/// `desktop/src/stores/settings.ts` 対応 (Phase 0 は subset)。
/// 既定アクセントは SoundCloud オレンジ `#ff5500`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsState {
    pub accent: [u8; 3],
    pub theme_preset: ThemePreset,
    /// NowPlaying バーの音量 (0..=100)。後方互換のため default 付き。
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// 高品質ストリーミング (Settings > Playback)。
    #[serde(default)]
    pub hq_streaming: bool,
    /// 音量ノーマライズ。
    #[serde(default)]
    pub normalize_volume: bool,
    /// イコライザー有効。
    #[serde(default)]
    pub eq_enabled: bool,
    /// イコライザー 10 バンドのゲイン (-12..=12)。
    #[serde(default)]
    pub eq_gains: Vec<f64>,
    /// 再生速度 (0.5–2.0)。
    #[serde(default = "default_rate")]
    pub playback_rate: f32,
    /// 手動ピッチ (セミトーン、-12..=12)。
    #[serde(default)]
    pub pitch_semitones: f32,
    /// ピッチ自動 (速度に追従) かどうか。
    #[serde(default = "default_true")]
    pub pitch_auto: bool,
    /// 起動時に開くページ ("home" | "search" | "library" | "settings")。
    #[serde(default = "default_startup")]
    pub startup_page: String,
    /// サイドバーのクイックアクセス (pin したプレイリスト)。
    #[serde(default)]
    pub pinned_playlists: Vec<PinnedPlaylist>,
    /// 出力デバイス名 (follow_default_output = false のとき有効)。
    #[serde(default)]
    pub output_device: Option<String>,
    /// OS の既定出力に追従するか。
    #[serde(default = "default_true")]
    pub follow_default_output: bool,
    /// Discord Rich Presence を有効にするか。
    #[serde(default)]
    pub discord_rpc: bool,
    /// Discord の「GitHub」ボタンを表示するか。
    #[serde(default = "default_true")]
    pub discord_show_button: bool,
    /// 最近の検索 (最大 10 件、新しい順)。
    #[serde(default)]
    pub search_history: Vec<String>,
    /// オーディオキャッシュ上限 (MB、0 = 無制限)。
    #[serde(default = "default_cache_limit")]
    pub cache_limit_mb: u64,
    /// キュー終端で関連曲を自動継続する (autopilot)。
    #[serde(default = "default_true")]
    pub autopilot: bool,
    /// コメントを再生中のフローティングピルで表示する
    /// (Tauri: settings.floatingComments、既定 ON)。
    #[serde(default = "default_true")]
    pub floating_comments: bool,
    /// 閉じるボタンでトレイに格納する。
    #[serde(default)]
    pub close_to_tray: bool,
}

fn default_cache_limit() -> u64 {
    1024
}

/// サイドバー pin 用の最小情報。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinnedPlaylist {
    pub urn: String,
    pub title: String,
}

fn default_startup() -> String {
    "home".to_string()
}

fn default_volume() -> f32 {
    80.0
}

fn default_rate() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

impl SettingsState {
    /// EQ ゲイン (10 バンドに満たない/超過した場合は既定値)。
    pub fn eq_gains_or_default(&self) -> Vec<f64> {
        if self.eq_gains.len() == 10 {
            self.eq_gains.clone()
        } else {
            vec![0.0; 10]
        }
    }

    /// 実効再生レート (Tauri 版 `getEffectivePlaybackRate` 相当)。
    pub fn effective_rate(&self) -> f64 {
        let rate = self.playback_rate.clamp(0.25, 4.0) as f64;
        if !self.pitch_auto && self.pitch_semitones.abs() > 0.001 {
            rate * 2f64.powf(self.pitch_semitones as f64 / 12.0)
        } else {
            rate
        }
    }
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
            volume: default_volume(),
            hq_streaming: false,
            normalize_volume: false,
            eq_enabled: false,
            eq_gains: Vec::new(),
            playback_rate: default_rate(),
            pitch_semitones: 0.0,
            pitch_auto: true,
            startup_page: default_startup(),
            pinned_playlists: Vec::new(),
            output_device: None,
            follow_default_output: true,
            discord_rpc: false,
            discord_show_button: true,
            search_history: Vec::new(),
            cache_limit_mb: default_cache_limit(),
            autopilot: true,
            floating_comments: true,
            close_to_tray: false,
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

/// トラックの右クリックメニュー状態。
#[derive(Clone)]
pub struct TrackMenuState {
    pub track: Track,
    pub pos: egui::Pos2,
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
    /// サイドバー表示 (`[` キーで切替)。
    pub sidebar_open: bool,
    /// ショートカット一覧ダイアログ (Ctrl+/)。
    pub show_shortcuts: bool,
    /// フルスクリーン状態 (F11)。
    pub fullscreen: bool,
    /// A-B ループ (秒)。両方 Some で有効、A のみは B 待ち。
    pub ab_a: Option<f64>,
    pub ab_b: Option<f64>,
    /// イコライザー窓 / サウンドチューニング窓の表示。
    pub show_eq: bool,
    pub show_tuning: bool,
    /// 「プレイリストに追加」ダイアログの対象トラック (None で閉)。
    pub add_to_playlist: Option<Track>,
    pub dialog_playlists: Query<ListPage<Playlist>>,
    pub new_playlist_title: String,
    /// トラックの右クリックメニュー。
    pub track_menu: Option<TrackMenuState>,
    pub menu_like: Query<LikedFlag>,
    pub menu_dislike: Query<DislikedFlag>,
    /// ダウンロードダイアログ (対象トラック・形式・状態)。
    pub download_track: Option<Track>,
    pub download_format: ExportFormat,
    pub download_status: Query<String>,
    /// Discord RPC 接続状態と最終送信キー (変化時のみ更新)。
    pub discord_active: bool,
    discord_key: Option<(String, bool)>,
    discord_last_attempt: Option<std::time::Instant>,
    /// キュー終端の autopilot (関連曲の継続取得)。
    continuation: Query<Vec<Track>>,
    /// Discover 棚の再生用 (ステーション/システムミックスの解決結果)。
    discover_play: Query<Vec<Track>>,
    /// 再生コンテキストの継続ソース (Tauri: `queue-continuation.ts`)。
    continuation_source: Option<ContinuationSource>,
    /// 継続ソースのページ取得 (async)。
    source_fetch: Query<SourcePage>,
    /// shuffle いいね: 全件の先行取得 (Tauri: `useShuffleLikes`)。
    likes_full: Query<Vec<Track>>,
    /// 全件先行取得を開始した時点のキュー世代。
    likes_full_generation: u64,
    /// `play_list` ごとに増えるキュー世代 (古い取得結果の破棄用)。
    queue_generation: u64,
    /// トレイからの操作 (UI スレッドで回収)。
    tray_rx: Option<std::sync::mpsc::Receiver<crate::tray::TrayCmd>>,
    /// トレイに格納中かどうか。
    pub window_hidden: bool,
    /// トレイの「終了」など明示的な終了要求 (close_to_tray を無視する)。
    pub force_quit: bool,
    pub theme_applied: Option<(ThemePreset, [u8; 3])>,
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

/// 簡易 Fisher-Yates (xorshift seed)。アーティストステーション用。
fn shuffle_tracks<T>(v: &mut [T]) {
    if v.len() < 2 {
        return;
    }
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
        | 1;
    for i in (1..v.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed as usize) % (i + 1);
        v.swap(i, j);
    }
}

/// 継続ソースの種類 (Tauri: `queue-continuation.ts` の kind)。
#[derive(Debug, Clone, PartialEq)]
enum ContinuationKind {
    Likes,
    Playlist(String),
}

/// キュー終端でページを追加供給するコンテキストソース。
enum ContinuationSource {
    /// ページ式 (いいね 50 / プレイリスト 200)。
    Paged {
        kind: ContinuationKind,
        next_page: usize,
        fetching: bool,
    },
    /// shuffle: 全件を遅延取得し 50 件ずつ供給。
    Shuffled {
        kind: ContinuationKind,
        buffer: Vec<Track>,
        pos: usize,
        fetched: bool,
        fetching: bool,
    },
}

const LIKES_PAGE_SIZE: usize = 50;
const PLAYLIST_PAGE_SIZE: usize = 200;

/// `{ collection, has_more }` の継続ページ。
struct SourcePage {
    tracks: Vec<Track>,
    has_more: bool,
}

/// `{ collection, has_more }` を `SourcePage` へ変換する。
fn source_page_from_value(v: &serde_json::Value) -> SourcePage {
    SourcePage {
        tracks: tracks_from_value(v),
        has_more: v.get("has_more").and_then(|b| b.as_bool()).unwrap_or(false),
    }
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
        let settings = crate::backend::prefs::load().unwrap_or_default();
        let mut player = PlayerState::default();
        player.volume = settings.volume;
        // システムトレイ (常駐アイコン)。
        let (tray_tx, tray_rx) = std::sync::mpsc::channel();
        crate::tray::spawn(cc.egui_ctx.clone(), tray_tx);
        let startup_route = match settings.startup_page.as_str() {
            "search" => Route::Search,
            "library" => Route::Library,
            "settings" => Route::Settings,
            _ => Route::Home,
        };
        let state = Self {
            route: if signed_in {
                startup_route
            } else {
                Route::Login
            },
            player,
            settings,
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
            sidebar_open: true,
            show_shortcuts: false,
            fullscreen: false,
            ab_a: None,
            ab_b: None,
            show_eq: false,
            show_tuning: false,
            add_to_playlist: None,
            dialog_playlists: Query::default(),
            new_playlist_title: String::new(),
            track_menu: None,
            menu_like: Query::default(),
            menu_dislike: Query::default(),
            download_track: None,
            download_format: ExportFormat::default(),
            download_status: Query::default(),
            discord_active: false,
            discord_key: None,
            discord_last_attempt: None,
            continuation: Query::default(),
            discover_play: Query::default(),
            continuation_source: None,
            source_fetch: Query::default(),
            likes_full: Query::default(),
            likes_full_generation: 0,
            queue_generation: 0,
            tray_rx: Some(tray_rx),
            window_hidden: false,
            force_quit: false,
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
        };
        // 永続化されたオーディオ設定を起動時から反映する。
        if let Some(audio) = state.audio() {
            crate::backend::audio::engine::set_volume(state.player.volume as f64, audio);
            crate::backend::audio::engine::set_normalization(
                state.settings.normalize_volume,
                audio,
            );
            crate::backend::audio::engine::set_eq(
                state.settings.eq_enabled,
                state.settings.eq_gains_or_default(),
                audio,
            );
            crate::backend::audio::engine::set_playback_rate(
                state.settings.effective_rate(),
                audio,
            );
            // 出力デバイス (追従 or 固定)。
            if state.settings.follow_default_output {
                crate::backend::audio::set_follow_default_output(audio, true);
            } else {
                crate::backend::audio::set_follow_default_output(audio, false);
                if let Some(name) = state.settings.output_device.clone() {
                    if let Err(e) = crate::backend::audio::switch_device(audio, Some(name)) {
                        eprintln!("[audio] device switch failed: {e}");
                    }
                }
            }
        }
        // キャッシュ上限 (0 = 無制限) を起動時に適用。
        if state.settings.cache_limit_mb > 0
            && let Some(backend) = &state.backend
        {
            backend
                .track_cache
                .enforce_limit(state.settings.cache_limit_mb);
        }
        state
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
            let result = crate::backend::audio::engine::load_file(path, None, None, false, &audio)
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

    /// SoundCloud トラックのストリーム再生を開始する (単曲)。
    /// `url` は旧 Tauri 呼び出しとの互換用で未使用 (direct-mode は cache 経由)。
    pub fn play_stream(&mut self, track: &Track, _url: String) {
        self.play_list(vec![track.clone()], 0);
    }

    /// リスト文脈での再生: リスト全体をキューにして `index` から再生する
    /// (Tauri 版 `useTrackPlay(track, queue)` 相当)。
    pub fn play_list(&mut self, tracks: Vec<Track>, index: usize) {
        if tracks.is_empty() {
            return;
        }
        // 新しい play は前のコンテキスト継続を無効化する。
        self.continuation_source = None;
        self.source_fetch = Query::default();
        self.queue_generation = self.queue_generation.wrapping_add(1);
        let index = index.min(tracks.len() - 1);
        let track = tracks[index].clone();
        self.player.set_queue(tracks, index);
        self.load_error = None;
        self.load_stream(&track);
    }

    /// 音量を設定してエンジンへ反映する (0..=100、ミュートは 0)。
    pub fn set_volume(&mut self, vol: f32) {
        let vol = vol.clamp(0.0, 100.0);
        self.player.volume = vol;
        self.settings.volume = vol;
        if let Some(audio) = self.audio() {
            crate::backend::audio::engine::set_volume(vol as f64, audio);
        }
    }

    /// ミュート切替 (退避音量から復元)。設定へも保存する。
    pub fn toggle_mute(&mut self) {
        if self.player.volume > 0.0 {
            self.player.volume_before_mute = self.player.volume;
            self.set_volume(0.0);
        } else {
            let v = if self.player.volume_before_mute > 0.0 {
                self.player.volume_before_mute
            } else {
                80.0
            };
            self.set_volume(v);
        }
        if let Err(e) = crate::backend::prefs::save(&self.settings) {
            eprintln!("[prefs] save failed: {e}");
        }
    }

    /// EQ 設定をエンジンへ反映し保存する。
    pub fn apply_eq(&mut self) {
        if let Some(audio) = self.audio() {
            crate::backend::audio::engine::set_eq(
                self.settings.eq_enabled,
                self.settings.eq_gains_or_default(),
                audio,
            );
        }
        if let Err(e) = crate::backend::prefs::save(&self.settings) {
            eprintln!("[prefs] save failed: {e}");
        }
    }

    /// 再生速度/ピッチをエンジンへ反映し保存する。
    pub fn apply_rate(&mut self) {
        if let Some(audio) = self.audio() {
            crate::backend::audio::engine::set_playback_rate(self.settings.effective_rate(), audio);
        }
        if let Err(e) = crate::backend::prefs::save(&self.settings) {
            eprintln!("[prefs] save failed: {e}");
        }
    }

    /// 「プレイリストに追加」ダイアログを開く (状態をリセットして表示)。
    pub fn open_add_to_playlist(&mut self, track: Track) {
        self.add_to_playlist = Some(track);
        self.dialog_playlists = Query::default();
        self.new_playlist_title.clear();
    }

    /// サイドバーの pin をトグルする (settings へ保存)。
    pub fn toggle_pin_playlist(&mut self, urn: String, title: String) {
        if let Some(pos) = self
            .settings
            .pinned_playlists
            .iter()
            .position(|p| p.urn == urn)
        {
            self.settings.pinned_playlists.remove(pos);
        } else {
            self.settings
                .pinned_playlists
                .push(PinnedPlaylist { urn, title });
        }
        if let Err(e) = crate::backend::prefs::save(&self.settings) {
            eprintln!("[prefs] save failed: {e}");
        }
    }

    /// ダウンロードダイアログを開く (状態をリセットして表示)。
    pub fn open_download(&mut self, track: Track) {
        self.download_status = Query::default();
        self.download_track = Some(track);
    }

    /// Discord RPC を設定に追従させる (毎フレーム呼ぶ)。
    pub fn sync_discord(&mut self) {
        let want = self.settings.discord_rpc;
        let Some(backend) = self.backend.as_ref() else {
            return;
        };
        if want && !self.discord_active {
            if self
                .discord_last_attempt
                .map(|t| t.elapsed().as_secs() >= 10)
                .unwrap_or(true)
            {
                self.discord_last_attempt = Some(std::time::Instant::now());
                if crate::backend::discord::discord_connect(&backend.discord).is_ok() {
                    self.discord_active = true;
                    self.discord_key = None;
                }
            }
        } else if !want && self.discord_active {
            let _ = crate::backend::discord::discord_clear_activity(&backend.discord);
            crate::backend::discord::discord_disconnect(&backend.discord);
            self.discord_active = false;
            self.discord_key = None;
        }
        if !self.discord_active {
            return;
        }
        // トラック / 再生状態が変わった時だけ送信する。
        let urn = self
            .player
            .current_queued()
            .map(|t| t.urn.clone())
            .unwrap_or_default();
        let key = (urn, self.player.is_playing);
        if self.discord_key.as_ref() == Some(&key) {
            return;
        }
        self.discord_key = Some(key);
        self.push_discord_activity();
    }

    /// 現在の再生状態を Discord Rich Presence に送る。
    pub fn push_discord_activity(&self) {
        if !self.discord_active {
            return;
        }
        let Some(backend) = self.backend.as_ref() else {
            return;
        };
        let Some(track) = self.player.current_queued().cloned() else {
            let _ = crate::backend::discord::discord_clear_activity(&backend.discord);
            return;
        };
        let elapsed = self
            .audio()
            .map(|a| crate::backend::audio::engine::get_position(a) as i64);
        let info = crate::backend::discord::DiscordTrackInfo {
            title: track.display_title().to_string(),
            artist: track.artist_name().to_string(),
            artwork_url: track.artwork("t500x500"),
            track_url: track.permalink_url.clone(),
            artist_url: track.user.as_ref().and_then(|u| u.permalink_url.clone()),
            duration_secs: (track.duration > 0).then_some(track.duration / 1000),
            elapsed_secs: elapsed,
            is_playing: Some(self.player.is_playing),
            mode: Some(crate::backend::discord::DiscordRpcMode::Track),
            show_button: Some(self.settings.discord_show_button),
        };
        if let Err(e) = crate::backend::discord::discord_set_activity(&backend.discord, info) {
            eprintln!("[Discord] set_activity failed: {e}");
        }
    }

    /// トラックの右クリックメニューを開く (like/dislike 状態も取得)。
    pub fn open_track_menu(&mut self, track: Track, pos: egui::Pos2) {
        if let Some(api) = self.api.clone() {
            let rt = self.runtime.handle().clone();
            let enc = urlencoding::encode(&track.urn).into_owned();
            self.menu_like = Query::default();
            {
                let api = api.clone();
                let path = format!("/likes/tracks/{enc}");
                self.menu_like.request(&rt, async move {
                    api.get_json(&path)
                        .await
                        .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
                });
            }
            self.menu_dislike = Query::default();
            {
                let path = format!("/dislikes/status/{enc}");
                self.menu_dislike.request(&rt, async move {
                    api.get_json(&path)
                        .await
                        .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
                });
            }
        }
        self.track_menu = Some(TrackMenuState { track, pos });
    }

    /// メニューから like をトグルする (ローカル即時 + writer 同期)。
    pub fn toggle_track_like(&mut self, urn: &str, next: bool) {
        if let Some(api) = self.api.clone() {
            let rt = self.runtime.handle().clone();
            let path = format!("/likes/tracks/{}", urlencoding::encode(urn));
            rt.spawn(async move {
                let method = if next { "POST" } else { "DELETE" };
                let _ = api.request_json(method, &path, None).await;
            });
        }
        self.menu_like.data = Some(LikedFlag { liked: next });
    }

    /// メニューから dislike をトグルする (ローカルのみ)。
    pub fn toggle_track_dislike(&mut self, urn: &str, next: bool) {
        if let Some(api) = self.api.clone() {
            let rt = self.runtime.handle().clone();
            let path = format!("/dislikes/{}", urlencoding::encode(urn));
            rt.spawn(async move {
                let method = if next { "POST" } else { "DELETE" };
                let _ = api.request_json(method, &path, None).await;
            });
        }
        self.menu_dislike.data = Some(DislikedFlag { disliked: next });
    }

    /// キュー内 index のトラックを再生する。
    pub fn play_queue_index(&mut self, index: usize) {
        let track = match self.player.queue.get(index).cloned() {
            Some(t) => t,
            None => return,
        };
        self.player.select_index(index);
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
        // トラック切替で A-B ループを解除する (Tauri 版 `clearAbLoop` 相当)。
        self.ab_a = None;
        self.ab_b = None;
        crate::backend::audio::engine::set_ab_loop(None, None, &audio);
        // SMTC / MPRIS へメタデータを通知する。
        crate::backend::audio::engine::set_metadata(
            title.clone(),
            artist.clone(),
            track.artwork("t500x500"),
            track.duration_secs(),
            &audio,
        );
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
        let hq = self.settings.hq_streaming;
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
                    .ensure_playable(&urn, session.as_deref(), hq, expected_ms)
                    .await
                {
                    Ok(entry) => entry,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                },
            };
            let result =
                crate::backend::audio::engine::load_file(entry.path, None, None, false, &audio)
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
            None => self.continue_or_stop(),
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
        let mut media_play = false;
        let mut media_pause = false;
        let mut media_toggle = false;
        let mut media_next = false;
        let mut media_prev = false;
        let mut media_seek_abs: Option<f64> = None;
        let mut media_seek_rel: Option<f64> = None;
        if let Some(rx) = self.backend_rx.as_mut() {
            while let Ok((event, payload)) = rx.try_recv() {
                match event.as_str() {
                    "audio:ended" => ended = true,
                    "direct:sync-error" => {
                        let s = payload.to_string();
                        sync_error = Some(s.chars().take(200).collect());
                    }
                    "auth:changed" => auth_changed = true,
                    "media:play" => media_play = true,
                    "media:pause" => media_pause = true,
                    "media:toggle" => media_toggle = true,
                    "media:next" => media_next = true,
                    "media:prev" => media_prev = true,
                    "media:seek" => media_seek_abs = payload.as_f64(),
                    "media:seek-relative" => media_seek_rel = payload.as_f64(),
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
                // キュー終端 → コンテキスト継続 → autopilot → 停止。
                self.continue_or_stop();
            }
        }
        // OS のメディア操作 (SMTC / MPRIS)。
        if media_play || media_pause || media_toggle {
            if let Some(audio) = self.audio().cloned() {
                let should_play = if media_toggle {
                    !self.player.is_playing
                } else {
                    media_play
                };
                if should_play {
                    crate::backend::audio::engine::play(&audio);
                    self.player.is_playing = true;
                } else {
                    crate::backend::audio::engine::pause(&audio);
                    self.player.is_playing = false;
                }
            }
        }
        if media_next {
            self.next_track();
        }
        if media_prev {
            if let Some(audio) = self.audio().cloned() {
                let pos = crate::backend::audio::engine::get_position(&audio);
                self.prev_track(pos);
            }
        }
        if let Some(target) = media_seek_abs {
            if let Some(audio) = self.audio().cloned() {
                let dur = self.player.duration_secs.unwrap_or(target).max(0.0);
                let _ = crate::backend::audio::engine::seek(target.clamp(0.0, dur), &audio);
            }
        }
        if let Some(delta) = media_seek_rel {
            if let (Some(audio), Some(dur)) = (self.audio().cloned(), self.player.duration_secs) {
                let pos = crate::backend::audio::engine::get_position(&audio);
                let _ = crate::backend::audio::engine::seek((pos + delta).clamp(0.0, dur), &audio);
            }
        }
        if sync_error.is_some() {
            self.last_sync_error = sync_error;
        }
    }

    /// キュー終端で関連曲を取得する (autopilot)。
    fn request_continuation(&mut self) {
        let Some(track) = self.player.current_queued().cloned() else {
            self.player.is_playing = false;
            return;
        };
        let Some(api) = self.api.clone() else {
            self.player.is_playing = false;
            return;
        };
        let rt = self.runtime.handle().clone();
        let path = format!(
            "/tracks/{}/related?limit=10&page=0",
            urlencoding::encode(&track.urn)
        );
        self.continuation = Query::default();
        self.continuation.request(&rt, async move {
            api.get_json(&path).await.map(|v| tracks_from_value(&v))
        });
    }

    /// autopilot の取得結果を回収して再生を継続する。
    pub fn poll_continuation(&mut self) {
        if self.continuation.poll() {
            if let Some(tracks) = self.continuation.data.take()
                && !tracks.is_empty()
            {
                // キューは維持したまま末尾へ足して次を再生する。
                self.append_and_play(tracks);
                return;
            }
            self.player.is_playing = false;
        }
    }

    /// いいね再生の継続を arm する (Tauri: `armLikesContinuation`)。
    pub fn arm_likes_continuation(&mut self) {
        self.continuation_source = Some(if self.player.shuffle {
            ContinuationSource::Shuffled {
                kind: ContinuationKind::Likes,
                buffer: Vec::new(),
                pos: 0,
                fetched: false,
                fetching: false,
            }
        } else {
            ContinuationSource::Paged {
                kind: ContinuationKind::Likes,
                next_page: self.player.queue.len() / LIKES_PAGE_SIZE,
                fetching: false,
            }
        });
    }

    /// プレイリスト再生の継続を arm する
    /// (Tauri: `armPlaylistContinuation`)。
    pub fn arm_playlist_continuation(&mut self, urn: String) {
        self.continuation_source = Some(if self.player.shuffle {
            ContinuationSource::Shuffled {
                kind: ContinuationKind::Playlist(urn),
                buffer: Vec::new(),
                pos: 0,
                fetched: false,
                fetching: false,
            }
        } else {
            ContinuationSource::Paged {
                kind: ContinuationKind::Playlist(urn),
                next_page: self.player.queue.len() / PLAYLIST_PAGE_SIZE,
                fetching: false,
            }
        });
    }

    /// いいね一括シャッフル (Tauri: `useShuffleLikes`)。
    /// 読み込み済みからランダム開始 → 継続を arm → 全件を先読みして追記。
    pub fn shuffle_likes(&mut self, tracks: Vec<Track>) {
        if tracks.is_empty() {
            return;
        }
        self.player.shuffle = true;
        let idx = rand_below(tracks.len());
        self.play_list(tracks, idx);
        self.arm_likes_continuation();
        self.start_likes_full_fetch();
    }

    /// プレイリストのシャッフル再生 (Tauri: `PlaylistPage.handleShuffle`)。
    pub fn shuffle_play_playlist(&mut self, tracks: Vec<Track>) {
        if tracks.is_empty() {
            return;
        }
        self.player.shuffle = true;
        let idx = rand_below(tracks.len());
        self.play_list(tracks, idx);
        if let Some(urn) = self.playlist.urn().map(str::to_string) {
            self.arm_playlist_continuation(urn);
        }
    }

    /// キュー終端 (repeat off): コンテキスト継続 → autopilot → 停止。
    fn continue_or_stop(&mut self) {
        if self.pump_source() {
            return;
        }
        self.fallback_after_source();
    }

    /// 継続ソース枯渇後: autopilot (関連曲) or 停止。
    fn fallback_after_source(&mut self) {
        if self.settings.autopilot {
            self.request_continuation();
        } else {
            self.player.is_playing = false;
        }
    }

    /// 継続ソースを1段進める。`true` = 継続処理中 or 再生開始済み。
    fn pump_source(&mut self) -> bool {
        let mut page_path: Option<String> = None;
        let mut fetch_all: Option<ContinuationKind> = None;
        let mut chunk: Option<Vec<Track>> = None;
        {
            match self.continuation_source.as_mut() {
                None => return false,
                Some(ContinuationSource::Paged {
                    kind,
                    next_page,
                    fetching,
                }) => {
                    if *fetching {
                        return true;
                    }
                    let path = match kind {
                        ContinuationKind::Likes => {
                            format!("/me/likes/tracks?limit={LIKES_PAGE_SIZE}&page={next_page}")
                        }
                        ContinuationKind::Playlist(urn) => format!(
                            "/playlists/{}/tracks?limit={PLAYLIST_PAGE_SIZE}&page={next_page}",
                            urlencoding::encode(urn)
                        ),
                    };
                    *next_page += 1;
                    *fetching = true;
                    page_path = Some(path);
                }
                Some(ContinuationSource::Shuffled {
                    kind,
                    buffer,
                    pos,
                    fetched,
                    fetching,
                }) => {
                    if *fetched {
                        let end = (*pos + LIKES_PAGE_SIZE).min(buffer.len());
                        let part: Vec<Track> = buffer[*pos..end].to_vec();
                        *pos = end;
                        if !part.is_empty() {
                            chunk = Some(part);
                        }
                    } else if !*fetching {
                        *fetching = true;
                        fetch_all = Some(kind.clone());
                    } else {
                        return true;
                    }
                }
            }
        }
        if let Some(part) = chunk {
            self.append_and_play(part);
            return true;
        }
        if let Some(kind) = fetch_all {
            self.start_shuffled_source_fetch(kind);
            return true;
        }
        if let Some(path) = page_path {
            self.start_source_fetch(path);
            return true;
        }
        // ソース枯渇 → autopilot / 停止。
        self.continuation_source = None;
        self.fallback_after_source();
        true
    }

    /// 継続分をキューへ足して次を再生する (Tauri: addToQueue + next)。
    fn append_and_play(&mut self, fresh: Vec<Track>) {
        let Some(queue_index) = self.player.queue_index else {
            self.player.append_queue(fresh);
            if !self.player.queue.is_empty() {
                self.play_queue_index(0);
            }
            return;
        };
        self.player.append_queue(fresh);
        let next = queue_index + 1;
        if next < self.player.queue.len() {
            self.play_queue_index(next);
        }
    }

    /// 継続ページ (paged ソース) を取得する。
    fn start_source_fetch(&mut self, path: String) {
        let Some(api) = self.api.clone() else {
            self.continuation_source = None;
            self.fallback_after_source();
            return;
        };
        let rt = self.runtime.handle().clone();
        self.source_fetch = Query::default();
        self.source_fetch.request(&rt, async move {
            let v = api.get_json(&path).await?;
            Ok(source_page_from_value(&v))
        });
    }

    /// shuffle ソース用: 全件を 200 件ずつ取得する (遅延)。
    fn start_shuffled_source_fetch(&mut self, kind: ContinuationKind) {
        let Some(api) = self.api.clone() else {
            self.continuation_source = None;
            self.fallback_after_source();
            return;
        };
        let rt = self.runtime.handle().clone();
        self.source_fetch = Query::default();
        self.source_fetch.request(&rt, async move {
            let mut all: Vec<Track> = Vec::new();
            let mut page = 0usize;
            loop {
                let path = match &kind {
                    ContinuationKind::Likes => {
                        format!("/me/likes/tracks?limit=200&page={page}")
                    }
                    ContinuationKind::Playlist(urn) => format!(
                        "/playlists/{}/tracks?limit=200&page={page}",
                        urlencoding::encode(urn)
                    ),
                };
                let v = api.get_json(&path).await?;
                let has_more = v.get("has_more").and_then(|b| b.as_bool()).unwrap_or(false);
                all.extend(tracks_from_value(&v));
                page += 1;
                if !has_more || page >= 100 {
                    break;
                }
            }
            Ok(SourcePage {
                tracks: all,
                has_more: false,
            })
        });
    }

    /// 継続ソースの取得結果を回収する。
    pub fn poll_source_fetch(&mut self) {
        if !self.source_fetch.poll() {
            return;
        }
        let fetched = self.source_fetch.data.take();
        // fetching フラグを解除。
        match self.continuation_source.as_mut() {
            Some(ContinuationSource::Paged { fetching, .. })
            | Some(ContinuationSource::Shuffled { fetching, .. }) => *fetching = false,
            None => return,
        }
        let Some(page) = fetched else {
            // 取得失敗 → ソース破棄 (Tauri と同じ: 停止系へ)。
            self.continuation_source = None;
            self.fallback_after_source();
            return;
        };
        let queued: std::collections::HashSet<String> =
            self.player.queue.iter().map(|t| t.urn.clone()).collect();
        let shuffled = matches!(
            self.continuation_source,
            Some(ContinuationSource::Shuffled { .. })
        );
        if shuffled {
            let mut buffer: Vec<Track> = page
                .tracks
                .into_iter()
                .filter(|t| !queued.contains(&t.urn))
                .collect();
            shuffle_tracks(&mut buffer);
            if let Some(ContinuationSource::Shuffled {
                buffer: slot,
                fetched: done,
                pos,
                ..
            }) = self.continuation_source.as_mut()
            {
                *slot = buffer;
                *done = true;
                *pos = 0;
            }
            self.pump_source();
            return;
        }
        let fresh: Vec<Track> = page
            .tracks
            .into_iter()
            .filter(|t| !queued.contains(&t.urn))
            .collect();
        if !fresh.is_empty() {
            if !page.has_more {
                self.continuation_source = None;
            }
            self.append_and_play(fresh);
        } else if page.has_more {
            // 全ページ重複 → 次ページへ。
            self.pump_source();
        } else {
            self.continuation_source = None;
            self.fallback_after_source();
        }
    }

    /// shuffle いいね: 全件を先読みして残りをキューへ足す
    /// (Tauri: `useShuffleLikes` の `fetchAllLikedTracks`)。
    pub fn start_likes_full_fetch(&mut self) {
        let Some(api) = self.api.clone() else {
            return;
        };
        let rt = self.runtime.handle().clone();
        self.likes_full_generation = self.queue_generation;
        self.likes_full = Query::default();
        self.likes_full.request(&rt, async move {
            let mut all: Vec<Track> = Vec::new();
            let mut page = 0usize;
            loop {
                let path = format!("/me/likes/tracks?limit=200&page={page}");
                let v = api.get_json(&path).await?;
                let has_more = v.get("has_more").and_then(|b| b.as_bool()).unwrap_or(false);
                all.extend(tracks_from_value(&v));
                page += 1;
                if !has_more || page >= 100 {
                    break;
                }
            }
            Ok(all)
        });
    }

    /// 全件先読みの結果をキューへ追記する (新しい play 後は破棄)。
    pub fn poll_likes_full_fetch(&mut self) {
        if !self.likes_full.poll() {
            return;
        }
        let Some(all) = self.likes_full.data.take() else {
            return;
        };
        if self.likes_full_generation != self.queue_generation {
            return;
        }
        let queued: std::collections::HashSet<String> =
            self.player.queue.iter().map(|t| t.urn.clone()).collect();
        let rest: Vec<Track> = all
            .into_iter()
            .filter(|t| !queued.contains(&t.urn))
            .collect();
        self.player.append_queue(rest);
    }

    /// Discover 棚のアイテムを解決して再生/遷移する
    /// (Tauri 版 `DiscoverSections.startDiscoverItem` 相当)。
    pub fn start_discover(&mut self, item: crate::backend::models::DiscoverItem) {
        let Some(urn) = item.urn.clone() else {
            return;
        };
        // アーティストはプロフィールへ。
        if item.kind.as_deref() == Some("user") || urn.starts_with("soundcloud:users:") {
            self.route = Route::User;
            self.nav_param = Some(urn);
            return;
        }
        // ジャンル項目は Tag ページへ (「Trending by genre」等)。
        if let Some(genre) = urn.strip_prefix("soundcloud:genres:") {
            self.route = Route::Tag;
            self.nav_param = Some(genre.to_string());
            return;
        }
        let digits_after = |needle: &str| -> Option<String> {
            let (_, rest) = urn.split_once(needle)?;
            let id: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            (!id.is_empty()).then_some(id)
        };
        let station_user = digits_after("artist-stations:");
        let station_track = digits_after("track-stations:");
        let is_station = station_user.is_some()
            || station_track.is_some()
            || item.playlist_type.as_deref() == Some("ARTIST_STATION");
        if is_station {
            let Some(api) = self.api.clone() else {
                return;
            };
            let rt = self.runtime.handle().clone();
            self.discover_play = Query::default();
            self.discover_play.request(&rt, async move {
                let mut list: Vec<Track> = Vec::new();
                if let Some(id) = station_user {
                    if let Ok(v) = api
                        .get_json(&format!("/users/{id}/tracks?limit=50&offset=0"))
                        .await
                    {
                        list = tracks_from_value(&v);
                        shuffle_tracks(&mut list);
                    }
                } else if let Some(id) = station_track {
                    if let Ok(v) = api
                        .get_json(&format!("/tracks/{id}/related?limit=30&offset=0"))
                        .await
                    {
                        list = tracks_from_value(&v);
                    }
                }
                Ok(list)
            });
            return;
        }
        // SoundCloud のシステムミックス (Your Mix 等) はその場で再生。
        if urn.starts_with("soundcloud:system-playlists:") {
            let Some(api) = self.api.clone() else {
                return;
            };
            let rt = self.runtime.handle().clone();
            self.discover_play = Query::default();
            self.discover_play.request(&rt, async move {
                let v = api
                    .get_json(&format!("/system-playlists/{}", urlencoding::encode(&urn)))
                    .await?;
                let raw = v.get("tracks").cloned().unwrap_or(serde_json::Value::Null);
                let list = if raw.is_array() {
                    tracks_from_value(&raw)
                } else {
                    tracks_from_value(
                        &raw.get("collection")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null),
                    )
                };
                Ok(list)
            });
            return;
        }
        // 通常のプレイリストはページへ。
        if urn.starts_with("soundcloud:playlists:") {
            self.route = Route::Playlist;
            self.nav_param = Some(urn);
        }
    }

    /// Discover 再生の解決結果を回収する。
    pub fn poll_discover_play(&mut self) {
        if self.discover_play.poll() {
            if let Some(tracks) = self.discover_play.data.take()
                && !tracks.is_empty()
            {
                self.play_list(tracks, 0);
            }
        }
    }

    /// トレイからの操作を回収する (UI スレッド)。
    pub fn poll_tray(&mut self, ctx: &egui::Context) {
        let cmds: Vec<crate::tray::TrayCmd> = self
            .tray_rx
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for cmd in cmds {
            match cmd {
                crate::tray::TrayCmd::ToggleWindow => {
                    self.window_hidden = !self.window_hidden;
                    if self.window_hidden {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
                crate::tray::TrayCmd::PlayPause => {
                    if let Some(audio) = self.audio().cloned() {
                        if self.player.is_playing {
                            crate::backend::audio::engine::pause(&audio);
                            self.player.is_playing = false;
                        } else {
                            crate::backend::audio::engine::play(&audio);
                            self.player.is_playing = true;
                        }
                    }
                }
                crate::tray::TrayCmd::Next => self.next_track(),
                crate::tray::TrayCmd::Prev => {
                    if let Some(audio) = self.audio().cloned() {
                        let pos = crate::backend::audio::engine::get_position(&audio);
                        self.prev_track(pos);
                    }
                }
                crate::tray::TrayCmd::Quit => {
                    self.force_quit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
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
    fn insert_next_places_after_current() {
        let mut s = queued(3);
        s.queue_index = Some(1);
        s.insert_next(vec![track("extra")]);
        assert_eq!(s.queue.len(), 4);
        assert_eq!(s.queue[2].urn, "extra");
        assert_eq!(s.queue_index, Some(1));
    }

    #[test]
    fn insert_next_appends_when_idle() {
        let mut s = PlayerState::default();
        s.insert_next(vec![track("a"), track("b")]);
        assert_eq!(s.queue.len(), 2);
        assert_eq!(s.queue_index, None);
    }

    #[test]
    fn move_queue_item_follows_current() {
        let mut s = queued(4);
        s.queue_index = Some(1);
        s.move_queue_item(1, 3);
        assert_eq!(s.queue[3].urn, "urn-1");
        assert_eq!(s.queue_index, Some(3));
        assert_eq!(s.current_title.as_deref(), Some("t-urn-1"));
    }

    #[test]
    fn move_queue_item_shifts_index_when_crossing() {
        let mut s = queued(4);
        s.queue_index = Some(2);
        s.move_queue_item(0, 3);
        assert_eq!(s.queue[3].urn, "urn-0");
        assert_eq!(s.queue_index, Some(1));
        assert_eq!(s.queue[1].urn, "urn-2");
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
    fn toggle_shuffle_shuffles_tail_and_restores() {
        let mut s = PlayerState::default();
        let tracks: Vec<Track> = ["a", "b", "c", "d", "e"].iter().map(|u| track(u)).collect();
        s.set_queue(tracks, 0);
        s.toggle_shuffle();
        assert!(s.shuffle);
        // 現在位置までは保持、全要素は失われない。
        assert_eq!(s.queue[0].urn, "a");
        let mut urns: Vec<String> = s.queue.iter().map(|t| t.urn.clone()).collect();
        urns.sort();
        assert_eq!(urns, ["a", "b", "c", "d", "e"]);
        // OFF で元の並びに戻る。
        s.toggle_shuffle();
        assert!(!s.shuffle);
        let urns: Vec<String> = s.queue.iter().map(|t| t.urn.clone()).collect();
        assert_eq!(urns, ["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn set_queue_with_shuffle_puts_chosen_first() {
        let mut s = PlayerState::default();
        s.shuffle = true;
        let tracks: Vec<Track> = ["a", "b", "c", "d"].iter().map(|u| track(u)).collect();
        s.set_queue(tracks, 2);
        assert_eq!(s.queue[0].urn, "c");
        assert_eq!(s.queue_index, Some(0));
        assert_eq!(s.queue.len(), 4);
        assert!(s.original_queue.is_some());
    }

    #[test]
    fn next_is_sequential_under_shuffle() {
        let mut s = queued(3);
        s.shuffle = true;
        s.queue_index = Some(1);
        assert_eq!(s.next_index(), Some(2));
        s.queue_index = Some(2);
        assert_eq!(s.next_index(), None);
    }
}
