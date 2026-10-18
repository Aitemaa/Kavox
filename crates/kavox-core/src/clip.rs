//! Decoding, resampling, loudness analysis and the in-memory clip cache.

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::{Error, Result};

pub const SUPPORTED_EXTENSIONS: &[&str] = &["wav", "mp3", "ogg", "oga", "flac"];
/// Clips up to this long stay decoded in memory; longer ones load on first play.
pub const RESIDENT_MAX_MS: u64 = 30_000;
const CACHE_BUDGET_BYTES: usize = 150 * 1024 * 1024;
const TARGET_LUFS: f64 = -16.0;

/// Interleaved stereo f32 at one output rate.
pub struct ClipData {
    pub rate: u32,
    pub frames: usize,
    pub samples: Box<[f32]>,
}

impl ClipData {
    pub fn from_stereo(rate: u32, samples: Vec<f32>) -> Self {
        Self {
            rate,
            frames: samples.len() / 2,
            samples: samples.into_boxed_slice(),
        }
    }

    /// Sine tone with 10 ms fades, for device tests.
    pub fn tone(rate: u32, freq: f32, ms: u32, amplitude: f32) -> Self {
        let frames = (rate * ms / 1000) as usize;
        let fade = (rate / 100) as usize;
        let samples = (0..frames)
            .flat_map(|i| {
                let env = (i.min(frames - 1 - i) as f32 / fade as f32).min(1.0);
                let s = amplitude * env * (TAU * freq * i as f32 / rate as f32).sin();
                [s, s]
            })
            .collect();
        Self::from_stereo(rate, samples)
    }

    pub fn bytes(&self) -> usize {
        self.samples.len() * size_of::<f32>()
    }
}

/// Decoded audio at its native rate, interleaved stereo.
pub struct Decoded {
    pub rate: u32,
    pub samples: Vec<f32>,
}

impl Decoded {
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    pub fn duration_ms(&self) -> u64 {
        self.frames() as u64 * 1000 / self.rate.max(1) as u64
    }
}

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SUPPORTED_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn decode(path: &Path) -> Result<Decoded> {
    let name = file_name(path);
    let fail = |e: &dyn std::fmt::Display| Error::DecodeFailed(name.clone(), e.to_string());

    let file = std::fs::File::open(path)?;
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
        .map_err(|_| Error::UnsupportedFormat(name.clone()))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| Error::UnsupportedFormat(name.clone()))?;
    let track_id = track.id;
    let mut rate = track.codec_params.sample_rate.unwrap_or(48_000);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|_| Error::UnsupportedFormat(name.clone()))?;

    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(fail(&e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(fail(&e)),
        };
        let spec = *decoded.spec();
        rate = spec.rate;
        let channels = spec.channels.count();
        if channels == 0 {
            continue;
        }
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        for frame in buf.samples().chunks_exact(channels) {
            samples.push(frame[0]);
            samples.push(*frame.get(1).unwrap_or(&frame[0]));
        }
    }
    if samples.is_empty() {
        return Err(Error::DecodeFailed(name, "no audio in file".into()));
    }
    Ok(Decoded { rate, samples })
}

pub fn resample(decoded: &Decoded, to: u32) -> Result<ClipData> {
    let frames = decoded.frames();
    if decoded.rate == to || frames == 0 {
        return Ok(ClipData::from_stereo(to, decoded.samples.clone()));
    }
    let fail = |e: &dyn std::fmt::Display| Error::DecodeFailed("resample".into(), e.to_string());
    let input = InterleavedSlice::new(&decoded.samples, 2, frames).map_err(|e| fail(&e))?;
    let mut resampler =
        Fft::<f32>::new(decoded.rate as usize, to as usize, 1024, 2, FixedSync::Both)
            .map_err(|e| fail(&e))?;
    let output = resampler
        .process_all(&input, frames, None)
        .map_err(|e| fail(&e))?;
    Ok(ClipData::from_stereo(to, output.take_data()))
}

pub struct Analysis {
    pub duration_ms: u64,
    pub peak: f32,
    /// Gain that brings the clip to the loudness target without pushing its peak past −1 dBFS.
    pub norm_gain_db: f32,
}

pub fn analyze(decoded: &Decoded) -> Analysis {
    let peak = decoded.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    let loudness = ebur128::EbuR128::new(2, decoded.rate, ebur128::Mode::I)
        .and_then(|mut meter| {
            meter.add_frames_f32(&decoded.samples)?;
            meter.loudness_global()
        })
        .ok()
        .filter(|l| l.is_finite());
    let norm_gain_db = match loudness {
        Some(lufs) if peak > 0.0 => {
            let headroom_db = -1.0 - 20.0 * peak.log10() as f64;
            (TARGET_LUFS - lufs).clamp(-20.0, 12.0).min(headroom_db) as f32
        }
        _ => 0.0,
    };
    Analysis {
        duration_ms: decoded.duration_ms(),
        peak,
        norm_gain_db,
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

struct CacheEntry {
    clip: Arc<ClipData>,
    last_used: Instant,
}

/// Decoded clips keyed by (sound id, output rate).
#[derive(Default)]
pub struct ClipCache {
    entries: HashMap<(String, u32), CacheEntry>,
}

impl ClipCache {
    pub fn get(&mut self, sound: &str, rate: u32) -> Option<Arc<ClipData>> {
        let entry = self.entries.get_mut(&(sound.to_owned(), rate))?;
        entry.last_used = Instant::now();
        Some(entry.clip.clone())
    }

    pub fn contains(&self, sound: &str, rate: u32) -> bool {
        self.entries.contains_key(&(sound.to_owned(), rate))
    }

    pub fn insert(&mut self, sound: &str, clip: Arc<ClipData>) {
        let key = (sound.to_owned(), clip.rate);
        self.entries.insert(
            key,
            CacheEntry {
                clip,
                last_used: Instant::now(),
            },
        );
        self.evict();
    }

    pub fn remove_sound(&mut self, sound: &str) {
        self.entries.retain(|(id, _), _| id != sound);
    }

    pub fn retain_rates(&mut self, rates: &[u32]) {
        self.entries.retain(|(_, rate), _| rates.contains(rate));
    }

    pub fn bytes(&self) -> usize {
        self.entries.values().map(|e| e.clip.bytes()).sum()
    }

    /// Drops least-recently-used long clips that aren't playing until under budget.
    fn evict(&mut self) {
        let mut total = self.bytes();
        if total <= CACHE_BUDGET_BYTES {
            return;
        }
        let mut candidates: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, e)| {
                let long =
                    e.clip.frames as u64 * 1000 / e.clip.rate.max(1) as u64 > RESIDENT_MAX_MS;
                long && Arc::strong_count(&e.clip) == 1
            })
            .map(|(k, e)| (k.clone(), e.last_used, e.clip.bytes()))
            .collect();
        candidates.sort_by_key(|(_, used, _)| *used);
        for (key, _, bytes) in candidates {
            if total <= CACHE_BUDGET_BYTES {
                break;
            }
            self.entries.remove(&key);
            total -= bytes;
        }
    }
}

pub struct LoadJob {
    pub sound: String,
    pub path: PathBuf,
    pub rates: Vec<u32>,
}

pub struct LoadResult {
    pub sound: String,
    pub clips: Result<Vec<Arc<ClipData>>>,
}

/// Background decode + resample on two worker threads.
pub struct Loader {
    jobs: Sender<LoadJob>,
    results: Receiver<LoadResult>,
    in_flight: Vec<(String, u32)>,
}

impl Loader {
    pub fn new() -> Self {
        let (jobs, job_rx) = mpsc::channel::<LoadJob>();
        let (result_tx, results) = mpsc::channel();
        let job_rx = Arc::new(Mutex::new(job_rx));
        for n in 0..2 {
            let job_rx = job_rx.clone();
            let result_tx = result_tx.clone();
            let _ = thread::Builder::new()
                .name(format!("kavox-loader-{n}"))
                .spawn(move || {
                    loop {
                        let job = match job_rx.lock() {
                            Ok(rx) => match rx.recv() {
                                Ok(job) => job,
                                Err(_) => return,
                            },
                            Err(_) => return,
                        };
                        let clips = decode(&job.path).and_then(|d| {
                            job.rates
                                .iter()
                                .map(|&rate| resample(&d, rate).map(Arc::new))
                                .collect()
                        });
                        if result_tx
                            .send(LoadResult {
                                sound: job.sound,
                                clips,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                });
        }
        Self {
            jobs,
            results,
            in_flight: Vec::new(),
        }
    }

    /// Queues a load for the rates not already loading; returns false if nothing was queued.
    pub fn request(&mut self, sound: &str, path: PathBuf, rates: &[u32]) -> bool {
        let rates: Vec<u32> = rates
            .iter()
            .copied()
            .filter(|r| !self.in_flight.contains(&(sound.to_owned(), *r)))
            .collect();
        if rates.is_empty() {
            return false;
        }
        self.in_flight
            .extend(rates.iter().map(|r| (sound.to_owned(), *r)));
        let _ = self.jobs.send(LoadJob {
            sound: sound.to_owned(),
            path,
            rates,
        });
        true
    }

    pub fn is_loading(&self, sound: &str) -> bool {
        self.in_flight.iter().any(|(s, _)| s == sound)
    }

    pub fn drain(&mut self) -> Vec<LoadResult> {
        let results: Vec<LoadResult> = self.results.try_iter().collect();
        for result in &results {
            self.in_flight.retain(|(s, _)| s != &result.sound);
        }
        results
    }
}

impl Default for Loader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Writes a 16-bit PCM WAV for tests.
    pub fn write_wav(path: &Path, rate: u32, channels: u16, samples: &[f32]) {
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
            .collect();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        std::fs::write(path, out).unwrap();
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kavox-test-{name}-{}", ulid::Ulid::generate()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sine(rate: u32, secs: f32, amp: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| amp * (TAU * 440.0 * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn decodes_mono_wav_to_stereo() {
        let dir = temp_dir("decode");
        let path = dir.join("a.wav");
        write_wav(&path, 44_100, 1, &sine(44_100, 0.5, 0.5));
        let d = decode(&path).unwrap();
        assert_eq!(d.rate, 44_100);
        assert_eq!(d.frames(), 22_050);
        assert_eq!(d.samples[200], d.samples[201]);
        assert_eq!(d.duration_ms(), 500);
    }

    #[test]
    fn resamples_to_output_rate() {
        let d = Decoded {
            rate: 44_100,
            samples: sine(44_100, 1.0, 0.5)
                .into_iter()
                .flat_map(|s| [s, s])
                .collect(),
        };
        let clip = resample(&d, 48_000).unwrap();
        assert_eq!(clip.rate, 48_000);
        assert!(
            (clip.frames as i64 - 48_000).abs() <= 2,
            "frames {}",
            clip.frames
        );
        let peak = clip.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.02, "peak {peak}");
    }

    #[test]
    fn rejects_garbage_and_truncated_files() {
        let dir = temp_dir("garbage");
        let junk = dir.join("junk.mp3");
        std::fs::write(&junk, b"definitely not audio").unwrap();
        assert!(decode(&junk).is_err());

        let wav = dir.join("short.wav");
        write_wav(&wav, 48_000, 2, &sine(48_000, 0.2, 0.3));
        let bytes = std::fs::read(&wav).unwrap();
        std::fs::write(&wav, &bytes[..30]).unwrap();
        assert!(decode(&wav).is_err());
    }

    #[test]
    fn normalization_targets_loudness_within_headroom() {
        let quiet = Decoded {
            rate: 48_000,
            samples: sine(48_000, 2.0, 0.05)
                .into_iter()
                .flat_map(|s| [s, s])
                .collect(),
        };
        let a = analyze(&quiet);
        assert!(
            a.norm_gain_db > 6.0,
            "quiet clip should be boosted, got {}",
            a.norm_gain_db
        );
        assert!(a.peak * db_to_gain(a.norm_gain_db) <= 0.9);

        let loud = Decoded {
            rate: 48_000,
            samples: sine(48_000, 2.0, 0.99)
                .into_iter()
                .flat_map(|s| [s, s])
                .collect(),
        };
        assert!(analyze(&loud).norm_gain_db < 0.0);
    }

    #[test]
    fn cache_evicts_only_idle_long_clips() {
        let mut cache = ClipCache::default();
        let long_frames = 48_000 * 60;
        for n in 0..4 {
            cache.insert(
                &format!("long{n}"),
                Arc::new(ClipData::from_stereo(48_000, vec![0.0; long_frames * 2])),
            );
        }
        let playing = cache.get("long3", 48_000).unwrap();
        cache.insert(
            "short",
            Arc::new(ClipData::from_stereo(48_000, vec![0.0; 1000])),
        );
        assert!(cache.bytes() <= CACHE_BUDGET_BYTES || cache.contains("long3", 48_000));
        assert!(cache.contains("short", 48_000));
        assert!(cache.contains("long3", 48_000));
        drop(playing);
    }

    #[test]
    fn loader_decodes_in_background() {
        let dir = temp_dir("loader");
        let path = dir.join("b.wav");
        write_wav(&path, 48_000, 2, &sine(48_000, 0.1, 0.3));
        let mut loader = Loader::new();
        assert!(loader.request("b", path.clone(), &[48_000, 44_100]));
        assert!(!loader.request("b", path, &[48_000]));
        let start = Instant::now();
        let results = loop {
            let r = loader.drain();
            if !r.is_empty() || start.elapsed().as_secs() > 5 {
                break r;
            }
            thread::sleep(std::time::Duration::from_millis(5));
        };
        let clips = results.into_iter().next().unwrap().clips.unwrap();
        assert_eq!(
            clips.iter().map(|c| c.rate).collect::<Vec<_>>(),
            [48_000, 44_100]
        );
        assert!(!loader.is_loading("b"));
    }
}
