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
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use serde_json::{json, Value};
use tokio::sync::oneshot;

static HOST: OnceLock<RwLock<Option<HostProxy>>> = OnceLock::new();

fn slot() -> &'static RwLock<Option<HostProxy>> {
    HOST.get_or_init(|| RwLock::new(None))
}

pub fn host() -> Option<HostProxy> {
    slot().read().ok().and_then(|g| g.clone())
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
pub const WRITER_HOME: &str = "https://soundcloud.com/";
const COOKIE_URL: &str = "https://soundcloud.com/";

type Reply<T> = oneshot::Sender<T>;

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
            if let Ok(mut p) = pending.lock() {
                for (_, tx) in p.drain() {
                    let _ = tx.send(json!({"ok": false, "error": "wry host gone"}));
                }
            }
        });
        Ok(Self { rpc })
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
    let port = rx
        .recv_timeout(std::time::Duration::from_secs(90))
        .map_err(|_| "wry host timeout".to_string())??;
    let rt: &'static tokio::runtime::Runtime = Box::leak(Box::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| format!("rt: {e}"))?,
    ));
    let proxy = rt.block_on(HostProxy::connect(port))?;
    Ok((proxy, child))
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
            if let Ok(mut p) = pending.lock() {
                for (_, tx) in p.drain() {
                    let _ = tx.send(json!({"ok": false, "error": "wry host gone"}));
                }
            }
        });
        Ok(Self { rpc })
    }
}

/// 子プロセス (`--wry-host <profile>`) の本体。戻らない。
pub fn run_child_main(profile_dir: PathBuf) -> ! {
    use std::io::Write;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    // 親が接続するまで待つ (起動直後の要求を取りこぼさない)。
    let (stream, _) = listener.accept().expect("accept");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");

    let event_loop = tao::event_loop::EventLoopBuilder::<HostCmd>::with_user_event().build();
    let loop_proxy = event_loop.create_proxy();

    let mut context = wry::WebContext::new(Some(profile_dir));
    eprintln!("[wry-host] webcontext ok");
    let build_window = |title: &str, w: f64, h: f64| {
        tao::window::WindowBuilder::new()
            .with_title(title)
            .with_inner_size(tao::dpi::LogicalSize::new(w, h))
            .with_visible(false)
            .build(&event_loop)
            .map_err(|e| format!("window: {e}"))
    };
    let login_window = build_window("SoundCloud", 1000.0, 800.0).expect("login window");
    eprintln!("[wry-host] login window ok");
    let writer_window = build_window("SoundCloud session", 520.0, 680.0).expect("writer window");
    eprintln!("[wry-host] writer window ok");
    let login_id = login_window.id();
    let writer_id = writer_window.id();
    let login_wv = wry::WebViewBuilder::new_with_web_context(&mut context)
        .with_url(LOGIN_HOME)
        .build(&login_window)
        .expect("login webview");
    eprintln!("[wry-host] login webview ok");
    let writer_wv = wry::WebViewBuilder::new_with_web_context(&mut context)
        .with_url(WRITER_HOME)
        .build(&writer_window)
        .expect("writer webview");
    eprintln!("[wry-host] writer webview ok");

    // NOTE: port 通知は window 構築の後 (親は接続後に要求を送れる)。
    println!("WRY_PORT={port}");
    let _ = std::io::stdout().flush();

    let rt_handle = rt.handle().clone();
    let pump_proxy = loop_proxy.clone();
    std::thread::spawn(move || pump_tcp(stream, pump_proxy, rt_handle));

    let mut login_visible = false;
    let mut writer_visible = false;

    event_loop.run(move |event, _, control_flow| {
        use tao::event::{Event, WindowEvent};
        *control_flow = tao::event_loop::ControlFlow::Wait;
        let pick = |t: Target| {
            if matches!(t, Target::Login) {
                (&login_window, &login_wv)
            } else {
                (&writer_window, &writer_wv)
            }
        };
        match event {
            Event::UserEvent(cmd) => match cmd {
                HostCmd::Eval { target, js, reply } => {
                    let (_, wv) = pick(target);
                    let _ = reply.send(wv.evaluate_script(&js).map_err(|e| e.to_string()));
                }
                HostCmd::Cookies { target, reply } => {
                    let (_, wv) = pick(target);
                    let out = wv
                        .cookies_for_url(COOKIE_URL)
                        .map(|cs| {
                            cs.into_iter()
                                .map(|c| (c.name().to_string(), c.value().to_string()))
                                .collect()
                        })
                        .map_err(|e| e.to_string());
                    let _ = reply.send(out);
                }
                HostCmd::SetCookie {
                    target,
                    name,
                    value,
                    reply,
                } => {
                    let (_, wv) = pick(target);
                    let cookie = cookie::Cookie::build((name, value))
                        .domain(".soundcloud.com")
                        .path("/")
                        .secure(true)
                        .http_only(true)
                        .build();
                    let _ = reply.send(wv.set_cookie(&cookie).map_err(|e| e.to_string()));
                }
                HostCmd::ClearBrowsing { target, reply } => {
                    let (_, wv) = pick(target);
                    let _ = reply.send(wv.clear_all_browsing_data().map_err(|e| e.to_string()));
                }
                HostCmd::Navigate { target, url, reply } => {
                    let (_, wv) = pick(target);
                    let _ = reply.send(wv.load_url(&url).map_err(|e| e.to_string()));
                }
                HostCmd::CurrentUrl { target, reply } => {
                    let (_, wv) = pick(target);
                    let _ = reply.send(wv.url().map_err(|e| e.to_string()));
                }
                HostCmd::Show { target } => {
                    let (w, _) = pick(target);
                    w.set_visible(true);
                    if matches!(target, Target::Login) {
                        login_visible = true;
                    } else {
                        writer_visible = true;
                    }
                }
                HostCmd::Hide { target } => {
                    let (w, _) = pick(target);
                    w.set_visible(false);
                    if matches!(target, Target::Login) {
                        login_visible = false;
                    } else {
                        writer_visible = false;
                    }
                }
                HostCmd::Focus { target } => {
                    let (w, _) = pick(target);
                    w.set_focus();
                }
                HostCmd::SetOnTop { target, top } => {
                    let (w, _) = pick(target);
                    w.set_always_on_top(top);
                }
                HostCmd::IsVisible { target, reply } => {
                    let v = if matches!(target, Target::Login) {
                        login_visible
                    } else {
                        writer_visible
                    };
                    let _ = reply.send(Ok(v));
                }
            },
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
                ..
            } => {
                if window_id == login_id {
                    login_window.set_visible(false);
                    login_visible = false;
                } else if window_id == writer_id {
                    writer_window.set_visible(false);
                    writer_visible = false;
                }
            }
            _ => {}
        }
    });
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
