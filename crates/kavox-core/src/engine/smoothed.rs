/// Gain that ramps linearly to its target to avoid zipper noise.
#[derive(Debug, Clone, Copy)]
pub struct Smoothed {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
}

impl Smoothed {
    pub fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
            step: 0.0,
            remaining: 0,
        }
    }

    pub fn set(&mut self, target: f32, ramp_frames: u32) {
        self.target = target;
        if ramp_frames == 0 {
            self.current = target;
            self.remaining = 0;
        } else {
            self.step = (target - self.current) / ramp_frames as f32;
            self.remaining = ramp_frames;
        }
    }

    pub fn current(&self) -> f32 {
        self.current
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            self.current = if self.remaining == 0 {
                self.target
            } else {
                self.current + self.step
            };
        }
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramps_linearly_and_lands_exactly() {
        let mut g = Smoothed::new(0.0);
        g.set(1.0, 4);
        let steps: Vec<f32> = (0..6).map(|_| g.next()).collect();
        assert_eq!(steps, [0.25, 0.5, 0.75, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn zero_ramp_jumps() {
        let mut g = Smoothed::new(0.3);
        g.set(0.8, 0);
        assert_eq!(g.next(), 0.8);
    }
}
