//! Serves `kavoxctl` requests and single-instance `Show` activation over the local socket.

use std::sync::mpsc::Sender;

use kavox_core::{Controller, Error};
use kavox_ipc::{MuteMode, Request, Response, SoundEntry};

use crate::state::AppCommand;

/// Checks if another Kavox instance is already running; if so, asks it to show its window.
pub fn focus_existing_instance() -> bool {
    matches!(kavox_ipc::send(Request::Show), Ok(resp) if resp.ok)
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
