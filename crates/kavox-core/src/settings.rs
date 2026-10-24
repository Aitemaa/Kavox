use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::engine::PitchParams;
use crate::persist::Versioned;
use crate::voice;

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
    pub voice: VoiceSettings,
    pub app: AppSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SETTINGS_SCHEMA,
            audio: AudioSettings::default(),
            hotkeys: GlobalHotkeys::default(),
            voice: VoiceSettings::default(),
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

/// Voice changer state. `preset` is a `voice::PRESETS` id, `normal` or `custom` (use `semitones`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VoiceSettings {
    pub enabled: bool,
    pub preset: String,
    /// Pitch dial position, used when `preset` is `custom`.
    pub semitones: f32,
    pub hear_myself: bool,
    /// Preset id to global hotkey.
    pub hotkeys: BTreeMap<String, String>,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            preset: voice::NORMAL.into(),
            semitones: 0.0,
            hear_myself: false,
            hotkeys: BTreeMap::new(),
        }
    }
}

impl VoiceSettings {
    pub fn pitch_params(&self) -> PitchParams {
        let (semitones, wobble_depth, wobble_hz) = match voice::preset(&self.preset) {
            Some(p) => (p.semitones, p.wobble_depth, p.wobble_hz),
            None if self.preset == voice::CUSTOM => (self.semitones, 0.0, 0.0),
            None => (0.0, 0.0, 0.0),
        };
        PitchParams {
            enabled: self.enabled,
            semitones,
            wobble_depth,
            wobble_hz,
        }
    }

    /// Falls back to safe values for anything out of range or unknown (e.g. from a newer version's file).
    pub fn sanitized(mut self) -> Self {
        self.semitones = if self.semitones.is_finite() {
            self.semitones.clamp(-12.0, 12.0)
        } else {
            0.0
        };
        if self.preset != voice::NORMAL
            && self.preset != voice::CUSTOM
            && voice::preset(&self.preset).is_none()
        {
            self.preset = voice::NORMAL.into();
        }
        self.hotkeys.retain(|id, _| voice::preset(id).is_some());
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

    #[test]
    fn old_settings_files_get_default_voice() {
        let s: Settings = serde_json::from_str(r#"{"schema":1,"audio":{}}"#).unwrap();
        assert_eq!(s.voice, VoiceSettings::default());
        assert!(!s.voice.enabled);
        assert_eq!(s.voice.pitch_params(), PitchParams::default());
    }

    #[test]
    fn voice_params_follow_the_preset() {
        let mut v = VoiceSettings {
            enabled: true,
            preset: "wobbly".into(),
            semitones: 7.0,
            ..Default::default()
        };
        let p = v.pitch_params();
        assert!(p.enabled);
        assert_eq!((p.semitones, p.wobble_depth, p.wobble_hz), (0.0, 1.5, 5.0));
        v.preset = voice::CUSTOM.into();
        assert_eq!(v.pitch_params().semitones, 7.0);
        assert_eq!(v.pitch_params().wobble_depth, 0.0);
    }

    #[test]
    fn sanitizes_voice() {
        let v = VoiceSettings {
            preset: "gone".into(),
            semitones: f32::NAN,
            hotkeys: BTreeMap::from([("gone".into(), "F1".into()), ("giant".into(), "F2".into())]),
            ..Default::default()
        }
        .sanitized();
        assert_eq!(v.preset, voice::NORMAL);
        assert_eq!(v.semitones, 0.0);
        assert_eq!(v.hotkeys.keys().collect::<Vec<_>>(), ["giant"]);
        let far = VoiceSettings {
            semitones: 40.0,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(far.semitones, 12.0);
    }
}
