/// −1 dBFS.
const THRESHOLD: f32 = 0.891;

/// Peak limiter (1 ms attack, 100 ms release) followed by a hard clamp, so output never exceeds ±1.0.
pub struct Limiter {
    gain: f32,
    attack: f32,
    release: f32,
}

impl Limiter {
    pub fn new(rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        Self {
            gain: 1.0,
            attack: (-1.0 / (0.001 * rate)).exp(),
            release: (-1.0 / (0.1 * rate)).exp(),
        }
    }

    /// Processes interleaved stereo in place and returns the output peak.
    pub fn process(&mut self, buf: &mut [f32]) -> f32 {
        let mut peak_out = 0f32;
        for frame in buf.as_chunks_mut::<2>().0 {
            let l = if frame[0].is_finite() { frame[0] } else { 0.0 };
            let r = if frame[1].is_finite() { frame[1] } else { 0.0 };
            let peak = l.abs().max(r.abs());
            let target = if peak > THRESHOLD {
                THRESHOLD / peak
            } else {
                1.0
            };
            let coeff = if target < self.gain {
                self.attack
            } else {
                self.release
            };
            self.gain = target + (self.gain - target) * coeff;
            frame[0] = (l * self.gain).clamp(-1.0, 1.0);
            frame[1] = (r * self.gain).clamp(-1.0, 1.0);
            peak_out = peak_out.max(frame[0].abs()).max(frame[1].abs());
        }
        peak_out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_exceeds_full_scale() {
        let mut limiter = Limiter::new(48_000);
        let mut seed = 0x2545_f491_u32;
        for _ in 0..200 {
            let mut buf: Vec<f32> = (0..960)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    (seed as f32 / u32::MAX as f32 - 0.5) * 16.0
                })
                .collect();
            limiter.process(&mut buf);
            assert!(buf.iter().all(|s| s.abs() <= 1.0));
        }
    }

    #[test]
    fn leaves_quiet_audio_alone() {
        let mut limiter = Limiter::new(48_000);
        let mut buf = vec![0.5f32; 960];
        limiter.process(&mut buf);
        assert!(buf.iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn silences_nan() {
        let mut limiter = Limiter::new(48_000);
        let mut buf = [f32::NAN, f32::INFINITY, 0.1, 0.1];
        limiter.process(&mut buf);
        assert!(buf.iter().all(|s| s.is_finite()));
    }
}
