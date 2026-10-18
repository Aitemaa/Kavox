use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} isn't a supported audio file (use WAV, MP3, OGG or FLAC)")]
    UnsupportedFormat(String),
    #[error("couldn't decode {0}: {1}")]
    DecodeFailed(String, String),
    #[error("audio device not found: {0}")]
    DeviceNotFound(String),
    #[error("audio device error: {0}")]
    Device(String),
    #[error("no sound matches \"{0}\"")]
    SoundNotFound(String),
    #[error("\"{0}\" matches several sounds: {1}")]
    AmbiguousSound(String, String),
    #[error("board not found")]
    BoardNotFound,
    #[error("{0} is already used by {1}")]
    HotkeyConflict(String, String),
    #[error("{0} is taken by another app")]
    HotkeyTaken(String),
    #[error("no audio output is open; check Settings → Audio")]
    NoOutput,
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Error::UnsupportedFormat(_) => "UnsupportedFormat",
            Error::DecodeFailed(..) => "DecodeFailed",
            Error::DeviceNotFound(_) => "DeviceNotFound",
            Error::Device(_) => "Device",
            Error::SoundNotFound(_) => "SoundNotFound",
            Error::AmbiguousSound(..) => "AmbiguousSound",
            Error::BoardNotFound => "BoardNotFound",
            Error::HotkeyConflict(..) => "HotkeyConflict",
            Error::HotkeyTaken(_) => "HotkeyTaken",
            Error::NoOutput => "NoOutput",
            Error::Invalid(_) => "Invalid",
            Error::Io(_) => "Io",
            Error::Json(_) => "Json",
        }
    }
}

/// Error shape sent to the UI and `kavoxctl`.
#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        Self {
            code: e.code(),
            message: e.to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
