//! Kavox native Rust desktop application.
//!
//! Wires `kavox-core` to the `egui`/`eframe` UI, system tray, global hotkeys, and `kavoxctl` IPC.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod hotkey;
mod icons;
mod ipc;
mod logging;
mod platform;
mod state;
mod theme;
mod tray;
mod views;
mod voices;
mod widgets;

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui;
use kavox_core::{Controller, EventSink, UiEvent};

use crate::app::{Hosted, KavoxApp};
use crate::hotkey::HotkeyService;
use crate::state::{AppCommand, UiState};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Bridges `kavox-core` events to the UI channel without blocking the core lock.
struct CoreSink {
    tx: Sender<AppCommand>,
}

impl EventSink for CoreSink {
    fn emit(&self, event: UiEvent) {
        let _ = self.tx.send(AppCommand::CoreEvent(event));
    }
}

fn main() {
    // Atomic single-instance lock: if another Kavox is running, ask it to show itself and exit.
    let Some(_instance_guard) = ipc::acquire_single_instance() else {
        return;
    };

    let paths = platform::app_paths();
    let log_dir = paths.log_dir.clone();
    let log_guard = logging::init(&log_dir);
    tracing::info!("Kavox {VERSION} starting on {}", std::env::consts::OS);

    // Every background producer sends to `raw_tx`; a lightweight wake thread forwards to
    // `ui_tx` and calls `egui::Context::request_repaint()` so events wake the window immediately
    // (including `ShowWindow` / `Quit` while hidden in the tray).
    let (raw_tx, raw_rx) = mpsc::channel::<AppCommand>();
    let (ui_tx, ui_rx) = mpsc::channel::<AppCommand>();
    let egui_ctx: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));
    {
        let egui_ctx = egui_ctx.clone();
        let _ = thread::Builder::new()
            .name("kavox-wake".into())
            .spawn(move || {
                while let Ok(cmd) = raw_rx.recv() {
                    let _ = ui_tx.send(cmd);
                    if let Some(ctx) = egui_ctx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                        ctx.request_repaint();
                    }
                }
            });
    }

    // Bind the IPC socket before audio initialization so a second launch can deliver `Show`
    // immediately instead of waiting on device enumeration.
    let ipc_controller = ipc::serve(raw_tx.clone());
    exit_on_signals(raw_tx.clone());

    let sink = Arc::new(CoreSink { tx: raw_tx.clone() });
    let controller = Controller::start(paths, kavox_core::platform_backend(), sink);
    ipc_controller.set(controller.clone());
    let snapshot = controller.lock().snapshot();
    let app_settings = snapshot.settings.app.clone();

    let mut hotkeys = HotkeyService::new(controller.clone());
    for (hk, err) in hotkeys.sync(&controller) {
        tracing::warn!("couldn't register {hk}: {err}");
    }

    let autostarted = std::env::args().any(|a| a == platform::AUTOSTART_FLAG);
    let start_hidden = app_settings.setup_complete && (autostarted || app_settings.start_minimized);
    if start_hidden {
        controller.lock().set_visible(false);
    }

    let state = UiState::from_snapshot(
        snapshot,
        VERSION,
        hotkeys.is_supported(),
        log_dir,
        start_hidden,
    );

    let mut app = KavoxApp::new(controller, hotkeys, raw_tx.clone(), ui_rx, state, log_guard);

    // Linux starts hidden by not opening a window at all (see `app::HIDE_BY_CLOSING`).
    let mut open_window = !(start_hidden && app::HIDE_BY_CLOSING);
    loop {
        if open_window {
            let visible = app.window_visible();
            let size = app.window_size();
            let wake_ctx = egui_ctx.clone();
            let app_ref = &mut app;
            let result = eframe::run_native(
                "Kavox",
                native_options(visible, size),
                Box::new(move |cc| {
                    *wake_ctx.lock().unwrap_or_else(|e| e.into_inner()) = Some(cc.egui_ctx.clone());
                    app_ref.attach(&cc.egui_ctx);
                    Ok(Box::new(Hosted(app_ref)))
                }),
            );
            *egui_ctx.lock().unwrap_or_else(|e| e.into_inner()) = None;
            if let Err(err) = result {
                tracing::error!("eframe exited with error: {err}");
                break;
            }
            if !app.closed_to_tray() {
                break;
            }
        }
        if !app.wait_while_hidden() {
            break;
        }
        open_window = true;
    }
    app.shutdown_once();
}

fn native_options(visible: bool, size: Option<egui::Vec2>) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Kavox")
        .with_app_id("kavox")
        .with_inner_size(size.unwrap_or(egui::vec2(1040.0, 720.0)))
        .with_min_inner_size([380.0, 520.0])
        .with_visible(visible)
        .with_drag_and_drop(true);
    if let Some(icon) = platform::viewport_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    eframe::NativeOptions {
        viewport,
        // Handle clipboard shortcuts in `KavoxApp::raw_input_hook` so hotkey recording receives
        // exact `Event::Key` presses for Ctrl+C/X/V, Shift+Insert, Ctrl+Insert, Shift+Delete.
        clipboard_shortcuts: false,
        // Return from `run_native` when the window closes so Linux can reopen it from the tray.
        run_and_return: true,
        ..Default::default()
    }
}

/// Quits cleanly on SIGINT/SIGTERM/SIGHUP so shutdown still removes Kavox Mic (e.g. on logout).
#[cfg(unix)]
fn exit_on_signals(cmd_tx: Sender<AppCommand>) {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    match signal_hook::iterator::Signals::new([SIGINT, SIGTERM, SIGHUP]) {
        Ok(mut signals) => {
            let _ = thread::Builder::new()
                .name("kavox-signals".into())
                .spawn(move || {
                    if let Some(signal) = signals.forever().next() {
                        tracing::info!("received signal {signal}, quitting");
                        let _ = cmd_tx.send(AppCommand::Quit);
                    }
                });
        }
        Err(e) => tracing::warn!("can't watch for quit signals: {e}"),
    }
}

#[cfg(not(unix))]
fn exit_on_signals(_: Sender<AppCommand>) {}
