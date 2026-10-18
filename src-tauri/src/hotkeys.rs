//! Global hotkeys through the OS (`RegisterHotKey` on Windows, X11 grabs on Linux).
//! Wayland has no general API for this, so there users bind `kavoxctl` in their compositor instead.

use std::collections::{HashMap, HashSet};

use kavox_core::HotkeyAction;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::{AppState, lock};

#[derive(Default)]
pub struct Registry {
    actions: HashMap<u32, HotkeyAction>,
    /// Shortcuts currently held down, so key repeat doesn't retrigger.
    pressed: HashSet<u32>,
}

pub fn supported() -> bool {
    let wayland = cfg!(target_os = "linux")
        && (std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s.eq_ignore_ascii_case("wayland"))
            || std::env::var_os("WAYLAND_DISPLAY").is_some());
    !wayland
}

pub fn validate(hotkey: &str) -> Result<(), String> {
    hotkey
        .parse::<Shortcut>()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Re-registers every binding; returns the ones the OS refused as (hotkey, reason).
pub fn sync(app: &AppHandle) -> Vec<(String, String)> {
    let state = app.state::<AppState>();
    if !state.hotkeys_supported {
        return Vec::new();
    }
    let bindings = state.controller.lock().hotkey_bindings();
    let shortcuts = app.global_shortcut();
    let mut registry = lock(&state.hotkeys);
    let _ = shortcuts.unregister_all();
    registry.actions.clear();
    registry.pressed.clear();

    let mut failures = Vec::new();
    for (hotkey, action) in bindings {
        let result = hotkey
            .parse::<Shortcut>()
            .map_err(|e| e.to_string())
            .and_then(|shortcut| {
                shortcuts.register(shortcut).map_err(|e| e.to_string())?;
                Ok(shortcut)
            });
        match result {
            Ok(shortcut) => {
                registry.actions.insert(shortcut.id(), action);
            }
            Err(e) => failures.push((hotkey, e)),
        }
    }
    failures
}

pub fn handle(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    let state = app.state::<AppState>();
    let action = {
        let mut registry = lock(&state.hotkeys);
        match event.state() {
            ShortcutState::Pressed if registry.pressed.insert(shortcut.id()) => {
                registry.actions.get(&shortcut.id()).cloned()
            }
            ShortcutState::Pressed => None,
            ShortcutState::Released => {
                registry.pressed.remove(&shortcut.id());
                None
            }
        }
    };
    if let Some(action) = action {
        run(app, action);
    }
}

pub fn run(app: &AppHandle, action: HotkeyAction) {
    let state = app.state::<AppState>();
    let mut core = state.controller.lock();
    match action {
        HotkeyAction::PlaySound(id) => {
            if let Err(e) = core.play_sound(&id) {
                tracing::warn!("hotkey play failed: {e}");
            }
        }
        HotkeyAction::StopAll => core.stop_all(),
        HotkeyAction::ToggleMute => {
            core.set_mic_muted(None);
        }
    }
}
