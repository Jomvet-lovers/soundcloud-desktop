//! Системный трей десктопа. Linux — нативный **StatusNotifierItem через `ksni`**
//! (D-Bus, без GTK): работает на Wayland/Hyprland и не требует GTK-инициализации
//! (GTK-путь tray_manager на этих сессиях не поднимает иконку). Чистая логика —
//! C-ABI обёртка живёт в `desktop-bridge`. Меню статичное (как в Tauri): показать/
//! мини-плеер/транспорт/выход; левый клик иконки → мини-плеер.
//!
//! Не-Linux — заглушка (там трей оболочка делает нативно своими средствами).

use std::sync::Arc;

/// Действие трея (пункт меню или левый клик иконки).
#[derive(Clone, Copy, Debug)]
pub enum TrayAction {
    Show,
    Mini,
    PlayPause,
    Prev,
    Next,
    Quit,
    /// Левый клик по иконке.
    Activate,
}

/// Обработчик действий. Зовётся НЕ с main-потока (задача D-Bus) — потребитель
/// сам маршалит в свой поток (Dart `NativeCallable.listener` это умеет).
pub type ActionHandler = Arc<dyn Fn(TrayAction) + Send + Sync>;

/// Поднять трей с иконкой из PNG-файла. Держит свой поток+рантайм живым до конца
/// процесса. `false` — не удалось (нет SNI-host / D-Bus / иконки).
#[cfg(target_os = "linux")]
pub fn spawn(icon_png_path: &str, on_action: ActionHandler) -> bool {
    let icon = linux::decode_icon(icon_png_path);
    linux::spawn(icon, on_action)
}

#[cfg(not(target_os = "linux"))]
pub fn spawn(_icon_png_path: &str, _on_action: ActionHandler) -> bool {
    false
}

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::mpsc;
    use std::time::Duration;

    use ksni::{Icon, MenuItem, Tray, TrayMethods};

    use super::{ActionHandler, TrayAction};

    struct ScTray {
        icon: Vec<Icon>,
        on_action: ActionHandler,
    }

    impl Tray for ScTray {
        fn id(&self) -> String {
            "com.soundcloud.desktop".into()
        }
        fn title(&self) -> String {
            "SoundCloud Desktop".into()
        }
        fn icon_pixmap(&self) -> Vec<Icon> {
            self.icon.clone()
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            (self.on_action)(TrayAction::Activate);
        }
        fn menu(&self) -> Vec<MenuItem<Self>> {
            use ksni::menu::StandardItem;
            let item = |label: &str, action: TrayAction| -> MenuItem<Self> {
                StandardItem {
                    label: label.into(),
                    activate: Box::new(move |t: &mut Self| (t.on_action)(action)),
                    ..Default::default()
                }
                .into()
            };
            vec![
                item("Показать", TrayAction::Show),
                item("Мини-плеер", TrayAction::Mini),
                MenuItem::Separator,
                item("Играть / Пауза", TrayAction::PlayPause),
                item("Назад", TrayAction::Prev),
                item("Вперёд", TrayAction::Next),
                MenuItem::Separator,
                item("Выход", TrayAction::Quit),
            ]
        }
    }

    /// PNG → ARGB32-пиксмап SNI (RGBA → A,R,G,B). Пустой вектор при ошибке.
    pub(super) fn decode_icon(path: &str) -> Vec<Icon> {
        let Ok(img) = image::open(path) else {
            return Vec::new();
        };
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for px in rgba.pixels() {
            data.extend_from_slice(&[px[3], px[0], px[1], px[2]]);
        }
        vec![Icon {
            width: width as i32,
            height: height as i32,
            data,
        }]
    }

    pub(super) fn spawn(icon: Vec<Icon>, on_action: ActionHandler) -> bool {
        let (tx, rx) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("sc-tray".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(_) => {
                        let _ = tx.send(false);
                        return;
                    }
                };
                rt.block_on(async move {
                    let tray = ScTray { icon, on_action };
                    match tray.spawn().await {
                        Ok(_handle) => {
                            let _ = tx.send(true);
                            // ksni держит D-Bus-сервис, пока жив хэндл/задача.
                            std::future::pending::<()>().await;
                        }
                        Err(_) => {
                            let _ = tx.send(false);
                        }
                    }
                });
            })
            .is_ok();
        if !started {
            return false;
        }
        rx.recv_timeout(Duration::from_secs(5)).unwrap_or(false)
    }
}
