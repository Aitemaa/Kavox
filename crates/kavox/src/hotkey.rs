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

#[derive(Default)]
struct HotkeyStateMap {
    actions: HashMap<u32, HotkeyAction>,
    pressed: HashSet<u32>,
}

pub struct HotkeyService {
    manager: Option<GlobalHotKeyManager>,
    registered: Vec<HotKey>,
    state: Arc<Mutex<HotkeyStateMap>>,
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

        let state = Arc::new(Mutex::new(HotkeyStateMap::default()));

        if manager.is_some() {
            let state_bg = state.clone();
            let _ = thread::Builder::new()
                .name("kavox-hotkeys".into())
                .spawn(move || {
                    let rx = GlobalHotKeyEvent::receiver();
                    while let Ok(event) = rx.recv() {
                        let action = {
                            let mut guard = state_bg.lock().unwrap_or_else(|e| e.into_inner());
                            match event.state {
                                HotKeyState::Pressed if guard.pressed.insert(event.id) => {
                                    guard.actions.get(&event.id).cloned()
                                }
                                HotKeyState::Pressed => None,
                                HotKeyState::Released => {
                                    guard.pressed.remove(&event.id);
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
            state,
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

        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        guard.actions.clear();
        // Drop any held-key tracking so re-registration while a key is held cannot strand it.
        guard.pressed.clear();

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
                    guard.actions.insert(hk.id(), action);
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
    Ignore,
    Cancel,
    Clear,
    Set(String),
    Unsupported(String),
}

pub fn is_modifier_key(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ShiftLeft
            | egui::Key::ShiftRight
            | egui::Key::ControlLeft
            | egui::Key::ControlRight
            | egui::Key::AltLeft
            | egui::Key::AltRight
            | egui::Key::SuperLeft
            | egui::Key::SuperRight
    )
}

/// Converts a recorded `egui` key event into a `global-hotkey` accelerator string.
///
/// Uses `physical_key` (falling back to logical `key`) so non-QWERTY layouts such as AZERTY
/// record the physical key that `global-hotkey` registers, detects numpad keys explicitly, and
/// includes `Super` when `super_down` is true on Linux/Windows.
pub fn from_egui_event(
    logical_key: egui::Key,
    physical_key: Option<egui::Key>,
    modifiers: egui::Modifiers,
    super_down: bool,
    platform_numpad: Option<&'static str>,
) -> RecordResult {
    let effective_key = physical_key.unwrap_or(logical_key);
    if is_modifier_key(effective_key) || is_modifier_key(logical_key) {
        return RecordResult::Ignore;
    }

    let mods = modifier_tokens(modifiers, super_down);

    if mods.is_empty() && effective_key == egui::Key::Escape {
        return RecordResult::Cancel;
    }
    if mods.is_empty() && matches!(effective_key, egui::Key::Backspace | egui::Key::Delete) {
        return RecordResult::Clear;
    }

    let token = platform_numpad
        .or_else(|| detect_numlock_off_numpad(logical_key, physical_key))
        .or_else(|| egui_key_token(effective_key));

    let Some(token) = token else {
        return RecordResult::Unsupported(effective_key.name().to_owned());
    };

    RecordResult::Set(format_combo(mods, token))
}

/// Formats an explicitly detected token (e.g. from `Event::Text` + hardware numpad check).
pub fn from_token(
    token: &'static str,
    modifiers: egui::Modifiers,
    super_down: bool,
) -> RecordResult {
    let mods = modifier_tokens(modifiers, super_down);
    RecordResult::Set(format_combo(mods, token))
}

fn modifier_tokens(modifiers: egui::Modifiers, super_down: bool) -> Vec<&'static str> {
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
    if super_down || modifiers.mac_cmd || (modifiers.command && !modifiers.ctrl) {
        mods.push("Super");
    }
    mods
}

fn format_combo(mut mods: Vec<&'static str>, token: &'static str) -> String {
    if mods.is_empty() {
        token.to_owned()
    } else {
        mods.push(token);
        mods.join("+")
    }
}

/// Cross-platform fallback when NumLock is off: `egui-winit` sends e.g. `logical_key = ArrowLeft`
/// paired with `physical_key = Some(Key::Num4)` for `Numpad4`.
fn detect_numlock_off_numpad(
    logical_key: egui::Key,
    physical_key: Option<egui::Key>,
) -> Option<&'static str> {
    use egui::Key::*;
    let phys = physical_key?;
    match (phys, logical_key) {
        (Num0, Insert) => Some("Numpad0"),
        (Num1, End) => Some("Numpad1"),
        (Num2, ArrowDown) => Some("Numpad2"),
        (Num3, PageDown) => Some("Numpad3"),
        (Num4, ArrowLeft) => Some("Numpad4"),
        (Num5, Clear) => Some("Numpad5"),
        (Num6, ArrowRight) => Some("Numpad6"),
        (Num7, Home) => Some("Numpad7"),
        (Num8, ArrowUp) => Some("Numpad8"),
        (Num9, PageUp) => Some("Numpad9"),
        _ => None,
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
        // `egui-winit` only maps `KeyCode::NumpadAdd` to physical `Key::Plus`.
        Plus => "NumpadAdd",
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
        PrintScreen => "PrintScreen",
        Pause => "Pause",
        MediaPlayPause => "MediaPlayPause",
        MediaTrackNext => "MediaTrackNext",
        MediaTrackPrevious => "MediaTrackPrevious",
        MediaStop => "MediaStop",
        AudioVolumeMute => "AudioVolumeMute",
        AudioVolumeDown => "AudioVolumeDown",
        AudioVolumeUp => "AudioVolumeUp",
        _ => return None,
    })
}

/// Platform hardware probe for numpad keys that `egui-winit` collapses into top-row keys or drops.
pub fn platform_numpad_for_key(key: egui::Key) -> Option<&'static str> {
    use egui::Key::*;
    let expected: &'static str = match key {
        Num0 => "Numpad0",
        Num1 => "Numpad1",
        Num2 => "Numpad2",
        Num3 => "Numpad3",
        Num4 => "Numpad4",
        Num5 => "Numpad5",
        Num6 => "Numpad6",
        Num7 => "Numpad7",
        Num8 => "Numpad8",
        Num9 => "Numpad9",
        Enter => "NumpadEnter",
        Slash => "NumpadDivide",
        Minus => "NumpadSubtract",
        Plus => "NumpadAdd",
        Period => "NumpadDecimal",
        Equals => "NumpadEqual",
        _ => return None,
    };
    platform_probe::is_numpad_down(expected).then_some(expected)
}

/// Checks if a dropped numpad operator (`*`, `.`, `=`) is physically pressed when `Event::Text` arrives.
pub fn platform_numpad_for_text(text: &str) -> Option<&'static str> {
    let expected = match text {
        "*" => "NumpadMultiply",
        "." | "," => "NumpadDecimal",
        "=" => "NumpadEqual",
        "+" => "NumpadAdd",
        "-" => "NumpadSubtract",
        "/" => "NumpadDivide",
        _ => return None,
    };
    platform_probe::is_numpad_down(expected).then_some(expected)
}

/// Extra OS-level check for Super/Win in case a Super press happened before window focus.
pub fn platform_super_down() -> bool {
    platform_probe::is_super_down()
}

#[cfg(target_os = "linux")]
mod platform_probe {
    use std::sync::{Mutex, OnceLock};
    use x11rb::protocol::xproto::ConnectionExt as _;
    use x11rb::rust_connection::RustConnection;

    static X11_CONN: OnceLock<Option<Mutex<RustConnection>>> = OnceLock::new();

    fn query_keymap() -> Option<[u8; 32]> {
        if !super::supported() {
            return None;
        }
        let conn = X11_CONN
            .get_or_init(|| x11rb::connect(None).ok().map(|(conn, _)| Mutex::new(conn)))
            .as_ref()?;
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        guard.query_keymap().ok()?.reply().ok().map(|r| r.keys)
    }

    fn keycode_down(keys: &[u8; 32], x11_keycode: u8) -> bool {
        let byte = (x11_keycode / 8) as usize;
        let bit = x11_keycode % 8;
        byte < keys.len() && (keys[byte] & (1 << bit)) != 0
    }

    pub fn is_numpad_down(token: &str) -> bool {
        // Standard Linux evdev + 8 X11 hardware keycodes for the numpad.
        let kc: u8 = match token {
            "Numpad0" => 90,
            "Numpad1" => 87,
            "Numpad2" => 88,
            "Numpad3" => 89,
            "Numpad4" => 83,
            "Numpad5" => 84,
            "Numpad6" => 85,
            "Numpad7" => 79,
            "Numpad8" => 80,
            "Numpad9" => 81,
            "NumpadMultiply" => 63,
            "NumpadAdd" => 86,
            "NumpadSubtract" => 82,
            "NumpadDecimal" => 91,
            "NumpadDivide" => 106,
            "NumpadEnter" => 104,
            "NumpadEqual" => 125,
            _ => return false,
        };
        query_keymap().is_some_and(|keys| keycode_down(&keys, kc))
    }

    pub fn is_super_down() -> bool {
        query_keymap().is_some_and(|keys| keycode_down(&keys, 133) || keycode_down(&keys, 134))
    }
}

#[cfg(windows)]
mod platform_probe {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetAsyncKeyState(vkey: i32) -> i16;
    }

    fn vk_down(vk: i32) -> bool {
        // SAFETY: `GetAsyncKeyState` is a thread-safe Win32 query with no pointer arguments.
        unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
    }

    pub fn is_numpad_down(token: &str) -> bool {
        let vk = match token {
            "Numpad0" => 0x60,
            "Numpad1" => 0x61,
            "Numpad2" => 0x62,
            "Numpad3" => 0x63,
            "Numpad4" => 0x64,
            "Numpad5" => 0x65,
            "Numpad6" => 0x66,
            "Numpad7" => 0x67,
            "Numpad8" => 0x68,
            "Numpad9" => 0x69,
            "NumpadMultiply" => 0x6A,
            "NumpadAdd" => 0x6B,
            "NumpadSubtract" => 0x6D,
            "NumpadDecimal" => 0x6E,
            "NumpadDivide" => 0x6F,
            _ => return false,
        };
        vk_down(vk)
    }

    pub fn is_super_down() -> bool {
        vk_down(0x5B) || vk_down(0x5C)
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod platform_probe {
    pub fn is_numpad_down(_token: &str) -> bool {
        false
    }

    pub fn is_super_down() -> bool {
        false
    }
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

    #[test]
    fn records_physical_key_super_and_numpad() {
        let none = egui::Modifiers::NONE;
        // AZERTY physical Q produces logical A, must record physical Q
        assert_eq!(
            from_egui_event(egui::Key::A, Some(egui::Key::Q), none, false, None),
            RecordResult::Set("Q".into())
        );
        // Super+F5 on Linux/Windows (where egui Modifiers has no super bit)
        assert_eq!(
            from_egui_event(egui::Key::F5, Some(egui::Key::F5), none, true, None),
            RecordResult::Set("Super+F5".into())
        );
        // Standalone modifier key press is ignored
        assert_eq!(
            from_egui_event(
                egui::Key::SuperLeft,
                Some(egui::Key::SuperLeft),
                none,
                true,
                None
            ),
            RecordResult::Ignore
        );
        // Physical Plus is NumpadAdd
        assert_eq!(
            from_egui_event(egui::Key::Plus, Some(egui::Key::Plus), none, false, None),
            RecordResult::Set("NumpadAdd".into())
        );
        // NumLock-off numpad 4 (logical ArrowLeft, physical Num4)
        assert_eq!(
            from_egui_event(
                egui::Key::ArrowLeft,
                Some(egui::Key::Num4),
                none,
                false,
                None
            ),
            RecordResult::Set("Numpad4".into())
        );
        // Platform-detected numpad 5 overrides top-row 5
        assert_eq!(
            from_egui_event(
                egui::Key::Num5,
                Some(egui::Key::Num5),
                egui::Modifiers::CTRL,
                false,
                Some("Numpad5")
            ),
            RecordResult::Set("Ctrl+Numpad5".into())
        );
    }

    #[test]
    fn all_emitted_tokens_parse_as_global_hotkeys() {
        for token in [
            "A",
            "0",
            "F1",
            "F24",
            "Backquote",
            "Backslash",
            "BracketLeft",
            "BracketRight",
            "Comma",
            "Equal",
            "Minus",
            "Period",
            "Quote",
            "Semicolon",
            "Slash",
            "Enter",
            "Space",
            "Tab",
            "Delete",
            "End",
            "Home",
            "Insert",
            "PageDown",
            "PageUp",
            "ArrowDown",
            "ArrowLeft",
            "ArrowRight",
            "ArrowUp",
            "Backspace",
            "PrintScreen",
            "Pause",
            "MediaPlayPause",
            "MediaTrackNext",
            "MediaTrackPrevious",
            "MediaStop",
            "AudioVolumeMute",
            "AudioVolumeDown",
            "AudioVolumeUp",
            "Numpad0",
            "Numpad5",
            "Numpad9",
            "NumpadAdd",
            "NumpadSubtract",
            "NumpadMultiply",
            "NumpadDivide",
            "NumpadDecimal",
            "NumpadEnter",
            "NumpadEqual",
            "Super+F5",
            "Ctrl+Alt+Shift+Numpad5",
        ] {
            assert!(
                validate(token).is_ok(),
                "token `{token}` failed global-hotkey validation: {:?}",
                validate(token)
            );
        }
    }
}
