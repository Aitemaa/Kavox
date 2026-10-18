use std::time::Instant;

use anyhow::{Context as _, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, Device, Host, SampleFormat, Stream, StreamConfig, SupportedBufferSize,
    SupportedStreamConfig,
};

use crate::engine::{self, Mixer};

const CABLE_PATTERNS: [&str; 2] = ["CABLE Input", "VoiceMeeter Input"];
const PERIOD: u32 = 480;

pub type ChunkFn = Box<dyn FnMut(&[f32], usize, u32, Instant) + Send>;

/// WASAPI streams: clips + mic into the virtual cable.
pub struct Platform {
    pub out_rate: u32,
    pub mic_rate: u32,
    host: Host,
    out_device: Device,
    out_config: StreamConfig,
    mic_device: Device,
    mic_config: StreamConfig,
    virtual_mic: String,
    streams: Vec<Stream>,
}

impl Platform {
    pub fn open(device: Option<&str>) -> Result<Self> {
        let host = cpal::default_host();
        let patterns: Vec<&str> = device.map_or(CABLE_PATTERNS.to_vec(), |d| vec![d]);
        let out_device = host
            .output_devices()?
            .find(|d| d.name().is_ok_and(|n| patterns.iter().any(|p| n.contains(p))))
            .ok_or_else(|| {
                anyhow!(
                    "no virtual cable found (looked for {patterns:?}). Install VB-CABLE from https://vb-audio.com/Cable/ or pass --device <name>"
                )
            })?;
        let out_name = out_device.name()?;
        let (out_config, out_rate) = stream_config(out_device.default_output_config()?)?;
        if host
            .default_output_device()
            .and_then(|d| d.name().ok())
            .as_deref()
            == Some(out_name.as_str())
        {
            eprintln!(
                "warning: {out_name} is the Windows default playback device, so all PC audio goes into the mic"
            );
        }

        let mic_device = host.default_input_device().context("no microphone found")?;
        let mic_name = mic_device.name()?;
        if mic_name.contains("CABLE Output") || mic_name.contains("VoiceMeeter Output") {
            bail!(
                "the Windows default microphone is \"{mic_name}\"; set your real mic as default so Kavox doesn't record itself"
            );
        }
        let (mic_config, mic_rate) = stream_config(mic_device.default_input_config()?)?;

        println!(
            "output: {out_name} @ {out_rate} Hz ({:?})",
            out_config.buffer_size
        );
        println!("mic: {mic_name} @ {mic_rate} Hz");
        Ok(Self {
            out_rate,
            mic_rate,
            host,
            out_device,
            out_config,
            mic_device,
            mic_config,
            virtual_mic: out_name.replace("Input", "Output"),
            streams: Vec::new(),
        })
    }

    pub fn virtual_mic_name(&self) -> &str {
        &self.virtual_mic
    }

    pub fn start(&mut self, mut mixer: Mixer, mic: Option<rtrb::Producer<f32>>) -> Result<()> {
        let channels = self.out_config.channels as usize;
        let mut stereo = vec![0.0f32; 8192];
        let output = self.out_device.build_output_stream(
            &self.out_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let frames = data.len() / channels;
                if stereo.len() < frames * 2 {
                    stereo.resize(frames * 2, 0.0);
                }
                let mixed = &mut stereo[..frames * 2];
                mixer.render(mixed);
                for (frame, s) in data
                    .chunks_exact_mut(channels)
                    .zip(mixed.as_chunks::<2>().0)
                {
                    for (i, x) in frame.iter_mut().enumerate() {
                        *x = if i < 2 { s[i] } else { 0.0 };
                    }
                }
            },
            |e| eprintln!("output stream error: {e}"),
            None,
        )?;
        output.play()?;
        self.streams.push(output);

        if let Some(mut ring) = mic {
            let channels = self.mic_config.channels as usize;
            let input = self.mic_device.build_input_stream(
                &self.mic_config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    engine::push_mic(
                        &mut ring,
                        data.chunks_exact(channels)
                            .map(|f| f.iter().sum::<f32>() / channels as f32),
                    );
                },
                |e| eprintln!("mic stream error: {e}"),
                None,
            )?;
            input.play()?;
            self.streams.push(input);
        }
        Ok(())
    }

    /// Records the cable's capture side, as a voice app would hear it.
    pub fn record_virtual(&mut self, mut on_chunk: ChunkFn) -> Result<()> {
        let device = self
            .host
            .input_devices()?
            .find(|d| d.name().is_ok_and(|n| n == self.virtual_mic))
            .with_context(|| format!("can't find \"{}\" to record from", self.virtual_mic))?;
        let (config, rate) = stream_config(device.default_input_config()?)?;
        let channels = config.channels as usize;
        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                on_chunk(data, channels, rate, Instant::now())
            },
            |e| eprintln!("probe stream error: {e}"),
            None,
        )?;
        stream.play()?;
        self.streams.push(stream);
        Ok(())
    }
}

fn stream_config(supported: SupportedStreamConfig) -> Result<(StreamConfig, u32)> {
    if supported.sample_format() != SampleFormat::F32 {
        bail!(
            "device uses {:?}; the spike only handles f32",
            supported.sample_format()
        );
    }
    let fixed = matches!(supported.buffer_size(), SupportedBufferSize::Range { min, max } if (*min..=*max).contains(&PERIOD));
    let mut config = supported.config();
    if fixed {
        config.buffer_size = BufferSize::Fixed(PERIOD);
    }
    let rate = config.sample_rate.0;
    Ok((config, rate))
}
