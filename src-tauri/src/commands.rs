//! Commands the UI invokes. Sync commands run on the main thread, so they only hold the core lock briefly;
//! importing (disk + decode) runs on a blocking worker.

use std::path::{Path, PathBuf};

use kavox_core::{
    ApiError, AppSettings, AudioSettings, Board, DeviceList, Error, GlobalHotkey, Snapshot, Sound,
    SoundPatch, StreamRole, clip, prepare_import,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, ipc::Invoke};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_opener::OpenerExt;

use crate::{AppState, VERSION, hotkeys};

type CmdResult<T> = Result<T, ApiError>;

pub fn handler() -> impl Fn(Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        get_state,
        refresh_devices,
        set_audio_settings,
        set_app_settings,
        import_sounds,
        update_sound,
        delete_sound,
        reorder_sounds,
        play_sound,
        stop_sound,
        stop_all,
        set_mic_muted,
        set_sound_hotkey,
        set_global_hotkey,
        play_test_tone,
        create_board,
        rename_board,
        delete_board,
        set_active_board,
        copy_diagnostics,
        open_logs_folder,
        open_sound_settings,
        set_window_visible,
    ]
}

fn api_error(code: &'static str, message: impl ToString) -> ApiError {
    ApiError {
        code,
        message: message.to_string(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UiState {
    #[serde(flatten)]
    snapshot: Snapshot,
    version: &'static str,
    hotkeys_supported: bool,
    autostart_enabled: bool,
}

#[tauri::command]
fn get_state(app: AppHandle, state: State<AppState>) -> UiState {
    let snapshot = state.controller.lock().snapshot();
    UiState {
        snapshot,
        version: VERSION,
        hotkeys_supported: state.hotkeys_supported,
        autostart_enabled: app.autolaunch().is_enabled().unwrap_or(false),
    }
}

#[tauri::command]
fn refresh_devices(state: State<AppState>) -> DeviceList {
    state.controller.lock().devices()
}

#[tauri::command]
fn set_audio_settings(state: State<AppState>, audio: AudioSettings) {
    state.controller.lock().set_audio_settings(audio);
}

#[tauri::command]
fn set_app_settings(
    app: AppHandle,
    state: State<AppState>,
    settings: AppSettings,
) -> CmdResult<()> {
    let autolaunch = app.autolaunch();
    if autolaunch.is_enabled().unwrap_or(false) != settings.launch_on_login {
        let result = if settings.launch_on_login {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        result
            .map_err(|e| api_error("Autostart", format!("Couldn't change launch on login: {e}")))?;
    }
    state.controller.lock().set_app_settings(settings);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportFailure {
    file: String,
    error: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportResult {
    added: Vec<Sound>,
    failed: Vec<ImportFailure>,
}

/// Imports files (and supported files directly inside dropped folders).
#[tauri::command]
async fn import_sounds(
    app: AppHandle,
    paths: Vec<String>,
    board: Option<String>,
) -> CmdResult<ImportResult> {
    let state = app.state::<AppState>();
    let (sounds_dir, mut hashes) = state.controller.lock().import_context();
    let (added, failed) = tauri::async_runtime::spawn_blocking(move || {
        let mut added = Vec::new();
        let mut failed = Vec::new();
        for path in expand(paths) {
            match prepare_import(&path, &sounds_dir, &hashes) {
                Ok(sound) => {
                    hashes.insert(sound.sha256.clone(), sound.file.clone());
                    added.push(sound);
                }
                Err(e) => failed.push(ImportFailure {
                    file: file_name(&path),
                    error: e.to_string(),
                }),
            }
        }
        (added, failed)
    })
    .await
    .map_err(|e| api_error("Import", e))?;
    let added = state
        .controller
        .lock()
        .add_sounds(board.as_deref(), added)?;
    Ok(ImportResult { added, failed })
}

fn expand(paths: Vec<String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths.into_iter().map(PathBuf::from) {
        if path.is_dir() {
            let mut files: Vec<PathBuf> = std::fs::read_dir(&path)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && clip::is_supported(p))
                .collect();
            files.sort();
            out.extend(files);
        } else {
            out.push(path);
        }
    }
    out
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[tauri::command]
fn update_sound(state: State<AppState>, id: String, patch: SoundPatch) -> CmdResult<Sound> {
    Ok(state.controller.lock().update_sound(&id, patch)?)
}

#[tauri::command]
fn delete_sound(app: AppHandle, state: State<AppState>, id: String) -> CmdResult<()> {
    state.controller.lock().delete_sound(&id)?;
    hotkeys::sync(&app);
    Ok(())
}

#[tauri::command]
fn reorder_sounds(state: State<AppState>, board: String, ids: Vec<String>) -> CmdResult<()> {
    Ok(state.controller.lock().reorder_sounds(&board, ids)?)
}

#[tauri::command]
fn play_sound(state: State<AppState>, id: String) -> CmdResult<()> {
    Ok(state.controller.lock().play_sound(&id)?)
}

#[tauri::command]
fn stop_sound(state: State<AppState>, id: String) {
    state.controller.lock().stop_sound(&id);
}

#[tauri::command]
fn stop_all(state: State<AppState>) {
    state.controller.lock().stop_all();
}

#[tauri::command]
fn set_mic_muted(state: State<AppState>, muted: Option<bool>) -> bool {
    state.controller.lock().set_mic_muted(muted)
}

/// Saves the binding, then asks the OS for it; if the OS refuses, the old binding is restored.
#[tauri::command]
fn set_sound_hotkey(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    hotkey: Option<String>,
) -> CmdResult<()> {
    if let Some(h) = &hotkey {
        hotkeys::validate(h)
            .map_err(|e| api_error("Invalid", format!("{h} can't be used as a hotkey: {e}")))?;
    }
    let previous = state.controller.lock().library().sound(&id)?.hotkey.clone();
    state
        .controller
        .lock()
        .set_sound_hotkey(&id, hotkey.clone())?;
    apply_or_revert(&app, hotkey, || {
        state.controller.lock().set_sound_hotkey(&id, previous)
    })
}

#[tauri::command]
fn set_global_hotkey(
    app: AppHandle,
    state: State<AppState>,
    which: GlobalHotkey,
    hotkey: Option<String>,
) -> CmdResult<()> {
    if let Some(h) = &hotkey {
        hotkeys::validate(h)
            .map_err(|e| api_error("Invalid", format!("{h} can't be used as a hotkey: {e}")))?;
    }
    let previous = {
        let core = state.controller.lock();
        let hotkeys = &core.settings().hotkeys;
        match which {
            GlobalHotkey::StopAll => hotkeys.stop_all.clone(),
            GlobalHotkey::ToggleMute => hotkeys.toggle_mute.clone(),
        }
    };
    state
        .controller
        .lock()
        .set_global_hotkey(which, hotkey.clone())?;
    apply_or_revert(&app, hotkey, || {
        state.controller.lock().set_global_hotkey(which, previous)
    })
}

fn apply_or_revert(
    app: &AppHandle,
    hotkey: Option<String>,
    revert: impl FnOnce() -> kavox_core::Result<()>,
) -> CmdResult<()> {
    let failures = hotkeys::sync(app);
    let Some(new) = hotkey else { return Ok(()) };
    if failures.iter().any(|(h, _)| h.eq_ignore_ascii_case(&new)) {
        revert()?;
        hotkeys::sync(app);
        return Err(Error::HotkeyTaken(new).into());
    }
    Ok(())
}

#[tauri::command]
fn play_test_tone(state: State<AppState>, output: String) -> CmdResult<()> {
    let role = match output.as_str() {
        "virtual" => StreamRole::Virtual,
        "monitor" => StreamRole::Monitor,
        _ => {
            return Err(api_error(
                "Invalid",
                "output must be \"virtual\" or \"monitor\"",
            ));
        }
    };
    Ok(state.controller.lock().play_test_tone(role)?)
}

#[tauri::command]
fn create_board(state: State<AppState>, name: String) -> CmdResult<Board> {
    Ok(state.controller.lock().create_board(&name)?)
}

#[tauri::command]
fn rename_board(state: State<AppState>, id: String, name: String) -> CmdResult<()> {
    Ok(state.controller.lock().rename_board(&id, &name)?)
}

#[tauri::command]
fn delete_board(app: AppHandle, state: State<AppState>, id: String) -> CmdResult<()> {
    state.controller.lock().delete_board(&id)?;
    hotkeys::sync(&app);
    Ok(())
}

#[tauri::command]
fn set_active_board(state: State<AppState>, id: String) -> CmdResult<()> {
    Ok(state.controller.lock().set_active_board(&id)?)
}

#[tauri::command]
fn copy_diagnostics(state: State<AppState>) -> String {
    state.controller.lock().diagnostics(VERSION)
}

#[tauri::command]
fn open_logs_folder(app: AppHandle) -> CmdResult<()> {
    let dir = app.path().app_log_dir().map_err(|e| api_error("Io", e))?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| api_error("Io", e))
}

/// Windows Sound settings, for fixing the default playback device.
#[tauri::command]
fn open_sound_settings(app: AppHandle) -> CmdResult<()> {
    if cfg!(windows) {
        app.opener()
            .open_url("ms-settings:sound", None::<&str>)
            .map_err(|e| api_error("Io", e))
    } else {
        Err(api_error(
            "Unsupported",
            "Open your desktop's sound settings to change devices.",
        ))
    }
}

#[tauri::command]
fn set_window_visible(state: State<AppState>, visible: bool) {
    state.controller.lock().set_visible(visible);
}
