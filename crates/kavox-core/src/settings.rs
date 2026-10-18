use serde::{Deserialize, Serialize};

use crate::persist::Versioned;

pub const SETTINGS_SCHEMA: u32 = 1;

impl Versioned for Settings {
    const CURRENT: u32 = SETTINGS_SCHEMA;
    fn schema(&self) -> u32 {
        self.schema
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema: u32,
    pub audio: AudioSettings,
    pub hotkeys: GlobalHotkeys,
    pub app: AppSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SETTINGS_SCHEMA,
            audio: AudioSettings::default(),
            hotkeys: GlobalHotkeys::default(),
            app: AppSettings::default(),
        }
    }
}

/// `None` device means "system default" (for the virtual output on Windows: "auto-detect a cable").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    pub mic_device: Option<String>,
    pub virtual_output: Option<String>,
    pub monitor_output: Option<String>,
    pub mic_passthrough: bool,
    pub mic_gain: f32,
    pub mic_muted: bool,
    pub sounds_to_mic_gain: f32,
    pub sounds_to_monitor_gain: f32,
    pub monitor_enabled: bool,
    pub normalize: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            mic_device: None,
            virtual_output: None,
            monitor_output: None,
            mic_passthrough: true,
            mic_gain: 1.0,
            mic_muted: false,
            sounds_to_mic_gain: 0.8,
            sounds_to_monitor_gain: 0.5,
            monitor_enabled: true,
            normalize: true,
        }
    }
}

impl AudioSettings {
    /// Clamps gains into their slider ranges.
    pub fn sanitized(mut self) -> Self {
        let clamp = |g: f32, max: f32| {
            if g.is_finite() {
                g.clamp(0.0, max)
            } else {
                1.0
            }
        };
        self.mic_gain = clamp(self.mic_gain, 2.0);
        self.sounds_to_mic_gain = clamp(self.sounds_to_mic_gain, 2.0);
        self.sounds_to_monitor_gain = clamp(self.sounds_to_monitor_gain, 2.0);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GlobalHotkeys {
    pub stop_all: Option<String>,
    pub toggle_mute: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub start_minimized: bool,
    pub close_to_tray: bool,
    pub launch_on_login: bool,
    pub theme: Theme,
    pub setup_complete: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            start_minimized: false,
            close_to_tray: true,
            launch_on_login: false,
            theme: Theme::System,
            setup_complete: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"schema":1,"audio":{"micGain":0.5}}"#).unwrap();
        assert_eq!(s.audio.mic_gain, 0.5);
        assert!(s.audio.mic_passthrough);
        assert!(s.app.close_to_tray);
    }

    #[test]
    fn sanitizes_gains() {
        let a = AudioSettings {
            mic_gain: 9.0,
            sounds_to_mic_gain: f32::NAN,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(a.mic_gain, 2.0);
        assert_eq!(a.sounds_to_mic_gain, 1.0);
    }
}
