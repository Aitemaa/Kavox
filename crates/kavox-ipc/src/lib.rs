//! Local-socket protocol between the Kavox app and `kavoxctl`: one JSON object per line, request then response.

use std::io::{self, BufRead, BufReader, Write};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Name, Stream};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum Request {
    Play {
        target: String,
    },
    Stop {
        target: String,
    },
    StopAll,
    Mute {
        mode: MuteMode,
    },
    List,
    Show,
    Status,
    ListBoards,
    SetBoard {
        target: String,
    },
    SetVoice {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        enabled: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        semitones: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hear_myself: Option<bool>,
    },
    SetMixer(MixerPatch),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MixerPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mic_muted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mic_passthrough: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mic_gain: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sounds_to_mic_gain: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sounds_to_monitor_gain: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalize: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_volume_override: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_sound_volume: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MuteMode {
    On,
    Off,
    Toggle,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sounds: Option<Vec<SoundEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boards: Option<Vec<BoardEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<StatusInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<VoiceState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixer: Option<MixerState>,
}

impl Response {
    pub fn ok() -> Self {
        Self {
            ok: true,
            ..Self::default()
        }
    }

    pub fn message(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: Some(message.into()),
            ..Self::default()
        }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(ErrorBody {
                code: code.into(),
                message: message.into(),
            }),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoundEntry {
    pub id: String,
    pub name: String,
    pub board: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub looping: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playing: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardEntry {
    pub id: String,
    pub name: String,
    pub sound_count: usize,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayingEntry {
    pub sound_id: String,
    pub sound_name: String,
    pub voice_id: u64,
    pub frame: usize,
    pub frames: usize,
    pub progress: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoicePresetEntry {
    pub id: String,
    pub label: String,
    pub semitones: f32,
    pub wobble_depth: f32,
    pub wobble_hz: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceState {
    pub enabled: bool,
    pub preset: String,
    pub semitones: f32,
    pub hear_myself: bool,
    pub presets: Vec<VoicePresetEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixerState {
    pub mic_muted: bool,
    pub mic_passthrough: bool,
    pub mic_gain: f32,
    pub sounds_to_mic_gain: f32,
    pub sounds_to_monitor_gain: f32,
    pub monitor_enabled: bool,
    pub normalize: bool,
    pub global_volume_override: bool,
    pub global_sound_volume: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamEntry {
    pub device: String,
    pub label: String,
    pub rate: u32,
    pub fallback: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamsState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mic: Option<StreamEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_out: Option<StreamEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor: Option<StreamEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub id: String,
    pub label: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicesState {
    pub inputs: Vec<DeviceEntry>,
    pub outputs: Vec<DeviceEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusInfo {
    pub active_board_id: String,
    pub active_board_name: String,
    pub boards: Vec<BoardEntry>,
    pub playing: Vec<PlayingEntry>,
    pub mixer: MixerState,
    pub voice: VoiceState,
    pub virtual_status: String,
    pub streams: StreamsState,
    pub devices: DevicesState,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    v: u32,
    #[serde(flatten)]
    request: Request,
}

/// Per-user socket: a file in `$XDG_RUNTIME_DIR` on Linux, a named pipe on Windows.
fn socket_name() -> io::Result<Name<'static>> {
    #[cfg(windows)]
    {
        use interprocess::local_socket::GenericNamespaced;
        let user = std::env::var("USERNAME").unwrap_or_default();
        format!("kavox-{user}").to_ns_name::<GenericNamespaced>()
    }
    #[cfg(not(windows))]
    {
        use interprocess::local_socket::GenericFilePath;
        let path = match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(dir) => std::path::PathBuf::from(dir).join("kavox.sock"),
            None => std::env::temp_dir().join(format!(
                "kavox-{}.sock",
                std::env::var("USER").unwrap_or_default()
            )),
        };
        path.to_fs_name::<GenericFilePath>()
    }
}

/// Sends one request to the running app using [`DEFAULT_TIMEOUT`].
pub fn send(request: Request) -> io::Result<Response> {
    send_with_timeout(request, DEFAULT_TIMEOUT)
}

/// Sends one request to the running app, failing with [`io::ErrorKind::TimedOut`] if the
/// peer does not respond within `timeout`.
pub fn send_with_timeout(request: Request, timeout: Duration) -> io::Result<Response> {
    #[cfg(not(windows))]
    {
        send_blocking(request, Some(timeout))
    }
    #[cfg(windows)]
    {
        // Named pipes on Windows don't support `set_recv_timeout`/`set_send_timeout` in
        // `interprocess`, so bound the exchange with a channel wait instead.
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = thread::Builder::new()
            .name("kavox-ipc-send".into())
            .spawn(move || {
                let _ = tx.send(send_blocking(request, Some(timeout)));
            });
        match rx.recv_timeout(timeout) {
            Ok(res) => res,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "IPC request timed out",
            )),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(io::Error::other("IPC worker terminated unexpectedly"))
            }
        }
    }
}

fn send_blocking(request: Request, timeout: Option<Duration>) -> io::Result<Response> {
    let stream = Stream::connect(socket_name()?)?;
    if timeout.is_some() {
        let _ = stream.set_recv_timeout(timeout);
        let _ = stream.set_send_timeout(timeout);
    }
    let mut conn = BufReader::new(stream);
    let mut line = serde_json::to_string(&Envelope {
        v: PROTOCOL_VERSION,
        request,
    })?;
    line.push('\n');
    conn.get_mut().write_all(line.as_bytes())?;
    let mut reply = String::new();
    conn.read_line(&mut reply)?;
    serde_json::from_str(&reply).map_err(io::Error::other)
}

/// Serves requests on a background thread until the process exits.
///
/// Each connection is handled on its own short-lived thread, so a client that stalls mid-request
/// (named pipes on Windows have no read timeout) or a request that waits on app startup can't
/// block other requests such as a second launch's `Show`.
pub fn serve(
    handler: impl Fn(Request) -> Response + Send + Sync + 'static,
) -> io::Result<JoinHandle<()>> {
    let listener = ListenerOptions::new()
        .name(socket_name()?)
        .try_overwrite(true)
        .create_sync()?;
    let handler = std::sync::Arc::new(handler);
    thread::Builder::new()
        .name("kavox-ipc".into())
        .spawn(move || {
            for conn in listener.incoming().filter_map(Result::ok) {
                let handler = handler.clone();
                let _ = thread::Builder::new()
                    .name("kavox-ipc-conn".into())
                    .spawn(move || serve_connection(conn, &*handler));
            }
        })
}

fn serve_connection(conn: Stream, handler: &dyn Fn(Request) -> Response) {
    // Ignored on Windows named pipes; harmless there since each connection has its own thread.
    let _ = conn.set_recv_timeout(Some(DEFAULT_TIMEOUT));
    let _ = conn.set_send_timeout(Some(DEFAULT_TIMEOUT));
    let mut conn = BufReader::new(conn);
    let mut line = String::new();
    if conn.read_line(&mut line).is_err() {
        return;
    }
    let response = match serde_json::from_str::<Envelope>(&line) {
        Ok(env) if env.v == PROTOCOL_VERSION => handler(env.request),
        Ok(env) => Response::error(
            "Protocol",
            format!("unsupported protocol version {}", env.v),
        ),
        Err(e) => Response::error("BadRequest", e.to_string()),
    };
    if let Ok(mut reply) = serde_json::to_string(&response) {
        reply.push('\n');
        let _ = conn.get_mut().write_all(reply.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_format() {
        let line = serde_json::to_string(&Envelope {
            v: 1,
            request: Request::Play {
                target: "airhorn".into(),
            },
        })
        .unwrap();
        assert_eq!(line, r#"{"v":1,"cmd":"play","target":"airhorn"}"#);
        let mute: Envelope =
            serde_json::from_str(r#"{"v":1,"cmd":"mute","mode":"toggle"}"#).unwrap();
        assert_eq!(
            mute.request,
            Request::Mute {
                mode: MuteMode::Toggle
            }
        );
        let stop_all: Envelope = serde_json::from_str(r#"{"v":1,"cmd":"stop-all"}"#).unwrap();
        assert_eq!(stop_all.request, Request::StopAll);
    }

    #[test]
    fn response_omits_empty_fields() {
        assert_eq!(
            serde_json::to_string(&Response::ok()).unwrap(),
            r#"{"ok":true}"#
        );
    }

    #[test]
    fn extended_requests_wire_format() {
        let status = serde_json::to_string(&Envelope {
            v: 1,
            request: Request::Status,
        })
        .unwrap();
        assert_eq!(status, r#"{"v":1,"cmd":"status"}"#);

        let voice = serde_json::to_string(&Envelope {
            v: 1,
            request: Request::SetVoice {
                enabled: Some(true),
                preset: Some("chipmunk".into()),
                semitones: None,
                hear_myself: Some(false),
            },
        })
        .unwrap();
        assert_eq!(
            voice,
            r#"{"v":1,"cmd":"set-voice","enabled":true,"preset":"chipmunk","hear_myself":false}"#
        );

        let mixer = serde_json::to_string(&Envelope {
            v: 1,
            request: Request::SetMixer(MixerPatch {
                mic_gain: Some(1.25),
                normalize: Some(true),
                ..MixerPatch::default()
            }),
        })
        .unwrap();
        assert_eq!(
            mixer,
            r#"{"v":1,"cmd":"set-mixer","mic_gain":1.25,"normalize":true}"#
        );
        let parsed: Envelope = serde_json::from_str(&mixer).unwrap();
        assert_eq!(
            parsed.request,
            Request::SetMixer(MixerPatch {
                mic_gain: Some(1.25),
                normalize: Some(true),
                ..MixerPatch::default()
            })
        );
    }
}
