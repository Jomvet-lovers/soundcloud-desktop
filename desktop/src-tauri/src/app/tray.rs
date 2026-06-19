use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

use crate::app::popover;

pub fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItemBuilder::with_id("show", "Show").build(app)?;
    let mini = MenuItemBuilder::with_id("mini", "Mini player").build(app)?;
    let play_pause = MenuItemBuilder::with_id("play_pause", "Play / Pause").build(app)?;
    let next = MenuItemBuilder::with_id("next", "Next").build(app)?;
    let prev = MenuItemBuilder::with_id("prev", "Previous").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[&show, &mini, &play_pause, &prev, &next, &quit])
        .build()?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().cloned().expect("no app icon"))
        .tooltip("SoundCloud Desktop")
        .menu(&menu)
        // Left-click opens the rich popover (below); the native menu is the right-click.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            match id {
                "show" => {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.unminimize();
                        let _ = w.set_focus();
                    }
                }
                "mini" => {
                    popover::open_pinned(app);
                }
                "play_pause" | "next" | "prev" => {
                    let _ = app.emit("tray-action", id);
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            // Left-click toggles the mini-player popover anchored at the click point.
            // (Linux/appindicator never emits this — those users use the "Mini player" menu.)
            // Release only: Click fires for both press and release; handling both toggles
            // twice per physical click (show→hide), so the popover would vanish instantly.
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                position,
                ..
            } = event
            {
                popover::toggle(tray.app_handle(), Some((position.x, position.y)));
            }
        })
        .build(app)?;

    Ok(())
}
