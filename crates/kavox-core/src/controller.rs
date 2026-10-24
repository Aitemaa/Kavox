//! The controller owns everything that isn't real-time: settings, library, streams, voices and saving.
//! UI commands, hotkeys and `kavoxctl` all lock it briefly; a ticker drains engine events at ~30 Hz.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::backend::{
    AudioBackend, DeviceList, StreamErrorFn, StreamHandle, StreamRole, VirtualStatus,
};
use crate::clip::{self, ClipCache, ClipData, Loader, RESIDENT_MAX_MS};
use crate::engine::{self, Command, EngineEvent, Meter, Meters, MicInput, RendererHandle, VoiceId};
use crate::error::{Error, Result};
use crate::library::{Board, Library, Retrigger, Sound, SoundPatch};
use crate::persist;
use crate::settings::{AppSettings, AudioSettings, Settings, VoiceSettings};
use crate::voice;

const TICK: Duration = Duration::from_millis(33);
const DEVICE_POLL: Duration = Duration::from_secs(2);
const SAVE_DELAY: Duration = Duration::from_millis(500);
const PENDING_PLAY_TIMEOUT: Duration = Duration::from_secs(3);
const RECENT_NOTICES: usize = 5;
const VIRTUAL_BIT: u8 = 1;
const MONITOR_BIT: u8 = 2;

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub sounds_dir: PathBuf,
    pub log_dir: PathBuf,
}

impl Paths {
    fn settings(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    fn library(&self) -> PathBuf {
        self.config_dir.join("boards.json")
    }
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: UiEvent);
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum UiEvent {
    Playback(PlaybackEvent),
    Levels(Levels),
    Devices(DeviceList),
    VirtualStatus(VirtualStatus),
    Library(Library),
    Settings(Settings),
    Streams(StreamStatus),
    Notice(Notice),
}

impl UiEvent {
    pub fn name(&self) -> &'static str {
        match self {
            UiEvent::Playback(_) => "playback",
            UiEvent::Levels(_) => "levels",
            UiEvent::Devices(_) => "devices-changed",
            UiEvent::VirtualStatus(_) => "virtual-status",
            UiEvent::Library(_) => "library-changed",
            UiEvent::Settings(_) => "settings-changed",
            UiEvent::Streams(_) => "streams-changed",
            UiEvent::Notice(_) => "notice",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackEvent {
    pub sound_id: String,
    pub voice_id: VoiceId,
    pub state: PlaybackState,
    pub frame: usize,
    pub frames: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackState {
    Started,
    Progress,
    Ended,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Levels {
    pub mic: f32,
    #[serde(rename = "virtual")]
    pub virtual_out: f32,
    pub monitor: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NoticeLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Notice {
    pub level: NoticeLevel,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StreamStatus {
    pub mic: Option<StreamInfo>,
    #[serde(rename = "virtual")]
    pub virtual_out: Option<StreamInfo>,
    pub monitor: Option<StreamInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfo {
    pub device: String,
    pub label: String,
    pub rate: u32,
    pub fallback: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub settings: Settings,
    pub library: Library,
    pub devices: DeviceList,
    pub virtual_status: VirtualStatus,
    pub streams: StreamStatus,
    pub playing: Vec<PlaybackEvent>,
    pub notices: Vec<Notice>,
    pub platform: &'static str,
    pub backend: &'static str,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyAction {
    PlaySound(String),
    StopAll,
    ToggleMute,
    SetVoicePreset(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GlobalHotkey {
    StopAll,
    ToggleMute,
}

/// Shared handle to the core plus its ticker thread.
#[derive(Clone)]
pub struct Controller {
    core: Arc<Mutex<Core>>,
    ticker: Arc<Mutex<Option<JoinHandle<()>>>>,
    stop: Arc<AtomicBool>,
}

impl Controller {
    pub fn start(paths: Paths, backend: Box<dyn AudioBackend>, sink: Arc<dyn EventSink>) -> Self {
        let mut core = Core::new(paths, backend, sink);
        core.refresh_devices();
        core.ensure_streams();
        let core = Arc::new(Mutex::new(core));
        let stop = Arc::new(AtomicBool::new(false));
        let ticker = {
            let core = core.clone();
            let stop = stop.clone();
            thread::Builder::new()
                .name("kavox-control".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        thread::sleep(TICK);
                        lock(&core).tick();
                    }
                })
                .ok()
        };
        Self {
            core,
            ticker: Arc::new(Mutex::new(ticker)),
            stop,
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, Core> {
        lock(&self.core)
    }

    /// Stops sounds and streams, saves pending changes and removes system resources.
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
        let ticker = self.ticker.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(ticker) = ticker {
            let _ = ticker.join();
        }
        self.lock().shutdown();
    }
}

fn lock(core: &Mutex<Core>) -> MutexGuard<'_, Core> {
    core.lock().unwrap_or_else(|e| e.into_inner())
}

struct Output {
    stream: Box<dyn StreamHandle>,
    handle: RendererHandle,
    rate: u32,
    label: String,
    fallback: bool,
    has_mic: bool,
    generation: u64,
}

struct MicStream {
    stream: Box<dyn StreamHandle>,
    label: String,
    fallback: bool,
    /// Whether the monitor also reads this mic ("hear myself").
    tapped: bool,
    generation: u64,
}

struct ActiveVoice {
    sound: String,
    frames: usize,
    frame: usize,
    /// Outputs still playing this voice (VIRTUAL_BIT | MONITOR_BIT).
    outputs: u8,
    /// Output whose progress the UI shows.
    primary: u8,
}

type StreamErrors = Arc<Mutex<Vec<(StreamRole, u64, String)>>>;

pub struct Core {
    paths: Paths,
    settings: Settings,
    library: Library,
    read_only: bool,
    backend: Box<dyn AudioBackend>,
    sink: Arc<dyn EventSink>,
    meters: Arc<Meters>,
    stream_errors: StreamErrors,
    generation: u64,
    virtual_out: Option<Output>,
    monitor: Option<Output>,
    mic: Option<MicStream>,
    /// Mic input waiting for a virtual renderer to be opened.
    pending_mic: Option<Box<MicInput>>,
    pending_monitor_mic: Option<Box<MicInput>>,
    cache: ClipCache,
    loader: Loader,
    voices: BTreeMap<VoiceId, ActiveVoice>,
    pending_plays: Vec<(String, Instant)>,
    next_voice: VoiceId,
    devices: DeviceList,
    virtual_status: VirtualStatus,
    last_errors: HashMap<StreamRole, String>,
    recent_notices: Vec<Notice>,
    visible: bool,
    last_poll: Instant,
    settings_dirty: Option<Instant>,
    library_dirty: Option<Instant>,
}

impl Core {
    fn new(paths: Paths, backend: Box<dyn AudioBackend>, sink: Arc<dyn EventSink>) -> Self {
        let _ = fs::create_dir_all(&paths.config_dir);
        let _ = fs::create_dir_all(&paths.sounds_dir);
        let settings = persist::load::<Settings>(&paths.settings());
        let mut library = persist::load::<Library>(&paths.library());
        library.value.repair();
        let mut core = Self {
            settings: Settings {
                audio: settings.value.audio.clone().sanitized(),
                voice: settings.value.voice.clone().sanitized(),
                ..settings.value
            },
            library: library.value,
            read_only: settings.read_only || library.read_only,
            paths,
            backend,
            sink,
            meters: Arc::new(Meters::default()),
            stream_errors: Arc::default(),
            generation: 0,
            virtual_out: None,
            monitor: None,
            mic: None,
            pending_mic: None,
            pending_monitor_mic: None,
            cache: ClipCache::default(),
            loader: Loader::new(),
            voices: BTreeMap::new(),
            pending_plays: Vec::new(),
            next_voice: 1,
            devices: DeviceList::default(),
            virtual_status: VirtualStatus::NotInstalled,
            last_errors: HashMap::new(),
            recent_notices: Vec::new(),
            visible: true,
            last_poll: Instant::now(),
            settings_dirty: None,
            library_dirty: None,
        };
        for message in [settings.notice, library.notice].into_iter().flatten() {
            core.notice(NoticeLevel::Warn, message);
        }
        core
    }

    // ---- Queries ----

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn virtual_status(&self) -> &VirtualStatus {
        &self.virtual_status
    }

    /// Full state for a freshly loaded UI; also hands over recent notices it missed.
    pub fn snapshot(&mut self) -> Snapshot {
        Snapshot {
            settings: self.settings.clone(),
            library: self.library.clone(),
            devices: self.devices.clone(),
            virtual_status: self.virtual_status.clone(),
            streams: self.stream_status(),
            playing: self
                .voices
                .keys()
                .filter_map(|&v| self.playback_event(v, PlaybackState::Progress))
                .collect(),
            notices: std::mem::take(&mut self.recent_notices),
            platform: std::env::consts::OS,
            backend: self.backend.name(),
            read_only: self.read_only,
        }
    }

    /// Re-enumerates devices now (the wizard's "Check again").
    pub fn devices(&mut self) -> DeviceList {
        self.refresh_devices();
        self.ensure_streams();
        self.devices.clone()
    }

    pub fn resolve(&self, target: &str) -> Result<Sound> {
        self.library.resolve(target).cloned()
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    // ---- Playback ----

    pub fn play_sound(&mut self, id: &str) -> Result<()> {
        let sound = self.library.sound(id)?.clone();
        let active = self.voices_of(id);
        let pending = self.pending_plays.iter().any(|(s, _)| s == id);
        match sound.retrigger {
            Retrigger::Toggle if !active.is_empty() || pending => {
                self.stop_sound(id);
                return Ok(());
            }
            Retrigger::Restart => {
                self.stop_voices(&active);
                if pending {
                    return Ok(());
                }
            }
            _ => {}
        }
        self.start_voice(&sound)
    }

    pub fn stop_sound(&mut self, id: &str) {
        self.pending_plays.retain(|(s, _)| s != id);
        let voices = self.voices_of(id);
        self.stop_voices(&voices);
    }

    pub fn stop_all(&mut self) {
        self.pending_plays.clear();
        for out in [&mut self.virtual_out, &mut self.monitor]
            .into_iter()
            .flatten()
        {
            let _ = out.handle.commands.push(Command::StopAll);
        }
    }

    /// `None` toggles. Returns the new state.
    pub fn set_mic_muted(&mut self, muted: Option<bool>) -> bool {
        let muted = muted.unwrap_or(!self.settings.audio.mic_muted);
        self.settings.audio.mic_muted = muted;
        for out in [&mut self.virtual_out, &mut self.monitor]
            .into_iter()
            .flatten()
        {
            let _ = out.handle.commands.push(Command::SetMicMuted(muted));
        }
        self.settings_changed();
        muted
    }

    /// Saves the voice changer state from the UI. Hotkeys have their own command, so they are kept as they are.
    pub fn set_voice_changer(&mut self, mut changer: VoiceSettings) {
        changer.hotkeys = std::mem::take(&mut self.settings.voice.hotkeys);
        let hear_before = self.settings.voice.hear_myself;
        self.settings.voice = changer.sanitized();
        self.push_pitch();
        if hear_before != self.settings.voice.hear_myself {
            // The monitor's mic reader is created together with the mic stream, so re-open it.
            self.ensure_streams();
            self.streams_changed();
        }
        self.settings_changed();
    }

    /// `None` toggles. Returns the new state.
    pub fn set_voice_enabled(&mut self, enabled: Option<bool>) -> bool {
        let enabled = enabled.unwrap_or(!self.settings.voice.enabled);
        self.settings.voice.enabled = enabled;
        self.push_pitch();
        self.settings_changed();
        enabled
    }

    /// Hotkey behaviour: picks the voice and turns the changer on, or turns it off if that voice is already live.
    pub fn toggle_voice_preset(&mut self, preset: &str) -> Result<bool> {
        if voice::preset(preset).is_none() {
            return Err(Error::Invalid(format!("unknown voice \"{preset}\"")));
        }
        let live = self.settings.voice.enabled && self.settings.voice.preset == preset;
        self.settings.voice.preset = preset.to_owned();
        self.settings.voice.enabled = !live;
        self.push_pitch();
        self.settings_changed();
        Ok(!live)
    }

    fn push_pitch(&mut self) {
        let params = self.settings.voice.pitch_params();
        for out in [&mut self.virtual_out, &mut self.monitor]
            .into_iter()
            .flatten()
        {
            let _ = out.handle.commands.push(Command::SetPitch(params));
        }
    }

    pub fn play_test_tone(&mut self, role: StreamRole) -> Result<()> {
        let voice = self.next_voice_id();
        let out = match role {
            StreamRole::Virtual => self.virtual_out.as_mut(),
            StreamRole::Monitor => self.monitor.as_mut(),
            StreamRole::Mic => None,
        }
        .ok_or(Error::NoOutput)?;
        let clip = Arc::new(ClipData::tone(out.rate, 660.0, 600, 0.4));
        out.handle
            .commands
            .push(Command::Play {
                voice,
                clip,
                gain: 1.0,
                looping: false,
            })
            .map_err(|_| Error::Device("audio engine is busy".into()))
    }

    fn start_voice(&mut self, sound: &Sound) -> Result<()> {
        let rates = self.output_rates();
        if rates.is_empty() {
            return Err(Error::NoOutput);
        }
        if !rates.iter().all(|&r| self.cache.contains(&sound.id, r)) {
            self.request_load(sound);
            self.pending_plays.push((sound.id.clone(), Instant::now()));
            return Ok(());
        }
        let voice = self.next_voice_id();
        let gain = self.sound_gain(sound);
        let (mut outputs, mut primary, mut frames) = (0u8, 0u8, 0usize);
        for (bit, slot) in [
            (VIRTUAL_BIT, &mut self.virtual_out),
            (MONITOR_BIT, &mut self.monitor),
        ] {
            let Some(out) = slot else { continue };
            let Some(clip) = self.cache.get(&sound.id, out.rate) else {
                continue;
            };
            let clip_frames = clip.frames;
            if out
                .handle
                .commands
                .push(Command::Play {
                    voice,
                    clip,
                    gain,
                    looping: sound.looping,
                })
                .is_ok()
            {
                outputs |= bit;
                if primary == 0 {
                    primary = bit;
                    frames = clip_frames;
                }
            }
        }
        if outputs == 0 {
            return Err(Error::Device("audio engine is busy".into()));
        }
        self.voices.insert(
            voice,
            ActiveVoice {
                sound: sound.id.clone(),
                frames,
                frame: 0,
                outputs,
                primary,
            },
        );
        self.emit_playback(voice, PlaybackState::Started);
        Ok(())
    }

    fn next_voice_id(&mut self) -> VoiceId {
        self.next_voice += 1;
        self.next_voice
    }

    fn sound_gain(&self, sound: &Sound) -> f32 {
        let norm = if self.settings.audio.normalize {
            clip::db_to_gain(sound.norm_gain_db)
        } else {
            1.0
        };
        sound.volume * norm
    }

    fn voices_of(&self, sound: &str) -> Vec<VoiceId> {
        self.voices
            .iter()
            .filter(|(_, v)| v.sound == sound)
            .map(|(&id, _)| id)
            .collect()
    }

    fn stop_voices(&mut self, voices: &[VoiceId]) {
        for out in [&mut self.virtual_out, &mut self.monitor]
            .into_iter()
            .flatten()
        {
            for &voice in voices {
                let _ = out.handle.commands.push(Command::Stop { voice });
            }
        }
    }

    fn update_voice_gains(&mut self, sound_filter: Option<&str>) {
        let updates: Vec<(VoiceId, f32)> = self
            .voices
            .iter()
            .filter(|(_, v)| sound_filter.is_none_or(|s| v.sound == s))
            .filter_map(|(&id, v)| {
                self.library
                    .sound(&v.sound)
                    .ok()
                    .map(|s| (id, self.sound_gain(s)))
            })
            .collect();
        for out in [&mut self.virtual_out, &mut self.monitor]
            .into_iter()
            .flatten()
        {
            for &(voice, gain) in &updates {
                let _ = out
                    .handle
                    .commands
                    .push(Command::SetVoiceGain { voice, gain });
            }
        }
    }

    // ---- Library ----

    /// Where imports go, and existing file hashes for de-duplication.
    pub fn import_context(&self) -> (PathBuf, HashMap<String, String>) {
        (self.paths.sounds_dir.clone(), self.library.hashes())
    }

    pub fn add_sounds(&mut self, board: Option<&str>, sounds: Vec<Sound>) -> Result<Vec<Sound>> {
        let board = board.map_or_else(|| self.library.active_board.clone(), str::to_owned);
        self.library.board(&board)?;
        for sound in &sounds {
            self.library.add_sound(&board, sound.clone())?;
            if sound.duration_ms <= RESIDENT_MAX_MS {
                self.request_load(sound);
            }
        }
        self.library_changed();
        Ok(sounds)
    }

    pub fn update_sound(&mut self, id: &str, patch: SoundPatch) -> Result<Sound> {
        let sound = self.library.update_sound(id, patch)?.clone();
        self.update_voice_gains(Some(id));
        self.library_changed();
        Ok(sound)
    }

    pub fn delete_sound(&mut self, id: &str) -> Result<()> {
        self.stop_sound(id);
        let sound = self.library.remove_sound(id)?;
        self.forget_sound(&sound);
        self.library_changed();
        Ok(())
    }

    pub fn reorder_sounds(&mut self, board: &str, ids: Vec<String>) -> Result<()> {
        self.library.reorder(board, ids)?;
        self.library_changed();
        Ok(())
    }

    pub fn create_board(&mut self, name: &str) -> Result<Board> {
        let board = self.library.create_board(name)?.clone();
        self.library.active_board = board.id.clone();
        self.library_changed();
        Ok(board)
    }

    pub fn rename_board(&mut self, id: &str, name: &str) -> Result<()> {
        self.library.rename_board(id, name)?;
        self.library_changed();
        Ok(())
    }

    pub fn delete_board(&mut self, id: &str) -> Result<()> {
        let ids = self.library.board(id)?.sounds.clone();
        for sound in &ids {
            self.stop_sound(sound);
        }
        for sound in self.library.delete_board(id)? {
            self.forget_sound(&sound);
        }
        self.library_changed();
        Ok(())
    }

    pub fn set_active_board(&mut self, id: &str) -> Result<()> {
        self.library.set_active_board(id)?;
        self.library_changed();
        Ok(())
    }

    fn forget_sound(&mut self, sound: &Sound) {
        self.cache.remove_sound(&sound.id);
        if !self.library.file_in_use(&sound.file) {
            let _ = fs::remove_file(self.paths.sounds_dir.join(&sound.file));
        }
    }

    // ---- Hotkeys ----

    pub fn set_sound_hotkey(&mut self, id: &str, hotkey: Option<String>) -> Result<()> {
        let hotkey = normalize_hotkey(hotkey);
        self.library.sound(id)?;
        if let Some(h) = &hotkey
            && let Some(owner) = self.hotkey_owner(h, Some(id), None, None)
        {
            return Err(Error::HotkeyConflict(h.clone(), owner));
        }
        self.library.set_hotkey(id, hotkey)?;
        self.library_changed();
        Ok(())
    }

    pub fn set_global_hotkey(&mut self, which: GlobalHotkey, hotkey: Option<String>) -> Result<()> {
        let hotkey = normalize_hotkey(hotkey);
        if let Some(h) = &hotkey
            && let Some(owner) = self.hotkey_owner(h, None, Some(which), None)
        {
            return Err(Error::HotkeyConflict(h.clone(), owner));
        }
        match which {
            GlobalHotkey::StopAll => self.settings.hotkeys.stop_all = hotkey,
            GlobalHotkey::ToggleMute => self.settings.hotkeys.toggle_mute = hotkey,
        }
        self.settings_changed();
        Ok(())
    }

    pub fn set_voice_hotkey(&mut self, preset: &str, hotkey: Option<String>) -> Result<()> {
        if voice::preset(preset).is_none() {
            return Err(Error::Invalid(format!("unknown voice \"{preset}\"")));
        }
        let hotkey = normalize_hotkey(hotkey);
        if let Some(h) = &hotkey
            && let Some(owner) = self.hotkey_owner(h, None, None, Some(preset))
        {
            return Err(Error::HotkeyConflict(h.clone(), owner));
        }
        match hotkey {
            Some(h) => self.settings.voice.hotkeys.insert(preset.to_owned(), h),
            None => self.settings.voice.hotkeys.remove(preset),
        };
        self.settings_changed();
        Ok(())
    }

    pub fn hotkey_bindings(&self) -> Vec<(String, HotkeyAction)> {
        let globals = [
            (&self.settings.hotkeys.stop_all, HotkeyAction::StopAll),
            (&self.settings.hotkeys.toggle_mute, HotkeyAction::ToggleMute),
        ];
        globals
            .into_iter()
            .filter_map(|(h, action)| h.clone().map(|h| (h, action)))
            .chain(
                self.settings
                    .voice
                    .hotkeys
                    .iter()
                    .map(|(id, h)| (h.clone(), HotkeyAction::SetVoicePreset(id.clone()))),
            )
            .chain(self.library.sounds.values().filter_map(|s| {
                s.hotkey
                    .clone()
                    .map(|h| (h, HotkeyAction::PlaySound(s.id.clone())))
            }))
            .collect()
    }

    fn hotkey_owner(
        &self,
        hotkey: &str,
        except_sound: Option<&str>,
        except_global: Option<GlobalHotkey>,
        except_preset: Option<&str>,
    ) -> Option<String> {
        let same =
            |h: &Option<String>| h.as_deref().is_some_and(|h| h.eq_ignore_ascii_case(hotkey));
        if except_global != Some(GlobalHotkey::StopAll) && same(&self.settings.hotkeys.stop_all) {
            return Some("Stop all".into());
        }
        if except_global != Some(GlobalHotkey::ToggleMute)
            && same(&self.settings.hotkeys.toggle_mute)
        {
            return Some("Mute mic".into());
        }
        if let Some((id, _)) =
            self.settings.voice.hotkeys.iter().find(|(id, h)| {
                Some(id.as_str()) != except_preset && h.eq_ignore_ascii_case(hotkey)
            })
        {
            return Some(format!("Voice: {}", voice::label(id)));
        }
        self.library
            .sounds
            .values()
            .find(|s| Some(s.id.as_str()) != except_sound && same(&s.hotkey))
            .map(|s| format!("\"{}\"", s.name))
    }

    // ---- Settings ----

    pub fn set_audio_settings(&mut self, audio: AudioSettings) {
        let audio = audio.sanitized();
        let old = std::mem::replace(&mut self.settings.audio, audio.clone());
        if old.virtual_output != audio.virtual_output {
            self.close_output(StreamRole::Virtual);
        }
        if old.monitor_output != audio.monitor_output || !audio.monitor_enabled {
            self.close_output(StreamRole::Monitor);
        }
        if old.mic_device != audio.mic_device || !audio.mic_passthrough {
            self.close_mic();
        }
        if let Some(out) = &mut self.virtual_out {
            let commands = &mut out.handle.commands;
            let _ = commands.push(Command::SetSoundsGain(audio.sounds_to_mic_gain));
            let _ = commands.push(Command::SetMicGain(audio.mic_gain));
            let _ = commands.push(Command::SetMicMuted(audio.mic_muted));
        }
        if let Some(out) = &mut self.monitor {
            let commands = &mut out.handle.commands;
            let _ = commands.push(Command::SetSoundsGain(audio.sounds_to_monitor_gain));
            let _ = commands.push(Command::SetMicGain(audio.mic_gain));
            let _ = commands.push(Command::SetMicMuted(audio.mic_muted));
        }
        if old.normalize != audio.normalize {
            self.update_voice_gains(None);
        }
        self.ensure_streams();
        self.streams_changed();
        self.settings_changed();
    }

    pub fn set_app_settings(&mut self, app: AppSettings) {
        self.settings.app = app;
        self.settings_changed();
    }

    // ---- Streams ----

    fn ensure_streams(&mut self) {
        let audio = self.settings.audio.clone();
        let mut changed = false;

        let (target, mut status) = self
            .backend
            .resolve_virtual(audio.virtual_output.as_deref(), &self.devices);
        if self
            .virtual_out
            .as_ref()
            .is_some_and(|v| Some(v.stream.device()) != target.as_deref())
        {
            self.close_output(StreamRole::Virtual);
            changed = true;
        }
        if self.virtual_out.is_none()
            && let Some(device) = &target
        {
            match self.open_output(StreamRole::Virtual, Some(device)) {
                Ok(mut out) => {
                    if let VirtualStatus::Ok { device, .. }
                    | VirtualStatus::CableIsDefaultOutput { device, .. } = &status
                    {
                        out.label = device.clone();
                    }
                    self.virtual_out = Some(out);
                    self.clear_error(StreamRole::Virtual);
                    changed = true;
                }
                Err(e) => {
                    status = VirtualStatus::Failed {
                        message: e.to_string(),
                    };
                    self.report_error(StreamRole::Virtual, &e);
                }
            }
        }
        self.set_virtual_status(status);

        if audio.mic_passthrough {
            let recover = self.mic.as_ref().is_some_and(|m| m.fallback)
                && audio
                    .mic_device
                    .as_deref()
                    .is_some_and(|d| self.devices.has_input(d));
            let starving =
                self.virtual_out.as_ref().is_some_and(|v| !v.has_mic) && self.pending_mic.is_none();
            let retap = self
                .mic
                .as_ref()
                .is_some_and(|m| m.tapped != self.wants_tap());
            if self.mic.is_none() || recover || starving || retap {
                self.close_mic();
                changed |= self.open_mic();
            }
        } else if self.mic.is_some() {
            self.close_mic();
            changed = true;
        }

        if audio.monitor_enabled {
            let recover = self.monitor.as_ref().is_some_and(|m| m.fallback)
                && audio
                    .monitor_output
                    .as_deref()
                    .is_some_and(|d| self.devices.has_output(d));
            if self.monitor.is_none() || recover {
                self.close_output(StreamRole::Monitor);
                match self.open_monitor(audio.monitor_output.as_deref()) {
                    Ok(out) => {
                        self.monitor = Some(out);
                        self.clear_error(StreamRole::Monitor);
                    }
                    Err(e) => self.report_error(StreamRole::Monitor, &e),
                }
                changed = true;
            }
        } else if self.monitor.is_some() {
            self.close_output(StreamRole::Monitor);
            changed = true;
        }

        // A monitor that was re-opened lost its mic reader along with the old stream; the mic makes a new one.
        if self.wants_tap()
            && self.mic.is_some()
            && self.monitor.as_ref().is_some_and(|m| !m.has_mic)
        {
            self.close_mic();
            changed |= self.open_mic();
        }

        if changed {
            self.streams_changed();
        }
    }

    fn open_output(&mut self, role: StreamRole, device: Option<&str>) -> Result<Output> {
        let meter = if role == StreamRole::Virtual {
            Meter::Virtual
        } else {
            Meter::Monitor
        };
        let (mut renderer, handle) = engine::renderer(meter, self.meters.clone());
        let audio = &self.settings.audio;
        let (bus, mic) = match role {
            StreamRole::Virtual => (audio.sounds_to_mic_gain, self.pending_mic.take()),
            _ => (
                audio.sounds_to_monitor_gain,
                self.pending_monitor_mic.take(),
            ),
        };
        let pitch = self.settings.voice.pitch_params();
        renderer.configure(bus, audio.mic_gain, audio.mic_muted, pitch, mic);
        let has_mic = renderer.has_mic();
        self.generation += 1;
        let generation = self.generation;
        let stream = self
            .backend
            .open_output(role, device, renderer, self.error_fn())?;
        let label = self.devices.label(stream.device());
        tracing::info!(
            "opened {role:?} output {} @ {} Hz",
            stream.device(),
            stream.rate()
        );
        Ok(Output {
            rate: stream.rate(),
            stream,
            handle,
            label,
            fallback: false,
            has_mic,
            generation,
        })
    }

    fn open_monitor(&mut self, preferred: Option<&str>) -> Result<Output> {
        match self.open_output(StreamRole::Monitor, preferred) {
            Err(Error::DeviceNotFound(_)) if preferred.is_some() => {
                let mut out = self.open_output(StreamRole::Monitor, None)?;
                out.fallback = true;
                let missing = self.devices.label(preferred.unwrap_or_default());
                self.notice(
                    NoticeLevel::Warn,
                    format!(
                        "{missing} isn't connected, so you'll hear sounds on the default output."
                    ),
                );
                Ok(out)
            }
            other => other,
        }
    }

    fn open_mic(&mut self) -> bool {
        let preferred = self.settings.audio.mic_device.clone();
        let opened = match self.try_open_mic(preferred.as_deref()) {
            Err(Error::DeviceNotFound(_)) if preferred.is_some() => {
                self.try_open_mic(None).map(|(s, i, t)| (s, i, t, true))
            }
            other => other.map(|(s, i, t)| (s, i, t, false)),
        };
        let (stream, mut input, tap, fallback) = match opened {
            Ok(opened) => opened,
            Err(e) => {
                self.report_error(StreamRole::Mic, &e);
                return false;
            }
        };
        if fallback {
            let missing = self.devices.label(preferred.as_deref().unwrap_or_default());
            self.notice(
                NoticeLevel::Warn,
                format!("{missing} isn't connected, so Kavox is using the default microphone."),
            );
        }
        input.set_input_rate(stream.rate());
        if let Some(out) = &mut self.virtual_out {
            input.prepare(out.rate);
            out.has_mic = out
                .handle
                .commands
                .push(Command::SetMic(Some(Box::new(input))))
                .is_ok();
        } else {
            self.pending_mic = Some(Box::new(input));
        }
        let tapped = tap.is_some();
        if let Some(mut tap) = tap {
            tap.set_input_rate(stream.rate());
            if let Some(out) = &mut self.monitor {
                tap.prepare(out.rate);
                out.has_mic = out
                    .handle
                    .commands
                    .push(Command::SetMic(Some(Box::new(tap))))
                    .is_ok();
            } else {
                self.pending_monitor_mic = Some(Box::new(tap));
            }
        }
        let label = self.devices.label(stream.device());
        tracing::info!("opened mic {} @ {} Hz", stream.device(), stream.rate());
        self.mic = Some(MicStream {
            stream,
            label,
            fallback,
            tapped,
            generation: self.generation,
        });
        self.clear_error(StreamRole::Mic);
        true
    }

    /// The monitor only listens to the mic when "hear myself" is on and the monitor is enabled.
    fn wants_tap(&self) -> bool {
        self.settings.voice.hear_myself && self.settings.audio.monitor_enabled
    }

    #[allow(clippy::type_complexity)]
    fn try_open_mic(
        &mut self,
        device: Option<&str>,
    ) -> Result<(Box<dyn StreamHandle>, MicInput, Option<MicInput>)> {
        let (sink, input, tap) = if self.wants_tap() {
            let (sink, input, tap) = engine::mic_channel_tapped(self.meters.clone());
            (sink, input, Some(tap))
        } else {
            let (sink, input) = engine::mic_channel(self.meters.clone());
            (sink, input, None)
        };
        self.generation += 1;
        let stream = self.backend.open_input(device, sink, self.error_fn())?;
        Ok((stream, input, tap))
    }

    fn close_mic(&mut self) {
        self.mic = None;
        self.pending_mic = None;
        self.pending_monitor_mic = None;
        for out in [&mut self.virtual_out, &mut self.monitor]
            .into_iter()
            .flatten()
        {
            if out.has_mic {
                let _ = out.handle.commands.push(Command::SetMic(None));
                out.has_mic = false;
            }
        }
    }

    fn close_output(&mut self, role: StreamRole) {
        let (slot, bit) = match role {
            StreamRole::Virtual => (&mut self.virtual_out, VIRTUAL_BIT),
            StreamRole::Monitor => (&mut self.monitor, MONITOR_BIT),
            StreamRole::Mic => return,
        };
        let Some(Output {
            stream, mut handle, ..
        }) = slot.take()
        else {
            return;
        };
        drop(stream);
        while handle.events.pop().is_ok() {}
        while handle.garbage.pop().is_ok() {}
        let affected: Vec<VoiceId> = self
            .voices
            .iter()
            .filter(|(_, v)| v.outputs & bit != 0)
            .map(|(&id, _)| id)
            .collect();
        for voice in affected {
            self.voice_output_done(voice, bit);
        }
    }

    fn error_fn(&self) -> StreamErrorFn {
        let errors = self.stream_errors.clone();
        let generation = self.generation;
        Arc::new(move |role, message| {
            if let Ok(mut errors) = errors.lock()
                && errors.len() < 32
            {
                errors.push((role, generation, message));
            }
        })
    }

    fn output_rates(&self) -> Vec<u32> {
        let mut rates: Vec<u32> = [&self.virtual_out, &self.monitor]
            .into_iter()
            .flatten()
            .map(|o| o.rate)
            .collect();
        rates.dedup();
        rates
    }

    fn stream_status(&self) -> StreamStatus {
        let info = |o: &Output| StreamInfo {
            device: o.stream.device().to_owned(),
            label: o.label.clone(),
            rate: o.rate,
            fallback: o.fallback,
        };
        StreamStatus {
            mic: self.mic.as_ref().map(|m| StreamInfo {
                device: m.stream.device().to_owned(),
                label: m.label.clone(),
                rate: m.stream.rate(),
                fallback: m.fallback,
            }),
            virtual_out: self.virtual_out.as_ref().map(info),
            monitor: self.monitor.as_ref().map(info),
        }
    }

    fn streams_changed(&mut self) {
        let rates = self.output_rates();
        self.cache.retain_rates(&rates);
        self.preload();
        self.sink.emit(UiEvent::Streams(self.stream_status()));
    }

    fn set_virtual_status(&mut self, status: VirtualStatus) {
        if status != self.virtual_status {
            self.virtual_status = status.clone();
            self.sink.emit(UiEvent::VirtualStatus(status));
        }
    }

    fn refresh_devices(&mut self) {
        match self.backend.list_devices() {
            Ok(list) if list != self.devices => {
                self.devices = list.clone();
                self.sink.emit(UiEvent::Devices(list));
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("listing devices failed: {e}"),
        }
    }

    // ---- Clip loading ----

    fn preload(&mut self) {
        let resident: Vec<Sound> = self
            .library
            .sounds
            .values()
            .filter(|s| s.duration_ms <= RESIDENT_MAX_MS)
            .cloned()
            .collect();
        for sound in &resident {
            self.request_load(sound);
        }
    }

    fn request_load(&mut self, sound: &Sound) -> bool {
        let missing: Vec<u32> = self
            .output_rates()
            .into_iter()
            .filter(|&r| !self.cache.contains(&sound.id, r))
            .collect();
        !missing.is_empty()
            && self
                .loader
                .request(&sound.id, self.paths.sounds_dir.join(&sound.file), &missing)
    }

    fn drain_loader(&mut self) {
        let rates = self.output_rates();
        for result in self.loader.drain() {
            match result.clips {
                Ok(clips) => {
                    for clip in clips.into_iter().filter(|c| rates.contains(&c.rate)) {
                        self.cache.insert(&result.sound, clip);
                    }
                }
                Err(e) => {
                    let name = self
                        .library
                        .sound(&result.sound)
                        .map_or_else(|_| result.sound.clone(), |s| s.name.clone());
                    self.pending_plays.retain(|(s, _)| s != &result.sound);
                    self.notice(NoticeLevel::Error, format!("Couldn't load \"{name}\": {e}"));
                }
            }
        }
        for (id, requested) in std::mem::take(&mut self.pending_plays) {
            let Ok(sound) = self.library.sound(&id).cloned() else {
                continue;
            };
            if requested.elapsed() > PENDING_PLAY_TIMEOUT {
                tracing::warn!("gave up waiting for \"{}\" to load", sound.name);
                continue;
            }
            if self
                .output_rates()
                .iter()
                .all(|&r| self.cache.contains(&id, r))
            {
                if let Err(e) = self.start_voice(&sound) {
                    tracing::warn!("couldn't play \"{}\": {e}", sound.name);
                }
            } else {
                if !self.loader.is_loading(&id) {
                    self.request_load(&sound);
                }
                self.pending_plays.push((id, requested));
            }
        }
    }

    // ---- Ticker ----

    fn tick(&mut self) {
        self.handle_stream_errors();
        self.drain_outputs();
        self.drain_loader();
        let levels = Levels {
            mic: self.meters.take(Meter::Mic),
            virtual_out: self.meters.take(Meter::Virtual),
            monitor: self.meters.take(Meter::Monitor),
        };
        if self.visible {
            self.sink.emit(UiEvent::Levels(levels));
        }
        if self.last_poll.elapsed() >= DEVICE_POLL {
            self.last_poll = Instant::now();
            self.refresh_devices();
            self.ensure_streams();
        }
        self.flush(false);
    }

    fn handle_stream_errors(&mut self) {
        let errors =
            std::mem::take(&mut *self.stream_errors.lock().unwrap_or_else(|e| e.into_inner()));
        let mut any = false;
        for (role, generation, message) in errors {
            let current = match role {
                StreamRole::Mic => self.mic.as_ref().map(|m| m.generation),
                StreamRole::Virtual => self.virtual_out.as_ref().map(|o| o.generation),
                StreamRole::Monitor => self.monitor.as_ref().map(|o| o.generation),
            };
            if current != Some(generation) {
                continue;
            }
            tracing::warn!("{role:?} stream failed: {message}");
            match role {
                StreamRole::Mic => self.close_mic(),
                _ => self.close_output(role),
            }
            any = true;
        }
        if any {
            self.refresh_devices();
            self.ensure_streams();
            self.streams_changed();
        }
    }

    fn drain_outputs(&mut self) {
        let mut events = Vec::new();
        for (bit, slot) in [
            (VIRTUAL_BIT, &mut self.virtual_out),
            (MONITOR_BIT, &mut self.monitor),
        ] {
            if let Some(out) = slot {
                while let Ok(event) = out.handle.events.pop() {
                    events.push((bit, event));
                }
                while out.handle.garbage.pop().is_ok() {}
            }
        }
        for (bit, event) in events {
            match event {
                EngineEvent::Started(_) => {}
                EngineEvent::Progress { voice, frame } => {
                    if let Some(v) = self.voices.get_mut(&voice)
                        && v.primary == bit
                    {
                        v.frame = frame;
                        self.emit_playback(voice, PlaybackState::Progress);
                    }
                }
                EngineEvent::Ended(voice) => self.voice_output_done(voice, bit),
            }
        }
    }

    fn voice_output_done(&mut self, voice: VoiceId, bit: u8) {
        let Some(v) = self.voices.get_mut(&voice) else {
            return;
        };
        v.outputs &= !bit;
        if v.outputs != 0 {
            if v.primary == bit {
                v.primary = v.outputs;
            }
            return;
        }
        self.emit_playback(voice, PlaybackState::Ended);
        self.voices.remove(&voice);
    }

    fn playback_event(&self, voice: VoiceId, state: PlaybackState) -> Option<PlaybackEvent> {
        let v = self.voices.get(&voice)?;
        Some(PlaybackEvent {
            sound_id: v.sound.clone(),
            voice_id: voice,
            state,
            frame: v.frame,
            frames: v.frames,
        })
    }

    fn emit_playback(&self, voice: VoiceId, state: PlaybackState) {
        if let Some(event) = self.playback_event(voice, state) {
            self.sink.emit(UiEvent::Playback(event));
        }
    }

    // ---- Notices and persistence ----

    fn notice(&mut self, level: NoticeLevel, message: String) {
        match level {
            NoticeLevel::Error => tracing::error!("{message}"),
            NoticeLevel::Warn => tracing::warn!("{message}"),
            NoticeLevel::Info => tracing::info!("{message}"),
        }
        let notice = Notice { level, message };
        if self.recent_notices.len() == RECENT_NOTICES {
            self.recent_notices.remove(0);
        }
        self.recent_notices.push(notice.clone());
        self.sink.emit(UiEvent::Notice(notice));
    }

    /// Notifies once per distinct failure so a missing device doesn't spam every poll.
    fn report_error(&mut self, role: StreamRole, error: &Error) {
        let message = error.to_string();
        if self.last_errors.get(&role) == Some(&message) {
            return;
        }
        self.last_errors.insert(role, message.clone());
        let what = match role {
            StreamRole::Mic => "the microphone",
            StreamRole::Virtual => "the virtual mic",
            StreamRole::Monitor => "your headphones",
        };
        self.notice(
            NoticeLevel::Warn,
            format!("Couldn't open {what}: {message}"),
        );
    }

    fn clear_error(&mut self, role: StreamRole) {
        self.last_errors.remove(&role);
    }

    fn settings_changed(&mut self) {
        self.settings_dirty = Some(Instant::now());
        self.sink.emit(UiEvent::Settings(self.settings.clone()));
    }

    fn library_changed(&mut self) {
        self.library_dirty = Some(Instant::now());
        self.sink.emit(UiEvent::Library(self.library.clone()));
    }

    fn flush(&mut self, force: bool) {
        if self.read_only {
            return;
        }
        let due =
            |dirty: Option<Instant>| dirty.is_some_and(|at| force || at.elapsed() >= SAVE_DELAY);
        if due(self.settings_dirty) {
            self.settings_dirty = None;
            if let Err(e) = persist::save(&self.paths.settings(), &self.settings) {
                tracing::error!("saving settings failed: {e}");
            }
        }
        if due(self.library_dirty) {
            self.library_dirty = None;
            if let Err(e) = persist::save(&self.paths.library(), &self.library) {
                tracing::error!("saving boards failed: {e}");
            }
        }
    }

    fn shutdown(&mut self) {
        self.stop_all();
        self.close_mic();
        self.close_output(StreamRole::Virtual);
        self.close_output(StreamRole::Monitor);
        self.flush(true);
        self.backend.shutdown();
    }

    /// Plain-text report for bug reports; no file paths beyond names.
    pub fn diagnostics(&mut self, version: &str) -> String {
        use std::fmt::Write;
        self.refresh_devices();
        let mut out = String::new();
        let _ = writeln!(
            out,
            "Kavox {version} on {} {} (backend: {})",
            std::env::consts::OS,
            std::env::consts::ARCH,
            self.backend.name()
        );
        let _ = writeln!(out, "Virtual status: {:?}", self.virtual_status);
        let status = self.stream_status();
        for (name, info) in [
            ("Mic", &status.mic),
            ("Virtual", &status.virtual_out),
            ("Monitor", &status.monitor),
        ] {
            match info {
                Some(i) => {
                    let _ = writeln!(
                        out,
                        "{name}: {} @ {} Hz{}",
                        i.label,
                        i.rate,
                        if i.fallback { " (fallback)" } else { "" }
                    );
                }
                None => {
                    let _ = writeln!(out, "{name}: closed");
                }
            }
        }
        let a = &self.settings.audio;
        let _ = writeln!(
            out,
            "Audio: passthrough={} muted={} micGain={:.2} toMic={:.2} toMonitor={:.2} monitor={} normalize={}",
            a.mic_passthrough,
            a.mic_muted,
            a.mic_gain,
            a.sounds_to_mic_gain,
            a.sounds_to_monitor_gain,
            a.monitor_enabled,
            a.normalize
        );
        let labels = |list: &[crate::backend::DeviceInfo]| {
            list.iter()
                .map(|d| d.label.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let _ = writeln!(out, "Inputs: {}", labels(&self.devices.inputs));
        let _ = writeln!(out, "Outputs: {}", labels(&self.devices.outputs));
        let _ = writeln!(
            out,
            "Library: {} sounds on {} boards, cache {:.1} MB, {} playing",
            self.library.sounds.len(),
            self.library.boards.len(),
            self.cache.bytes() as f64 / 1_048_576.0,
            self.voices.len()
        );
        let _ = writeln!(out, "\nRecent log:");
        out.push_str(&recent_log_lines(&self.paths.log_dir, 50));
        out
    }
}

fn normalize_hotkey(hotkey: Option<String>) -> Option<String> {
    hotkey
        .map(|h| h.trim().to_owned())
        .filter(|h| !h.is_empty())
}

fn recent_log_lines(dir: &std::path::Path, count: usize) -> String {
    let newest = fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(modified, _)| *modified);
    let Some((_, path)) = newest else {
        return "(no log yet)\n".into();
    };
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(count)..].join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::DeviceInfo;
    use crate::clip::tests::{temp_dir, write_wav};
    use crate::engine::{MicSink, Renderer};
    use crate::library::prepare_import;

    type Renderers = Arc<Mutex<Vec<(StreamRole, Renderer)>>>;

    struct FakeBackend {
        renderers: Renderers,
        outputs: Vec<&'static str>,
    }

    struct FakeStream(String);

    impl StreamHandle for FakeStream {
        fn rate(&self) -> u32 {
            48_000
        }
        fn device(&self) -> &str {
            &self.0
        }
    }

    fn device(id: &str, is_default: bool) -> DeviceInfo {
        DeviceInfo {
            id: id.into(),
            label: id.to_uppercase(),
            is_default,
        }
    }

    impl AudioBackend for FakeBackend {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn list_devices(&mut self) -> Result<DeviceList> {
            Ok(DeviceList {
                inputs: vec![device("mic", true)],
                outputs: self
                    .outputs
                    .iter()
                    .map(|o| device(o, *o == "speakers"))
                    .collect(),
                virtual_candidates: vec!["cable".into()],
            })
        }
        fn resolve_virtual(
            &mut self,
            _: Option<&str>,
            _: &DeviceList,
        ) -> (Option<String>, VirtualStatus) {
            (
                Some("cable".into()),
                VirtualStatus::Ok {
                    device: "cable".into(),
                    mic_label: "cable out".into(),
                },
            )
        }
        fn open_output(
            &mut self,
            role: StreamRole,
            device: Option<&str>,
            mut renderer: Renderer,
            _: StreamErrorFn,
        ) -> Result<Box<dyn StreamHandle>> {
            let device = device.unwrap_or("speakers");
            if !self.outputs.contains(&device) {
                return Err(Error::DeviceNotFound(device.into()));
            }
            renderer.prepare(48_000);
            self.renderers.lock().unwrap().push((role, renderer));
            Ok(Box::new(FakeStream(device.into())))
        }
        fn open_input(
            &mut self,
            device: Option<&str>,
            _: MicSink,
            _: StreamErrorFn,
        ) -> Result<Box<dyn StreamHandle>> {
            Ok(Box::new(FakeStream(device.unwrap_or("mic").into())))
        }
    }

    #[derive(Default)]
    struct Events(Mutex<Vec<UiEvent>>);

    impl EventSink for Events {
        fn emit(&self, event: UiEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    impl Events {
        fn notices(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter_map(|e| {
                    if let UiEvent::Notice(n) = e {
                        Some(n.message.clone())
                    } else {
                        None
                    }
                })
                .collect()
        }
    }

    struct Rig {
        core: Core,
        renderers: Renderers,
        events: Arc<Events>,
        paths: Paths,
    }

    fn rig_with(paths: Paths, outputs: Vec<&'static str>, monitor_output: Option<&str>) -> Rig {
        let renderers = Renderers::default();
        let events = Arc::new(Events::default());
        let backend = FakeBackend {
            renderers: renderers.clone(),
            outputs,
        };
        let mut core = Core::new(paths.clone(), Box::new(backend), events.clone());
        core.settings.audio.monitor_output = monitor_output.map(str::to_owned);
        core.refresh_devices();
        core.ensure_streams();
        Rig {
            core,
            renderers,
            events,
            paths,
        }
    }

    fn paths() -> Paths {
        let dir = temp_dir("core");
        Paths {
            config_dir: dir.join("config"),
            sounds_dir: dir.join("sounds"),
            log_dir: dir.join("logs"),
        }
    }

    fn rig() -> Rig {
        rig_with(paths(), vec!["speakers", "cable", "headphones"], None)
    }

    impl Rig {
        fn import(&mut self, name: &str, retrigger: Retrigger) -> String {
            let src = self.paths.config_dir.join(format!("{name}.wav"));
            write_wav(&src, 44_100, 1, &vec![0.3; 44_100]);
            let (dir, hashes) = self.core.import_context();
            let mut sound = prepare_import(&src, &dir, &hashes).unwrap();
            sound.retrigger = retrigger;
            let id = sound.id.clone();
            self.core.add_sounds(None, vec![sound]).unwrap();
            let start = Instant::now();
            while !self.core.cache.contains(&id, 48_000) {
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "clip never loaded"
                );
                thread::sleep(Duration::from_millis(5));
                self.core.drain_loader();
            }
            id
        }

        /// Runs every renderer for one 10 ms block, then lets the core drain events.
        fn step(&mut self) {
            let mut out = vec![0f32; 960];
            for (_, r) in self.renderers.lock().unwrap().iter_mut() {
                r.render(&mut out);
            }
            self.core.drain_outputs();
        }

        fn playing(&self, id: &str) -> usize {
            self.core.voices_of(id).len()
        }
    }

    #[test]
    fn opens_virtual_monitor_and_mic() {
        let rig = rig();
        let status = rig.core.stream_status();
        assert_eq!(status.virtual_out.unwrap().device, "cable");
        assert_eq!(status.monitor.unwrap().device, "speakers");
        assert!(status.mic.is_some());
        assert!(rig.core.virtual_out.as_ref().unwrap().has_mic);
    }

    #[test]
    fn retrigger_modes() {
        let mut rig = rig();
        let restart = rig.import("restart", Retrigger::Restart);
        rig.core.play_sound(&restart).unwrap();
        rig.step();
        rig.core.play_sound(&restart).unwrap();
        assert_eq!(rig.playing(&restart), 2);
        for _ in 0..3 {
            rig.step();
        }
        assert_eq!(rig.playing(&restart), 1, "restart stops the previous voice");

        let overlap = rig.import("overlap", Retrigger::Overlap);
        rig.core.play_sound(&overlap).unwrap();
        rig.core.play_sound(&overlap).unwrap();
        rig.step();
        assert_eq!(rig.playing(&overlap), 2);

        let toggle = rig.import("toggle", Retrigger::Toggle);
        rig.core.play_sound(&toggle).unwrap();
        rig.step();
        rig.core.play_sound(&toggle).unwrap();
        for _ in 0..3 {
            rig.step();
        }
        assert_eq!(rig.playing(&toggle), 0, "second press stops a toggle sound");

        rig.core.stop_all();
        for _ in 0..3 {
            rig.step();
        }
        assert!(rig.core.voices.is_empty());
    }

    #[test]
    fn voice_ends_on_both_outputs_and_reports_once() {
        let mut rig = rig();
        let id = rig.import("short", Retrigger::Restart);
        rig.core.play_sound(&id).unwrap();
        for _ in 0..110 {
            rig.step();
        }
        assert!(rig.core.voices.is_empty());
        let ended = rig
            .events
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, UiEvent::Playback(p) if p.state == PlaybackState::Ended))
            .count();
        assert_eq!(ended, 1);
    }

    #[test]
    fn hotkey_conflicts_are_rejected() {
        let mut rig = rig();
        let a = rig.import("a", Retrigger::Restart);
        let b = rig.import("b", Retrigger::Restart);
        rig.core
            .set_sound_hotkey(&a, Some("Ctrl+1".into()))
            .unwrap();
        assert!(matches!(
            rig.core.set_sound_hotkey(&b, Some("ctrl+1".into())),
            Err(Error::HotkeyConflict(..))
        ));
        assert!(matches!(
            rig.core
                .set_global_hotkey(GlobalHotkey::StopAll, Some("Ctrl+1".into())),
            Err(Error::HotkeyConflict(..))
        ));
        rig.core
            .set_sound_hotkey(&a, Some("Ctrl+1".into()))
            .unwrap();
        rig.core
            .set_global_hotkey(GlobalHotkey::ToggleMute, Some("Ctrl+M".into()))
            .unwrap();
        assert_eq!(rig.core.hotkey_bindings().len(), 2);
        rig.core.set_sound_hotkey(&a, Some("  ".into())).unwrap();
        assert_eq!(
            rig.core.hotkey_bindings(),
            [("Ctrl+M".to_string(), HotkeyAction::ToggleMute)]
        );
    }

    #[test]
    fn voice_hotkeys_toggle_presets_and_conflict_with_others() {
        let mut rig = rig();
        let a = rig.import("a", Retrigger::Restart);
        rig.core
            .set_voice_hotkey("chipmunk", Some("Ctrl+F1".into()))
            .unwrap();
        assert!(matches!(
            rig.core.set_sound_hotkey(&a, Some("ctrl+f1".into())),
            Err(Error::HotkeyConflict(_, owner)) if owner == "Voice: Chipmunk"
        ));
        assert!(matches!(
            rig.core.set_voice_hotkey("giant", Some("Ctrl+F1".into())),
            Err(Error::HotkeyConflict(..))
        ));
        rig.core
            .set_voice_hotkey("chipmunk", Some("Ctrl+F1".into()))
            .unwrap();
        assert!(rig.core.set_voice_hotkey("nope", None).is_err());
        assert_eq!(
            rig.core.hotkey_bindings(),
            [(
                "Ctrl+F1".to_string(),
                HotkeyAction::SetVoicePreset("chipmunk".into())
            )]
        );

        assert!(rig.core.toggle_voice_preset("chipmunk").unwrap());
        assert!(rig.core.settings.voice.enabled);
        assert!(rig.core.toggle_voice_preset("giant").unwrap());
        assert_eq!(rig.core.settings.voice.preset, "giant");
        assert!(!rig.core.toggle_voice_preset("giant").unwrap());
        assert!(!rig.core.settings.voice.enabled);
        assert!(rig.core.toggle_voice_preset("nope").is_err());
    }

    #[test]
    fn hear_myself_gives_the_monitor_a_mic_reader() {
        let mut rig = rig();
        assert!(!rig.core.monitor.as_ref().unwrap().has_mic);
        assert!(!rig.core.mic.as_ref().unwrap().tapped);

        let on = VoiceSettings {
            hear_myself: true,
            ..Default::default()
        };
        rig.core.set_voice_changer(on);
        assert!(rig.core.mic.as_ref().unwrap().tapped);
        assert!(rig.core.monitor.as_ref().unwrap().has_mic);
        assert!(rig.core.virtual_out.as_ref().unwrap().has_mic);

        // A re-opened monitor gets a fresh reader because the mic is re-opened with it.
        rig.core.close_output(StreamRole::Monitor);
        rig.core.ensure_streams();
        assert!(rig.core.monitor.as_ref().unwrap().has_mic);

        rig.core.set_voice_changer(VoiceSettings::default());
        assert!(!rig.core.mic.as_ref().unwrap().tapped);
        assert!(!rig.core.monitor.as_ref().unwrap().has_mic);
        assert!(rig.core.virtual_out.as_ref().unwrap().has_mic);
    }

    #[test]
    fn saving_the_voice_changer_keeps_its_hotkeys() {
        let mut rig = rig();
        rig.core
            .set_voice_hotkey("alien", Some("F9".into()))
            .unwrap();
        rig.core.set_voice_changer(VoiceSettings {
            enabled: true,
            preset: "custom".into(),
            semitones: 99.0,
            ..Default::default()
        });
        let v = &rig.core.settings.voice;
        assert!(v.enabled);
        assert_eq!(v.semitones, 12.0);
        assert_eq!(v.hotkeys.get("alien").map(String::as_str), Some("F9"));
        assert!(!rig.core.set_voice_enabled(None));
        assert!(rig.core.set_voice_enabled(Some(true)));
    }

    #[test]
    fn monitor_falls_back_and_recovers() {
        let paths = paths();
        let mut rig = rig_with(paths, vec!["speakers", "cable"], Some("headphones"));
        let monitor = rig.core.monitor.as_ref().unwrap();
        assert_eq!(monitor.stream.device(), "speakers");
        assert!(monitor.fallback);
        assert!(
            rig.events
                .notices()
                .iter()
                .any(|n| n.contains("HEADPHONES") || n.contains("headphones"))
        );

        let renderers = rig.renderers.clone();
        rig.core.backend = Box::new(FakeBackend {
            renderers,
            outputs: vec!["speakers", "cable", "headphones"],
        });
        rig.core.refresh_devices();
        rig.core.ensure_streams();
        let monitor = rig.core.monitor.as_ref().unwrap();
        assert_eq!(monitor.stream.device(), "headphones");
        assert!(!monitor.fallback);
    }

    #[test]
    fn library_and_settings_persist() {
        let paths = paths();
        let id = {
            let mut rig = rig_with(paths.clone(), vec!["speakers", "cable"], None);
            let id = rig.import("keep", Retrigger::Toggle);
            rig.core.set_mic_muted(Some(true));
            rig.core.flush(true);
            id
        };
        let rig = rig_with(paths, vec!["speakers", "cable"], None);
        assert_eq!(
            rig.core.library.sound(&id).unwrap().retrigger,
            Retrigger::Toggle
        );
        assert!(rig.core.settings.audio.mic_muted);
    }

    #[test]
    fn delete_removes_unused_files_only() {
        let mut rig = rig();
        let a = rig.import("same", Retrigger::Restart);
        let b = rig.import("same", Retrigger::Restart);
        let file = rig.core.library.sound(&a).unwrap().file.clone();
        assert_eq!(rig.core.library.sound(&b).unwrap().file, file);
        rig.core.delete_sound(&a).unwrap();
        assert!(rig.paths.sounds_dir.join(&file).exists());
        rig.core.delete_sound(&b).unwrap();
        assert!(!rig.paths.sounds_dir.join(&file).exists());
    }

    #[test]
    fn stale_stream_errors_are_ignored() {
        let mut rig = rig();
        let old_generation = rig.core.monitor.as_ref().unwrap().generation;
        rig.core.stream_errors.lock().unwrap().push((
            StreamRole::Monitor,
            old_generation + 100,
            "other stream".into(),
        ));
        rig.core.handle_stream_errors();
        assert_eq!(
            rig.core.monitor.as_ref().unwrap().generation,
            old_generation
        );

        rig.core.stream_errors.lock().unwrap().push((
            StreamRole::Monitor,
            old_generation,
            "unplugged".into(),
        ));
        rig.core.handle_stream_errors();
        assert!(
            rig.core.monitor.as_ref().unwrap().generation > old_generation,
            "monitor reopened"
        );
    }
}
