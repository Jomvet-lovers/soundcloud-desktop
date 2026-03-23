use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rodio::mixer::Mixer;
use rodio::stream::{DeviceSinkBuilder, MixerDeviceSink};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::app::diagnostics;
use crate::audio::engine;
use crate::audio::state::AudioState;
use crate::audio::types::{AudioSink, AudioThreadCmd};

pub fn open_device_sink(
    device_id: Option<&str>,
    reconnect_tx: &std::sync::mpsc::Sender<AudioThreadCmd>,
    error_flag: &Arc<AtomicBool>,
) -> Result<MixerDeviceSink, String> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let sent = Arc::new(AtomicBool::new(false));
    let sent_clone = sent.clone();
    let tx = reconnect_tx.clone();
    let err_flag = error_flag.clone();
    let error_cb = move |err: cpal::StreamError| {
        eprintln!("[audio] stream error: {err}");
        err_flag.store(true, Ordering::Relaxed);
        if !sent_clone.swap(true, Ordering::Relaxed) {
            tx.send(AudioThreadCmd::Reconnect).ok();
        }
    };

    if let Some(id) = device_id {
        let host = cpal::default_host();
        if let Ok(devices) = host.output_devices() {
            for dev in devices {
                if dev.id().ok().map(|d| d.to_string()).as_deref() == Some(id) {
                    let mut sink = DeviceSinkBuilder::from_device(dev)
                        .map_err(|e| format!("Failed to open device '{}': {}", id, e))?
                        .with_error_callback(error_cb)
                        .open_stream()
                        .map_err(|e| format!("Failed to open device '{}': {}", id, e))?;
                    sink.log_on_drop(false);
                    return Ok(sink);
                }
            }
        }
        return Err(format!("Device '{}' not found", id));
    }

    let mut sink = DeviceSinkBuilder::from_default_device()
        .map_err(|e| format!("No audio output: {}", e))?
        .with_error_callback(error_cb)
        .open_stream()
        .map_err(|e| format!("No audio output: {}", e))?;
    sink.log_on_drop(false);
    Ok(sink)
}

pub fn list_devices() -> Vec<AudioSink> {
    #[cfg(target_os = "linux")]
    {
        audio_list_devices_pactl()
    }
    #[cfg(not(target_os = "linux"))]
    {
        audio_list_devices_cpal()
    }
}

pub fn switch_device(
    state: State<'_, AudioState>,
    device_name: Option<String>,
) -> Result<(), String> {
    let switch_to_default = device_name.is_none();

    #[cfg(target_os = "linux")]
    let switch_name: Option<String> = {
        if let Some(ref name) = device_name {
            std::process::Command::new("pactl")
                .args(["set-default-sink", name])
                .status()
                .map_err(|e| format!("pactl failed: {}", e))?;
        }
        None
    };
    #[cfg(not(target_os = "linux"))]
    let switch_name: Option<String> = device_name;

    let preserved_default_name = if switch_to_default {
        current_default_output_name()
    } else {
        None
    };

    {
        let mut player = state.player.lock().unwrap();
        if let Some(old) = player.take() {
            old.stop();
        }
        state.has_track.store(false, Ordering::Relaxed);
        state.load_gen.fetch_add(1, Ordering::Relaxed);
    }

    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    state
        .audio_tx
        .send(AudioThreadCmd::SwitchDevice {
            name: switch_name,
            reply: reply_tx,
        })
        .map_err(|e| e.to_string())?;

    let new_mixer: Mixer = reply_rx
        .recv()
        .map_err(|e| format!("Device switch failed: {}", e))??;

    *state.mixer.lock().unwrap() = new_mixer;
    if let Some(name) = preserved_default_name {
        *state.last_known_default_output.lock().unwrap() = Some(name);
    }
    engine::reload_current_track(&state)?;
    Ok(())
}

pub fn set_follow_default_output(state: State<'_, AudioState>, follow: bool) {
    state.follow_default_output.store(follow, Ordering::Relaxed);
    if follow {
        *state.last_known_default_output.lock().unwrap() = current_default_output_name();
    }
}

pub fn start_default_output_monitor(app: &AppHandle) {
    let handle = app.clone();
    {
        let state = handle.state::<AudioState>();
        *state.last_known_default_output.lock().unwrap() = current_default_output_name();
    }

    std::thread::Builder::new()
        .name("audio-default-output".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(10));

            let state = handle.state::<AudioState>();
            if !state.follow_default_output.load(Ordering::Relaxed) {
                continue;
            }

            let Some(current_default) = current_default_output_name() else {
                continue;
            };

            let mut known_default = state.last_known_default_output.lock().unwrap();
            if known_default.as_deref() == Some(current_default.as_str()) {
                continue;
            }
            *known_default = Some(current_default.clone());
            drop(known_default);

            diagnostics::log_native(
                &handle,
                "INFO",
                format!("[Audio] Default output changed to '{current_default}'"),
            );

            if let Err(error) = switch_to_device_internal(&state, None) {
                diagnostics::log_native(
                    &handle,
                    "WARN",
                    format!("[Audio] Failed to follow default output: {error}"),
                );
                continue;
            }

            handle
                .emit("audio:default-device-changed", current_default)
                .ok();
        })
        .expect("failed to spawn default-output monitor");
}

#[cfg(target_os = "linux")]
fn audio_list_devices_pactl() -> Vec<AudioSink> {
    let output = match std::process::Command::new("pactl")
        .args(["--format=json", "list", "sinks"])
        .output()
    {
        Ok(output) if output.status.success() => output.stdout,
        _ => return Vec::new(),
    };

    let default_sink = std::process::Command::new("pactl")
        .args(["get-default-sink"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default();

    let sinks: Vec<serde_json::Value> = match serde_json::from_slice(&output) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };

    sinks
        .iter()
        .filter_map(|sink| {
            let name = sink.get("name")?.as_str()?.to_string();
            let description = sink.get("description")?.as_str()?.to_string();
            Some(AudioSink {
                is_default: name == default_sink,
                name,
                description,
            })
        })
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn audio_list_devices_cpal() -> Vec<AudioSink> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let default_id = host
        .default_output_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());

    let devices = match host.output_devices() {
        Ok(devices) => devices,
        Err(_) => return Vec::new(),
    };

    devices
        .filter_map(|dev| {
            let id = dev.id().ok()?.to_string();
            let description = dev
                .description()
                .ok()
                .map(|desc| desc.name().to_string())
                .unwrap_or_else(|| id.clone());
            Some(AudioSink {
                is_default: default_id.as_deref() == Some(id.as_str()),
                name: id,
                description,
            })
        })
        .collect()
}

pub fn current_default_output_name() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("pactl")
            .args(["get-default-sink"])
            .output()
            .ok()
            .and_then(|output| {
                if output.status.success() {
                    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    (!value.is_empty()).then_some(value)
                } else {
                    None
                }
            })
    }
    #[cfg(not(target_os = "linux"))]
    {
        use cpal::traits::{DeviceTrait, HostTrait};

        cpal::default_host()
            .default_output_device()
            .and_then(|device| device.id().ok())
            .map(|id| id.to_string())
    }
}

fn switch_to_device_internal(
    state: &AudioState,
    device_name: Option<String>,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let switch_name: Option<String> = {
        if let Some(ref name) = device_name {
            std::process::Command::new("pactl")
                .args(["set-default-sink", name])
                .status()
                .map_err(|e| format!("pactl failed: {}", e))?;
        }
        None
    };
    #[cfg(not(target_os = "linux"))]
    let switch_name: Option<String> = device_name;

    {
        let mut player = state.player.lock().unwrap();
        if let Some(old) = player.take() {
            old.stop();
        }
        state.has_track.store(false, Ordering::Relaxed);
        state.load_gen.fetch_add(1, Ordering::Relaxed);
    }

    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    state
        .audio_tx
        .send(AudioThreadCmd::SwitchDevice {
            name: switch_name,
            reply: reply_tx,
        })
        .map_err(|e| e.to_string())?;

    let new_mixer: Mixer = reply_rx
        .recv()
        .map_err(|e| format!("Device switch failed: {}", e))??;

    *state.mixer.lock().unwrap() = new_mixer;
    engine::reload_current_track(state)?;
    Ok(())
}
