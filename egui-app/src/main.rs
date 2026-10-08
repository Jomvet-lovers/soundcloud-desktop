// Phase 1 は移植のみ (UI 配線は Phase 2)。未使用警告を抑制する。
#[allow(dead_code)]
mod backend;
mod images;
mod pager;
mod query;
mod shell;
mod state;
mod theme;
mod tray;
mod views;
mod widgets;

use eframe::egui;
use state::AppState;

impl eframe::App for AppState {
    // eframe 0.36: `update(&Context)` は廃止され `ui(&mut Ui)` が唯一の描画点。
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // 閉じるボタン → トレイに格納 (設定で有効時)。トレイの「終了」は除外。
        if self.settings.close_to_tray
            && !self.force_quit
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.window_hidden = true;
        }
        shell::show_shell(self, ui);
    }
}

/// スレッド/プロセス構成 (Phase 4):
/// - GUI は常にメインプロセスのメインスレッド (eframe)。
/// - wry host (login window + hidden writer) は子プロセス
///   (`--wry-host <profile>`) のメインスレッド。IPC は TCP JSON-RPC。
///   同一プロセス内の二重 GUI loop は当環境で不可のため。
/// Web が不要な smoke (`--smoke`, `--smoke-tao`, `--smoke <file>`) は
/// 子なしでそのまま実行する (CI 安全)。
fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let is_wry_child = args.get(1).map(|s| s.as_str()) == Some("--wry-host");
    let is_smoke = args.iter().any(|a| {
        a == "--smoke" || a == "--smoke-login" || a == "--smoke-writer" || a == "--smoke-tao"
    });
    if !is_wry_child && !is_smoke && !single_instance_guard() {
        eprintln!("SCD-Direct is already running");
        return Ok(());
    }
    if args.get(1).map(|s| s.as_str()) == Some("--wry-host") {
        let dir = args
            .get(2)
            .map(|s| s.as_str())
            .unwrap_or("webprofile");
        backend::webhost::run_child_main(std::path::PathBuf::from(dir));
    }
    if args.iter().any(|a| a == "--smoke-tao") {
        let event_loop = tao::event_loop::EventLoopBuilder::<()>::with_user_event().build();
        let _window = tao::window::WindowBuilder::new()
            .with_visible(false)
            .build(&event_loop)
            .map_err(|e| format!("tao window: {e}"))
            .unwrap();
        eprintln!("smoke tao: window ok on main thread");
        return Ok(());
    }
    let wants_web = !args.iter().any(|a| a == "--smoke")
        || args.iter().any(|a| a == "--smoke-login" || a == "--smoke-writer");
    let mut child: Option<std::process::Child> = None;
    if wants_web {
        match backend::webhost::spawn_child(backend::paths::app_cache_dir().join("webprofile")) {
            Ok((proxy, c)) => {
                backend::webhost::set_host(proxy);
                child = Some(c);
            }
            Err(e) => eprintln!("[boot] wry host unavailable: {e}"),
        }
    }
    let result = app_main(args);
    if let Some(mut c) = child {
        let _ = c.kill();
    }
    result
}

/// 二重起動防止 (Windows の名前付き mutex)。GUI モードでのみ使用する。
#[cfg(windows)]
fn single_instance_guard() -> bool {
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    static HANDLE: OnceLock<usize> = OnceLock::new();
    let name: Vec<u16> = "Local\\scd-egui-single-instance\0".encode_utf16().collect();
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        let already = GetLastError() == ERROR_ALREADY_EXISTS;
        if !handle.is_null() {
            let _ = HANDLE.set(handle as usize);
        }
        if already {
            return false;
        }
    }
    true
}

#[cfg(not(windows))]
fn single_instance_guard() -> bool {
    true
}

/// GUI は戻らず、smoke はコードで終了する。
fn app_main(args: Vec<String>) -> eframe::Result {
    if args
        .iter()
        .any(|a| a == "--smoke" || a == "--smoke-login" || a == "--smoke-writer")
    {
        return smoke();
    }
    let options = eframe::NativeOptions {
        // 現行 `tauri.conf.json` の 1200x800 に合わせる。
        // カスタムタイトルバー (Tauri Titlebar.tsx 相当) のため OS 装飾は外す。
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "SCD-Direct (egui)",
        options,
        Box::new(|cc| Ok(Box::new(AppState::new(cc)) as Box<dyn eframe::App>)),
    )
}

/// Headless smoke test: boot the backend (states, servers, audio probe)
/// without opening a window. Used for CI regression and local 疎通確認.
/// With a file argument (`--smoke <audio-file>`), also loads, plays and
/// samples the position to verify the engine end to end.
fn smoke() -> eframe::Result {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let bus = backend::events::EventBus::new(tx);
    let handle = match backend::boot::boot(&runtime, bus) {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("smoke boot failed: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "smoke ok: api={} static={} proxy={} audio={}",
        handle.servers.api_port,
        handle.servers.static_port,
        handle.servers.proxy_port,
        handle.audio.is_some(),
    );

    // データ面の疎通: Discover (公開) は必須、/me/cold はセッション任意。
    let api = backend::api::ApiClient::new(handle.servers.api_port);
    match runtime.block_on(api.get_json("/discover/mixed")) {
        Ok(v) => {
            let mixed: backend::models::DiscoverMixed =
                serde_json::from_value(v).unwrap_or_default();
            let titles: Vec<&str> = mixed
                .collection
                .iter()
                .take(3)
                .map(|s| s.title.as_str())
                .collect();
            println!(
                "smoke discover: {} selections ({})",
                mixed.collection.len(),
                titles.join(" / ")
            );
            if mixed.collection.is_empty() {
                eprintln!("smoke discover: empty");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("smoke discover failed: {e}");
            std::process::exit(1);
        }
    }
    match runtime.block_on(api.get_json("/me/cold")) {
        Ok(v) => println!(
            "smoke me: {}",
            v.get("username").and_then(|u| u.as_str()).unwrap_or("?")
        ),
        Err(e) => println!("smoke me: no session ({e})"),
    }

    // 手動の Web 経路検証 (表示環境でのみ実行する)。
    if args_has("--smoke-login") {
        if backend::webhost::host().is_none() {
            println!("smoke login: SKIP (wry host unavailable)");
            return Ok(());
        }
        backend::weblogin::open_login_window(
            runtime.handle(),
            handle.direct.clone(),
            handle.session.clone(),
            backend::events::EventBus::null(),
        );
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        println!("smoke login: window stayed up 10s");
        return Ok(());
    }
    if args_has("--smoke-writer") {
        if backend::webhost::host().is_none() {
            println!("smoke writer: SKIP (wry host unavailable)");
            return Ok(());
        }
        let bus = backend::events::EventBus::null();
        match runtime.block_on(backend::writer::execute(
            &bus,
            None,
            "GET",
            "https://soundcloud.com/",
            None,
        )) {
            Ok(o) => println!(
                "smoke writer: status={} bytes={} captcha={}",
                o.status,
                o.payload.len(),
                o.captcha.is_some()
            ),
            Err(e) => {
                eprintln!("smoke writer failed: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    let file = std::env::args().nth(2);
    if let Some(path) = file {
        if path.starts_with("--") {
            return Ok(());
        }
        let Some(audio) = handle.audio else {
            eprintln!("smoke play failed: no audio device");
            std::process::exit(1);
        };
        let out = runtime.block_on(backend::audio::engine::load_file(
            path,
            None,
            None,
            false,
            &audio,
        ));
        match out {
            Ok(loaded) => {
                println!("smoke loaded: duration={:?}", loaded.duration_secs);
                backend::audio::engine::play(&audio);
                for _ in 0..6 {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    // Drain tick noise so the channel never blocks the backend.
                    while rx.try_recv().is_ok() {}
                    println!(
                        "smoke pos: {:.2} playing={}",
                        backend::audio::engine::get_position(&audio),
                        backend::audio::engine::is_playing(&audio),
                    );
                }
            }
            Err(e) => {
                eprintln!("smoke load failed: {e}");
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn args_has(flag: &str) -> bool {
    std::env::args().any(|a| a == flag)
}
