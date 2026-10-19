use std::process::Command;

/// Show the Windows touch keyboard (TabTip.exe) on demand.
///
/// Windows never pops the touch keyboard for desktop apps automatically, so
/// when the user turns on Settings > General > Touch > "Show touch keyboard
/// automatically" (off by default), the frontend calls this right after a text
/// field is tapped in touch mode. Fire-and-forget: spawn the shared ink TabTip
/// and never surface an error (missing binary or a non-Windows build is a
/// silent no-op). The frontend rate-limits the calls.
#[tauri::command]
pub fn launch_touch_keyboard() {
    #[cfg(target_os = "windows")]
    {
        let Some(program_files) = std::env::var_os("ProgramFiles") else {
            return;
        };
        let tabtip = std::path::Path::new(&program_files)
            .join("Common Files")
            .join("microsoft shared")
            .join("ink")
            .join("TabTip.exe");
        if tabtip.exists() {
            let _ = Command::new(&tabtip).spawn();
        }
    }
}
