use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meter {
    Mic = 0,
    Virtual = 1,
    Monitor = 2,
}

/// Peak levels written by audio threads and read (and reset) by the control thread.
#[derive(Default)]
pub struct Meters {
    peaks: [AtomicU32; 3],
}

impl Meters {
    pub fn record(&self, meter: Meter, peak: f32) {
        // Bit patterns of non-negative floats sort like their values, so fetch_max works on the bits.
        if peak.is_finite() && peak > 0.0 {
            self.peaks[meter as usize].fetch_max(peak.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn take(&self, meter: Meter) -> f32 {
        f32::from_bits(self.peaks[meter as usize].swap(0, Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_max_until_taken() {
        let meters = Meters::default();
        meters.record(Meter::Mic, 0.2);
        meters.record(Meter::Mic, 0.7);
        meters.record(Meter::Mic, 0.4);
        assert_eq!(meters.take(Meter::Mic), 0.7);
        assert_eq!(meters.take(Meter::Mic), 0.0);
        assert_eq!(meters.take(Meter::Monitor), 0.0);
    }
}
