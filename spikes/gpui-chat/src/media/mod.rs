//! What the Media scene shows, and the pure code behind it: pictures made at
//! runtime, the conversion to GPUI's pixel format, the Lottie sticker and the
//! synthetic call video.

pub mod frames;
pub mod lottie;
pub mod synth;
pub mod video;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How many times something happened in the last second: renders of a tile,
/// shown on it so GPUI's `img()` animation can be compared with a timer.
#[derive(Default)]
pub struct RateMeter {
    ticks: RefCell<VecDeque<Instant>>,
}

impl RateMeter {
    pub fn tick(&self, now: Instant) {
        let mut ticks = self.ticks.borrow_mut();
        ticks.push_back(now);
        while ticks
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(1))
        {
            ticks.pop_front();
        }
    }

    pub fn per_second(&self, now: Instant) -> usize {
        self.ticks
            .borrow()
            .iter()
            .filter(|t| now.duration_since(**t) <= Duration::from_secs(1))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meter_counts_the_last_second() {
        let meter = RateMeter::default();
        let now = Instant::now();
        meter.tick(now - Duration::from_millis(1500));
        meter.tick(now - Duration::from_millis(900));
        meter.tick(now - Duration::from_millis(10));
        meter.tick(now);
        assert_eq!(meter.per_second(now), 3);
    }
}
