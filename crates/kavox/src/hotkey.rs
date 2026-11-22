//! Global hotkeys (`RegisterHotKey` on Windows, X11 grabs on Linux) plus hotkey recording & formatting.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use kavox_core::{Controller, HotkeyAction};

pub fn supported() -> bool {
    let wayland = cfg!(target_os = "linux")
        && (std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s.eq_ignore_ascii_case("wayland"))
            || std::env::var_os("WAYLAND_DISPLAY").is_some());
    !wayland
}

pub fn validate(hotkey: &str) -> Result<(), String> {
    hotkey
        .parse::<HotKey>()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub struct HotkeyService {
    manager: Option<GlobalHotKeyManager>,
    registered: Vec<HotKey>,
    actions: Arc<Mutex<HashMap<u32, HotkeyAction>>>,
    supported: bool,
}

impl HotkeyService {
    pub fn new(controller: Controller) -> Self {
        let is_supported = supported();
        let manager = if is_supported {
            match GlobalHotKeyManager::new() {
                Ok(m) => Some(m),
                Err(e) => {
                    tracing::warn!("global hotkeys unavailable: {e}");
                    None
                }
            }
        } else {
            None
        };

        let actions: Arc<Mutex<HashMap<u32, HotkeyAction>>> = Arc::new(Mutex::new(HashMap::new()));

        if manager.is_some() {
            let actions_bg = actions.clone();
            let _ = thread::Builder::new()
                .name("kavox-hotkeys".into())
                .spawn(move || {
                    let rx = GlobalHotKeyEvent::receiver();
                    let mut pressed: HashSet<u32> = HashSet::new();
                    while let Ok(event) = rx.recv() {
                        let action = {
                            let map = actions_bg.lock().unwrap_or_else(|e| e.into_inner());
                            match event.state {
                                HotKeyState::Pressed if pressed.insert(event.id) => {
                                    map.get(&event.id).cloned()
                                }
                                HotKeyState::Pressed => None,
                                HotKeyState::Released => {
                                    pressed.remove(&event.id);
                                    None
                                }
                            }
                        };
                        if let Some(action) = action {
                            run_action(&controller, action);
                        }
                    }
                });
        }

        Self {
            manager,
            registered: Vec::new(),
            actions,
            supported: is_supported,
        }
    }

    pub fn is_supported(&self) -> bool {
        self.supported
    }

    /// Re-registers every binding; returns the ones the OS refused as `(hotkey, reason)`.
    pub fn sync(&mut self, controller: &Controller) -> Vec<(String, String)> {
        if !self.supported {
            return Vec::new();
        }
        let Some(manager) = &self.manager else {
            return Vec::new();
        };

        let bindings = controller.lock().hotkey_bindings();

        if !self.registered.is_empty() {
            let _ = manager.unregister_all(&self.registered);
            self.registered.clear();
        }

        let mut map = self.actions.lock().unwrap_or_else(|e| e.into_inner());
        map.clear();

        let mut failures = Vec::new();
        for (hotkey, action) in bindings {
            let result = hotkey
                .parse::<HotKey>()
                .map_err(|e| e.to_string())
                .and_then(|hk| {
                    manager.register(hk).map_err(|e| e.to_string())?;
                    Ok(hk)
                });
            match result {
                Ok(hk) => {
                    map.insert(hk.id(), action);
                    self.registered.push(hk);
                }
                Err(e) => failures.push((hotkey, e)),
            }
        }
        failures
    }
}

fn run_action(controller: &Controller, action: HotkeyAction) {
    let mut core = controller.lock();
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
        HotkeyAction::SetVoicePreset(id) => {
            if let Err(e) = core.toggle_voice_preset(&id) {
                tracing::warn!("hotkey voice failed: {e}");
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordResult {
    Cancel,
    Clear,
    Set(String),
    Unsupported(String),
}

/// Converts an `egui::Key` + `Modifiers` press into a `global-hotkey` accelerator string.
pub fn from_egui_key(key: egui::Key, modifiers: egui::Modifiers) -> RecordResult {
    let mut mods: Vec<&'static str> = Vec::new();
    if modifiers.ctrl {
        mods.push("Ctrl");
    }
    if modifiers.alt {
        mods.push("Alt");
    }
    if modifiers.shift {
        mods.push("Shift");
    }
    if modifiers.mac_cmd || (modifiers.command && !modifiers.ctrl) {
        mods.push("Super");
    }

    if mods.is_empty() && key == egui::Key::Escape {
        return RecordResult::Cancel;
    }
    if mods.is_empty() && matches!(key, egui::Key::Backspace | egui::Key::Delete) {
        return RecordResult::Clear;
    }

    let Some(token) = egui_key_token(key) else {
        return RecordResult::Unsupported(key.name().to_owned());
    };

    if mods.is_empty() {
        RecordResult::Set(token.to_owned())
    } else {
        mods.push(token);
        RecordResult::Set(mods.join("+"))
    }
}

fn egui_key_token(key: egui::Key) -> Option<&'static str> {
    use egui::Key::*;
    Some(match key {
        A => "A",
        B => "B",
        C => "C",
        D => "D",
        E => "E",
        F => "F",
        G => "G",
        H => "H",
        I => "I",
        J => "J",
        K => "K",
        L => "L",
        M => "M",
        N => "N",
        O => "O",
        P => "P",
        Q => "Q",
        R => "R",
        S => "S",
        T => "T",
        U => "U",
        V => "V",
        W => "W",
        X => "X",
        Y => "Y",
        Z => "Z",
        Num0 => "0",
        Num1 => "1",
        Num2 => "2",
        Num3 => "3",
        Num4 => "4",
        Num5 => "5",
        Num6 => "6",
        Num7 => "7",
        Num8 => "8",
        Num9 => "9",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        F13 => "F13",
        F14 => "F14",
        F15 => "F15",
        F16 => "F16",
        F17 => "F17",
        F18 => "F18",
        F19 => "F19",
        F20 => "F20",
        F21 => "F21",
        F22 => "F22",
        F23 => "F23",
        F24 => "F24",
        Backtick => "Backquote",
        Backslash => "Backslash",
        OpenBracket => "BracketLeft",
        CloseBracket => "BracketRight",
        Comma => "Comma",
        Equals => "Equal",
        Minus => "Minus",
        Period => "Period",
        Quote => "Quote",
        Semicolon => "Semicolon",
        Slash => "Slash",
        Enter => "Enter",
        Space => "Space",
        Tab => "Tab",
        Delete => "Delete",
        End => "End",
        Home => "Home",
        Insert => "Insert",
        PageDown => "PageDown",
        PageUp => "PageUp",
        ArrowDown => "ArrowDown",
        ArrowLeft => "ArrowLeft",
        ArrowRight => "ArrowRight",
        ArrowUp => "ArrowUp",
        Backspace => "Backspace",
        _ => return None,
    })
}

/// Keycap labels for display, e.g. `"Ctrl+Numpad5"` -> `["Ctrl", "Num 5"]`.
pub fn keycaps(hotkey: &str, platform: &str) -> Vec<String> {
    hotkey
        .split('+')
        .map(|part| {
            if part == "Super" {
                return if platform == "windows" {
                    "Win".into()
                } else {
                    "Super".into()
                };
            }
            if let Some(digit) = part.strip_prefix("Numpad")
                && digit.len() == 1
                && digit.chars().all(|c| c.is_ascii_digit())
            {
                return format!("Num {digit}");
            }
            match part {
                "Backquote" => "`",
                "Backslash" => "\\",
                "BracketLeft" => "[",
                "BracketRight" => "]",
                "Comma" => ",",
                "Equal" => "=",
                "Minus" => "-",
                "Period" => ".",
                "Quote" => "'",
                "Semicolon" => ";",
                "Slash" => "/",
                "ArrowUp" => "↑",
                "ArrowDown" => "↓",
                "ArrowLeft" => "←",
                "ArrowRight" => "→",
                "NumpadAdd" => "Num +",
                "NumpadSubtract" => "Num −",
                "NumpadMultiply" => "Num ×",
                "NumpadDivide" => "Num ÷",
                "NumpadDecimal" => "Num .",
                "NumpadEnter" => "Num Enter",
                "NumpadEqual" => "Num =",
                "PageUp" => "Page Up",
                "PageDown" => "Page Down",
                "PrintScreen" => "Print Screen",
                "AudioVolumeUp" => "Volume up",
                "AudioVolumeDown" => "Volume down",
                "AudioVolumeMute" => "Mute",
                "MediaPlayPause" => "Play/Pause",
                "MediaStop" => "Media stop",
                "MediaTrackNext" => "Next track",
                "MediaTrackPrevious" => "Previous track",
                other => other,
            }
            .to_owned()
        })
        .collect()
}

/// True when a binding would swallow a key people type with.
pub fn is_risky(hotkey: &str) -> bool {
    let parts: Vec<&str> = hotkey.split('+').collect();
    let Some(&key) = parts.last() else {
        return false;
    };
    let mods = &parts[..parts.len() - 1];
    if mods.iter().any(|&m| m != "Shift") {
        return false;
    }
    if key.len() == 1
        && key
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return true;
    }
    matches!(
        key,
        "Space"
            | "Enter"
            | "Tab"
            | "Backspace"
            | "Minus"
            | "Equal"
            | "Comma"
            | "Period"
            | "Slash"
            | "Semicolon"
            | "Quote"
            | "BracketLeft"
            | "BracketRight"
            | "Backslash"
            | "Backquote"
            | "Delete"
            | "CapsLock"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_keycaps_for_platform() {
        assert_eq!(keycaps("Ctrl+Numpad5", "linux"), vec!["Ctrl", "Num 5"]);
        assert_eq!(keycaps("Super+ArrowUp", "windows"), vec!["Win", "↑"]);
        assert_eq!(keycaps("Super+F13", "linux"), vec!["Super", "F13"]);
    }

    #[test]
    fn detects_risky_typing_hotkeys() {
        assert!(is_risky("A"));
        assert!(is_risky("Shift+A"));
        assert!(is_risky("Space"));
        assert!(!is_risky("Ctrl+A"));
        assert!(!is_risky("F13"));
        assert!(!is_risky("Numpad5"));
    }
}
