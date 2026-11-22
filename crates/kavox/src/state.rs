//! UI state, background command messages, and shared format/import helpers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use kavox_core::{
    AppSettings, AudioSettings, Board, Controller, DeviceList, Levels, Library, NoticeLevel,
    PlaybackEvent, PlaybackState, Settings, Sound, StreamStatus, UiEvent, VirtualStatus,
    VoiceSettings, clip, prepare_import,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Sounds,
    Voice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Audio,
    Hotkeys,
    App,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    Settings,
    Wizard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compositor {
    Niri,
    Hyprland,
    Sway,
    Other,
}

#[derive(Debug, Clone)]
pub struct VoiceProgress {
    pub voice_id: u64,
    pub frame: usize,
    pub frames: usize,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: u64,
    pub level: NoticeLevel,
    pub message: String,
    pub expires_at: Instant,
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    DeleteSound(String),
    DeleteBoard(String),
}

#[derive(Debug, Clone)]
pub struct Confirm {
    pub title: String,
    pub message: String,
    pub action_label: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone)]
pub struct ImportFailure {
    pub file: String,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct ImportDone {
    pub added: Vec<Sound>,
    pub failed: Vec<ImportFailure>,
}

/// Messages sent from background threads (core events, tray, IPC, signals, file importer) to the UI.
#[derive(Debug)]
pub enum AppCommand {
    CoreEvent(UiEvent),
    ImportFinished(Result<ImportDone, String>),
    ShowWindow,
    Quit,
}

pub struct UiState {
    pub version: &'static str,
    pub platform: &'static str,
    pub hotkeys_supported: bool,
    pub read_only: bool,
    pub log_dir: PathBuf,

    pub settings: Settings,
    pub library: Library,
    pub devices: DeviceList,
    pub virtual_status: VirtualStatus,
    pub streams: StreamStatus,
    pub playing: HashMap<String, Vec<VoiceProgress>>,
    pub levels: Levels,

    pub toasts: Vec<Toast>,
    pub next_toast_id: u64,

    pub view: View,
    pub search: String,
    pub focus_search: bool,

    pub editing: Option<String>,
    pub editor_name_draft: String,
    pub editor_sound_id: Option<String>,

    pub dialog: Option<Dialog>,
    pub settings_tab: SettingsTab,
    pub wizard_step: usize,
    pub wizard_checking: bool,
    pub compositor: Compositor,
    pub confirm: Option<Confirm>,

    // TopBar board management state
    pub renaming_board: Option<String>,
    pub creating_board: bool,
    pub board_name_draft: String,
    pub board_name_focus: bool,
    pub board_menu_id: Option<String>,
    pub board_menu_pos: eframe::egui::Pos2,

    // Pad interaction & animation state
    pub pad_hit_at: HashMap<String, Instant>,
    pub dragging_pad: Option<String>,
    pub drag_origin: eframe::egui::Pos2,
    pub drag_active: bool,
    pub focused_pad: Option<String>,

    // Hotkey recorder state
    pub recording_hotkey: Option<String>,
    pub hotkey_errors: HashMap<String, String>,

    // Voice hotkeys disclosure state
    pub voice_hotkeys_open: bool,

    // Keep in-flight slider edits from jumping back on echoed Settings events
    pub last_audio_edit: Option<Instant>,
    pub last_voice_edit: Option<Instant>,

    pub window_visible: bool,
    pub quitting: bool,
}

impl UiState {
    pub fn from_snapshot(
        snapshot: kavox_core::Snapshot,
        version: &'static str,
        hotkeys_supported: bool,
        log_dir: PathBuf,
        start_hidden: bool,
    ) -> Self {
        let mut playing: HashMap<String, Vec<VoiceProgress>> = HashMap::new();
        for p in snapshot.playing {
            playing.entry(p.sound_id).or_default().push(VoiceProgress {
                voice_id: p.voice_id,
                frame: p.frame,
                frames: p.frames,
            });
        }

        let dialog = if snapshot.settings.app.setup_complete {
            None
        } else {
            Some(Dialog::Wizard)
        };

        let mut state = Self {
            version,
            platform: snapshot.platform,
            hotkeys_supported,
            read_only: snapshot.read_only,
            log_dir,
            settings: snapshot.settings,
            library: snapshot.library,
            devices: snapshot.devices,
            virtual_status: snapshot.virtual_status,
            streams: snapshot.streams,
            playing,
            levels: Levels {
                mic: 0.0,
                virtual_out: 0.0,
                monitor: 0.0,
            },
            toasts: Vec::new(),
            next_toast_id: 0,
            view: View::Sounds,
            search: String::new(),
            focus_search: false,
            editing: None,
            editor_name_draft: String::new(),
            editor_sound_id: None,
            dialog,
            settings_tab: SettingsTab::Audio,
            wizard_step: 0,
            wizard_checking: false,
            compositor: Compositor::Niri,
            confirm: None,
            renaming_board: None,
            creating_board: false,
            board_name_draft: String::new(),
            board_name_focus: false,
            board_menu_id: None,
            board_menu_pos: eframe::egui::Pos2::ZERO,
            pad_hit_at: HashMap::new(),
            dragging_pad: None,
            drag_origin: eframe::egui::Pos2::ZERO,
            drag_active: false,
            focused_pad: None,
            recording_hotkey: None,
            hotkey_errors: HashMap::new(),
            voice_hotkeys_open: false,
            last_audio_edit: None,
            last_voice_edit: None,
            window_visible: !start_hidden,
            quitting: false,
        };

        for n in snapshot.notices {
            state.push_toast(n.level, n.message);
        }
        state
    }

    pub fn push_toast(&mut self, level: NoticeLevel, message: impl Into<String>) {
        self.next_toast_id += 1;
        let ttl = if level == NoticeLevel::Error {
            Duration::from_secs(10)
        } else {
            Duration::from_secs(6)
        };
        if self.toasts.len() >= 4 {
            let drain = self.toasts.len() - 3;
            self.toasts.drain(0..drain);
        }
        self.toasts.push(Toast {
            id: self.next_toast_id,
            level,
            message: message.into(),
            expires_at: Instant::now() + ttl,
        });
    }

    pub fn dismiss_toast(&mut self, id: u64) {
        self.toasts.retain(|t| t.id != id);
    }

    pub fn prune_toasts(&mut self) {
        let now = Instant::now();
        self.toasts.retain(|t| t.expires_at > now);
    }

    pub fn active_board(&self) -> Option<&Board> {
        self.library
            .boards
            .iter()
            .find(|b| b.id == self.library.active_board)
            .or_else(|| self.library.boards.first())
    }

    pub fn open_settings(&mut self, tab: SettingsTab) {
        self.dialog = Some(Dialog::Settings);
        self.settings_tab = tab;
        self.board_menu_id = None;
    }

    pub fn set_view(&mut self, view: View) {
        self.view = view;
        self.editing = None;
        self.board_menu_id = None;
    }

    pub fn start_editing(&mut self, id: String) {
        if let Some(sound) = self.library.sounds.get(&id) {
            self.editor_name_draft = sound.name.clone();
            self.editor_sound_id = Some(id.clone());
            self.editing = Some(id);
            self.recording_hotkey = None;
        }
    }

    pub fn confirm_delete_sound(&mut self, id: &str) {
        let Some(sound) = self.library.sounds.get(id) else {
            return;
        };
        self.confirm = Some(Confirm {
            title: format!("Delete \"{}\"?", sound.name),
            message: "The sound and its hotkey are removed from Kavox. The original file on your computer stays where it is.".into(),
            action_label: "Delete sound".into(),
            action: ConfirmAction::DeleteSound(id.to_owned()),
        });
    }

    pub fn confirm_delete_board(&mut self, board: &Board) {
        let count = board.sounds.len();
        let message = if count > 0 {
            format!(
                "Its {count} {} removed from Kavox too. Original files on your computer aren't touched.",
                if count == 1 { "sound is" } else { "sounds are" }
            )
        } else {
            "The board is empty, so nothing else is removed.".into()
        };
        self.confirm = Some(Confirm {
            title: format!("Delete {}?", board.name),
            message,
            action_label: "Delete board".into(),
            action: ConfirmAction::DeleteBoard(board.id.clone()),
        });
    }

    pub fn apply_core_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::Playback(p) => self.apply_playback(p),
            UiEvent::Levels(levels) => self.levels = levels,
            UiEvent::Devices(devices) => {
                self.devices = devices;
                self.wizard_checking = false;
            }
            UiEvent::VirtualStatus(status) => self.virtual_status = status,
            UiEvent::Library(library) => {
                if let Some(id) = &self.editing {
                    if let Some(sound) = library.sounds.get(id) {
                        if self.editor_sound_id.as_deref() != Some(id.as_str()) {
                            self.editor_name_draft = sound.name.clone();
                            self.editor_sound_id = Some(id.clone());
                        }
                    } else {
                        self.editing = None;
                        self.editor_sound_id = None;
                    }
                }
                self.library = library;
            }
            UiEvent::Settings(next) => {
                let now = Instant::now();
                let keep_audio = self
                    .last_audio_edit
                    .is_some_and(|t| now.duration_since(t) < Duration::from_millis(400));
                let keep_voice = self
                    .last_voice_edit
                    .is_some_and(|t| now.duration_since(t) < Duration::from_millis(400));

                let audio = if keep_audio {
                    self.settings.audio.clone()
                } else {
                    next.audio
                };
                let voice = if keep_voice {
                    VoiceSettings {
                        hotkeys: next.voice.hotkeys,
                        ..self.settings.voice.clone()
                    }
                } else {
                    next.voice
                };

                self.settings = Settings {
                    schema: next.schema,
                    audio,
                    hotkeys: next.hotkeys,
                    voice,
                    app: next.app,
                };
            }
            UiEvent::Streams(streams) => self.streams = streams,
            UiEvent::Notice(notice) => self.push_toast(notice.level, notice.message),
        }
    }

    fn apply_playback(&mut self, p: PlaybackEvent) {
        let voices = self.playing.entry(p.sound_id.clone()).or_default();
        if p.state == PlaybackState::Ended {
            voices.retain(|v| v.voice_id != p.voice_id);
            if voices.is_empty() {
                self.playing.remove(&p.sound_id);
            }
        } else {
            let voice = VoiceProgress {
                voice_id: p.voice_id,
                frame: p.frame,
                frames: p.frames,
            };
            if let Some(existing) = voices.iter_mut().find(|v| v.voice_id == p.voice_id) {
                *existing = voice;
            } else {
                voices.push(voice);
            }
        }
    }

    pub fn update_audio(
        &mut self,
        controller: &Controller,
        update: impl FnOnce(&mut AudioSettings),
    ) {
        update(&mut self.settings.audio);
        self.last_audio_edit = Some(Instant::now());
        controller
            .lock()
            .set_audio_settings(self.settings.audio.clone());
    }

    pub fn update_voice(
        &mut self,
        controller: &Controller,
        update: impl FnOnce(&mut VoiceSettings),
    ) {
        update(&mut self.settings.voice);
        self.last_voice_edit = Some(Instant::now());
        controller
            .lock()
            .set_voice_changer(self.settings.voice.clone());
    }

    pub fn update_app(&mut self, controller: &Controller, update: impl FnOnce(&mut AppSettings)) {
        let prev = self.settings.app.clone();
        update(&mut self.settings.app);
        if self.settings.app.launch_on_login != prev.launch_on_login
            && let Err(err) = crate::platform::set_autostart(self.settings.app.launch_on_login)
        {
            self.settings.app = prev;
            self.push_toast(NoticeLevel::Error, err);
            return;
        }
        controller
            .lock()
            .set_app_settings(self.settings.app.clone());
    }
}

pub fn format_duration(ms: u64) -> String {
    if ms < 10_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        let total = (ms as f64 / 1000.0).round() as u64;
        if total < 60 {
            format!("{total} s")
        } else {
            format!("{}:{:02}", total / 60, total % 60)
        }
    }
}

/// What voice apps should pick as their microphone, if Kavox knows.
pub fn mic_label(status: &VirtualStatus) -> Option<&str> {
    match status {
        VirtualStatus::Ok { mic_label, .. }
        | VirtualStatus::CableIsDefaultOutput { mic_label, .. } => Some(mic_label.as_str()),
        _ => None,
    }
}

/// Peak (0–1) to meter fill on a −60…0 dBFS scale.
pub fn meter_fill(peak: f32) -> f32 {
    if peak <= 0.0 {
        return 0.0;
    }
    ((20.0 * peak.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

pub fn expand_import_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
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

pub fn spawn_import_paths(
    controller: Controller,
    board: Option<String>,
    paths: Vec<PathBuf>,
    tx: Sender<AppCommand>,
    ctx: eframe::egui::Context,
) {
    if paths.is_empty() {
        return;
    }
    thread::Builder::new()
        .name("kavox-import".into())
        .spawn(move || {
            let result = import_blocking(&controller, board.as_deref(), paths);
            let _ = tx.send(AppCommand::ImportFinished(result));
            ctx.request_repaint();
        })
        .ok();
}

pub fn spawn_pick_and_import(
    controller: Controller,
    board: Option<String>,
    tx: Sender<AppCommand>,
    ctx: eframe::egui::Context,
) {
    thread::Builder::new()
        .name("kavox-picker".into())
        .spawn(move || {
            let paths = crate::platform::pick_audio_files();
            if paths.is_empty() {
                return;
            }
            let result = import_blocking(&controller, board.as_deref(), paths);
            let _ = tx.send(AppCommand::ImportFinished(result));
            ctx.request_repaint();
        })
        .ok();
}

fn import_blocking(
    controller: &Controller,
    board: Option<&str>,
    paths: Vec<PathBuf>,
) -> Result<ImportDone, String> {
    let (sounds_dir, mut hashes) = controller.lock().import_context();
    let mut prepared = Vec::new();
    let mut failed = Vec::new();
    for path in expand_import_paths(paths) {
        match prepare_import(&path, &sounds_dir, &hashes) {
            Ok(sound) => {
                hashes.insert(sound.sha256.clone(), sound.file.clone());
                prepared.push(sound);
            }
            Err(e) => failed.push(ImportFailure {
                file: file_name(&path),
                error: e.to_string(),
            }),
        }
    }
    let added = controller
        .lock()
        .add_sounds(board, prepared)
        .map_err(|e| e.to_string())?;
    Ok(ImportDone { added, failed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_durations_like_original_ui() {
        assert_eq!(format_duration(500), "0.5 s");
        assert_eq!(format_duration(2310), "2.3 s");
        assert_eq!(format_duration(12_400), "12 s");
        assert_eq!(format_duration(65_000), "1:05");
    }

    #[test]
    fn computes_meter_fill_db_scale() {
        assert_eq!(meter_fill(0.0), 0.0);
        assert_eq!(meter_fill(1.0), 1.0);
        assert!((meter_fill(0.001) - 0.0).abs() < 1e-4);
        assert!((meter_fill(0.1) - 2.0 / 3.0).abs() < 1e-3);
    }
}
