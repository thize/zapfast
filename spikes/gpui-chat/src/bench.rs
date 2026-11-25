//! Command line flags and the instrumentation the verifier relies on.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchKind {
    Idle,
    Scroll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SceneKind {
    #[default]
    Chat,
    Media,
    Platform,
}

impl SceneKind {
    pub const ALL: [SceneKind; 3] = [SceneKind::Chat, SceneKind::Media, SceneKind::Platform];

    pub fn label(self) -> &'static str {
        match self {
            SceneKind::Chat => "Chat",
            SceneKind::Media => "Media",
            SceneKind::Platform => "Platform",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            SceneKind::Chat => "chat",
            SceneKind::Media => "media",
            SceneKind::Platform => "platform",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.slug() == text)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Cli {
    pub bench: Option<(BenchKind, u64)>,
    pub snapshot: Option<PathBuf>,
    pub scene: SceneKind,
    /// Opens with this message at the top, as Cmd+J would (for snapshots).
    pub jump: Option<usize>,
    /// Forces light or dark instead of following the OS (`Some(true)` is dark).
    pub dark: Option<bool>,
}

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut cli = Cli::default();
        let mut kind = None;
        let mut seconds = 8;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--bench" => {
                    kind = Some(match args.next().as_deref() {
                        Some("idle") => BenchKind::Idle,
                        Some("scroll") => BenchKind::Scroll,
                        other => {
                            return Err(format!("--bench takes idle or scroll, not {other:?}"));
                        }
                    });
                }
                "--seconds" => {
                    seconds = args
                        .next()
                        .and_then(|n| n.parse().ok())
                        .filter(|n| *n >= 1)
                        .ok_or("--seconds takes a whole number of seconds")?;
                }
                "--snapshot" => {
                    cli.snapshot = Some(args.next().ok_or("--snapshot takes a directory")?.into());
                }
                "--scene" => {
                    cli.scene = args
                        .next()
                        .as_deref()
                        .and_then(SceneKind::parse)
                        .ok_or("--scene takes chat, media or platform")?;
                }
                "--appearance" => {
                    cli.dark = Some(match args.next().as_deref() {
                        Some("dark") => true,
                        Some("light") => false,
                        _ => return Err("--appearance takes light or dark".into()),
                    });
                }
                "--jump" => {
                    cli.jump = Some(
                        args.next()
                            .and_then(|n| n.parse().ok())
                            .ok_or("--jump takes a message index")?,
                    );
                }
                other => return Err(format!("unknown argument {other}")),
            }
        }
        cli.bench = kind.map(|kind| (kind, seconds));
        Ok(cli)
    }
}

/// GPUI redraws every view when the window gains or loses focus.
pub const ACTIVATION_GRACE: Duration = Duration::from_millis(300);

/// Counters the conversation view feeds: how often it rendered, and when each
/// frame was painted.
#[derive(Default)]
pub struct Stats {
    renders: RefCell<Vec<Instant>>,
    frames: RefCell<Vec<Instant>>,
    recording_frames: Cell<bool>,
    activations: RefCell<Vec<Instant>>,
    /// Time from `render` to the paint of the same frame, in milliseconds: the
    /// CPU cost of the frame (render, layout, prepaint, paint).
    work_ms: RefCell<Vec<f64>>,
    last_render: Cell<Option<Instant>>,
}

impl Stats {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub fn render(&self) {
        let now = Instant::now();
        self.renders.borrow_mut().push(now);
        self.last_render.set(Some(now));
    }

    pub fn frame(&self) {
        if self.recording_frames.get() {
            let now = Instant::now();
            self.frames.borrow_mut().push(now);
            if let Some(start) = self.last_render.take() {
                self.work_ms
                    .borrow_mut()
                    .push((now - start).as_secs_f64() * 1000.0);
            }
        }
    }

    /// `(p50, p95, max)` of the per-frame CPU cost, for the stderr trace.
    pub fn work_summary_ms(&self) -> (f64, f64, f64) {
        let mut work = self.work_ms.borrow().clone();
        work.sort_by(f64::total_cmp);
        (
            percentile(&work, 0.5),
            percentile(&work, 0.95),
            work.last().copied().unwrap_or(0.0),
        )
    }

    pub fn record_frames(&self, on: bool) {
        self.recording_frames.set(on);
    }

    /// Seconds since `origin` of every render, for debugging (`ZAPFAST_SPIKE_TRACE`).
    pub fn render_offsets(&self, origin: Instant) -> Vec<f64> {
        self.renders
            .borrow()
            .iter()
            .map(|t| t.duration_since(origin).as_secs_f64())
            .collect()
    }

    pub fn activation_changed(&self) {
        self.activations.borrow_mut().push(Instant::now());
    }

    /// Renders at or after `since`, not counting those within
    /// [`ACTIVATION_GRACE`] of the window gaining or losing focus.
    pub fn renders_since(&self, since: Instant) -> usize {
        let activations = self.activations.borrow();
        self.renders
            .borrow()
            .iter()
            .filter(|t| **t >= since)
            .filter(|t| {
                !activations
                    .iter()
                    .any(|a| a.max(t).duration_since(*a.min(t)) < ACTIVATION_GRACE)
            })
            .count()
    }

    /// Milliseconds between consecutive painted frames.
    pub fn frame_gaps_ms(&self) -> Vec<f64> {
        self.frames
            .borrow()
            .windows(2)
            .map(|w| (w[1] - w[0]).as_secs_f64() * 1000.0)
            .collect()
    }
}

/// The value at percentile `p` (0.0..=1.0) of ascending-sorted `sorted`,
/// by nearest rank.
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

pub fn scroll_line(gaps_ms: &[f64]) -> String {
    let mut sorted = gaps_ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    format!(
        "BENCH scroll frames={} p50_ms={:.2} p95_ms={:.2} max_ms={:.2}",
        gaps_ms.len() + usize::from(!gaps_ms.is_empty()),
        percentile(&sorted, 0.50),
        percentile(&sorted, 0.95),
        sorted.last().copied().unwrap_or(0.0),
    )
}

/// A note (printed to stderr) when the numbers are probably affected by the
/// window running unfocused: frames then come at about half the display rate.
pub fn scroll_note(gaps_ms: &[f64]) -> Option<String> {
    let mut sorted = gaps_ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    (percentile(&sorted, 0.50) > 24.0).then(|| {
        "BENCH note=frame gaps cluster near 33 ms: the window is probably unfocused or occluded, which paces frames at 30 Hz; see the CPU cost line for what the frames themselves take".to_owned()
    })
}

pub fn idle_line(renders: usize, seconds: u64) -> String {
    format!("BENCH idle renders={renders} seconds={seconds}")
}

/// Drives the scroll benchmark: a short warm-up at the bottom, then a constant
/// rate from the last row to the first.
pub struct ScrollBench {
    pub duration: Duration,
    warmup_until: Instant,
    started: Option<Instant>,
}

pub enum BenchPhase {
    Warmup,
    /// How far along, from 0.0 (bottom) to 1.0 (top).
    Running(f32),
    Done,
}

impl ScrollBench {
    pub fn new(seconds: u64) -> Self {
        Self {
            duration: Duration::from_secs(seconds),
            warmup_until: Instant::now() + Duration::from_millis(1500),
            started: None,
        }
    }

    pub fn tick(&mut self, now: Instant) -> BenchPhase {
        if now < self.warmup_until {
            return BenchPhase::Warmup;
        }
        let started = *self.started.get_or_insert(now);
        let progress = (now - started).as_secs_f32() / self.duration.as_secs_f32();
        if progress >= 1.0 {
            BenchPhase::Done
        } else {
            BenchPhase::Running(progress)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, String> {
        Cli::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn flags_parse() {
        let cli = parse(&["--bench", "scroll", "--seconds", "5"]).unwrap();
        assert_eq!(cli.bench, Some((BenchKind::Scroll, 5)));
        let cli = parse(&["--snapshot", "/tmp/x", "--scene", "media"]).unwrap();
        assert_eq!(
            cli.snapshot.as_deref(),
            Some(std::path::Path::new("/tmp/x"))
        );
        assert_eq!(cli.scene, SceneKind::Media);
        assert_eq!(parse(&[]).unwrap().bench, None);
        assert_eq!(
            parse(&["--bench", "idle"]).unwrap().bench,
            Some((BenchKind::Idle, 8))
        );
        assert!(parse(&["--bench", "x"]).is_err());
        assert!(parse(&["--seconds", "0"]).is_err());
        assert!(parse(&["--nope"]).is_err());
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let data = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert_eq!(percentile(&data, 0.5), 5.0);
        assert_eq!(percentile(&data, 0.95), 10.0);
        assert_eq!(percentile(&data, 0.0), 1.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
    }

    #[test]
    fn the_bench_lines_have_the_documented_shape() {
        assert_eq!(idle_line(0, 8), "BENCH idle renders=0 seconds=8");
        let line = scroll_line(&[16.0, 17.0, 33.0]);
        assert_eq!(
            line,
            "BENCH scroll frames=4 p50_ms=17.00 p95_ms=33.00 max_ms=33.00"
        );
    }

    #[test]
    fn the_scroll_bench_warms_up_then_runs_then_finishes() {
        let mut bench = ScrollBench::new(2);
        let start = Instant::now();
        assert!(matches!(bench.tick(start), BenchPhase::Warmup));
        let after = start + Duration::from_millis(1600);
        assert!(matches!(bench.tick(after), BenchPhase::Running(p) if p < 0.01));
        assert!(
            matches!(bench.tick(after + Duration::from_secs(1)), BenchPhase::Running(p) if (0.45..0.55).contains(&p))
        );
        assert!(matches!(
            bench.tick(after + Duration::from_secs(3)),
            BenchPhase::Done
        ));
    }
}
