//! Measuring the synthetic call video: how many frames reached the screen, at
//! what rate, and what each cost between `render` and the end of `paint`.

use crate::bench::percentile;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct VideoStats {
    /// When each video frame was painted, and what its render-to-paint took.
    painted: RefCell<Vec<(Instant, f64)>>,
    /// What generating each frame (off the main thread) took, in ms.
    generated_ms: RefCell<Vec<f64>>,
    pushed: Cell<u64>,
}

/// What the benchmark reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    pub frames: usize,
    pub fps: f64,
    /// 95th percentile of the render-to-paint cost of a frame, in ms.
    pub p95_ms: f64,
    pub avg_ms: f64,
    /// 50th and 95th percentile of the gap between painted frames, in ms.
    pub gap_p50_ms: f64,
    pub gap_p95_ms: f64,
    pub avg_generate_ms: f64,
    /// Frames made but replaced before they were painted.
    pub dropped: u64,
}

impl VideoStats {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub fn generated(&self, ms: f64) {
        self.pushed.set(self.pushed.get() + 1);
        self.generated_ms.borrow_mut().push(ms);
    }

    pub fn painted(&self, at: Instant, cost_ms: f64) {
        self.painted.borrow_mut().push((at, cost_ms));
    }

    /// Seconds since `origin` of every painted frame (`ZAPFAST_SPIKE_TRACE`).
    pub fn offsets(&self, origin: Instant) -> Vec<f64> {
        self.painted
            .borrow()
            .iter()
            .map(|(at, _)| at.duration_since(origin).as_secs_f64())
            .collect()
    }

    /// Frames painted in the second before `now` and their mean cost, for the
    /// numbers on screen.
    pub fn live(&self, now: Instant) -> (usize, f64) {
        let painted = self.painted.borrow();
        let recent: Vec<f64> = painted
            .iter()
            .rev()
            .take_while(|(at, _)| now.duration_since(*at) <= Duration::from_secs(1))
            .map(|(_, cost)| *cost)
            .collect();
        let mean = if recent.is_empty() {
            0.0
        } else {
            recent.iter().sum::<f64>() / recent.len() as f64
        };
        (recent.len(), mean)
    }

    /// The summary of the frames painted at or after `since`, over the time
    /// from the first of them to `until`.
    pub fn summary(&self, since: Instant, until: Instant) -> Summary {
        let painted = self.painted.borrow();
        let window: Vec<&(Instant, f64)> = painted
            .iter()
            .filter(|(at, _)| *at >= since && *at <= until)
            .collect();
        let mut costs: Vec<f64> = window.iter().map(|(_, c)| *c).collect();
        costs.sort_by(f64::total_cmp);
        let mut gaps: Vec<f64> = window
            .windows(2)
            .map(|w| (w[1].0 - w[0].0).as_secs_f64() * 1000.0)
            .collect();
        gaps.sort_by(f64::total_cmp);
        let seconds = window
            .first()
            .map_or(0.0, |(first, _)| until.duration_since(*first).as_secs_f64());
        let generated = self.generated_ms.borrow();
        Summary {
            frames: window.len(),
            fps: if seconds > 0.0 {
                window.len() as f64 / seconds
            } else {
                0.0
            },
            p95_ms: percentile(&costs, 0.95),
            avg_ms: mean(&costs),
            gap_p50_ms: percentile(&gaps, 0.5),
            gap_p95_ms: percentile(&gaps, 0.95),
            avg_generate_ms: mean(&generated),
            dropped: self.pushed.get().saturating_sub(painted.len() as u64),
        }
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

pub fn bench_line(summary: &Summary) -> String {
    format!(
        "BENCH video frames={} fps={:.1} p95_ms={:.2}",
        summary.frames, summary.fps, summary.p95_ms
    )
}

/// The detail printed to stderr beside the BENCH line.
pub fn bench_detail(summary: &Summary) -> String {
    format!(
        "video: render-to-paint avg={:.2} ms p95={:.2} ms; gap between painted frames p50={:.1} ms p95={:.1} ms; generating a frame (background thread) avg={:.2} ms; frames made but never painted: {}",
        summary.avg_ms,
        summary.p95_ms,
        summary.gap_p50_ms,
        summary.gap_p95_ms,
        summary.avg_generate_ms,
        summary.dropped
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_counts_rates_and_percentiles() {
        let stats = VideoStats::new();
        let start = Instant::now();
        for i in 0..30u64 {
            stats.generated(1.0);
            stats.painted(start + Duration::from_millis(i * 33), 2.0 + (i % 10) as f64);
        }
        // One generated frame never reached the screen.
        stats.generated(1.0);
        let summary = stats.summary(start, start + Duration::from_secs(1));
        assert_eq!(summary.frames, 30);
        assert!((summary.fps - 30.0).abs() < 0.1, "{}", summary.fps);
        assert_eq!(summary.p95_ms, 11.0);
        assert_eq!(summary.gap_p50_ms.round(), 33.0);
        assert_eq!(summary.dropped, 1);
        assert_eq!(
            bench_line(&summary),
            "BENCH video frames=30 fps=30.0 p95_ms=11.00"
        );
        // Frames before `since` are not counted.
        let later = stats.summary(
            start + Duration::from_millis(500),
            start + Duration::from_secs(1),
        );
        assert!(later.frames < 30);
    }

    #[test]
    fn live_numbers_cover_the_last_second() {
        let stats = VideoStats::new();
        let now = Instant::now();
        stats.painted(now - Duration::from_millis(2500), 50.0);
        stats.painted(now - Duration::from_millis(400), 4.0);
        stats.painted(now - Duration::from_millis(100), 2.0);
        let (fps, mean) = stats.live(now);
        assert_eq!(fps, 2);
        assert_eq!(mean, 3.0);
        assert_eq!(VideoStats::new().live(now), (0, 0.0));
    }
}
