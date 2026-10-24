use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use super::meters::{Meter, Meters};

/// Mono samples; 340 ms at 96 kHz.
const RING_CAPACITY: usize = 32_768;
const TARGET_MS: usize = 20;
/// Drift correction is clamped to ±0.2%, far more than real clock drift (~100 ppm).
const MAX_ADJUST: f64 = 0.002;
/// Settles in ~10 s with ~0.45 damping for a 20 ms target at 10 ms blocks.
const KP: f64 = 0.004;
const KI: f64 = 0.000_01;

pub fn mic_channel(meters: Arc<Meters>) -> (MicSink, MicInput) {
    let (producer, consumer) = RingBuffer::new(RING_CAPACITY);
    (
        MicSink {
            ring: producer,
            tap: None,
            meters,
        },
        MicInput::new(consumer),
    )
}

/// Like `mic_channel`, with a second reader of the same samples (the monitor, for "hear myself").
pub fn mic_channel_tapped(meters: Arc<Meters>) -> (MicSink, MicInput, MicInput) {
    let (mut sink, input) = mic_channel(meters);
    let (tap_producer, tap_consumer) = RingBuffer::new(RING_CAPACITY);
    sink.tap = Some(tap_producer);
    (sink, input, MicInput::new(tap_consumer))
}

/// Capture side: downmixes to mono, meters, and feeds the ring.
pub struct MicSink {
    ring: Producer<f32>,
    tap: Option<Producer<f32>>,
    meters: Arc<Meters>,
}

impl MicSink {
    pub fn push_interleaved<T: Copy>(
        &mut self,
        data: &[T],
        channels: usize,
        to_f32: impl Fn(T) -> f32,
    ) {
        let channels = channels.max(1);
        let scale = 1.0 / channels as f32;
        let mut peak = 0f32;
        for frame in data.chunks_exact(channels) {
            let s = frame.iter().map(|&x| to_f32(x)).sum::<f32>() * scale;
            peak = peak.max(s.abs());
            // A full ring means the renderer stalled; dropping new samples is the cheapest recovery.
            let _ = self.ring.push(s);
            if let Some(tap) = &mut self.tap {
                let _ = tap.push(s);
            }
        }
        self.meters.record(Meter::Mic, peak);
    }
}

/// Render side: resamples the mic to the output rate and absorbs clock drift with a PI controller on ring fill.
pub struct MicInput {
    ring: Consumer<f32>,
    in_rate: u32,
    out_rate: u32,
    base_step: f64,
    frac: f64,
    s0: f32,
    s1: f32,
    target: usize,
    filling: bool,
    avg_fill: f64,
    integral: f64,
}

impl MicInput {
    fn new(ring: Consumer<f32>) -> Self {
        let mut input = Self {
            ring,
            in_rate: 48_000,
            out_rate: 48_000,
            base_step: 1.0,
            frac: 1.0,
            s0: 0.0,
            s1: 0.0,
            target: 0,
            filling: true,
            avg_fill: 0.0,
            integral: 0.0,
        };
        input.reset();
        input
    }

    pub fn set_input_rate(&mut self, rate: u32) {
        self.in_rate = rate.max(1);
        self.reset();
    }

    pub fn prepare(&mut self, out_rate: u32) {
        self.out_rate = out_rate.max(1);
        self.reset();
    }

    fn reset(&mut self) {
        self.base_step = self.in_rate as f64 / self.out_rate as f64;
        self.target = (self.in_rate as usize * TARGET_MS / 1000).max(1);
        self.avg_fill = self.target as f64;
        self.integral = 0.0;
        self.filling = true;
    }

    /// Fills `out` with mono samples at the output rate; silence while buffering.
    pub fn read(&mut self, out: &mut [f32]) {
        let available = self.ring.slots();
        if available > self.target * 4 {
            if let Ok(chunk) = self.ring.read_chunk(available - self.target) {
                chunk.commit_all();
            }
            self.avg_fill = self.target as f64;
            self.integral = 0.0;
        }
        if self.filling {
            if self.ring.slots() < self.target {
                out.fill(0.0);
                return;
            }
            self.filling = false;
        }

        self.avg_fill += (self.ring.slots() as f64 - self.avg_fill) * 0.05;
        let error = (self.avg_fill - self.target as f64) / self.target as f64;
        self.integral = (self.integral + error * KI).clamp(-MAX_ADJUST, MAX_ADJUST);
        let step =
            self.base_step * (1.0 + (error * KP + self.integral).clamp(-MAX_ADJUST, MAX_ADJUST));

        for i in 0..out.len() {
            while self.frac >= 1.0 {
                let Ok(s) = self.ring.pop() else {
                    self.filling = true;
                    self.avg_fill = self.target as f64;
                    out[i..].fill(0.0);
                    return;
                };
                self.s0 = self.s1;
                self.s1 = s;
                self.frac -= 1.0;
            }
            out[i] = self.s0 + (self.s1 - self.s0) * self.frac as f32;
            self.frac += step;
        }
    }

    #[cfg(test)]
    fn fill(&self) -> usize {
        self.ring.slots()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds a mic whose clock runs `ppm` fast or slow, in 10 ms chunks, for `seconds`.
    fn simulate(in_rate: u32, out_rate: u32, ppm: f64, seconds: usize) -> (usize, usize, usize) {
        let meters = Arc::new(Meters::default());
        let (mut sink, mut input) = mic_channel(meters);
        input.set_input_rate(in_rate);
        input.prepare(out_rate);
        let block = out_rate as usize / 100;
        let mut out = vec![0f32; block];
        let produced_per_tick = in_rate as f64 * (1.0 + ppm / 1e6) / 100.0;
        let mut owed = 0.0;
        let (mut min_fill, mut max_fill, mut underruns) = (usize::MAX, 0, 0);
        for tick in 0..seconds * 100 {
            owed += produced_per_tick;
            let n = owed as usize;
            owed -= n as f64;
            sink.push_interleaved(&vec![0.25f32; n], 1, |s| s);
            let was_filling = input.filling;
            input.read(&mut out);
            if input.filling && !was_filling {
                underruns += 1;
            }
            // Fill is measured after the read, so it sits one block below the 20 ms target once settled.
            if tick > 3000 {
                min_fill = min_fill.min(input.fill());
                max_fill = max_fill.max(input.fill());
            }
        }
        (min_fill, max_fill, underruns)
    }

    #[test]
    fn absorbs_fast_and_slow_clocks() {
        for ppm in [-1000.0, -150.0, 0.0, 150.0, 1000.0] {
            let (min, max, underruns) = simulate(48_000, 48_000, ppm, 600);
            assert_eq!(underruns, 0, "{ppm} ppm underran");
            // Target is 960 samples; chunked delivery swings fill by one 480-sample block.
            assert!(
                min >= 300 && max <= 2000,
                "{ppm} ppm drifted: fill {min}..{max}"
            );
        }
    }

    #[test]
    fn tap_reads_the_same_samples() {
        let meters = Arc::new(Meters::default());
        let (mut sink, mut a, mut b) = mic_channel_tapped(meters);
        for input in [&mut a, &mut b] {
            input.set_input_rate(48_000);
            input.prepare(48_000);
        }
        let samples: Vec<f32> = (0..2000).map(|i| (i % 100) as f32 / 100.0).collect();
        sink.push_interleaved(&samples, 1, |s| s);
        let (mut out_a, mut out_b) = (vec![0f32; 480], vec![0f32; 480]);
        a.read(&mut out_a);
        b.read(&mut out_b);
        assert_eq!(out_a, out_b);
        assert!(out_a.iter().any(|&s| s != 0.0));
    }

    #[test]
    fn converts_between_rates() {
        let (min, max, underruns) = simulate(44_100, 48_000, 100.0, 120);
        assert_eq!(underruns, 0);
        assert!(min >= 250 && max <= 1900, "fill {min}..{max}");
    }

    #[test]
    fn outputs_mic_signal_once_buffered() {
        let (mut sink, mut input) = mic_channel(Arc::new(Meters::default()));
        let mut out = vec![1f32; 480];
        input.read(&mut out);
        assert!(out.iter().all(|&s| s == 0.0));
        sink.push_interleaved(&vec![0.5f32; 2000], 1, |s| s);
        input.read(&mut out);
        assert!((out[100] - 0.5).abs() < 1e-6);
    }
}
