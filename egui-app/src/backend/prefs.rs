//! egui UI 設定 (テーマ・アクセント) の永続化。
//! 保存先は `paths::app_data_dir().join("egui_ui.json")`。
//! `desktop/src/stores/settings.ts` (zustand persist `sc-settings`) の
//! egui subset。読込失敗時は `None` を返し、呼出側で `Default` に落とす。

use crate::backend::paths;
use crate::state::SettingsState;

const FILE_NAME: &str = "egui_ui.json";

fn path() -> std::path::PathBuf {
    paths::app_data_dir().join(FILE_NAME)
}

pub fn load() -> Option<SettingsState> {
    let bytes = std::fs::read(path()).ok()?;
    serde_json::from_slice::<SettingsState>(&bytes).ok()
}

pub fn save(settings: &SettingsState) -> Result<(), String> {
    let path = path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())
}
