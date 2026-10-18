//! Linux: Kavox creates its own "Kavox Mic" (null sink + remap source) and streams through libpulse,
//! which works on both PulseAudio and PipeWire (pipewire-pulse).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::{Context, FlagSet, State};
use libpulse_binding::def::BufferAttr;
use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};
use libpulse_binding::operation::{Operation, State as OpState};
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;

use super::{
    AudioBackend, DeviceInfo, DeviceList, StreamErrorFn, StreamHandle, StreamRole, VirtualStatus,
};
use crate::engine::{MicSink, Renderer};
use crate::error::{Error, Result};

const RATE: u32 = 48_000;
const PERIOD_FRAMES: usize = 480;
const SINK: &str = "kavox_sink";
const SOURCE: &str = "kavox_mic";
const MIC_LABEL: &str = "Kavox Mic";

pub struct PulseBackend {
    modules: Vec<u32>,
    module_error: Option<String>,
    /// The default mic from before Kavox Mic existed, in case Kavox Mic becomes the default.
    original_default_source: Option<String>,
}

impl PulseBackend {
    pub fn new() -> Self {
        let mut backend = Self {
            modules: Vec::new(),
            module_error: None,
            original_default_source: None,
        };
        if let Err(e) = backend.create_virtual_mic() {
            tracing::error!("couldn't create Kavox Mic: {e}");
            backend.module_error = Some(e.to_string());
        }
        backend
    }

    fn create_virtual_mic(&mut self) -> Result<()> {
        let mut pulse = Pulse::connect()?;
        for (index, args) in pulse.modules()? {
            if args.contains(&format!("sink_name={SINK}"))
                || args.contains(&format!("source_name={SOURCE}"))
            {
                tracing::info!("removing leftover module {index} from a previous run");
                pulse.unload_module(index)?;
            }
        }
        self.original_default_source = pulse.server_defaults()?.1;
        self.modules.push(pulse.load_module(
            "module-null-sink",
            &format!("sink_name={SINK} rate={RATE} channels=2 sink_properties=\"device.description='Kavox internal'\""),
        )?);
        self.modules.push(pulse.load_module(
            "module-remap-source",
            &format!("master={SINK}.monitor source_name={SOURCE} source_properties=\"device.description='{MIC_LABEL}'\""),
        )?);
        tracing::info!("created {MIC_LABEL} (modules {:?})", self.modules);
        Ok(())
    }
}

impl AudioBackend for PulseBackend {
    fn name(&self) -> &'static str {
        "pulse"
    }

    fn list_devices(&mut self) -> Result<DeviceList> {
        let mut pulse = Pulse::connect()?;
        let (default_sink, default_source) = pulse.server_defaults()?;
        let (sinks, sources) = pulse.devices()?;
        let info = |(id, label): (String, String), default: &Option<String>| DeviceInfo {
            is_default: default.as_deref() == Some(id.as_str()),
            id,
            label,
        };
        Ok(DeviceList {
            outputs: sinks
                .into_iter()
                .filter(|(id, _)| id != SINK)
                .map(|d| info(d, &default_sink))
                .collect(),
            inputs: sources
                .into_iter()
                .filter(|(id, _)| id != SOURCE)
                .map(|d| info(d, &default_source))
                .collect(),
            virtual_candidates: Vec::new(),
        })
    }

    fn resolve_virtual(
        &mut self,
        _: Option<&str>,
        _: &DeviceList,
    ) -> (Option<String>, VirtualStatus) {
        match &self.module_error {
            Some(message) => (
                None,
                VirtualStatus::Failed {
                    message: format!("Couldn't create {MIC_LABEL}: {message}"),
                },
            ),
            None => (
                Some(SINK.to_owned()),
                VirtualStatus::Ok {
                    device: MIC_LABEL.into(),
                    mic_label: MIC_LABEL.into(),
                },
            ),
        }
    }

    fn open_output(
        &mut self,
        role: StreamRole,
        device: Option<&str>,
        mut renderer: Renderer,
        on_error: StreamErrorFn,
    ) -> Result<Box<dyn StreamHandle>> {
        let device = match role {
            StreamRole::Virtual => SINK.to_owned(),
            _ => match device {
                Some(d) => d.to_owned(),
                None => Pulse::connect()?
                    .server_defaults()?
                    .0
                    .ok_or_else(|| Error::DeviceNotFound("default output".into()))?,
            },
        };
        renderer.prepare(RATE);
        let mut buf = vec![0f32; PERIOD_FRAMES * 2];
        let mut bytes = vec![0u8; PERIOD_FRAMES * 2 * 4];
        spawn_stream(
            role,
            device,
            Direction::Playback,
            2,
            on_error,
            move |stream| {
                renderer.render(&mut buf);
                for (b, s) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(&buf) {
                    *b = s.to_le_bytes();
                }
                stream.write(&bytes).map_err(|e| format!("{e}"))
            },
        )
    }

    fn open_input(
        &mut self,
        device: Option<&str>,
        mut sink: MicSink,
        on_error: StreamErrorFn,
    ) -> Result<Box<dyn StreamHandle>> {
        let device = match device {
            Some(d) => d.to_owned(),
            None => {
                let default = Pulse::connect()?.server_defaults()?.1;
                default
                    .filter(|d| d != SOURCE)
                    .or_else(|| self.original_default_source.clone())
                    .ok_or_else(|| Error::DeviceNotFound("default microphone".into()))?
            }
        };
        let mut bytes = vec![0u8; PERIOD_FRAMES * 4];
        spawn_stream(
            StreamRole::Mic,
            device,
            Direction::Record,
            1,
            on_error,
            move |stream| {
                stream.read(&mut bytes).map_err(|e| format!("{e}"))?;
                sink.push_interleaved(bytes.as_chunks::<4>().0, 1, f32::from_le_bytes);
                Ok(())
            },
        )
    }

    fn shutdown(&mut self) {
        if self.modules.is_empty() {
            return;
        }
        match Pulse::connect() {
            Ok(mut pulse) => {
                for index in self.modules.drain(..).rev() {
                    if let Err(e) = pulse.unload_module(index) {
                        tracing::warn!("couldn't unload module {index}: {e}");
                    }
                }
                tracing::info!("removed {MIC_LABEL}");
            }
            Err(e) => tracing::warn!("couldn't remove {MIC_LABEL}: {e}"),
        }
    }
}

impl Drop for PulseBackend {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct PulseStream {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    device: String,
}

impl StreamHandle for PulseStream {
    fn rate(&self) -> u32 {
        RATE
    }

    fn device(&self) -> &str {
        &self.device
    }
}

impl Drop for PulseStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Opens a pa_simple stream on its own thread (pa_simple isn't Send) and runs `step` until stopped.
fn spawn_stream(
    role: StreamRole,
    device: String,
    direction: Direction,
    channels: u8,
    on_error: StreamErrorFn,
    mut step: impl FnMut(&Simple) -> std::result::Result<(), String> + Send + 'static,
) -> Result<Box<dyn StreamHandle>> {
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel();
    let thread_stop = stop.clone();
    let thread_device = device.clone();
    let thread = thread::Builder::new()
        .name(format!("kavox-{role:?}").to_lowercase())
        .spawn(move || {
            let stream = match open_simple(direction, &thread_device, channels) {
                Ok(s) => {
                    let _ = ready_tx.send(Ok(()));
                    s
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            while !thread_stop.load(Ordering::Relaxed) {
                if let Err(e) = step(&stream) {
                    if !thread_stop.load(Ordering::Relaxed) {
                        on_error(role, e);
                    }
                    return;
                }
            }
        })?;
    match ready_rx.recv() {
        Ok(Ok(())) => Ok(Box::new(PulseStream {
            stop,
            thread: Some(thread),
            device,
        })),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(Error::Device("audio thread exited".into())),
    }
}

fn open_simple(direction: Direction, device: &str, channels: u8) -> Result<Simple> {
    let spec = Spec {
        format: Format::F32le,
        channels,
        rate: RATE,
    };
    let period = (PERIOD_FRAMES * channels as usize * 4) as u32;
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength: period * 2,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: period,
    };
    Simple::new(
        None,
        "Kavox",
        direction,
        Some(device),
        "Kavox",
        &spec,
        None,
        Some(&attr),
    )
    .map_err(|e| {
        let message = format!("{e}");
        if message.contains("No such entity") {
            Error::DeviceNotFound(device.to_owned())
        } else {
            Error::Device(format!("{device}: {message}"))
        }
    })
}

/// Blocking wrapper over the async libpulse API, for queries and module management.
struct Pulse {
    context: Context,
    mainloop: Mainloop,
}

impl Pulse {
    fn connect() -> Result<Self> {
        let fail = |what: &str| Error::Device(format!("PulseAudio/PipeWire: {what}"));
        let mut mainloop = Mainloop::new().ok_or_else(|| fail("no mainloop"))?;
        let mut context = Context::new(&mainloop, "Kavox").ok_or_else(|| fail("no context"))?;
        context
            .connect(None, FlagSet::NOFLAGS, None)
            .map_err(|e| fail(&format!("{e}")))?;
        loop {
            iterate(&mut mainloop)?;
            match context.get_state() {
                State::Ready => return Ok(Self { context, mainloop }),
                State::Failed | State::Terminated => {
                    return Err(fail("connection failed; is the sound server running?"));
                }
                _ => {}
            }
        }
    }

    fn wait<T: ?Sized>(&mut self, op: Operation<T>) -> Result<()> {
        while matches!(op.get_state(), OpState::Running) {
            iterate(&mut self.mainloop)?;
        }
        Ok(())
    }

    fn load_module(&mut self, name: &str, args: &str) -> Result<u32> {
        let index = Rc::new(Cell::new(u32::MAX));
        let out = index.clone();
        let op = self
            .context
            .introspect()
            .load_module(name, args, move |i| out.set(i));
        self.wait(op)?;
        match index.get() {
            u32::MAX => Err(Error::Device(format!("loading {name} failed"))),
            i => Ok(i),
        }
    }

    fn unload_module(&mut self, index: u32) -> Result<()> {
        let op = self.context.introspect().unload_module(index, |_| {});
        self.wait(op)
    }

    fn modules(&mut self) -> Result<Vec<(u32, String)>> {
        let list = Rc::new(RefCell::new(Vec::new()));
        let out = list.clone();
        let op = self.context.introspect().get_module_info_list(move |item| {
            if let ListResult::Item(m) = item {
                out.borrow_mut().push((
                    m.index,
                    m.argument.as_deref().unwrap_or_default().to_owned(),
                ));
            }
        });
        self.wait(op)?;
        Ok(list.take())
    }

    /// (default sink, default source)
    fn server_defaults(&mut self) -> Result<(Option<String>, Option<String>)> {
        let names = Rc::new(RefCell::new((None::<String>, None::<String>)));
        let out = names.clone();
        let op = self.context.introspect().get_server_info(move |info| {
            *out.borrow_mut() = (
                info.default_sink_name.as_deref().map(str::to_owned),
                info.default_source_name.as_deref().map(str::to_owned),
            );
        });
        self.wait(op)?;
        Ok(names.take())
    }

    /// (sinks, sources) as (name, description), without monitor sources.
    #[allow(clippy::type_complexity)]
    fn devices(&mut self) -> Result<(Vec<(String, String)>, Vec<(String, String)>)> {
        let sinks = Rc::new(RefCell::new(Vec::new()));
        let out = sinks.clone();
        let op = self.context.introspect().get_sink_info_list(move |item| {
            if let ListResult::Item(s) = item {
                let id = s.name.as_deref().unwrap_or_default().to_owned();
                let label = s
                    .description
                    .as_deref()
                    .map_or_else(|| id.clone(), str::to_owned);
                out.borrow_mut().push((id, label));
            }
        });
        self.wait(op)?;

        let sources = Rc::new(RefCell::new(Vec::new()));
        let out = sources.clone();
        let op = self.context.introspect().get_source_info_list(move |item| {
            if let ListResult::Item(s) = item
                && s.monitor_of_sink.is_none()
            {
                let id = s.name.as_deref().unwrap_or_default().to_owned();
                let label = s
                    .description
                    .as_deref()
                    .map_or_else(|| id.clone(), str::to_owned);
                out.borrow_mut().push((id, label));
            }
        });
        self.wait(op)?;
        Ok((sinks.take(), sources.take()))
    }
}

impl Drop for Pulse {
    fn drop(&mut self) {
        self.context.disconnect();
    }
}

fn iterate(mainloop: &mut Mainloop) -> Result<()> {
    match mainloop.iterate(true) {
        IterateResult::Success(_) => Ok(()),
        IterateResult::Quit(_) => Err(Error::Device("pulse mainloop quit".into())),
        IterateResult::Err(e) => Err(Error::Device(format!("pulse mainloop: {e}"))),
    }
}
