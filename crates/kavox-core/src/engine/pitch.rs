use std::f32::consts::{PI, TAU};

use super::smoothed::Smoothed;

/// Length of the crossfade window. The two read heads sit half a window apart, so latency averages half of this.
const WINDOW_MS: f32 = 25.0;
/// Pitch changes glide instead of jumping, which also makes switching voices sound fun.
const GLIDE_MS: f32 = 80.0;
/// Fade between dry and shifted audio when the effect is switched on or off.
const FADE_MS: f32 = 20.0;
/// Below this the shift is inaudible, so the effect is bypassed rather than combed.
const MIN_SEMITONES: f32 = 0.05;

/// What the voice changer should do; sent to the renderer as one unit.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PitchParams {
    pub enabled: bool,
    pub semitones: f32,
    /// Peak swing of the pitch wobble, in semitones.
    pub wobble_depth: f32,
    pub wobble_hz: f32,
}

/// Granular pitch shifter for mono mic audio.
///
/// Audio is written into a ring. Two read heads trail the write head and drift by `1 - ratio` samples per sample,
/// which resamples the signal; each head fades out as it wraps, with the other fading in half a window later.
/// An LFO can wobble the pitch around the base shift.
pub struct PitchShifter {
    buf: Vec<f32>,
    mask: usize,
    write: usize,
    rate: f32,
    window: f32,
    /// Phase of the first head in `0..1`; the second head is half a cycle ahead.
    phase: f32,
    semitones: Smoothed,
    wobble_depth: Smoothed,
    wobble_hz: f32,
    lfo: f32,
    wet: Smoothed,
    active: bool,
}

impl PitchShifter {
    pub fn new(rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        let window = (rate * WINDOW_MS / 1000.0).round().max(8.0);
        let len = (window as usize + 4).next_power_of_two();
        Self {
            buf: vec![0.0; len],
            mask: len - 1,
            write: 0,
            rate,
            window,
            phase: 0.0,
            semitones: Smoothed::new(0.0),
            wobble_depth: Smoothed::new(0.0),
            wobble_hz: 0.0,
            lfo: 0.0,
            wet: Smoothed::new(0.0),
            active: false,
        }
    }

    pub fn apply(&mut self, p: PitchParams) {
        self.set(p.enabled, p.semitones, p.wobble_depth, p.wobble_hz);
    }

    /// Applies new voice parameters. Pitch glides to the new value unless the effect was fully off.
    pub fn set(&mut self, enabled: bool, semitones: f32, wobble_depth: f32, wobble_hz: f32) {
        let semitones = if semitones.is_finite() {
            semitones
        } else {
            0.0
        };
        let depth = if wobble_depth.is_finite() {
            wobble_depth.max(0.0)
        } else {
            0.0
        };
        let active = enabled && (semitones.abs() > MIN_SEMITONES || depth > 0.0);
        let was_idle = !self.active && self.wet.current() == 0.0;
        let glide = if was_idle {
            0
        } else {
            self.ms_to_frames(GLIDE_MS)
        };
        self.semitones.set(semitones, glide);
        self.wobble_depth.set(depth, glide);
        self.wobble_hz = if wobble_hz.is_finite() {
            wobble_hz.max(0.0)
        } else {
            0.0
        };
        self.active = active;
        self.wet
            .set(if active { 1.0 } else { 0.0 }, self.ms_to_frames(FADE_MS));
    }

    /// Clears audio history, e.g. when the mic device changes.
    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.phase = 0.0;
        self.lfo = 0.0;
    }

    fn ms_to_frames(&self, ms: f32) -> u32 {
        (self.rate * ms / 1000.0) as u32
    }

    #[inline]
    fn tap(&self, delay: f32) -> f32 {
        let whole = delay as usize;
        let frac = delay - whole as f32;
        let a = self.buf[self.write.wrapping_sub(whole) & self.mask];
        let b = self.buf[self.write.wrapping_sub(whole + 1) & self.mask];
        a + (b - a) * frac
    }

    /// Shifts `mono` in place. Handles any block size and never allocates.
    pub fn process(&mut self, mono: &mut [f32]) {
        let lfo_step = self.wobble_hz / self.rate;
        for sample in mono.iter_mut() {
            let dry = if sample.is_finite() { *sample } else { 0.0 };
            let wet = self.wet.next();
            self.buf[self.write & self.mask] = dry;

            if !self.active && wet == 0.0 {
                self.write = self.write.wrapping_add(1);
                *sample = dry;
                continue;
            }

            let wobble = self.wobble_depth.next() * (self.lfo * TAU).sin();
            self.lfo += lfo_step;
            self.lfo -= self.lfo.floor();
            let ratio = ((self.semitones.next() + wobble) / 12.0).exp2();

            self.phase += (1.0 - ratio) / self.window;
            self.phase -= self.phase.floor();
            let other = if self.phase >= 0.5 {
                self.phase - 0.5
            } else {
                self.phase + 0.5
            };
            let fade_a = (PI * self.phase).sin().powi(2);
            let shifted = self.tap(self.phase * self.window) * fade_a
                + self.tap(other * self.window) * (1.0 - fade_a);

            self.write = self.write.wrapping_add(1);
            *sample = dry + (shifted - dry) * wet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(hz: f32, frames: usize, amp: f32) -> Vec<f32> {
        (0..frames)
            .map(|i| amp * (TAU * hz * i as f32 / RATE as f32).sin())
            .collect()
    }

    fn run(shifter: &mut PitchShifter, input: &[f32], block: usize) -> Vec<f32> {
        let mut out = input.to_vec();
        for chunk in out.chunks_mut(block) {
            shifter.process(chunk);
        }
        out
    }

    /// Strongest frequency between 100 Hz and 2 kHz, found by scanning DFT bins. Zero crossings are unreliable here
    /// because the crossfade dips add the odd extra crossing.
    fn frequency(signal: &[f32]) -> f32 {
        let power = |hz: f32| {
            let w = TAU * hz / RATE as f32;
            let (re, im) = signal
                .iter()
                .enumerate()
                .fold((0f32, 0f32), |(re, im), (i, s)| {
                    (re + s * (w * i as f32).cos(), im + s * (w * i as f32).sin())
                });
            re * re + im * im
        };
        (20..400)
            .map(|step| step as f32 * 5.0)
            .max_by(|&a, &b| power(a).total_cmp(&power(b)))
            .unwrap()
    }

    fn rms(signal: &[f32]) -> f32 {
        (signal.iter().map(|s| s * s).sum::<f32>() / signal.len() as f32).sqrt()
    }

    /// Test tone. Not a multiple of 80 Hz: the heads are 12.5 ms apart, so a tone with a whole or half number of
    /// cycles in that gap (e.g. 440 Hz) would have both heads in antiphase and null out while they crossfade.
    const TONE: f32 = 500.0;

    fn shifted(semitones: f32) -> Vec<f32> {
        let mut shifter = PitchShifter::new(RATE);
        shifter.set(true, semitones, 0.0, 0.0);
        let out = run(&mut shifter, &sine(TONE, RATE as usize * 3 / 2, 0.5), 480);
        out[RATE as usize..].to_vec()
    }

    #[test]
    fn octave_up_doubles_frequency() {
        let hz = frequency(&shifted(12.0));
        assert!((hz - 1000.0).abs() <= 20.0, "got {hz} Hz");
    }

    #[test]
    fn octave_down_halves_frequency() {
        let hz = frequency(&shifted(-12.0));
        assert!((hz - 250.0).abs() <= 10.0, "got {hz} Hz");
    }

    #[test]
    fn keeps_level_roughly_constant() {
        for semitones in [-8.0, -3.0, 3.0, 10.0] {
            let level = rms(&shifted(semitones)) / rms(&sine(TONE, 4800, 0.5));
            assert!(
                (0.6..1.1).contains(&level),
                "{semitones} st gave level {level}"
            );
        }
    }

    #[test]
    fn bypass_is_exact() {
        let input = sine(440.0, 4800, 0.5);
        let mut shifter = PitchShifter::new(RATE);
        shifter.set(false, 7.0, 0.0, 0.0);
        assert_eq!(run(&mut shifter, &input, 480), input);
        shifter.set(true, 0.0, 0.0, 0.0);
        assert_eq!(run(&mut shifter, &input, 480), input);
    }

    #[test]
    fn block_size_does_not_change_output() {
        let input = sine(330.0, 9600, 0.4);
        let mut a = PitchShifter::new(RATE);
        let mut b = PitchShifter::new(RATE);
        a.set(true, 5.0, 1.0, 5.0);
        b.set(true, 5.0, 1.0, 5.0);
        assert_eq!(run(&mut a, &input, 64), run(&mut b, &input, 480));
    }

    #[test]
    fn output_stays_finite() {
        let mut shifter = PitchShifter::new(RATE);
        let mut seed = 0x2545_f491_u32;
        let mut noise = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32 * 2.0 - 1.0
        };
        shifter.set(true, 12.0, 3.0, 40.0);
        for round in 0..200 {
            let mut buf: Vec<f32> = (0..480).map(|_| noise()).collect();
            if round % 50 == 0 {
                buf[3] = f32::NAN;
                buf[4] = f32::INFINITY;
            }
            if round % 20 == 0 {
                shifter.set(true, (noise() * 40.0).clamp(-12.0, 12.0), 0.0, 0.0);
            }
            shifter.process(&mut buf);
            assert!(buf.iter().all(|s| s.is_finite() && s.abs() <= 1.5));
        }
        shifter.process(&mut [0.0; 480]);
    }

    #[test]
    fn switching_off_fades_without_clicks() {
        let mut shifter = PitchShifter::new(RATE);
        shifter.set(true, 7.0, 0.0, 0.0);
        let input = sine(220.0, RATE as usize, 0.5);
        let mut out = run(&mut shifter, &input[..RATE as usize / 2], 480);
        shifter.set(false, 7.0, 0.0, 0.0);
        out.extend(run(&mut shifter, &input[RATE as usize / 2..], 480));
        let max_step = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        // A clean 220 Hz sine at 0.5 never moves more than ~0.03 per sample.
        assert!(max_step < 0.1, "click of {max_step}");
        assert_eq!(out[out.len() - 100..], input[input.len() - 100..]);
    }

    #[test]
    fn glides_between_voices() {
        let mut shifter = PitchShifter::new(RATE);
        shifter.set(true, 0.0, 0.0, 0.0);
        shifter.set(true, 12.0, 0.0, 0.0);
        // Effect was idle, so the first target applies immediately.
        let out = run(&mut shifter, &sine(TONE, RATE as usize / 2, 0.5), 480);
        assert!((frequency(&out[RATE as usize / 4..]) - 1000.0).abs() < 60.0);
        shifter.set(true, -12.0, 0.0, 0.0);
        let early = run(&mut shifter, &sine(TONE, 960, 0.5), 480);
        assert!(rms(&early) > 0.1);
    }
}
