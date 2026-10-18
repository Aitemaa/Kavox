//! Windows: WASAPI shared mode through cpal. Sounds reach voice apps through a virtual cable (VB-CABLE or VoiceMeeter).

use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, Host, SampleFormat, SizedSample, StreamConfig};

use super::{
    AudioBackend, DeviceInfo, DeviceList, StreamErrorFn, StreamHandle, StreamRole, VirtualStatus,
};
use crate::engine::{MAX_BLOCK_FRAMES, MicSink, Renderer};
use crate::error::{Error, Result};

/// Render-side names of virtual cables, most preferred first.
const CABLE_PATTERNS: &[&str] = &[
    "CABLE Input",
    "CABLE-A Input",
    "CABLE-B Input",
    "CABLE-C Input",
    "CABLE-D Input",
    "VoiceMeeter Input",
    "VoiceMeeter Aux Input",
    "VoiceMeeter VAIO3 Input",
];

pub struct WasapiBackend;

fn is_cable(name: &str) -> bool {
    CABLE_PATTERNS.iter().any(|p| name.contains(p))
}

/// What voice apps call the capture side of a cable, e.g. "CABLE Output (VB-Audio Virtual Cable)".
fn mic_label(render_name: &str) -> String {
    if is_cable(render_name) {
        render_name.replacen("Input", "Output", 1)
    } else {
        render_name.to_owned()
    }
}

fn names(devices: impl Iterator<Item = Device>) -> Vec<String> {
    devices.filter_map(|d| d.name().ok()).collect()
}

fn find_device(host: &Host, role: StreamRole, name: Option<&str>) -> Result<Device> {
    let device = match (role, name) {
        (StreamRole::Mic, None) => host.default_input_device(),
        (_, None) => host.default_output_device(),
        (StreamRole::Mic, Some(n)) => host
            .input_devices()
            .map_err(device_err)?
            .find(|d| d.name().is_ok_and(|x| x == n)),
        (_, Some(n)) => host
            .output_devices()
            .map_err(device_err)?
            .find(|d| d.name().is_ok_and(|x| x == n)),
    };
    device.ok_or_else(|| Error::DeviceNotFound(name.unwrap_or("default device").to_owned()))
}

fn device_err(e: impl std::fmt::Display) -> Error {
    Error::Device(e.to_string())
}

impl AudioBackend for WasapiBackend {
    fn name(&self) -> &'static str {
        "wasapi"
    }

    fn list_devices(&mut self) -> Result<DeviceList> {
        let host = cpal::default_host();
        let default_in = host.default_input_device().and_then(|d| d.name().ok());
        let default_out = host.default_output_device().and_then(|d| d.name().ok());
        let info = |name: String, default: &Option<String>| DeviceInfo {
            is_default: default.as_deref() == Some(name.as_str()),
            label: name.clone(),
            id: name,
        };
        let outputs: Vec<DeviceInfo> = names(host.output_devices().map_err(device_err)?)
            .into_iter()
            .map(|n| info(n, &default_out))
            .collect();
        let mut virtual_candidates: Vec<String> = outputs
            .iter()
            .filter(|d| is_cable(&d.id))
            .map(|d| d.id.clone())
            .collect();
        virtual_candidates.sort_by_key(|id| CABLE_PATTERNS.iter().position(|p| id.contains(p)));
        Ok(DeviceList {
            inputs: names(host.input_devices().map_err(device_err)?)
                .into_iter()
                .map(|n| info(n, &default_in))
                .collect(),
            outputs,
            virtual_candidates,
        })
    }

    fn resolve_virtual(
        &mut self,
        selected: Option<&str>,
        devices: &DeviceList,
    ) -> (Option<String>, VirtualStatus) {
        let chosen = match selected {
            Some(s) if devices.has_output(s) => s.to_owned(),
            Some(s) => {
                return (
                    None,
                    VirtualStatus::Missing {
                        device: s.to_owned(),
                    },
                );
            }
            None => match devices.virtual_candidates.first() {
                Some(c) => c.clone(),
                None => return (None, VirtualStatus::NotInstalled),
            },
        };
        let is_default = devices
            .outputs
            .iter()
            .any(|d| d.id == chosen && d.is_default);
        let status = if is_default && is_cable(&chosen) {
            VirtualStatus::CableIsDefaultOutput {
                mic_label: mic_label(&chosen),
                device: chosen.clone(),
            }
        } else {
            VirtualStatus::Ok {
                mic_label: mic_label(&chosen),
                device: chosen.clone(),
            }
        };
        (Some(chosen), status)
    }

    fn open_output(
        &mut self,
        role: StreamRole,
        device: Option<&str>,
        mut renderer: Renderer,
        on_error: StreamErrorFn,
    ) -> Result<Box<dyn StreamHandle>> {
        let name = device.map(str::to_owned);
        spawn_stream(role, move || {
            let host = cpal::default_host();
            let device = find_device(&host, role, name.as_deref())?;
            let device_name = device.name().map_err(device_err)?;
            let supported = device.default_output_config().map_err(device_err)?;
            let rate = supported.sample_rate().0;
            let channels = supported.channels() as usize;
            let format = supported.sample_format();
            let config: StreamConfig = supported.config();
            renderer.prepare(rate);
            let stream = match format {
                SampleFormat::F32 => {
                    build_output::<f32>(&device, &config, channels, renderer, role, on_error)
                }
                SampleFormat::I16 => {
                    build_output::<i16>(&device, &config, channels, renderer, role, on_error)
                }
                SampleFormat::I32 => {
                    build_output::<i32>(&device, &config, channels, renderer, role, on_error)
                }
                SampleFormat::U16 => {
                    build_output::<u16>(&device, &config, channels, renderer, role, on_error)
                }
                other => Err(Error::Device(format!(
                    "{device_name} uses unsupported format {other:?}"
                ))),
            }?;
            Ok((stream, rate, device_name))
        })
    }

    fn open_input(
        &mut self,
        device: Option<&str>,
        sink: MicSink,
        on_error: StreamErrorFn,
    ) -> Result<Box<dyn StreamHandle>> {
        let name = device.map(str::to_owned);
        spawn_stream(StreamRole::Mic, move || {
            let host = cpal::default_host();
            let device = find_device(&host, StreamRole::Mic, name.as_deref())?;
            let device_name = device.name().map_err(device_err)?;
            if is_cable(&device_name.replacen("Output", "Input", 1)) {
                return Err(Error::Device(format!(
                    "\"{device_name}\" is Kavox's own output; pick your real microphone so Kavox doesn't record itself"
                )));
            }
            let supported = device.default_input_config().map_err(device_err)?;
            let rate = supported.sample_rate().0;
            let channels = supported.channels() as usize;
            let format = supported.sample_format();
            let config: StreamConfig = supported.config();
            let stream = match format {
                SampleFormat::F32 => build_input::<f32>(&device, &config, channels, sink, on_error),
                SampleFormat::I16 => build_input::<i16>(&device, &config, channels, sink, on_error),
                SampleFormat::I32 => build_input::<i32>(&device, &config, channels, sink, on_error),
                SampleFormat::U16 => build_input::<u16>(&device, &config, channels, sink, on_error),
                other => Err(Error::Device(format!(
                    "{device_name} uses unsupported format {other:?}"
                ))),
            }?;
            Ok((stream, rate, device_name))
        })
    }
}

fn build_output<T>(
    device: &Device,
    config: &StreamConfig,
    channels: usize,
    mut renderer: Renderer,
    role: StreamRole,
    on_error: StreamErrorFn,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let mut mix = vec![0f32; MAX_BLOCK_FRAMES * 2];
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                for chunk in data.chunks_mut(MAX_BLOCK_FRAMES * channels) {
                    let frames = chunk.len() / channels;
                    let stereo = &mut mix[..frames * 2];
                    renderer.render(stereo);
                    for (frame, s) in chunk
                        .chunks_exact_mut(channels)
                        .zip(stereo.as_chunks::<2>().0)
                    {
                        if channels == 1 {
                            frame[0] = T::from_sample((s[0] + s[1]) * 0.5);
                            continue;
                        }
                        for (i, x) in frame.iter_mut().enumerate() {
                            *x = T::from_sample(if i < 2 { s[i] } else { 0.0 });
                        }
                    }
                }
            },
            move |e| on_error(role, e.to_string()),
            None,
        )
        .map_err(device_err)
}

fn build_input<T>(
    device: &Device,
    config: &StreamConfig,
    channels: usize,
    mut sink: MicSink,
    on_error: StreamErrorFn,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                sink.push_interleaved(data, channels, |x| x.to_sample::<f32>())
            },
            move |e| on_error(StreamRole::Mic, e.to_string()),
            None,
        )
        .map_err(device_err)
}

/// cpal streams aren't Send, so each one lives on its own thread until the handle drops.
struct CpalStream {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    rate: u32,
    device: String,
}

impl StreamHandle for CpalStream {
    fn rate(&self) -> u32 {
        self.rate
    }

    fn device(&self) -> &str {
        &self.device
    }
}

impl Drop for CpalStream {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn spawn_stream(
    role: StreamRole,
    build: impl FnOnce() -> Result<(cpal::Stream, u32, String)> + Send + 'static,
) -> Result<Box<dyn StreamHandle>> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let thread = thread::Builder::new()
        .name(format!("kavox-{role:?}").to_lowercase())
        .spawn(move || {
            let started = build().and_then(|(stream, rate, name)| {
                stream.play().map_err(device_err)?;
                Ok((stream, rate, name))
            });
            match started {
                Ok((stream, rate, name)) => {
                    let _ = ready_tx.send(Ok((rate, name)));
                    // Blocks until the handle drops its sender.
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })?;
    match ready_rx.recv() {
        Ok(Ok((rate, device))) => Ok(Box::new(CpalStream {
            stop: Some(stop_tx),
            thread: Some(thread),
            rate,
            device,
        })),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(Error::Device("audio thread exited".into())),
    }
}
