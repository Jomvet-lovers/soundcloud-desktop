//! System tray (resident icon + menu).
//!
//! Kept off the eframe event loop: a dedicated thread runs the tray-icon
//! Win32 message loop. Events go to the UI through a std mpsc channel and the
//! egui context is woken with `request_repaint`.

/// Commands sent from the tray thread to the app.
#[derive(Debug, Clone, Copy)]
pub enum TrayCmd {
    /// Toggle the main window visibility.
    ToggleWindow,
    PlayPause,
    Next,
    Prev,
    Quit,
}

/// Spawn the tray. Failures are logged and ignored (app keeps running).
pub fn spawn(ctx: egui::Context, tx: std::sync::mpsc::Sender<TrayCmd>) {
    #[cfg(windows)]
    {
        std::thread::Builder::new()
            .name("scd-tray".into())
            .spawn(move || windows_impl(ctx, tx))
            .ok();
    }
    #[cfg(not(windows))]
    {
        let _ = (ctx, tx);
    }
}

#[cfg(windows)]
fn windows_impl(ctx: egui::Context, tx: std::sync::mpsc::Sender<TrayCmd>) {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let wake = {
        let ctx = ctx.clone();
        move || ctx.request_repaint()
    };

    // Left click (on release) toggles the window.
    TrayIconEvent::set_event_handler({
        let tx = tx.clone();
        let wake = wake.clone();
        Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = tx.send(TrayCmd::ToggleWindow);
                wake();
            }
        })
    });

    MenuEvent::set_event_handler({
        let tx = tx.clone();
        let wake = wake.clone();
        Some(move |event: MenuEvent| {
            let cmd = match event.id().as_ref() {
                "show" => TrayCmd::ToggleWindow,
                "toggle" => TrayCmd::PlayPause,
                "next" => TrayCmd::Next,
                "prev" => TrayCmd::Prev,
                "quit" => TrayCmd::Quit,
                _ => return,
            };
            let _ = tx.send(cmd);
            wake();
        })
    });

    let menu = Menu::new();
    let show = MenuItem::with_id("show", "Show / Hide", true, None);
    let toggle = MenuItem::with_id("toggle", "Play / Pause", true, None);
    let next = MenuItem::with_id("next", "Next", true, None);
    let prev = MenuItem::with_id("prev", "Previous", true, None);
    let sep = PredefinedMenuItem::separator();
    let quit = MenuItem::with_id("quit", "Quit", true, None);
    let _ = menu.append_items(&[&show, &toggle, &next, &prev, &sep, &quit]);

    let tray = TrayIconBuilder::new()
        .with_tooltip("SCD-Direct")
        .with_icon(make_icon())
        .with_menu(Box::new(menu))
        .build();
    let _tray = match tray {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[tray] failed to create: {e}");
            return;
        }
    };

    // Pump Win32 messages on this thread (tray-icon creates an internal
    // message-only window here). Leaving the loop removes the tray icon.
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, GetMessageW, TranslateMessage, MSG,
        };
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// 32x32 icon: SoundCloud-orange diamond.
#[cfg(windows)]
fn make_icon() -> tray_icon::Icon {
    let size = 32usize;
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let i = (y * size + x) * 4;
            let dx = (x as i32 - 16).abs();
            let dy = (y as i32 - 16).abs();
            if dx + dy <= 26 {
                rgba[i] = 0xff;
                rgba[i + 1] = 0x55;
                rgba[i + 2] = 0x00;
                rgba[i + 3] = 0xff;
            }
        }
    }
    tray_icon::Icon::from_rgba(rgba, size as u32, size as u32).expect("tray icon")
}
