//! Kavox core: audio engine, platform backends, library, settings and the controller that ties them together.

pub mod backend;
pub mod clip;
pub mod controller;
pub mod engine;
pub mod error;
pub mod library;
pub mod persist;
pub mod settings;

pub use backend::{DeviceInfo, DeviceList, StreamRole, VirtualStatus, platform_backend};
pub use controller::{
    Controller, Core, EventSink, GlobalHotkey, HotkeyAction, Levels, Notice, NoticeLevel, Paths,
    PlaybackEvent, PlaybackState, Snapshot, StreamInfo, StreamStatus, UiEvent,
};
pub use error::{ApiError, Error, Result};
pub use library::{Board, Library, Retrigger, Sound, SoundPatch, prepare_import};
pub use settings::{AppSettings, AudioSettings, GlobalHotkeys, Settings, Theme};
