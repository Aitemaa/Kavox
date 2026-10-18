use super::{AudioBackend, DeviceList, StreamErrorFn, StreamHandle, StreamRole, VirtualStatus};
use crate::engine::{MicSink, Renderer};
use crate::error::{Error, Result};

/// Placeholder for platforms Kavox doesn't support yet.
pub struct NullBackend;

impl AudioBackend for NullBackend {
    fn name(&self) -> &'static str {
        "none"
    }

    fn list_devices(&mut self) -> Result<DeviceList> {
        Ok(DeviceList::default())
    }

    fn resolve_virtual(
        &mut self,
        _: Option<&str>,
        _: &DeviceList,
    ) -> (Option<String>, VirtualStatus) {
        (
            None,
            VirtualStatus::Failed {
                message: "This platform isn't supported yet".into(),
            },
        )
    }

    fn open_output(
        &mut self,
        _: StreamRole,
        _: Option<&str>,
        _: Renderer,
        _: StreamErrorFn,
    ) -> Result<Box<dyn StreamHandle>> {
        Err(Error::Device("This platform isn't supported yet".into()))
    }

    fn open_input(
        &mut self,
        _: Option<&str>,
        _: MicSink,
        _: StreamErrorFn,
    ) -> Result<Box<dyn StreamHandle>> {
        Err(Error::Device("This platform isn't supported yet".into()))
    }
}
