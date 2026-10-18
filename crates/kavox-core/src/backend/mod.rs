//! Platform audio I/O behind one trait: WASAPI on Windows, PulseAudio/PipeWire on Linux.

use std::sync::Arc;

use serde::Serialize;

use crate::engine::{MicSink, Renderer};
use crate::error::Result;

#[cfg(not(any(windows, target_os = "linux")))]
mod null;
#[cfg(target_os = "linux")]
mod pulse;
#[cfg(windows)]
mod wasapi;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamRole {
    Mic,
    Virtual,
    Monitor,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Stable name used in settings.
    pub id: String,
    pub label: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceList {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
    /// Output ids that look like virtual cables (Windows).
    pub virtual_candidates: Vec<String>,
}

impl DeviceList {
    pub fn label(&self, id: &str) -> String {
        self.inputs
            .iter()
            .chain(&self.outputs)
            .find(|d| d.id == id)
            .map_or_else(|| id.to_owned(), |d| d.label.clone())
    }

    pub fn has_input(&self, id: &str) -> bool {
        self.inputs.iter().any(|d| d.id == id)
    }

    pub fn has_output(&self, id: &str) -> bool {
        self.outputs.iter().any(|d| d.id == id)
    }
}

/// Whether sounds can reach voice apps, and if not, why.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum VirtualStatus {
    Ok {
        device: String,
        mic_label: String,
    },
    /// Windows: no virtual cable installed.
    NotInstalled,
    /// The selected device isn't connected.
    Missing {
        device: String,
    },
    /// Windows: the cable is the default playback device, so all PC audio goes into the mic.
    CableIsDefaultOutput {
        device: String,
        mic_label: String,
    },
    Failed {
        message: String,
    },
}

pub type StreamErrorFn = Arc<dyn Fn(StreamRole, String) + Send + Sync>;

/// A running stream; dropping it stops the stream.
pub trait StreamHandle: Send {
    fn rate(&self) -> u32;
    fn device(&self) -> &str;
}

pub trait AudioBackend: Send {
    fn name(&self) -> &'static str;
    fn list_devices(&mut self) -> Result<DeviceList>;
    /// Device the virtual renderer should open (if any) and the status to show.
    fn resolve_virtual(
        &mut self,
        selected: Option<&str>,
        devices: &DeviceList,
    ) -> (Option<String>, VirtualStatus);
    /// `device: None` means the system default.
    fn open_output(
        &mut self,
        role: StreamRole,
        device: Option<&str>,
        renderer: Renderer,
        on_error: StreamErrorFn,
    ) -> Result<Box<dyn StreamHandle>>;
    fn open_input(
        &mut self,
        device: Option<&str>,
        sink: MicSink,
        on_error: StreamErrorFn,
    ) -> Result<Box<dyn StreamHandle>>;
    /// Releases system resources (e.g. Linux virtual mic modules).
    fn shutdown(&mut self) {}
}

pub fn platform_backend() -> Box<dyn AudioBackend> {
    #[cfg(target_os = "linux")]
    return Box::new(pulse::PulseBackend::new());
    #[cfg(windows)]
    return Box::new(wasapi::WasapiBackend);
    #[cfg(not(any(windows, target_os = "linux")))]
    return Box::new(null::NullBackend);
}
