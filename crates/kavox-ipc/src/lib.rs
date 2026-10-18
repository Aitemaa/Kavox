//! Local-socket protocol between the Kavox app and `kavoxctl`: one JSON object per line, request then response.

use std::io::{self, BufRead, BufReader, Write};
use std::thread::{self, JoinHandle};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Name, Stream};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum Request {
    Play { target: String },
    Stop { target: String },
    StopAll,
    Mute { mode: MuteMode },
    List,
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

/// Sends one request to the running app.
pub fn send(request: Request) -> io::Result<Response> {
    let mut conn = BufReader::new(Stream::connect(socket_name()?)?);
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
pub fn serve(handler: impl Fn(Request) -> Response + Send + 'static) -> io::Result<JoinHandle<()>> {
    let listener = ListenerOptions::new()
        .name(socket_name()?)
        .try_overwrite(true)
        .create_sync()?;
    thread::Builder::new()
        .name("kavox-ipc".into())
        .spawn(move || {
            for conn in listener.incoming().filter_map(Result::ok) {
                let mut conn = BufReader::new(conn);
                let mut line = String::new();
                if conn.read_line(&mut line).is_err() {
                    continue;
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
        })
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
}
