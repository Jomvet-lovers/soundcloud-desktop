//! wry host: login window + hidden writer window.
//!
//! Tauri の WebviewWindow 層の代替。プロセス分離方式:
//! - 親プロセス (eframe): `HostProxy` (async) で操作。tao/wry には触らない。
//! - 子プロセス (`--wry-host`): メインスレッドで tao loop + 2 window を回し、
//!   TCP (JSON-RPC) で要求を受ける。どちらも main スレッドで GUI を作る必要が
//!   ある環境向け (同一プロセス内の二重 loop は不可)。
//! 2 window は同一 WebContext (永続プロファイル) を共有するため、
//! ログインセッションと writer は Tauri 版と同じ cookie jar を見る。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use serde_json::{json, Value};
use tokio::sync::oneshot;

static HOST: OnceLock<RwLock<Option<HostProxy>>> = OnceLock::new();

fn slot() -> &'static RwLock<Option<HostProxy>> {
    HOST.get_or_init(|| RwLock::new(None))
}

pub fn host() -> Option<HostProxy> {
    let h = slot().read().ok().and_then(|g| g.clone())?;
    if h.alive.load(std::sync::atomic::Ordering::Relaxed) {
        return Some(h);
    }
    // アイドル終了した子の残骸は捨てる (次の ensure_host が再生成する)。
    clear_host();
    None
}

pub fn set_host(proxy: HostProxy) {
    if let Ok(mut g) = slot().write() {
        *g = Some(proxy);
    }
}

pub fn clear_host() {
    if let Ok(mut g) = slot().write() {
        *g = None;
    }
}

pub const LOGIN_HOME: &str = "https://soundcloud.com/signin";
pub const WRITER_HOME: &str = "https://soundcloud.com/404";
const COOKIE_URL: &str = "https://soundcloud.com/";

type Reply<T> = oneshot::Sender<T>;

/// WebView2 の環境引数。環境は最初に作られた webview の指定で固定される。
/// 非表示の writer とログイン窓しか使わないため、既定の軽量化フラグのみ渡す。
const BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// Windows (WebView2) のみ browser args を適用する。他 OS では素通し。
fn with_browser_args(builder: wry::WebViewBuilder<'_>) -> wry::WebViewBuilder<'_> {
    #[cfg(windows)]
    {
        use wry::WebViewBuilderExtWindows;
        builder.with_additional_browser_args(BROWSER_ARGS)
    }
    #[cfg(not(windows))]
    {
        builder
    }
}

#[derive(Clone, Copy)]
enum Target {
    Login,
    Writer,
}

fn target_name(t: Target) -> &'static str {
    match t {
        Target::Login => "login",
        Target::Writer => "writer",
    }
}

fn target_of(name: &str) -> Option<Target> {
    match name {
        "login" => Some(Target::Login),
        "writer" => Some(Target::Writer),
        _ => None,
    }
}

enum HostCmd {
    Eval {
        target: Target,
        js: String,
        reply: Reply<Result<(), String>>,
    },
    Cookies {
        target: Target,
        reply: Reply<Result<Vec<(String, String)>, String>>,
    },
    SetCookie {
        target: Target,
        name: String,
        value: String,
        reply: Reply<Result<(), String>>,
    },
    ClearBrowsing {
        target: Target,
        reply: Reply<Result<(), String>>,
    },
    Navigate {
        target: Target,
        url: String,
        reply: Reply<Result<(), String>>,
    },
    CurrentUrl {
        target: Target,
        reply: Reply<Result<String, String>>,
    },
    Show {
        target: Target,
    },
    Hide {
        target: Target,
    },
    Focus {
        target: Target,
    },
    SetOnTop {
        target: Target,
        top: bool,
    },
    IsVisible {
        target: Target,
        reply: Reply<Result<bool, String>>,
    },
}

struct RpcClient {
    writer: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    next_id: AtomicU64,
    pending: Arc<std::sync::Mutex<HashMap<u64, Reply<Value>>>>,
}

#[derive(Clone)]
pub struct HostProxy {
    rpc: Arc<RpcClient>,
    /// 子プロセスの生存 (TCP が切れたら false)。アイドル終了の検知に使う。
    alive: Arc<std::sync::atomic::AtomicBool>,
}

impl HostProxy {
    fn from_stream(stream: std::net::TcpStream) -> Result<Self, String> {
        stream
            .set_nonblocking(true)
            .map_err(|e| format!("tcp: {e}"))?;
        let tokio_stream = tokio::net::TcpStream::from_std(stream)
            .map_err(|e| format!("tcp: {e}"))?;
        let (reader, writer) = tokio_stream.into_split();
        let rpc = Arc::new(RpcClient {
            writer: Arc::new(tokio::sync::Mutex::new(writer)),
            next_id: AtomicU64::new(1),
            pending: Arc::new(std::sync::Mutex::new(HashMap::new())),
        });
        let pending = rpc.pending.clone();
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let alive_reader = alive.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(reader).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let v: Value = match serde_json::from_str(&line) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                            if let Some(tx) =
                                pending.lock().map(|mut p| p.remove(&id)).unwrap_or(None)
                            {
                                let _ = tx.send(v);
                            }
                        }
                    }
                    _ => break,
                }
            }
            // 子が死んだ: 残リクエストを全て失敗させる。
            alive_reader.store(false, std::sync::atomic::Ordering::Relaxed);
            if let Ok(mut p) = pending.lock() {
                for (_, tx) in p.drain() {
                    let _ = tx.send(json!({"ok": false, "error": "wry host gone"}));
                }
            }
        });
        Ok(Self { rpc, alive })
    }

    /// JSON-RPC 往復。応答は `{"ok":bool,"result":...,"error":...}`。
    async fn call(
        &self,
        op: &str,
        target: Target,
        params: serde_json::Map<String, Value>,
    ) -> Result<Value, String> {
        let id = self.rpc.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.rpc
            .pending
            .lock()
            .map_err(|_| "lock".to_string())?
            .insert(id, tx);
        let mut req = serde_json::Map::new();
        req.insert("id".into(), json!(id));
        req.insert("op".into(), Value::String(op.into()));
        req.insert("target".into(), Value::String(target_name(target).into()));
        for (k, v) in params {
            req.insert(k, v);
        }
        let line = serde_json::Value::Object(req).to_string() + "\n";
        {
            let mut w = self.rpc.writer.lock().await;
            tokio::io::AsyncWriteExt::write_all(&mut *w, line.as_bytes())
                .await
                .map_err(|e| {
                    self.rpc
                        .pending
                        .lock()
                        .map(|mut p| p.remove(&id))
                        .ok();
                    format!("tcp: {e}")
                })?;
        }
        let resp = rx.await.map_err(|_| "wry host gone".to_string())?;
        if resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            Ok(resp.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(resp
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("rpc error")
                .to_string())
        }
    }

    fn str_param(params: &mut serde_json::Map<String, Value>, key: &str, value: &str) {
        params.insert(key.into(), Value::String(value.into()));
    }

    async fn eval(&self, target: Target, js: &str) -> Result<(), String> {
        let mut p = serde_json::Map::new();
        Self::str_param(&mut p, "js", js);
        self.call("eval", target, p).await.map(|_| ())
    }

    async fn cookies(&self, target: Target) -> Result<Vec<(String, String)>, String> {
        let v = self
            .call("cookies", target, serde_json::Map::new())
            .await?;
        let arr = v.as_array().cloned().unwrap_or_default();
        Ok(arr
            .into_iter()
            .filter_map(|e| {
                let a = e.as_array()?;
                Some((a.first()?.as_str()?.to_string(), a.get(1)?.as_str()?.to_string()))
            })
            .collect())
    }

    async fn set_cookie(&self, target: Target, name: &str, value: &str) -> Result<(), String> {
        let mut p = serde_json::Map::new();
        Self::str_param(&mut p, "name", name);
        Self::str_param(&mut p, "value", value);
        self.call("set_cookie", target, p).await.map(|_| ())
    }

    async fn clear_browsing(&self, target: Target) -> Result<(), String> {
        self.call("clear", target, serde_json::Map::new())
            .await
            .map(|_| ())
    }

    async fn navigate(&self, target: Target, url: &str) -> Result<(), String> {
        let mut p = serde_json::Map::new();
        Self::str_param(&mut p, "url", url);
        self.call("navigate", target, p).await.map(|_| ())
    }

    async fn current_url(&self, target: Target) -> Result<String, String> {
        let v = self
            .call("url", target, serde_json::Map::new())
            .await?;
        v.as_str()
            .map(str::to_string)
            .ok_or_else(|| "bad url reply".to_string())
    }

    async fn fire(&self, op: &str, target: Target, params: serde_json::Map<String, Value>) {
        let _ = self.call(op, target, params).await;
    }

    async fn is_visible(&self, target: Target) -> Result<bool, String> {
        let v = self
            .call("visible", target, serde_json::Map::new())
            .await?;
        v.as_bool().ok_or_else(|| "bad visible reply".to_string())
    }

    pub async fn login_eval(&self, js: &str) -> Result<(), String> {
        self.eval(Target::Login, js).await
    }

    pub async fn login_cookies(&self) -> Result<Vec<(String, String)>, String> {
        self.cookies(Target::Login).await
    }

    pub async fn login_clear(&self) -> Result<(), String> {
        self.clear_browsing(Target::Login).await
    }

    pub async fn login_navigate(&self, url: &str) -> Result<(), String> {
        self.navigate(Target::Login, url).await
    }

    pub async fn login_visible(&self) -> Result<bool, String> {
        self.is_visible(Target::Login).await
    }

    pub async fn login_show(&self) {
        self.fire("show", Target::Login, serde_json::Map::new())
            .await;
    }

    pub async fn login_hide(&self) {
        self.fire("hide", Target::Login, serde_json::Map::new())
            .await;
    }

    pub async fn login_focus(&self) {
        self.fire("focus", Target::Login, serde_json::Map::new())
            .await;
    }

    /// 閉じる = 隠す + 白紙化 (window 自体は使い回す)。
    pub async fn login_close(&self) {
        self.login_hide().await;
        let _ = self.login_navigate("about:blank").await;
    }

    pub async fn writer_eval(&self, js: &str) -> Result<(), String> {
        self.eval(Target::Writer, js).await
    }

    pub async fn writer_cookies(&self) -> Result<Vec<(String, String)>, String> {
        self.cookies(Target::Writer).await
    }

    pub async fn writer_set_session(&self, token: &str) -> Result<(), String> {
        self.set_cookie(Target::Writer, "oauth_token", token).await
    }

    pub async fn writer_navigate(&self, url: &str) -> Result<(), String> {
        self.navigate(Target::Writer, url).await
    }

    pub async fn writer_url(&self) -> Result<String, String> {
        self.current_url(Target::Writer).await
    }

    pub async fn writer_show(&self) {
        self.fire("show", Target::Writer, serde_json::Map::new())
            .await;
    }

    pub async fn writer_hide(&self) {
        self.fire("hide", Target::Writer, serde_json::Map::new())
            .await;
    }

    pub async fn writer_focus(&self) {
        self.fire("focus", Target::Writer, serde_json::Map::new())
            .await;
    }

    pub async fn writer_on_top(&self, top: bool) {
        let mut p = serde_json::Map::new();
        p.insert("top".into(), Value::Bool(top));
        self.fire("ontop", Target::Writer, p).await;
    }

    pub async fn writer_visible(&self) -> Result<bool, String> {
        self.is_visible(Target::Writer).await
    }
}

/// 子プロセスを起動し、wry host に接続する。失敗時は degraded (None 相当)。
/// 戻り値は接続済み proxy と子プロセスハンドル (終了時に kill する)。
/// 接続専用の小 runtime をリークする (プロセス寿命に紐づく IPC のため)。
pub fn spawn_child(profile_dir: PathBuf) -> Result<(HostProxy, std::process::Child), String> {
    let exe = std::env::current_exe().map_err(|e| format!("exe: {e}"))?;
    let mut child = std::process::Command::new(exe)
        .arg("--wry-host")
        .arg(&profile_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn: {e}"))?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = std::sync::mpsc::channel::<Result<u16, String>>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                let _ = tx.send(Err("eof".into()));
            }
            Ok(_) => {
                let port = line
                    .trim()
                    .strip_prefix("WRY_PORT=")
                    .and_then(|s| s.parse::<u16>().ok());
                let _ = tx.send(port.ok_or_else(|| format!("bad line: {line}")));
            }
            Err(e) => {
                let _ = tx.send(Err(format!("read: {e}")));
            }
        }
    });
    let port = match rx.recv_timeout(std::time::Duration::from_secs(90)) {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            let _ = child.kill();
            return Err(e);
        }
        Err(_) => {
            let _ = child.kill();
            return Err("wry host timeout".to_string());
        }
    };
    let rt: &'static tokio::runtime::Runtime = Box::leak(Box::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| format!("rt: {e}"))?,
    ));
    let proxy = match rt.block_on(HostProxy::connect(port)) {
        Ok(p) => p,
        Err(e) => {
            let _ = child.kill();
            return Err(e);
        }
    };
    Ok((proxy, child))
}

static SPAWN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// wry host を必要時に起動する。アイドル終了した子はここで再生成される。
/// 呼び出しは書き込み (writer) とログインの直前のみ。常駐させない。
pub async fn ensure_host() -> Result<HostProxy, String> {
    if let Some(h) = host() {
        return Ok(h);
    }
    let _guard = SPAWN_LOCK.lock().await;
    if let Some(h) = host() {
        return Ok(h);
    }
    let dir = crate::backend::paths::app_cache_dir().join("webprofile");
    let spawned = tokio::task::spawn_blocking(move || spawn_child(dir))
        .await
        .map_err(|e| format!("spawn join: {e}"))?;
    match spawned {
        Ok((proxy, child)) => {
            // 子は親の TCP が切れると自分で終了する。ハンドルは保持しない。
            drop(child);
            set_host(proxy.clone());
            Ok(proxy)
        }
        Err(e) => Err(e),
    }
}

impl HostProxy {
    async fn connect(port: u16) -> Result<Self, String> {
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .map_err(|e| format!("tcp: {e}"))?;
        let (reader, writer) = stream.into_split();
        let rpc = Arc::new(RpcClient {
            writer: Arc::new(tokio::sync::Mutex::new(writer)),
            next_id: AtomicU64::new(1),
            pending: Arc::new(std::sync::Mutex::new(HashMap::new())),
        });
        let pending = rpc.pending.clone();
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let alive_reader = alive.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(reader).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let v: Value = match serde_json::from_str(&line) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                            if let Some(tx) =
                                pending.lock().map(|mut p| p.remove(&id)).unwrap_or(None)
                            {
                                let _ = tx.send(v);
                            }
                        }
                    }
                    _ => break,
                }
            }
            alive_reader.store(false, std::sync::atomic::Ordering::Relaxed);
            if let Ok(mut p) = pending.lock() {
                for (_, tx) in p.drain() {
                    let _ = tx.send(json!({"ok": false, "error": "wry host gone"}));
                }
            }
        });
        Ok(Self { rpc, alive })
    }
}

/// 子プロセス (`--wry-host <profile>`) の本体。戻らない。
pub fn run_child_main(profile_dir: PathBuf) -> ! {
    use std::io::Write;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");

    let event_loop = tao::event_loop::EventLoopBuilder::<HostCmd>::with_user_event().build();
    let loop_proxy = event_loop.create_proxy();

    let mut context = wry::WebContext::new(Some(profile_dir));
    eprintln!("[wry-host] webcontext ok");
    // writer は常設 (書き込み同期に必要)。ログイン窓は必要になるまで作らない
    // (サインイン済みの通常利用では signin の SPA を読み込まない)。
    let writer_window = tao::window::WindowBuilder::new()
        .with_title("SoundCloud session")
        .with_inner_size(tao::dpi::LogicalSize::new(520.0, 680.0))
        .with_visible(false)
        .build(&event_loop)
        .expect("writer window");
    eprintln!("[wry-host] writer window ok");
    let writer_wv = with_browser_args(
        wry::WebViewBuilder::new_with_web_context(&mut context).with_url(WRITER_HOME),
    )
    .build(&writer_window)
    .expect("writer webview");
    eprintln!("[wry-host] writer webview ok");

    // NOTE: bind/port 通知は window 構築の後。親はこの行を読んでから接続する。
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    println!("WRY_PORT={port}");
    let _ = std::io::stdout().flush();

    // 親の接続を待つ (タイムアウト付き: 親が死んだ場合の孤児化を防ぐ)。
    listener.set_nonblocking(true).expect("nonblocking");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let stream = loop {
        match listener.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    eprintln!("[wry-host] parent did not connect; exiting");
                    std::process::exit(1);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => panic!("accept: {e}"),
        }
    };
    stream.set_nonblocking(false).expect("blocking");

    let rt_handle = rt.handle().clone();
    let pump_proxy = loop_proxy.clone();
    std::thread::spawn(move || pump_tcp(stream, pump_proxy, rt_handle));

    let mut state = ChildState {
        context,
        login: None,
        writer_window,
        writer_wv,
        login_visible: false,
        writer_visible: false,
    };
    // 書き込み/ログインが無いままこの時間が経つと子プロセスを終了する
    // (常駐 WebView2 をゼロにする)。次の必要時に親が ensure_host で再生成する。
    const IDLE_EXIT: std::time::Duration = std::time::Duration::from_secs(90);
    let mut last_activity = std::time::Instant::now();

    event_loop.run(move |event, event_loop, control_flow| {
        use tao::event::{Event, StartCause, WindowEvent};
        match event {
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                if !state.login_visible
                    && !state.writer_visible
                    && last_activity.elapsed() >= IDLE_EXIT
                {
                    eprintln!("[wry-host] idle; exiting");
                    std::process::exit(0);
                }
            }
            Event::UserEvent(cmd) => {
                last_activity = std::time::Instant::now();
                handle_cmd(&mut state, event_loop, cmd);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
                ..
            } => {
                if state.login.as_ref().map(|(w, _)| w.id()) == Some(window_id) {
                    if let Some((w, _)) = &state.login {
                        w.set_visible(false);
                    }
                    state.login_visible = false;
                } else if state.writer_window.id() == window_id {
                    state.writer_window.set_visible(false);
                    state.writer_visible = false;
                }
            }
            _ => {}
        }
        // 見えている窓がある間は終了しない。それ以外は最後の操作から
        // IDLE_EXIT 後に起床して終了判定する。
        *control_flow = if state.login_visible || state.writer_visible {
            tao::event_loop::ControlFlow::Wait
        } else {
            tao::event_loop::ControlFlow::WaitUntil(last_activity + IDLE_EXIT)
        };
    });
}

/// 子プロセスの webview 状態 (login は遅延生成)。
struct ChildState {
    context: wry::WebContext,
    login: Option<(tao::window::Window, wry::WebView)>,
    writer_window: tao::window::Window,
    writer_wv: wry::WebView,
    login_visible: bool,
    writer_visible: bool,
}

fn cmd_targets_login(cmd: &HostCmd) -> bool {
    let t = match cmd {
        HostCmd::Eval { target, .. }
        | HostCmd::Cookies { target, .. }
        | HostCmd::SetCookie { target, .. }
        | HostCmd::ClearBrowsing { target, .. }
        | HostCmd::Navigate { target, .. }
        | HostCmd::CurrentUrl { target, .. }
        | HostCmd::Show { target }
        | HostCmd::Hide { target }
        | HostCmd::Focus { target }
        | HostCmd::SetOnTop { target, .. }
        | HostCmd::IsVisible { target, .. } => target,
    };
    matches!(t, Target::Login)
}

/// 遅延生成: ログイン窓 + webview を作る。
fn ensure_login(
    event_loop: &tao::event_loop::EventLoopWindowTarget<HostCmd>,
    context: &mut wry::WebContext,
    login: &mut Option<(tao::window::Window, wry::WebView)>,
) -> Result<(), String> {
    if login.is_some() {
        return Ok(());
    }
    let window = tao::window::WindowBuilder::new()
        .with_title("SoundCloud")
        .with_inner_size(tao::dpi::LogicalSize::new(1000.0, 800.0))
        .with_visible(false)
        .build(event_loop)
        .map_err(|e| format!("login window: {e}"))?;
    let wv = with_browser_args(
        wry::WebViewBuilder::new_with_web_context(context).with_url(LOGIN_HOME),
    )
    .build(&window)
    .map_err(|e| format!("login webview: {e}"))?;
    *login = Some((window, wv));
    eprintln!("[wry-host] login window created (lazy)");
    Ok(())
}

/// コマンドを処理する。ログイン webview が必要なら先に作る。
fn handle_cmd(
    state: &mut ChildState,
    event_loop: &tao::event_loop::EventLoopWindowTarget<HostCmd>,
    cmd: HostCmd,
) {
    if state.login.is_none() && cmd_targets_login(&cmd) {
        match cmd {
            // 照会系は生成せずに既定値を返す。
            HostCmd::IsVisible { reply, .. } => {
                let _ = reply.send(Ok(false));
                return;
            }
            HostCmd::Hide { .. } => {
                state.login_visible = false;
                return;
            }
            cmd => {
                if let Err(e) = ensure_login(event_loop, &mut state.context, &mut state.login) {
                    eprintln!("[wry-host] {e}");
                }
                handle_cmd_ready(state, cmd);
            }
        }
        return;
    }
    handle_cmd_ready(state, cmd);
}

/// 対象の webview (ログイン未生成なら None)。
fn wv_of<'a>(state: &'a ChildState, target: Target) -> Option<&'a wry::WebView> {
    match target {
        Target::Login => state.login.as_ref().map(|(_, wv)| wv),
        Target::Writer => Some(&state.writer_wv),
    }
}

/// 対象のウィンドウ (ログイン未生成なら None)。
fn win_of<'a>(state: &'a ChildState, target: Target) -> Option<&'a tao::window::Window> {
    match target {
        Target::Login => state.login.as_ref().map(|(w, _)| w),
        Target::Writer => Some(&state.writer_window),
    }
}

fn handle_cmd_ready(state: &mut ChildState, cmd: HostCmd) {
    match cmd {
        HostCmd::Eval { target, js, reply } => {
            let out = match wv_of(state, target) {
                Some(w) => w.evaluate_script(&js).map_err(|e| e.to_string()),
                None => Err("login webview not open".into()),
            };
            let _ = reply.send(out);
        }
        HostCmd::Cookies { target, reply } => {
            let out = match wv_of(state, target) {
                Some(w) => w
                    .cookies_for_url(COOKIE_URL)
                    .map(|cs| {
                        cs.into_iter()
                            .map(|c| (c.name().to_string(), c.value().to_string()))
                            .collect()
                    })
                    .map_err(|e| e.to_string()),
                None => Err("login webview not open".into()),
            };
            let _ = reply.send(out);
        }
        HostCmd::SetCookie {
            target,
            name,
            value,
            reply,
        } => {
            let out = match wv_of(state, target) {
                Some(w) => {
                    let cookie = cookie::Cookie::build((name, value))
                        .domain(".soundcloud.com")
                        .path("/")
                        .secure(true)
                        .http_only(true)
                        .build();
                    w.set_cookie(&cookie).map_err(|e| e.to_string())
                }
                None => Err("login webview not open".into()),
            };
            let _ = reply.send(out);
        }
        HostCmd::ClearBrowsing { target, reply } => {
            let out = match wv_of(state, target) {
                Some(w) => w.clear_all_browsing_data().map_err(|e| e.to_string()),
                None => Err("login webview not open".into()),
            };
            let _ = reply.send(out);
        }
        HostCmd::Navigate { target, url, reply } => {
            let out = match wv_of(state, target) {
                Some(w) => w.load_url(&url).map_err(|e| e.to_string()),
                None => Err("login webview not open".into()),
            };
            let _ = reply.send(out);
        }
        HostCmd::CurrentUrl { target, reply } => {
            let out = match wv_of(state, target) {
                Some(w) => w.url().map_err(|e| e.to_string()),
                None => Err("login webview not open".into()),
            };
            let _ = reply.send(out);
        }
        HostCmd::Show { target } => {
            if let Some(w) = win_of(state, target) {
                w.set_visible(true);
            }
            match target {
                Target::Login => state.login_visible = true,
                Target::Writer => state.writer_visible = true,
            }
        }
        HostCmd::Hide { target } => {
            if let Some(w) = win_of(state, target) {
                w.set_visible(false);
            }
            match target {
                Target::Login => state.login_visible = false,
                Target::Writer => state.writer_visible = false,
            }
        }
        HostCmd::Focus { target } => {
            if let Some(w) = win_of(state, target) {
                w.set_focus();
            }
        }
        HostCmd::SetOnTop { target, top } => {
            if let Some(w) = win_of(state, target) {
                w.set_always_on_top(top);
            }
        }
        HostCmd::IsVisible { target, reply } => {
            let v = match target {
                Target::Login => state.login_visible,
                Target::Writer => state.writer_visible,
            };
            let _ = reply.send(Ok(v));
        }
    }
}

/// TCP 要求を tao loop へ中継し、応答を返す (子プロセス側)。
fn pump_tcp(
    stream: std::net::TcpStream,
    proxy: tao::event_loop::EventLoopProxy<HostCmd>,
    rt: tokio::runtime::Handle,
) {
    use std::io::{BufRead, Write};

    let reader = std::io::BufReader::new(stream.try_clone().expect("clone"));
    let writer = Arc::new(std::sync::Mutex::new(stream));
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let writer = writer.clone();
        let proxy = proxy.clone();
        rt.spawn(async move {
            let resp = dispatch(line, &proxy).await;
            if let Ok(mut w) = writer.lock() {
                let _ = writeln!(w, "{}", resp);
                let _ = w.flush();
            }
        });
    }
    // 親が死んだ: 子も終わる。
    std::process::exit(0);
}

async fn dispatch(line: String, proxy: &tao::event_loop::EventLoopProxy<HostCmd>) -> String {
    let v: Value = match serde_json::from_str(&line) {
        Ok(v) => v,
        Err(e) => return json!({"id": 0, "ok": false, "error": format!("bad json: {e}")}).to_string(),
    };
    let id = v.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
    let op = v.get("op").and_then(|o| o.as_str()).unwrap_or("");
    let target = v
        .get("target")
        .and_then(|t| t.as_str())
        .and_then(target_of);
    let fail = |msg: String| json!({"id": id, "ok": false, "error": msg}).to_string();
    let ok = |result: Value| json!({"id": id, "ok": true, "result": result}).to_string();
    let Some(target) = target else {
        return fail("bad target".into());
    };
    let str_param = |key: &str| {
        v.get(key)
            .and_then(|x| x.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("missing {key}"))
    };
    // oneshot 往復の定型。
    macro_rules! roundtrip {
        ($make:expr) => {{
            let (tx, rx) = tokio::sync::oneshot::channel();
            let cmd = $make(tx);
            if proxy.send_event(cmd).is_err() {
                return fail("loop gone".into());
            }
            match rx.await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => return fail(e),
                Err(_) => return fail("no reply".into()),
            }
        }};
    }
    match op {
        "eval" => match str_param("js") {
            Ok(js) => {
                roundtrip!(|reply| HostCmd::Eval { target, js, reply });
                ok(Value::Null)
            }
            Err(e) => fail(e),
        },
        "cookies" => {
            let pairs: Vec<(String, String)> =
                roundtrip!(|reply| HostCmd::Cookies { target, reply });
            ok(Value::Array(
                pairs
                    .into_iter()
                    .map(|(n, v)| Value::Array(vec![Value::String(n), Value::String(v)]))
                    .collect(),
            ))
        }
        "set_cookie" => match (str_param("name"), str_param("value")) {
            (Ok(name), Ok(value)) => {
                roundtrip!(|reply| HostCmd::SetCookie { target, name, value, reply });
                ok(Value::Null)
            }
            _ => fail("missing name/value".into()),
        },
        "clear" => {
            roundtrip!(|reply| HostCmd::ClearBrowsing { target, reply });
            ok(Value::Null)
        }
        "navigate" => match str_param("url") {
            Ok(url) => {
                roundtrip!(|reply| HostCmd::Navigate { target, url, reply });
                ok(Value::Null)
            }
            Err(e) => fail(e),
        },
        "url" => {
            let url: String = roundtrip!(|reply| HostCmd::CurrentUrl { target, reply });
            ok(Value::String(url))
        }
        "show" => {
            let _ = proxy.send_event(HostCmd::Show { target });
            ok(Value::Null)
        }
        "hide" => {
            let _ = proxy.send_event(HostCmd::Hide { target });
            ok(Value::Null)
        }
        "focus" => {
            let _ = proxy.send_event(HostCmd::Focus { target });
            ok(Value::Null)
        }
        "ontop" => {
            let top = v.get("top").and_then(|t| t.as_bool()).unwrap_or(false);
            let _ = proxy.send_event(HostCmd::SetOnTop { target, top });
            ok(Value::Null)
        }
        "visible" => {
            let visible: bool = roundtrip!(|reply| HostCmd::IsVisible { target, reply });
            ok(Value::Bool(visible))
        }
        _ => fail(format!("unknown op: {op}")),
    }
}
