//! Serves `kavoxctl` requests and single-instance enforcement over an OS lock file + local socket.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use kavox_core::{Controller, Error};
use kavox_ipc::{MuteMode, Request, Response, SoundEntry};

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
                        })
                })
                .collect();
            Ok(Response {
                ok: true,
                sounds: Some(sounds),
                ..Response::default()
            })
        }
        Request::Show => unreachable!(),
    };
    result.unwrap_or_else(|e| Response::error(e.code(), e.to_string()))
}
