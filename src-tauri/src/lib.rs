//! Tauri shell: wires the core controller to the UI, tray, global hotkeys and `kavoxctl`.

mod commands;
mod hotkeys;
mod ipc;
mod logging;
mod tray;

use std::sync::{Arc, Mutex, MutexGuard};

use kavox_core::{Controller, EventSink, Paths, UiEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewWindow, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTOSTART_FLAG: &str = "--autostart";

pub struct AppState {
    pub controller: Controller,
    pub hotkeys: Mutex<hotkeys::Registry>,
    pub hotkeys_supported: bool,
    pub tray: Mutex<Option<tray::TrayItems>>,
    _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// Locks a std mutex, recovering from poisoning (a panicked holder shouldn't take the app down).
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Forwards core events to the webview. Runs with the core locked, so it must never block on the main thread.
struct TauriSink {
    app: AppHandle,
}

impl EventSink for TauriSink {
    fn emit(&self, event: UiEvent) {
        if let UiEvent::Settings(settings) = &event {
            tray::set_muted(&self.app, settings.audio.mic_muted);
            tray::set_voice_enabled(&self.app, settings.voice.enabled);
        }
        let _ = self.app.emit(event.name(), &event);
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app)
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(hotkeys::handle)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_FLAG]),
        ))
        .setup(|app| {
            setup(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                let close_to_tray = state.controller.lock().settings().app.close_to_tray;
                if close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                    state.controller.lock().set_visible(false);
                }
            }
        })
        .invoke_handler(commands::handler())
        .build(tauri::generate_context!())
        .expect("error while building Kavox")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                tracing::info!("shutting down");
                if let Some(state) = app.try_state::<AppState>() {
                    state.controller.shutdown();
                }
            }
        });
}

fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let paths = Paths {
        config_dir: app.path().app_config_dir()?,
        sounds_dir: app.path().app_data_dir()?.join("sounds"),
        log_dir: app.path().app_log_dir()?,
    };
    let log_guard = logging::init(&paths.log_dir);
    tracing::info!("Kavox {VERSION} starting on {}", std::env::consts::OS);

    let sink = Arc::new(TauriSink { app: app.clone() });
    let controller = Controller::start(paths, kavox_core::platform_backend(), sink);
    let app_settings = controller.lock().settings().app.clone();
    app.manage(AppState {
        controller,
        hotkeys: Mutex::default(),
        hotkeys_supported: hotkeys::supported(),
        tray: Mutex::default(),
        _log_guard: log_guard,
    });

    for (hotkey, error) in hotkeys::sync(app) {
        tracing::warn!("couldn't register {hotkey}: {error}");
    }
    if let Err(e) = tray::create(app) {
        tracing::warn!("no tray icon: {e}");
    }
    ipc::serve(app.clone());
    exit_on_signals(app);

    let autostarted = std::env::args().any(|a| a == AUTOSTART_FLAG);
    let start_hidden = app_settings.setup_complete && (autostarted || app_settings.start_minimized);
    if start_hidden {
        app.state::<AppState>().controller.lock().set_visible(false);
    } else {
        show_main_window(app);
    }
    Ok(())
}

/// Quits through Tauri on SIGINT/SIGTERM/SIGHUP so shutdown still removes Kavox Mic (e.g. on logout).
#[cfg(unix)]
fn exit_on_signals(app: &AppHandle) {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    match signal_hook::iterator::Signals::new([SIGINT, SIGTERM, SIGHUP]) {
        Ok(mut signals) => {
            let app = app.clone();
            std::thread::spawn(move || {
                if let Some(signal) = signals.forever().next() {
                    tracing::info!("received signal {signal}, quitting");
                    app.exit(0);
                }
            });
        }
        Err(e) => tracing::warn!("can't watch for quit signals: {e}"),
    }
}

#[cfg(not(unix))]
fn exit_on_signals(_: &AppHandle) {}

pub fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = main_window(app) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if let Some(state) = app.try_state::<AppState>() {
        state.controller.lock().set_visible(true);
    }
}
