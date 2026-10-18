//! Serves `kavoxctl` requests.

use kavox_core::Error;
use kavox_ipc::{MuteMode, Request, Response, SoundEntry};
use tauri::{AppHandle, Manager};

use crate::AppState;

pub fn serve(app: AppHandle) {
    match kavox_ipc::serve(move |request| handle(&app, request)) {
        Ok(_) => tracing::info!("kavoxctl socket ready"),
        Err(e) => tracing::warn!("kavoxctl socket unavailable: {e}"),
    }
}

fn handle(app: &AppHandle, request: Request) -> Response {
    let state = app.state::<AppState>();
    let mut core = state.controller.lock();
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
    };
    result.unwrap_or_else(|e| Response::error(e.code(), e.to_string()))
}
