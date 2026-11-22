//! Serves `kavoxctl` requests and single-instance enforcement over an OS lock file + local socket.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

use kavox_core::{Controller, Error};
use kavox_ipc::{MuteMode, Request, Response, SoundEntry};

use crate::state::AppCommand;

/// Holds the OS-level exclusive single-instance lock until the process exits.
pub struct SingleInstanceGuard {
    _lock_file: Option<File>,
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
            return if kavox_ipc::send(Request::Show).is_ok() {
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
            if kavox_ipc::send(Request::Show).is_ok() {
                return None;
            }
            Some(SingleInstanceGuard {
                _lock_file: Some(file),
            })
        }
        Err(TryLockError::WouldBlock) => {
            // Another native instance holds the lock. It may still be starting up and about to
            // bind its IPC socket, so retry briefly so its window gets focused.
            for attempt in 0..12 {
                if kavox_ipc::send(Request::Show).is_ok() {
                    break;
                }
                if attempt < 11 {
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            None
        }
        Err(_) => {
            // Unexpected OS locking error: fall back to probing the socket.
            if kavox_ipc::send(Request::Show).is_ok() {
                None
            } else {
                Some(SingleInstanceGuard {
                    _lock_file: Some(file),
                })
            }
        }
    }
}

pub fn serve(controller: Controller, cmd_tx: Sender<AppCommand>) {
    match kavox_ipc::serve(move |request| handle(&controller, &cmd_tx, request)) {
        Ok(_) => tracing::info!("kavoxctl socket ready"),
        Err(e) => tracing::warn!("kavoxctl socket unavailable: {e}"),
    }
}

fn handle(controller: &Controller, cmd_tx: &Sender<AppCommand>, request: Request) -> Response {
    if request == Request::Show {
        let _ = cmd_tx.send(AppCommand::ShowWindow);
        return Response::ok();
    }

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
