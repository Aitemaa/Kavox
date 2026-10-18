use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use anyhow::{Context as _, Result, anyhow, bail};
use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::{Context, FlagSet, State};
use libpulse_binding::def::BufferAttr;
use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};
use libpulse_binding::operation::{Operation, State as OpState};
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;

use crate::engine::{self, Mixer};

const RATE: u32 = 48_000;
const PERIOD: usize = 480;
const SINK: &str = "kavox_sink";
const SOURCE: &str = "kavox_mic";

pub type ChunkFn = Box<dyn FnMut(&[f32], usize, u32, Instant) + Send>;

/// Kavox's own virtual mic (null sink + remap source) and the streams that feed it.
pub struct Platform {
    pub out_rate: u32,
    pub mic_rate: u32,
    mic_source: String,
    modules: Vec<u32>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Platform {
    pub fn open(device: Option<&str>) -> Result<Self> {
        if device.is_some() {
            eprintln!("--device is ignored on Linux: Kavox creates its own virtual mic");
        }
        let mut pulse = Pulse::connect()?;
        for (index, args) in pulse.modules()? {
            if args.contains(&format!("sink_name={SINK}"))
                || args.contains(&format!("source_name={SOURCE}"))
            {
                println!("removing leftover module {index} from a previous run");
                pulse.unload_module(index)?;
            }
        }
        // Pick the mic before the virtual source exists so we never record ourselves.
        let mic_source = pulse.defaults()?.1.context("no default microphone")?;

        let mut platform = Self {
            out_rate: RATE,
            mic_rate: RATE,
            mic_source,
            modules: Vec::new(),
            stop: Arc::new(AtomicBool::new(false)),
            threads: Vec::new(),
        };
        platform.modules.push(pulse.load_module(
            "module-null-sink",
            &format!("sink_name={SINK} rate={RATE} channels=2 sink_properties=\"device.description='Kavox internal'\""),
        )?);
        platform.modules.push(pulse.load_module(
            "module-remap-source",
            &format!("master={SINK}.monitor source_name={SOURCE} source_properties=\"device.description='Kavox Mic'\""),
        )?);
        if pulse.defaults()?.0.as_deref() == Some(SINK) {
            eprintln!(
                "warning: Kavox's sink became the default output, so system audio will go into the mic"
            );
        }
        println!("mic: {}", platform.mic_source);
        Ok(platform)
    }

    pub fn virtual_mic_name(&self) -> &str {
        "Kavox Mic"
    }

    pub fn start(&mut self, mut mixer: Mixer, mic: Option<rtrb::Producer<f32>>) -> Result<()> {
        let stop = self.stop.clone();
        self.threads.push(spawn_loop("output", move || {
            let stream = open_stream(Direction::Playback, SINK, "Output", 2)?;
            let mut buf = vec![0.0f32; PERIOD * 2];
            let mut bytes = vec![0u8; PERIOD * 2 * 4];
            while !stop.load(Ordering::Relaxed) {
                mixer.render(&mut buf);
                for (b, s) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(&buf) {
                    *b = s.to_le_bytes();
                }
                stream.write(&bytes).map_err(|e| anyhow!("write: {e}"))?;
            }
            Ok(())
        }));

        if let Some(mut ring) = mic {
            let stop = self.stop.clone();
            let source = self.mic_source.clone();
            self.threads.push(spawn_loop("mic", move || {
                let stream = open_stream(Direction::Record, &source, "Mic", 1)?;
                let mut bytes = vec![0u8; PERIOD * 4];
                while !stop.load(Ordering::Relaxed) {
                    stream.read(&mut bytes).map_err(|e| anyhow!("read: {e}"))?;
                    engine::push_mic(
                        &mut ring,
                        bytes
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|b| f32::from_le_bytes(*b)),
                    );
                }
                Ok(())
            }));
        }
        Ok(())
    }

    /// Records Kavox Mic, as a voice app would hear it.
    pub fn record_virtual(&mut self, mut on_chunk: ChunkFn) -> Result<()> {
        let stop = self.stop.clone();
        self.threads.push(spawn_loop("probe", move || {
            let stream = open_stream(Direction::Record, SOURCE, "Probe", 2)?;
            let mut bytes = vec![0u8; PERIOD * 2 * 4];
            let mut buf = vec![0.0f32; PERIOD * 2];
            while !stop.load(Ordering::Relaxed) {
                stream.read(&mut bytes).map_err(|e| anyhow!("read: {e}"))?;
                let received = Instant::now();
                for (s, b) in buf.iter_mut().zip(bytes.as_chunks::<4>().0) {
                    *s = f32::from_le_bytes(*b);
                }
                on_chunk(&buf, 2, RATE, received);
            }
            Ok(())
        }));
        Ok(())
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
        if self.modules.is_empty() {
            return;
        }
        match Pulse::connect() {
            Ok(mut pulse) => {
                for index in self.modules.iter().rev() {
                    if let Err(e) = pulse.unload_module(*index) {
                        eprintln!("unload module {index}: {e:#}");
                    }
                }
            }
            Err(e) => eprintln!("could not remove the virtual mic: {e:#}"),
        }
    }
}

fn spawn_loop(name: &str, body: impl FnOnce() -> Result<()> + Send + 'static) -> JoinHandle<()> {
    let label = name.to_owned();
    thread::Builder::new()
        .name(format!("kavox-{name}"))
        .spawn(move || {
            if let Err(e) = body() {
                eprintln!("{label} thread: {e:#}");
            }
        })
        .expect("spawn thread")
}

fn open_stream(direction: Direction, device: &str, name: &str, channels: u8) -> Result<Simple> {
    let spec = Spec {
        format: Format::F32le,
        channels,
        rate: RATE,
    };
    let period = (PERIOD * channels as usize * 4) as u32;
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
        name,
        &spec,
        None,
        Some(&attr),
    )
    .map_err(|e| anyhow!("open {name} stream on {device}: {e}"))
}

/// Minimal blocking wrapper over the async libpulse API, for module management.
struct Pulse {
    context: Context,
    mainloop: Mainloop,
}

impl Pulse {
    fn connect() -> Result<Self> {
        let mut mainloop = Mainloop::new().context("create pulse mainloop")?;
        let mut context = Context::new(&mainloop, "Kavox").context("create pulse context")?;
        context
            .connect(None, FlagSet::NOFLAGS, None)
            .map_err(|e| anyhow!("connect to PulseAudio/PipeWire: {e}"))?;
        loop {
            iterate(&mut mainloop)?;
            match context.get_state() {
                State::Ready => return Ok(Self { context, mainloop }),
                State::Failed | State::Terminated => bail!("connect to PulseAudio/PipeWire failed"),
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
            u32::MAX => bail!("load {name} failed (args: {args})"),
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

    /// Default (sink, source) names.
    fn defaults(&mut self) -> Result<(Option<String>, Option<String>)> {
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
}

impl Drop for Pulse {
    fn drop(&mut self) {
        self.context.disconnect();
    }
}

fn iterate(mainloop: &mut Mainloop) -> Result<()> {
    match mainloop.iterate(true) {
        IterateResult::Success(_) => Ok(()),
        IterateResult::Quit(_) => bail!("pulse mainloop quit"),
        IterateResult::Err(e) => bail!("pulse mainloop: {e}"),
    }
}
