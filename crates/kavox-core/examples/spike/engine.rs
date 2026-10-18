use std::f32::consts::TAU;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use rtrb::{Consumer, Producer};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const MAX_VOICES: usize = 32;

/// Decoded clip, interleaved stereo at the output rate.
pub struct Clip {
    pub name: String,
    pub samples: Vec<f32>,
}

pub enum Command {
    Play(Arc<Clip>),
    StopAll,
    SetMicEnabled(bool),
}

struct Voice {
    clip: Arc<Clip>,
    pos: usize,
}

pub struct Mixer {
    voices: Vec<Voice>,
    commands: Consumer<Command>,
    mic: Option<MicIn>,
    mic_enabled: bool,
}

impl Mixer {
    pub fn new(commands: Consumer<Command>, mic: Option<MicIn>) -> Self {
        Self {
            voices: Vec::with_capacity(MAX_VOICES),
            commands,
            mic,
            mic_enabled: true,
        }
    }

    /// Fills `out` (interleaved stereo) with the playing clips plus the mic.
    pub fn render(&mut self, out: &mut [f32]) {
        while let Ok(command) = self.commands.pop() {
            match command {
                Command::Play(clip) => {
                    if self.voices.len() == MAX_VOICES {
                        self.voices.remove(0);
                    }
                    self.voices.push(Voice { clip, pos: 0 });
                }
                Command::StopAll => self.voices.clear(),
                Command::SetMicEnabled(on) => self.mic_enabled = on,
            }
        }

        out.fill(0.0);
        for voice in &mut self.voices {
            let src = &voice.clip.samples[voice.pos..];
            let n = src.len().min(out.len());
            for (o, s) in out[..n].iter_mut().zip(src) {
                *o += s;
            }
            voice.pos += n;
        }
        self.voices.retain(|v| v.pos < v.clip.samples.len());

        if let Some(mic) = &mut self.mic {
            mic.mix_into(out, self.mic_enabled);
        }
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }
}

/// Mono mic samples resampled to the output rate through a 20 ms jitter buffer.
pub struct MicIn {
    ring: Consumer<f32>,
    step: f64,
    frac: f64,
    s0: f32,
    s1: f32,
    target: usize,
    filling: bool,
}

impl MicIn {
    pub fn new(ring: Consumer<f32>, mic_rate: u32, out_rate: u32) -> Self {
        Self {
            ring,
            step: mic_rate as f64 / out_rate as f64,
            frac: 1.0,
            s0: 0.0,
            s1: 0.0,
            target: mic_rate as usize / 50,
            filling: true,
        }
    }

    fn mix_into(&mut self, out: &mut [f32], enabled: bool) {
        // Crude drift handling for the spike: drop backlog, re-prefill on underrun.
        let available = self.ring.slots();
        if available > self.target * 4
            && let Ok(chunk) = self.ring.read_chunk(available - self.target)
        {
            chunk.commit_all();
        }
        if self.filling {
            if self.ring.slots() < self.target {
                return;
            }
            self.filling = false;
        }

        for frame in out.as_chunks_mut::<2>().0 {
            while self.frac >= 1.0 {
                let Ok(s) = self.ring.pop() else {
                    self.filling = true;
                    return;
                };
                self.s0 = self.s1;
                self.s1 = s;
                self.frac -= 1.0;
            }
            let s = self.s0 + (self.s1 - self.s0) * self.frac as f32;
            self.frac += self.step;
            if enabled {
                frame[0] += s;
                frame[1] += s;
            }
        }
    }
}

/// Pushes mic samples, dropping the rest if the ring is full.
pub fn push_mic(ring: &mut Producer<f32>, samples: impl IntoIterator<Item = f32>) {
    for s in samples {
        if ring.push(s).is_err() {
            break;
        }
    }
}

pub fn load_clip(path: &Path, out_rate: u32) -> Result<Clip> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .with_context(|| format!("unsupported format: {}", path.display()))?;
    let mut format = probed.format;
    let track = format.default_track().context("no audio track")?;
    let track_id = track.id;
    let mut rate = track.codec_params.sample_rate.unwrap_or(out_rate);
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut stereo = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let spec = *decoded.spec();
        rate = spec.rate;
        let channels = spec.channels.count();
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        for frame in buf.samples().chunks_exact(channels) {
            stereo.push(frame[0]);
            stereo.push(*frame.get(1).unwrap_or(&frame[0]));
        }
    }

    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Clip {
        name,
        samples: resample_linear(&stereo, rate, out_rate),
    })
}

/// 20 ms 1 kHz tone used by the latency probe.
pub fn beep(out_rate: u32) -> Clip {
    let frames = out_rate as usize / 50;
    let samples = (0..frames)
        .flat_map(|i| {
            let s = 0.9 * (TAU * 1000.0 * i as f32 / out_rate as f32).sin();
            [s, s]
        })
        .collect();
    Clip {
        name: "beep".into(),
        samples,
    }
}

/// Index of the first frame whose first channel exceeds `threshold`.
pub fn find_onset(samples: &[f32], channels: usize, threshold: f32) -> Option<usize> {
    samples
        .chunks_exact(channels)
        .position(|f| f[0].abs() > threshold)
}

fn resample_linear(stereo: &[f32], from: u32, to: u32) -> Vec<f32> {
    let frames_in = stereo.len() / 2;
    if from == to || frames_in == 0 {
        return stereo.to_vec();
    }
    let frames_out = (frames_in as u64 * to as u64 / from as u64) as usize;
    let step = from as f64 / to as f64;
    let mut out = Vec::with_capacity(frames_out * 2);
    for i in 0..frames_out {
        let pos = i as f64 * step;
        let i0 = pos as usize;
        let i1 = (i0 + 1).min(frames_in - 1);
        let t = (pos - i0 as f64) as f32;
        for c in 0..2 {
            let a = stereo[i0 * 2 + c];
            let b = stereo[i1 * 2 + c];
            out.push(a + (b - a) * t);
        }
    }
    out
}
