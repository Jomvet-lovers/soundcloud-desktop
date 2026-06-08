//! Tray-popover window: a frameless, transparent, always-on-top mini-player webview
//! anchored to the tray icon. Created lazily on first open and kept alive (hidden on
//! blur) thereafter. Left-click on the tray toggles it; the `last_hide` instant
//! debounces the focus-loss-then-click race so a click never reopens what the blur
//! just closed. Linux (appindicator) emits no left-click — the "Mini player" menu
//! item opens it in a screen corner instead.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

pub const LABEL: &str = "tray-popover";

/// Logical popover size (px). Physical size is scaled per-monitor when positioning.
const W: f64 = 384.0;
const H: f64 = 248.0;
const GAP: f64 = 10.0;
const MARGIN: f64 = 8.0;
const REOPEN_DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct TrayState {
    last_hide: Mutex<Option<Instant>>,
    /// Pinned (opened from the "Mini player" menu) → a persistent widget: focus loss
    /// does NOT dismiss it, only the ✕ does. Transient opens (tray left-click on
    /// Win/Mac) leave this false and auto-hide on blur, like a glance flyout.
    pinned: Mutex<bool>,
}

impl TrayState {
    pub fn mark_hidden(&self) {
        if let Ok(mut g) = self.last_hide.lock() {
            *g = Some(Instant::now());
        }
    }

    fn recently_hidden(&self) -> bool {
        self.last_hide
            .lock()
            .ok()
            .and_then(|g| *g)
            .map(|t| t.elapsed() < REOPEN_DEBOUNCE)
            .unwrap_or(false)
    }

    fn set_pinned(&self, value: bool) {
        if let Ok(mut g) = self.pinned.lock() {
            *g = value;
        }
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned.lock().map(|g| *g).unwrap_or(false)
    }
}

fn get_or_create(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(w) = app.get_webview_window(LABEL) {
        return Some(w);
    }
    match WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("tray.html".into()))
        .title("Mini Player")
        .inner_size(W, H)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .visible(false)
        .focused(false)
        .build()
    {
        Ok(w) => Some(w),
        Err(err) => {
            eprintln!("[tray] failed to create popover: {err}");
            None
        }
    }
}

fn pick_monitor(win: &WebviewWindow, cursor: Option<(f64, f64)>) -> Option<Monitor> {
    let monitors = win.available_monitors().unwrap_or_default();
    if let Some((cx, cy)) = cursor {
        for m in &monitors {
            let p = m.position();
            let s = m.size();
            if cx >= p.x as f64
                && cx < p.x as f64 + s.width as f64
                && cy >= p.y as f64
                && cy < p.y as f64 + s.height as f64
            {
                return Some(m.clone());
            }
        }
    }
    win.primary_monitor()
        .ok()
        .flatten()
        .or_else(|| monitors.into_iter().next())
}

fn place(win: &WebviewWindow, cursor: Option<(f64, f64)>) {
    let Some(mon) = pick_monitor(win, cursor) else {
        return;
    };
    let scale = mon.scale_factor();
    let mp = mon.position();
    let ms = mon.size();
    let (mx, my, mw, mh) = (
        mp.x as f64,
        mp.y as f64,
        ms.width as f64,
        ms.height as f64,
    );
    let (pw, ph, gap, margin) = (W * scale, H * scale, GAP * scale, MARGIN * scale);

    // No cursor (Linux menu path) → anchor to the bottom-right corner of the monitor.
    let (ax, ay) = cursor.unwrap_or((mx + mw - pw / 2.0 - margin, my + mh - margin));

    // Icon in the top half of the screen → drop the popover below it; bottom half → above.
    let y = if ay < my + mh / 2.0 {
        ay + gap
    } else {
        ay - ph - gap
    };
    let x = (ax - pw / 2.0)
        .max(mx + margin)
        .min(mx + mw - pw - margin);
    let y = y.max(my + margin).min(my + mh - ph - margin);

    let _ = win.set_position(PhysicalPosition::new(x as i32, y as i32));
}

fn show(app: &AppHandle, cursor: Option<(f64, f64)>, pinned: bool) {
    let Some(win) = get_or_create(app) else {
        return;
    };
    app.state::<TrayState>().set_pinned(pinned);
    // Re-assert on every show — keeps the flyout above other windows even after a
    // re-open (honored on Win/Mac; on Wayland/Hyprland the compositor owns stacking,
    // so pair this with a `pin` windowrule).
    let _ = win.set_always_on_top(true);
    place(&win, cursor);
    let _ = win.show();
    let _ = win.set_focus();
}

fn hide_if_visible(app: &AppHandle) -> bool {
    if let Some(win) = app.get_webview_window(LABEL) {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
            app.state::<TrayState>().mark_hidden();
            return true;
        }
    }
    false
}

/// Tray left-click (Win/Mac): transient glance flyout anchored at the click point —
/// auto-hides on blur. `cursor` is the click point in physical px.
pub fn toggle(app: &AppHandle, cursor: Option<(f64, f64)>) {
    if hide_if_visible(app) {
        return;
    }
    // A focus-loss auto-hide just fired → swallow this click (it's the "close" half).
    if app.state::<TrayState>().recently_hidden() {
        return;
    }
    show(app, cursor, false);
}

/// "Mini player" menu: pinned, corner-positioned, persistent widget (no blur-dismiss).
/// Toggles — a second menu click hides it. The only path on Linux (no tray left-click).
pub fn open_pinned(app: &AppHandle) {
    if hide_if_visible(app) {
        return;
    }
    show(app, None, true);
}
