//! Serves `kavoxctl` requests and single-instance enforcement over an OS lock file + local socket.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use kavox_core::{Controller, Core, Error, Retrigger, StreamInfo, VirtualStatus, voice};
use kavox_ipc::{
    BoardEntry, DeviceEntry, DevicesState, MixerPatch, MixerState, MuteMode, PlayingEntry, Request,
    Response, SoundEntry, StatusInfo, StreamEntry, StreamsState, VoicePresetEntry, VoiceState,
};

use crate::state::AppCommand;

const PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const SHOW_RETRY_WINDOW: Duration = Duration::from_secs(5);
const SHOW_RETRY_INTERVAL: Duration = Duration::from_millis(50);
/// How long a quitting instance may take to release the lock (e.g. removing Kavox Mic).
const SHUTDOWN_RETRY_WINDOW: Duration = Duration::from_secs(15);
/// How long a request waits for audio startup; below the client's 5 s timeout so the client
/// gets a clear "still starting" error instead of timing out and the action running later.
const STARTUP_WAIT: Duration = Duration::from_secs(4);
const SHUTTING_DOWN_CODE: &str = "ShuttingDown";
const STARTING_CODE: &str = "Starting";

static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Makes IPC answer every request (including a second launch's `Show`) with `ShuttingDown`, so
/// a relaunch during shutdown waits for the lock instead of exiting.
pub fn mark_shutting_down() {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
}

/// Holds the OS-level exclusive single-instance lock until the process exits.
pub struct SingleInstanceGuard {
    _lock_file: Option<File>,
}

/// Deferred controller handle so the IPC socket can bind before audio initialization finishes
/// and immediately answer `Request::Show` from a second launch.
#[derive(Clone)]
pub struct IpcControllerSlot {
    inner: Arc<(Mutex<Option<Controller>>, Condvar)>,
}

impl IpcControllerSlot {
    pub fn new() -> Self {
        Self {
            inner: Arc::new((Mutex::new(None), Condvar::new())),
        }
    }

    pub fn set(&self, controller: Controller) {
        let (lock, cvar) = &*self.inner;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = Some(controller);
        cvar.notify_all();
    }

    fn wait(&self, timeout: Duration) -> Option<Controller> {
        let (lock, cvar) = &*self.inner;
        let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let (guard, _) = cvar
            .wait_timeout_while(guard, timeout, |c| c.is_none())
            .unwrap_or_else(|e| e.into_inner());
        guard.clone()
    }
}

fn lock_file_path() -> PathBuf {
    #[cfg(windows)]
    {
        dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(crate::platform::APP_ID)
            .join("kavox.lock")
    }
    #[cfg(not(windows))]
    {
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(dir) => PathBuf::from(dir).join("kavox.lock"),
            None => std::env::temp_dir().join(format!(
                "kavox-{}.lock",
                std::env::var("USER").unwrap_or_default()
            )),
        }
    }
}

/// Acquires the single-instance lock before starting audio or binding the IPC socket.
///
/// Returns `None` if another Kavox instance is already running (including an older 0.1.1 build
/// that doesn't recognize `Request::Show` or use the lock file) after asking it to show itself.
pub fn acquire_single_instance() -> Option<SingleInstanceGuard> {
    let path = lock_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
    {
        Ok(f) => f,
        Err(_) => {
            return if instance_alive() {
                None
            } else {
                Some(SingleInstanceGuard { _lock_file: None })
            };
        }
    };

    match file.try_lock() {
        Ok(()) => {
            // Lock acquired, but an older 0.1.1 instance (which didn't use a lock file) might
            // still be serving the IPC socket. Any valid JSON reply—even `ok: false` for an
            // unknown `Show` command—means an instance is already alive.
            if instance_alive() {
                return None;
            }
            Some(SingleInstanceGuard {
                _lock_file: Some(file),
            })
        }
        Err(TryLockError::WouldBlock) => {
            // Another native instance holds the lock. It may still be starting up and about to
            // bind its IPC socket, or quitting and about to release the lock, so retry until it
            // answers `Show` (or take over once the lock is free).
            let mut deadline = Instant::now() + SHOW_RETRY_WINDOW;
            // Extend the wait once for a quitting instance, so one stuck mid-shutdown (still
            // answering `ShuttingDown`) can't keep this launch waiting forever.
            let mut shutdown_extended = false;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return None;
                }
                match kavox_ipc::send_with_timeout(Request::Show, remaining.min(PROBE_TIMEOUT)) {
                    Ok(reply) if is_shutting_down(&reply) => {
                        if !shutdown_extended {
                            shutdown_extended = true;
                            deadline = deadline.max(Instant::now() + SHUTDOWN_RETRY_WINDOW);
                        }
                    }
                    Ok(_) => return None,
                    Err(_) => {}
                }
                if file.try_lock().is_ok() {
                    return Some(SingleInstanceGuard {
                        _lock_file: Some(file),
                    });
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return None;
                }
                std::thread::sleep(remaining.min(SHOW_RETRY_INTERVAL));
            }
        }
        Err(_) => {
            // Unexpected OS locking error: fall back to probing the socket.
            if instance_alive() {
                None
            } else {
                Some(SingleInstanceGuard {
                    _lock_file: Some(file),
                })
            }
        }
    }
}

/// Asks a running instance to show itself. An instance that's quitting doesn't count as alive.
fn instance_alive() -> bool {
    kavox_ipc::send_with_timeout(Request::Show, PROBE_TIMEOUT)
        .is_ok_and(|reply| !is_shutting_down(&reply))
}

fn is_shutting_down(reply: &Response) -> bool {
    reply
        .error
        .as_ref()
        .is_some_and(|e| e.code == SHUTTING_DOWN_CODE)
}

pub fn serve(cmd_tx: Sender<AppCommand>) -> IpcControllerSlot {
    let slot = IpcControllerSlot::new();
    let slot_bg = slot.clone();
    match kavox_ipc::serve(move |request| handle(&slot_bg, &cmd_tx, request)) {
        Ok(_) => tracing::info!("kavoxctl socket ready"),
        Err(e) => tracing::warn!("kavoxctl socket unavailable: {e}"),
    }
    slot
}

fn handle(slot: &IpcControllerSlot, cmd_tx: &Sender<AppCommand>, request: Request) -> Response {
    if SHUTTING_DOWN.load(Ordering::SeqCst) {
        return Response::error(SHUTTING_DOWN_CODE, "Kavox is quitting");
    }
    if request == Request::Show {
        let _ = cmd_tx.send(AppCommand::ShowWindow);
        return Response::ok();
    }

    let Some(controller) = slot.wait(STARTUP_WAIT) else {
        return Response::error(
            STARTING_CODE,
            "Kavox is still starting up; try again in a moment",
        );
    };
    let mut core = controller.lock();
    let result: Result<Response, Error> = match request {
        Request::Play { target } => core
            .resolve(&target)
            .and_then(|s| core.play_sound(&s.id))
            .map(|_| Response::ok()),
        Request::Stop { target } => core.resolve(&target).map(|s| {
            core.stop_sound(&s.id);
            Response::ok()
        }),
        Request::StopAll => {
            core.stop_all();
            Ok(Response::ok())
        }
        Request::Mute { mode } => {
            let muted = core.set_mic_muted(match mode {
                MuteMode::On => Some(true),
                MuteMode::Off => Some(false),
                MuteMode::Toggle => None,
            });
            Ok(Response::message(if muted {
                "mic muted"
            } else {
                "mic live"
            }))
        }
        Request::List => {
            let playing_ids: std::collections::HashSet<String> =
                core.playing().into_iter().map(|p| p.sound_id).collect();
            let library = core.library();
            let sounds = library
                .boards
                .iter()
                .flat_map(|board| {
                    board
                        .sounds
                        .iter()
                        .filter_map(|id| library.sounds.get(id))
                        .map(|s| SoundEntry {
                            id: s.id.clone(),
                            name: s.name.clone(),
                            board: board.name.clone(),
                            hotkey: s.hotkey.clone(),
                            board_id: Some(board.id.clone()),
                            duration_ms: Some(s.duration_ms),
                            volume: Some(s.volume),
                            looping: Some(s.looping),
                            retrigger: Some(retrigger_str(s.retrigger).to_owned()),
                            playing: Some(playing_ids.contains(&s.id)),
                        })
                })
                .collect();
            Ok(Response {
                ok: true,
                sounds: Some(sounds),
                ..Response::default()
            })
        }
        Request::ListBoards => Ok(Response {
            ok: true,
            boards: Some(boards_list(&core)),
            ..Response::default()
        }),
        Request::SetBoard { target } => set_board(&mut core, &target),
        Request::SetVoice {
            enabled,
            preset,
            semitones,
            hear_myself,
        } => apply_voice(&mut core, enabled, preset, semitones, hear_myself),
        Request::SetMixer(patch) => apply_mixer(&mut core, patch),
        Request::Status => Ok(Response {
            ok: true,
            status: Some(status_info(&core)),
            ..Response::default()
        }),
        Request::Show => unreachable!(),
    };
    result.unwrap_or_else(|e| Response::error(e.code(), e.to_string()))
}

fn retrigger_str(mode: Retrigger) -> &'static str {
    match mode {
        Retrigger::Restart => "restart",
        Retrigger::Overlap => "overlap",
        Retrigger::Toggle => "toggle",
    }
}

fn set_board(core: &mut Core, target: &str) -> Result<Response, Error> {
    let board = core.resolve_board(target)?;
    core.set_active_board(&board.id)?;
    Ok(Response {
        ok: true,
        message: Some(format!("active board: {}", board.name)),
        boards: Some(boards_list(core)),
        ..Response::default()
    })
}

fn boards_list(core: &Core) -> Vec<BoardEntry> {
    let library = core.library();
    library
        .boards
        .iter()
        .map(|b| BoardEntry {
            id: b.id.clone(),
            name: b.name.clone(),
            sound_count: b.sounds.len(),
            active: b.id == library.active_board,
        })
        .collect()
}

fn voice_state(core: &Core) -> VoiceState {
    let v = &core.settings().voice;
    VoiceState {
        enabled: v.enabled,
        preset: v.preset.clone(),
        semitones: v.semitones,
        hear_myself: v.hear_myself,
        presets: voice::PRESETS
            .iter()
            .map(|p| VoicePresetEntry {
                id: p.id.to_owned(),
                label: voice::label(p.id),
                semitones: p.semitones,
                wobble_depth: p.wobble_depth,
                wobble_hz: p.wobble_hz,
            })
            .collect(),
    }
}

fn mixer_state(core: &Core) -> MixerState {
    let a = &core.settings().audio;
    MixerState {
        mic_muted: a.mic_muted,
        mic_passthrough: a.mic_passthrough,
        mic_gain: a.mic_gain,
        sounds_to_mic_gain: a.sounds_to_mic_gain,
        sounds_to_monitor_gain: a.sounds_to_monitor_gain,
        monitor_enabled: a.monitor_enabled,
        normalize: a.normalize,
        global_volume_override: a.global_volume_override,
        global_sound_volume: a.global_sound_volume,
    }
}

fn apply_voice(
    core: &mut Core,
    enabled: Option<bool>,
    preset: Option<String>,
    semitones: Option<f32>,
    hear_myself: Option<bool>,
) -> Result<Response, Error> {
    let mut changer = core.settings().voice.clone();
    let has_preset = preset.is_some();

    if let Some(raw_preset) = preset {
        let normalized = raw_preset.trim().to_lowercase();
        let valid = normalized == voice::NORMAL
            || normalized == voice::CUSTOM
            || voice::preset(&normalized).is_some();
        if !valid {
            let valid_names: Vec<&str> = [voice::NORMAL, voice::CUSTOM]
                .into_iter()
                .chain(voice::PRESETS.iter().map(|p| p.id))
                .collect();
            return Err(Error::Invalid(format!(
                "unknown voice preset \"{raw_preset}\"; valid presets: {}",
                valid_names.join(", ")
            )));
        }
        changer.preset = normalized;
        if enabled.is_none() {
            changer.enabled = changer.preset != voice::NORMAL;
        }
    }

    if let Some(st) = semitones {
        if !st.is_finite() {
            return Err(Error::Invalid("semitones must be a finite number".into()));
        }
        changer.semitones = st.clamp(-12.0, 12.0);
        if !has_preset {
            changer.preset = voice::CUSTOM.to_owned();
            if enabled.is_none() {
                changer.enabled = true;
            }
        }
    }

    if let Some(on) = enabled {
        changer.enabled = on;
    }

    if let Some(hear) = hear_myself {
        changer.hear_myself = hear;
    }

    core.set_voice_changer(changer);
    let state = voice_state(core);
    let summary = if state.enabled {
        if state.preset == voice::CUSTOM {
            format!("voice changer on (custom {:+.1} st)", state.semitones)
        } else {
            format!("voice changer on ({})", state.preset)
        }
    } else {
        "voice changer off".to_owned()
    };
    Ok(Response {
        ok: true,
        message: Some(summary),
        voice: Some(state),
        ..Response::default()
    })
}

fn apply_mixer(core: &mut Core, patch: MixerPatch) -> Result<Response, Error> {
    let check_gain = |name: &str, v: Option<f32>| -> Result<(), Error> {
        if let Some(val) = v
            && !val.is_finite()
        {
            return Err(Error::Invalid(format!("{name} must be a finite number")));
        }
        Ok(())
    };
    check_gain("mic_gain", patch.mic_gain)?;
    check_gain("sounds_to_mic_gain", patch.sounds_to_mic_gain)?;
    check_gain("sounds_to_monitor_gain", patch.sounds_to_monitor_gain)?;
    check_gain("global_sound_volume", patch.global_sound_volume)?;

    let mut audio = core.settings().audio.clone();
    if let Some(v) = patch.mic_muted {
        audio.mic_muted = v;
    }
    if let Some(v) = patch.mic_passthrough {
        audio.mic_passthrough = v;
    }
    if let Some(v) = patch.mic_gain {
        audio.mic_gain = v;
    }
    if let Some(v) = patch.sounds_to_mic_gain {
        audio.sounds_to_mic_gain = v;
    }
    if let Some(v) = patch.sounds_to_monitor_gain {
        audio.sounds_to_monitor_gain = v;
    }
    if let Some(v) = patch.monitor_enabled {
        audio.monitor_enabled = v;
    }
    if let Some(v) = patch.normalize {
        audio.normalize = v;
    }
    if let Some(v) = patch.global_volume_override {
        audio.global_volume_override = v;
    }
    if let Some(v) = patch.global_sound_volume {
        audio.global_sound_volume = v;
    }
    core.set_audio_settings(audio);
    Ok(Response {
        ok: true,
        message: Some("mixer updated".to_owned()),
        mixer: Some(mixer_state(core)),
        ..Response::default()
    })
}

fn status_info(core: &Core) -> StatusInfo {
    let library = core.library();
    let active_board_id = library.active_board.clone();
    let active_board_name = library
        .board(&active_board_id)
        .map(|b| b.name.clone())
        .unwrap_or_default();

    let playing = core
        .playing()
        .into_iter()
        .map(|p| {
            let sound_name = library
                .sounds
                .get(&p.sound_id)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| p.sound_id.clone());
            let progress = if p.frames > 0 {
                (p.frame as f32 / p.frames as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            PlayingEntry {
                sound_id: p.sound_id,
                sound_name,
                voice_id: p.voice_id,
                frame: p.frame,
                frames: p.frames,
                progress,
            }
        })
        .collect();

    let stream_entry = |s: StreamInfo| StreamEntry {
        device: s.device,
        label: s.label,
        rate: s.rate,
        fallback: s.fallback,
    };
    let s = core.stream_status();
    let streams = StreamsState {
        mic: s.mic.map(stream_entry),
        virtual_out: s.virtual_out.map(stream_entry),
        monitor: s.monitor.map(stream_entry),
    };

    let d = core.device_list();
    let devices = DevicesState {
        inputs: d
            .inputs
            .iter()
            .map(|dev| DeviceEntry {
                id: dev.id.clone(),
                label: dev.label.clone(),
                is_default: dev.is_default,
            })
            .collect(),
        outputs: d
            .outputs
            .iter()
            .map(|dev| DeviceEntry {
                id: dev.id.clone(),
                label: dev.label.clone(),
                is_default: dev.is_default,
            })
            .collect(),
    };

    let virtual_status = match core.virtual_status() {
        VirtualStatus::Ok { device, mic_label } => format!("ok ({mic_label} via {device})"),
        VirtualStatus::NotInstalled => "not installed".to_owned(),
        VirtualStatus::Missing { device } => format!("missing ({device})"),
        VirtualStatus::CableIsDefaultOutput { device, mic_label } => {
            format!("warning: cable is default output ({mic_label} via {device})")
        }
        VirtualStatus::Failed { message } => format!("failed: {message}"),
    };

    StatusInfo {
        active_board_id,
        active_board_name,
        boards: boards_list(core),
        playing,
        mixer: mixer_state(core),
        voice: voice_state(core),
        virtual_status,
        streams,
        devices,
    }
}
