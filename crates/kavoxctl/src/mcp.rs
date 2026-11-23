//! Model Context Protocol (MCP) stdio server for controlling a running Kavox instance.
//!
//! Implements newline-delimited JSON-RPC 2.0 over stdin/stdout with zero external runtime
//! dependencies, forwarding live-control requests to the running Kavox app over `kavox-ipc`.

use std::io::{self, BufRead, Write};

use kavox_ipc::{MixerPatch, MuteMode, Request, Response, SoundEntry};
use serde::Deserialize;
use serde_json::{Value, json};

const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
const DEFAULT_PROTOCOL_VERSION: &str = "2025-03-26";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;

const SERVER_INSTRUCTIONS: &str = "\
Control a running Kavox soundboard and voice changer desktop app. \
Use `get_status` or `list_sounds` first to discover available sounds, boards, and voice presets. \
Sounds and boards can be targeted by ID, exact name, or unique case-insensitive prefix.";

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<Value>,
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
}

/// Runs the MCP server on standard input/output against the local Kavox IPC socket.
pub fn run_stdio() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve_io(stdin.lock(), stdout.lock(), kavox_ipc::send)
}

/// Core JSON-RPC 2.0 stdio loop with injectable IPC transport for testing.
pub fn serve_io<R, W, F>(mut reader: R, mut writer: W, mut send: F) -> io::Result<()>
where
    R: BufRead,
    W: Write,
    F: FnMut(Request) -> io::Result<Response>,
{
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Value>(trimmed) {
            Ok(Value::Object(obj)) => handle_object(Value::Object(obj), &mut send),
            Ok(Value::Array(batch)) => handle_batch(batch, &mut send),
            Ok(_) => Some(jsonrpc_error(
                Value::Null,
                INVALID_REQUEST,
                "JSON-RPC request must be an object",
            )),
            Err(err) => Some(jsonrpc_error(
                Value::Null,
                PARSE_ERROR,
                format!("invalid JSON: {err}"),
            )),
        };

        if let Some(reply) = response {
            write_message(&mut writer, &reply)?;
        }
    }
    Ok(())
}

fn write_message<W: Write>(writer: &mut W, value: &Value) -> io::Result<()> {
    let encoded = serde_json::to_string(value).map_err(io::Error::other)?;
    writer.write_all(encoded.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn handle_batch<F>(batch: Vec<Value>, send: &mut F) -> Option<Value>
where
    F: FnMut(Request) -> io::Result<Response>,
{
    if batch.is_empty() {
        return Some(jsonrpc_error(
            Value::Null,
            INVALID_REQUEST,
            "empty JSON-RPC batch",
        ));
    }
    let replies: Vec<Value> = batch
        .into_iter()
        .filter_map(|item| match item {
            Value::Object(obj) => handle_object(Value::Object(obj), send),
            _ => Some(jsonrpc_error(
                Value::Null,
                INVALID_REQUEST,
                "batch entry must be an object",
            )),
        })
        .collect();
    if replies.is_empty() {
        None
    } else {
        Some(Value::Array(replies))
    }
}

fn handle_object<F>(raw: Value, send: &mut F) -> Option<Value>
where
    F: FnMut(Request) -> io::Result<Response>,
{
    let req: JsonRpcRequest = match serde_json::from_value(raw) {
        Ok(r) => r,
        Err(err) => {
            return Some(jsonrpc_error(
                Value::Null,
                INVALID_REQUEST,
                format!("invalid JSON-RPC message: {err}"),
            ));
        }
    };

    if req.jsonrpc.as_deref() != Some("2.0") {
        let id = req.id.unwrap_or(Value::Null);
        return Some(jsonrpc_error(
            id,
            INVALID_REQUEST,
            "jsonrpc version must be \"2.0\"",
        ));
    }

    // Responses sent from client to server have no method, and notifications omit `id`.
    let method = req.method?;
    let id = req.id?;

    let params = req.params.unwrap_or_else(|| json!({}));
    match dispatch_method(&method, params, send) {
        Ok(result) => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        })),
        Err((code, message)) => Some(jsonrpc_error(id, code, message)),
    }
}

fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into(),
        }
    })
}

fn dispatch_method<F>(method: &str, params: Value, send: &mut F) -> Result<Value, (i64, String)>
where
    F: FnMut(Request) -> io::Result<Response>,
{
    match method {
        "initialize" => Ok(handle_initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => handle_tool_call(params, send),
        "resources/list" => Ok(json!({ "resources": resource_definitions() })),
        "resources/read" => handle_resource_read(params, send),
        _ => Err((METHOD_NOT_FOUND, format!("unknown method \"{method}\""))),
    }
}

fn handle_initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let negotiated = requested
        .filter(|v| SUPPORTED_PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(DEFAULT_PROTOCOL_VERSION);

    json!({
        "protocolVersion": negotiated,
        "capabilities": {
            "tools": {},
            "resources": {}
        },
        "serverInfo": {
            "name": "kavox",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": SERVER_INSTRUCTIONS
    })
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "get_status",
            "description": "Get live Kavox status: active board, all boards, currently playing sounds, mic mute and mixer levels, voice changer state and available presets, virtual mic status, active streams, and audio devices.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": true,
                "idempotentHint": true,
                "openWorldHint": false
            }
        },
        {
            "name": "list_sounds",
            "description": "List sounds in the Kavox soundboard library. Optionally filter by board name or ID, name substring query, or currently playing sounds.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "board": {
                        "type": "string",
                        "description": "Optional board ID or board name (case-insensitive) to filter by."
                    },
                    "query": {
                        "type": "string",
                        "description": "Optional case-insensitive substring to match against sound names."
                    },
                    "playing_only": {
                        "type": "boolean",
                        "description": "If true, return only sounds that are currently playing."
                    }
                },
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": true,
                "idempotentHint": true,
                "openWorldHint": false
            }
        },
        {
            "name": "play_sound",
            "description": "Play a soundboard clip into the Kavox virtual mic and monitor by sound ID, exact name, or unique case-insensitive name prefix. Respects the pad's retrigger mode (restart, overlap, or toggle).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Sound ID, exact name, or unique case-insensitive name prefix."
                    }
                },
                "required": ["target"],
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "openWorldHint": false
            }
        },
        {
            "name": "stop_sound",
            "description": "Stop all active playback instances of a specific sound by ID, exact name, or unique prefix.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Sound ID, exact name, or unique case-insensitive name prefix."
                    }
                },
                "required": ["target"],
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        },
        {
            "name": "stop_all_sounds",
            "description": "Immediately stop every currently playing sound in Kavox.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        },
        {
            "name": "list_boards",
            "description": "List all Kavox soundboards with their IDs, names, sound counts, and active indicator.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": true,
                "idempotentHint": true,
                "openWorldHint": false
            }
        },
        {
            "name": "set_active_board",
            "description": "Switch the active soundboard in Kavox by board ID, exact name, or unique case-insensitive prefix.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Board ID, exact name, or unique case-insensitive name prefix."
                    }
                },
                "required": ["target"],
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        },
        {
            "name": "set_mic_mute",
            "description": "Mute, unmute, or toggle microphone passthrough into the Kavox virtual mic.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["on", "off", "toggle"],
                        "description": "Mute action: 'on' to mute, 'off' to unmute, or 'toggle' (default)."
                    }
                },
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "openWorldHint": false
            }
        },
        {
            "name": "set_voice_changer",
            "description": "Control the live voice changer. Pick a built-in preset (automatically turns it on unless enabled=false), set custom pitch in semitones (-12 to +12), turn it on/off, or toggle hear-myself monitoring.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "enabled": {
                        "type": "boolean",
                        "description": "Explicitly enable or disable the voice changer."
                    },
                    "preset": {
                        "type": "string",
                        "enum": ["normal", "custom", "chipmunk", "pixie", "deep", "giant", "monster", "wobbly", "drunk", "alien"],
                        "description": "Voice preset ID. Setting a named preset enables the voice changer unless enabled=false; setting 'normal' disables it unless enabled=true."
                    },
                    "semitones": {
                        "type": "number",
                        "minimum": -12,
                        "maximum": 12,
                        "description": "Custom pitch shift in semitones (-12.0 to +12.0). If preset is omitted, switches to 'custom' and enables the voice changer unless enabled=false."
                    },
                    "hear_myself": {
                        "type": "boolean",
                        "description": "Whether to monitor your own voice-changed microphone in headphones."
                    }
                },
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        },
        {
            "name": "set_mixer",
            "description": "Adjust Kavox live mixer gains and audio toggles. Only provided fields are changed. Gain values range from 0.0 (silent) to 2.0 (200%), with 1.0 as unity gain.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mic_muted": {
                        "type": "boolean",
                        "description": "Mute or unmute microphone passthrough."
                    },
                    "mic_passthrough": {
                        "type": "boolean",
                        "description": "Enable or disable microphone capture/passthrough."
                    },
                    "mic_gain": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 2,
                        "description": "Microphone gain (0.0 to 2.0, default 1.0)."
                    },
                    "sounds_to_mic_gain": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 2,
                        "description": "Soundboard volume sent to the virtual mic (0.0 to 2.0)."
                    },
                    "sounds_to_monitor_gain": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 2,
                        "description": "Soundboard volume sent to headphones/monitor (0.0 to 2.0)."
                    },
                    "monitor_enabled": {
                        "type": "boolean",
                        "description": "Enable or disable headphone monitoring output."
                    },
                    "normalize": {
                        "type": "boolean",
                        "description": "Enable or disable automatic loudness normalization on sound clips."
                    },
                    "global_volume_override": {
                        "type": "boolean",
                        "description": "Use global_sound_volume for all pads instead of per-pad volume."
                    },
                    "global_sound_volume": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 2,
                        "description": "Override volume applied to all sounds when global_volume_override is true (0.0 to 2.0)."
                    }
                },
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        },
        {
            "name": "show_window",
            "description": "Show and bring the Kavox desktop window to the foreground.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }
    ])
}

fn resource_definitions() -> Value {
    json!([
        {
            "uri": "kavox://status",
            "name": "Kavox Live Status",
            "description": "Current Kavox state: active board, playing sounds, mic & mixer settings, voice changer state, virtual mic status, streams, and audio devices.",
            "mimeType": "application/json"
        },
        {
            "uri": "kavox://sounds",
            "name": "Kavox Soundboard Library",
            "description": "All sounds in the Kavox library across all boards, including IDs, durations, hotkeys, and playing state.",
            "mimeType": "application/json"
        }
    ])
}

fn handle_resource_read<F>(params: Value, send: &mut F) -> Result<Value, (i64, String)>
where
    F: FnMut(Request) -> io::Result<Response>,
{
    let uri = params.get("uri").and_then(Value::as_str).ok_or_else(|| {
        (
            INVALID_PARAMS,
            "missing string parameter \"uri\"".to_owned(),
        )
    })?;

    let payload = match uri {
        "kavox://status" => {
            let resp = ipc_call(Request::Status, send).map_err(|e| (INTERNAL_ERROR, e))?;
            serde_json::to_string_pretty(&resp.status)
                .map_err(|e| (INTERNAL_ERROR, e.to_string()))?
        }
        "kavox://sounds" => {
            let resp = ipc_call(Request::List, send).map_err(|e| (INTERNAL_ERROR, e))?;
            let sounds = resp.sounds.unwrap_or_default();
            serde_json::to_string_pretty(&sounds).map_err(|e| (INTERNAL_ERROR, e.to_string()))?
        }
        _ => {
            return Err((INVALID_PARAMS, format!("unknown resource URI \"{uri}\"")));
        }
    };

    Ok(json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/json",
            "text": payload
        }]
    }))
}

fn handle_tool_call<F>(params: Value, send: &mut F) -> Result<Value, (i64, String)>
where
    F: FnMut(Request) -> io::Result<Response>,
{
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| (INVALID_PARAMS, "missing tool \"name\"".to_owned()))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !args.is_object() {
        return Err((
            INVALID_PARAMS,
            "tool \"arguments\" must be a JSON object".to_owned(),
        ));
    }

    match name {
        "get_status" => Ok(tool_from_result(ipc_call(Request::Status, send).and_then(
            |resp| serde_json::to_string_pretty(&resp.status).map_err(|e| e.to_string()),
        ))),
        "list_sounds" => Ok(handle_list_sounds(&args, send)),
        "play_sound" => {
            let target = match require_non_empty_str(&args, "target") {
                Ok(t) => t,
                Err(msg) => return Ok(tool_error(msg)),
            };
            Ok(tool_from_result(
                ipc_call(
                    Request::Play {
                        target: target.clone(),
                    },
                    send,
                )
                .map(|resp| {
                    resp.message
                        .unwrap_or_else(|| format!("Played \"{target}\""))
                }),
            ))
        }
        "stop_sound" => {
            let target = match require_non_empty_str(&args, "target") {
                Ok(t) => t,
                Err(msg) => return Ok(tool_error(msg)),
            };
            Ok(tool_from_result(
                ipc_call(
                    Request::Stop {
                        target: target.clone(),
                    },
                    send,
                )
                .map(|resp| {
                    resp.message
                        .unwrap_or_else(|| format!("Stopped \"{target}\""))
                }),
            ))
        }
        "stop_all_sounds" => Ok(tool_from_result(ipc_call(Request::StopAll, send).map(
            |resp| {
                resp.message
                    .unwrap_or_else(|| "Stopped all sounds".to_owned())
            },
        ))),
        "list_boards" => Ok(tool_from_result(
            ipc_call(Request::ListBoards, send).and_then(|resp| {
                let boards = resp.boards.unwrap_or_default();
                serde_json::to_string_pretty(&boards).map_err(|e| e.to_string())
            }),
        )),
        "set_active_board" => {
            let target = match require_non_empty_str(&args, "target") {
                Ok(t) => t,
                Err(msg) => return Ok(tool_error(msg)),
            };
            Ok(tool_from_result(
                ipc_call(Request::SetBoard { target }, send).and_then(|resp| {
                    serde_json::to_string_pretty(&json!({
                        "message": resp.message,
                        "boards": resp.boards.unwrap_or_default()
                    }))
                    .map_err(|e| e.to_string())
                }),
            ))
        }
        "set_mic_mute" => {
            let mode = match args.get("mode").and_then(Value::as_str) {
                None | Some("toggle") => MuteMode::Toggle,
                Some("on") => MuteMode::On,
                Some("off") => MuteMode::Off,
                Some(other) => {
                    return Ok(tool_error(format!(
                        "invalid mode \"{other}\"; expected \"on\", \"off\", or \"toggle\""
                    )));
                }
            };
            Ok(tool_from_result(
                ipc_call(Request::Mute { mode }, send).map(|resp| {
                    resp.message
                        .unwrap_or_else(|| "Updated mic mute".to_owned())
                }),
            ))
        }
        "set_voice_changer" => Ok(handle_set_voice(&args, send)),
        "set_mixer" => Ok(handle_set_mixer(&args, send)),
        "show_window" => Ok(tool_from_result(ipc_call(Request::Show, send).map(
            |resp| {
                resp.message
                    .unwrap_or_else(|| "Shown Kavox window".to_owned())
            },
        ))),
        _ => Err((INVALID_PARAMS, format!("unknown tool \"{name}\""))),
    }
}

fn handle_list_sounds<F>(args: &Value, send: &mut F) -> Value
where
    F: FnMut(Request) -> io::Result<Response>,
{
    let board_filter = match optional_str(args, "board") {
        Ok(v) => v.map(|s| s.trim().to_lowercase()),
        Err(msg) => return tool_error(msg),
    };
    let query_filter = match optional_str(args, "query") {
        Ok(v) => v.map(|s| s.trim().to_lowercase()),
        Err(msg) => return tool_error(msg),
    };
    let playing_only = match optional_bool(args, "playing_only") {
        Ok(v) => v.unwrap_or(false),
        Err(msg) => return tool_error(msg),
    };

    tool_from_result(ipc_call(Request::List, send).and_then(|resp| {
        let sounds: Vec<SoundEntry> = resp
            .sounds
            .unwrap_or_default()
            .into_iter()
            .filter(|s| {
                if let Some(board) = &board_filter {
                    let matches_name = s.board.to_lowercase() == *board;
                    let matches_id = s
                        .board_id
                        .as_deref()
                        .is_some_and(|id| id.to_lowercase() == *board);
                    if !matches_name && !matches_id {
                        return false;
                    }
                }
                if let Some(q) = &query_filter
                    && !q.is_empty()
                    && !s.name.to_lowercase().contains(q)
                {
                    return false;
                }
                if playing_only && !s.playing.unwrap_or(false) {
                    return false;
                }
                true
            })
            .collect();
        serde_json::to_string_pretty(&sounds).map_err(|e| e.to_string())
    }))
}

fn handle_set_voice<F>(args: &Value, send: &mut F) -> Value
where
    F: FnMut(Request) -> io::Result<Response>,
{
    let enabled = match optional_bool(args, "enabled") {
        Ok(v) => v,
        Err(msg) => return tool_error(msg),
    };
    let preset = match optional_str(args, "preset") {
        Ok(v) => v,
        Err(msg) => return tool_error(msg),
    };
    let semitones = match optional_f32(args, "semitones") {
        Ok(v) => v,
        Err(msg) => return tool_error(msg),
    };
    let hear_myself = match optional_bool(args, "hear_myself") {
        Ok(v) => v,
        Err(msg) => return tool_error(msg),
    };

    tool_from_result(
        ipc_call(
            Request::SetVoice {
                enabled,
                preset,
                semitones,
                hear_myself,
            },
            send,
        )
        .and_then(|resp| {
            serde_json::to_string_pretty(&json!({
                "message": resp.message,
                "voice": resp.voice
            }))
            .map_err(|e| e.to_string())
        }),
    )
}

fn handle_set_mixer<F>(args: &Value, send: &mut F) -> Value
where
    F: FnMut(Request) -> io::Result<Response>,
{
    let patch = match (
        optional_bool(args, "mic_muted"),
        optional_bool(args, "mic_passthrough"),
        optional_f32(args, "mic_gain"),
        optional_f32(args, "sounds_to_mic_gain"),
        optional_f32(args, "sounds_to_monitor_gain"),
        optional_bool(args, "monitor_enabled"),
        optional_bool(args, "normalize"),
        optional_bool(args, "global_volume_override"),
        optional_f32(args, "global_sound_volume"),
    ) {
        (
            Ok(mic_muted),
            Ok(mic_passthrough),
            Ok(mic_gain),
            Ok(sounds_to_mic_gain),
            Ok(sounds_to_monitor_gain),
            Ok(monitor_enabled),
            Ok(normalize),
            Ok(global_volume_override),
            Ok(global_sound_volume),
        ) => MixerPatch {
            mic_muted,
            mic_passthrough,
            mic_gain,
            sounds_to_mic_gain,
            sounds_to_monitor_gain,
            monitor_enabled,
            normalize,
            global_volume_override,
            global_sound_volume,
        },
        (Err(e), ..)
        | (_, Err(e), ..)
        | (_, _, Err(e), ..)
        | (_, _, _, Err(e), ..)
        | (_, _, _, _, Err(e), ..)
        | (_, _, _, _, _, Err(e), ..)
        | (_, _, _, _, _, _, Err(e), ..)
        | (_, _, _, _, _, _, _, Err(e), ..)
        | (_, _, _, _, _, _, _, _, Err(e)) => return tool_error(e),
    };

    tool_from_result(ipc_call(Request::SetMixer(patch), send).and_then(|resp| {
        serde_json::to_string_pretty(&json!({
            "message": resp.message,
            "mixer": resp.mixer
        }))
        .map_err(|e| e.to_string())
    }))
}

fn ipc_call<F>(request: Request, send: &mut F) -> Result<Response, String>
where
    F: FnMut(Request) -> io::Result<Response>,
{
    let response = send(request)
        .map_err(|e| format!("Kavox isn't running or IPC failed ({e}). Launch Kavox first."))?;
    if let Some(err) = response.error {
        return Err(format!("{}: {}", err.code, err.message));
    }
    if !response.ok {
        return Err("Kavox returned an unsuccessful response".to_owned());
    }
    Ok(response)
}

fn tool_from_result(result: Result<String, String>) -> Value {
    match result {
        Ok(text) => tool_ok(text),
        Err(err) => tool_error(err),
    }
}

fn tool_ok(text: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": text.into() }],
        "isError": false
    })
}

fn tool_error(message: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": message.into() }],
        "isError": true
    })
}

fn require_non_empty_str(args: &Value, key: &str) -> Result<String, String> {
    let Some(val) = args.get(key) else {
        return Err(format!("missing required parameter \"{key}\""));
    };
    let Some(s) = val.as_str() else {
        return Err(format!("parameter \"{key}\" must be a string"));
    };
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(format!("parameter \"{key}\" cannot be empty"));
    }
    Ok(trimmed.to_owned())
}

fn optional_str(args: &Value, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("parameter \"{key}\" must be a string")),
    }
}

fn optional_bool(args: &Value, key: &str) -> Result<Option<bool>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("parameter \"{key}\" must be a boolean")),
    }
}

fn optional_f32(args: &Value, key: &str) -> Result<Option<f32>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_f64()
            .filter(|v| v.is_finite())
            .map(|v| Some(v as f32))
            .ok_or_else(|| format!("parameter \"{key}\" must be a finite number")),
        Some(_) => Err(format!("parameter \"{key}\" must be a number")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kavox_ipc::{
        BoardEntry, DevicesState, MixerState, StatusInfo, StreamsState, VoicePresetEntry,
        VoiceState,
    };

    fn run_session(
        input_lines: &[&str],
        mut handler: impl FnMut(Request) -> io::Result<Response>,
    ) -> Vec<Value> {
        let input = input_lines.join("\n") + "\n";
        let mut output = Vec::new();
        serve_io(input.as_bytes(), &mut output, &mut handler).unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn sample_status() -> StatusInfo {
        StatusInfo {
            active_board_id: "b1".into(),
            active_board_name: "Main".into(),
            boards: vec![
                BoardEntry {
                    id: "b1".into(),
                    name: "Main".into(),
                    sound_count: 2,
                    active: true,
                },
                BoardEntry {
                    id: "b2".into(),
                    name: "Memes".into(),
                    sound_count: 1,
                    active: false,
                },
            ],
            playing: vec![],
            mixer: MixerState {
                mic_muted: false,
                mic_passthrough: true,
                mic_gain: 1.0,
                sounds_to_mic_gain: 0.8,
                sounds_to_monitor_gain: 0.5,
                monitor_enabled: true,
                normalize: true,
                global_volume_override: false,
                global_sound_volume: 1.0,
            },
            voice: VoiceState {
                enabled: false,
                preset: "normal".into(),
                semitones: 0.0,
                hear_myself: false,
                presets: vec![VoicePresetEntry {
                    id: "chipmunk".into(),
                    label: "Chipmunk".into(),
                    semitones: 10.0,
                    wobble_depth: 0.0,
                    wobble_hz: 0.0,
                }],
            },
            virtual_status: "ok (Kavox Mic via kavox_sink)".into(),
            streams: StreamsState::default(),
            devices: DevicesState::default(),
        }
    }

    #[test]
    fn initialize_notifications_ping_and_tools_list() {
        let replies = run_session(
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
                r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#,
            ],
            |_| Ok(Response::ok()),
        );

        assert_eq!(replies.len(), 4);
        assert_eq!(replies[0]["id"], 1);
        assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(replies[0]["result"]["serverInfo"]["name"], "kavox");

        assert_eq!(replies[1]["id"], 2);
        assert_eq!(replies[1]["result"], json!({}));

        assert_eq!(replies[2]["id"], 3);
        let tools = replies[2]["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 11);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"play_sound"));
        assert!(names.contains(&"set_voice_changer"));
        assert!(names.contains(&"set_mixer"));
        assert!(names.contains(&"get_status"));

        assert_eq!(replies[3]["id"], 4);
        assert_eq!(
            replies[3]["result"]["resources"].as_array().unwrap().len(),
            2
        );
    }

    #[test]
    fn tools_forward_to_ipc_and_filter_sounds() {
        let mut seen = Vec::new();
        let replies = run_session(
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"play_sound","arguments":{"target":"airhorn"}}}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_sounds","arguments":{"board":"memes","query":"boom"}}}"#,
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"set_voice_changer","arguments":{"preset":"chipmunk","hear_myself":true}}}"#,
                r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"set_mixer","arguments":{"mic_gain":1.5,"normalize":false}}}"#,
                r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"set_active_board","arguments":{"target":"Memes"}}}"#,
            ],
            |req| {
                seen.push(req.clone());
                match req {
                    Request::Play { .. } => Ok(Response::ok()),
                    Request::List => Ok(Response {
                        ok: true,
                        sounds: Some(vec![
                            SoundEntry {
                                id: "s1".into(),
                                name: "Airhorn".into(),
                                board: "Main".into(),
                                hotkey: None,
                                board_id: Some("b1".into()),
                                duration_ms: Some(900),
                                volume: Some(1.0),
                                looping: Some(false),
                                retrigger: Some("restart".into()),
                                playing: Some(false),
                            },
                            SoundEntry {
                                id: "s2".into(),
                                name: "Vine Boom".into(),
                                board: "Memes".into(),
                                hotkey: Some("F1".into()),
                                board_id: Some("b2".into()),
                                duration_ms: Some(1200),
                                volume: Some(0.9),
                                looping: Some(false),
                                retrigger: Some("overlap".into()),
                                playing: Some(true),
                            },
                        ]),
                        ..Response::default()
                    }),
                    Request::SetVoice { .. } => Ok(Response {
                        ok: true,
                        message: Some("voice changer on (chipmunk)".into()),
                        voice: Some(VoiceState {
                            enabled: true,
                            preset: "chipmunk".into(),
                            semitones: 0.0,
                            hear_myself: true,
                            presets: vec![],
                        }),
                        ..Response::default()
                    }),
                    Request::SetMixer(patch) => Ok(Response {
                        ok: true,
                        message: Some("mixer updated".into()),
                        mixer: Some(MixerState {
                            mic_muted: false,
                            mic_passthrough: true,
                            mic_gain: patch.mic_gain.unwrap_or(1.0),
                            sounds_to_mic_gain: 0.8,
                            sounds_to_monitor_gain: 0.5,
                            monitor_enabled: true,
                            normalize: patch.normalize.unwrap_or(true),
                            global_volume_override: false,
                            global_sound_volume: 1.0,
                        }),
                        ..Response::default()
                    }),
                    Request::SetBoard { .. } => Ok(Response {
                        ok: true,
                        message: Some("active board: Memes".into()),
                        boards: Some(sample_status().boards),
                        ..Response::default()
                    }),
                    _ => Ok(Response::ok()),
                }
            },
        );

        assert_eq!(seen.len(), 5);
        assert_eq!(
            seen[0],
            Request::Play {
                target: "airhorn".into()
            }
        );
        assert_eq!(
            seen[2],
            Request::SetVoice {
                enabled: None,
                preset: Some("chipmunk".into()),
                semitones: None,
                hear_myself: Some(true),
            }
        );
        assert_eq!(
            seen[3],
            Request::SetMixer(MixerPatch {
                mic_gain: Some(1.5),
                normalize: Some(false),
                ..MixerPatch::default()
            })
        );

        for r in &replies {
            assert_eq!(r["result"]["isError"], false);
        }
        let filtered_json = replies[1]["result"]["content"][0]["text"].as_str().unwrap();
        let filtered: Vec<SoundEntry> = serde_json::from_str(filtered_json).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "s2");
    }

    #[test]
    fn resources_and_error_handling() {
        let replies = run_session(
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"kavox://status"}}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"play_sound","arguments":{"target":"missing"}}}"#,
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"unknown_tool","arguments":{}}}"#,
                r#"not json"#,
            ],
            |req| match req {
                Request::Status => Ok(Response {
                    ok: true,
                    status: Some(sample_status()),
                    ..Response::default()
                }),
                Request::Play { .. } => Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    "connection refused",
                )),
                _ => Ok(Response::ok()),
            },
        );

        assert_eq!(replies.len(), 4);
        assert_eq!(replies[0]["result"]["contents"][0]["uri"], "kavox://status");
        assert!(
            replies[0]["result"]["contents"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Kavox Mic")
        );

        assert_eq!(replies[1]["result"]["isError"], true);
        assert!(
            replies[1]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Kavox isn't running")
        );

        assert_eq!(replies[2]["error"]["code"], INVALID_PARAMS);
        assert_eq!(replies[3]["error"]["code"], PARSE_ERROR);
    }
}
