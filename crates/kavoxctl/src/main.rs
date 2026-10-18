//! `kavoxctl`: control a running Kavox from scripts and compositor key bindings.

use std::process::ExitCode;

use kavox_ipc::{MuteMode, Request};

const USAGE: &str = "usage: kavoxctl <command>

  play <name|id>          play a sound (name match: exact, then unique prefix)
  stop <name|id>          stop a sound
  stop-all                stop every sound
  mute [on|off|toggle]    mute the mic (default: toggle)
  list                    list sounds";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(request) = parse(&args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(64);
    };
    let listing = request == Request::List;

    let response = match kavox_ipc::send(request) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("kavoxctl: Kavox isn't running ({e})");
            return ExitCode::from(2);
        }
    };
    if let Some(error) = response.error {
        eprintln!("kavoxctl: {}", error.message);
        return ExitCode::FAILURE;
    }
    if let Some(message) = response.message {
        println!("{message}");
    }
    if listing {
        for sound in response.sounds.unwrap_or_default() {
            let hotkey = sound.hotkey.map(|h| format!("  [{h}]")).unwrap_or_default();
            println!("{}  {}  ({}){hotkey}", sound.id, sound.name, sound.board);
        }
    }
    ExitCode::SUCCESS
}

fn parse(args: &[String]) -> Option<Request> {
    let rest = args.get(1..).unwrap_or_default().join(" ");
    match args.first()?.as_str() {
        "play" if !rest.is_empty() => Some(Request::Play { target: rest }),
        "stop" if !rest.is_empty() => Some(Request::Stop { target: rest }),
        "stop-all" => Some(Request::StopAll),
        "mute" => Some(Request::Mute {
            mode: match rest.as_str() {
                "" | "toggle" => MuteMode::Toggle,
                "on" => MuteMode::On,
                "off" => MuteMode::Off,
                _ => return None,
            },
        }),
        "list" => Some(Request::List),
        _ => None,
    }
}
