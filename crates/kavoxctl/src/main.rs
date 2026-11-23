//! `kavoxctl`: control a running Kavox from scripts, compositor key bindings, and MCP clients.

mod mcp;

use std::process::ExitCode;

use kavox_ipc::{MuteMode, Request};

const USAGE: &str = "usage: kavoxctl <command>

  play <name|id>              play a sound (name match: exact, then unique prefix)
  stop <name|id>              stop a sound
  stop-all                    stop every sound
  mute [on|off|toggle]        mute the mic (default: toggle)
  list                        list sounds
  boards                      list boards
  board <name|id>             switch active board
  voice on|off|<preset>       enable/disable voice changer or pick a preset
  voice pitch <semitones>     set custom pitch shift (-12 to +12)
  status                      print live status as JSON
  mcp                         run an MCP (Model Context Protocol) server over stdio";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "mcp" {
        return match mcp::run_stdio() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("kavoxctl mcp: {e}");
                ExitCode::FAILURE
            }
        };
    }

    let Some(request) = parse(&args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(64);
    };
    let listing = request == Request::List;
    let listing_boards = request == Request::ListBoards;
    let showing_status = request == Request::Status;

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
    if listing_boards {
        for board in response.boards.unwrap_or_default() {
            let marker = if board.active { "*" } else { " " };
            println!(
                "{marker} {}  {}  ({} sounds)",
                board.id, board.name, board.sound_count
            );
        }
    }
    if showing_status && let Some(status) = response.status {
        match serde_json::to_string_pretty(&status) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("kavoxctl: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

fn parse(args: &[String]) -> Option<Request> {
    let rest = args.get(1..).unwrap_or_default().join(" ");
    match args.first()?.as_str() {
        "play" if !rest.is_empty() => Some(Request::Play { target: rest }),
        "stop" if !rest.is_empty() => Some(Request::Stop { target: rest }),
        "stop-all" if args.len() == 1 => Some(Request::StopAll),
        "mute" => Some(Request::Mute {
            mode: match rest.as_str() {
                "" | "toggle" => MuteMode::Toggle,
                "on" => MuteMode::On,
                "off" => MuteMode::Off,
                _ => return None,
            },
        }),
        "list" if args.len() == 1 => Some(Request::List),
        "boards" if args.len() == 1 => Some(Request::ListBoards),
        "board" if !rest.is_empty() => Some(Request::SetBoard { target: rest }),
        "status" if args.len() == 1 => Some(Request::Status),
        "voice" => parse_voice(&args[1..]),
        _ => None,
    }
}

fn parse_voice(args: &[String]) -> Option<Request> {
    match args {
        [flag] if flag == "on" => Some(Request::SetVoice {
            enabled: Some(true),
            preset: None,
            semitones: None,
            hear_myself: None,
        }),
        [flag] if flag == "off" => Some(Request::SetVoice {
            enabled: Some(false),
            preset: None,
            semitones: None,
            hear_myself: None,
        }),
        [cmd, value] if cmd == "pitch" => {
            let semitones: f32 = value.parse().ok()?;
            Some(Request::SetVoice {
                enabled: None,
                preset: None,
                semitones: Some(semitones),
                hear_myself: None,
            })
        }
        [preset] if !preset.is_empty() => Some(Request::SetVoice {
            enabled: None,
            preset: Some(preset.clone()),
            semitones: None,
            hear_myself: None,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn parses_cli_commands() {
        assert_eq!(
            parse(&args(&["play", "vine", "boom"])),
            Some(Request::Play {
                target: "vine boom".into()
            })
        );
        assert_eq!(parse(&args(&["boards"])), Some(Request::ListBoards));
        assert_eq!(
            parse(&args(&["board", "Memes"])),
            Some(Request::SetBoard {
                target: "Memes".into()
            })
        );
        assert_eq!(parse(&args(&["status"])), Some(Request::Status));
        assert_eq!(
            parse(&args(&["voice", "chipmunk"])),
            Some(Request::SetVoice {
                enabled: None,
                preset: Some("chipmunk".into()),
                semitones: None,
                hear_myself: None,
            })
        );
        assert_eq!(
            parse(&args(&["voice", "pitch", "-4.5"])),
            Some(Request::SetVoice {
                enabled: None,
                preset: None,
                semitones: Some(-4.5),
                hear_myself: None,
            })
        );
        assert_eq!(parse(&args(&["voice", "pitch", "nope"])), None);
    }
}
