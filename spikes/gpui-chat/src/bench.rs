//! Command line flags and the instrumentation the verifier relies on.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchKind {
    Idle,
    Scroll,
    Video,
}

/// Which notification API a probe uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyBackend {
    Gpui,
    Fastframe,
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
    /// Writes the accessibility tree (`Window::debug_a11y_tree_json`) to this
    /// file on every frame while a screen reader or an AX client is attached.
    pub a11y_dump: Option<PathBuf>,
    /// Posts a test notification through one backend, prints what happened as
    /// `PROBE notify ...` lines, and quits.
    pub probe_notify: Option<NotifyBackend>,
    /// Shows a Dock badge of 3 for ten seconds (a regular, unfocused app, so
    /// the Dock has a tile to put it on), then quits.
    pub probe_badge: bool,
    /// `--bench scroll` takes its frames from the display link, as an
    /// interactive run does, instead of drawing them back to back.
    pub paced: bool,
    /// Minimizes the window right after opening it: the platform then reports
    /// it `Hidden` and delivers no frames, which is how a window covered by
    /// other windows behaves. For checking that the benchmarks do not depend
    /// on the window being shown.
    pub minimize: bool,
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
                        Some("video") => BenchKind::Video,
                        other => {
                            return Err(format!(
                                "--bench takes idle, scroll or video, not {other:?}"
                            ));
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
                "--a11y-dump" => {
                    cli.a11y_dump = Some(args.next().ok_or("--a11y-dump takes a file")?.into());
                }
                "--probe" => match args.next().as_deref() {
                    Some("badge") => cli.probe_badge = true,
                    Some("notify-gpui") => cli.probe_notify = Some(NotifyBackend::Gpui),
                    Some("notify-fastframe") => cli.probe_notify = Some(NotifyBackend::Fastframe),
                    other => {
                        return Err(format!(
                            "--probe takes notify-gpui, notify-fastframe or badge, not {other:?}"
                        ));
                    }
                },
                "--paced" => cli.paced = true,
                "--minimize" => cli.minimize = true,
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
    /// When each frame painted, and when each began (its render), recorded
    /// only while the benchmark runs.
    frames: RefCell<Vec<Instant>>,
    starts: RefCell<Vec<Instant>>,
    /// Frames the benchmark had to draw itself because the platform delivered
    /// none (the window was hidden), see `pump`.
    pumped: Cell<usize>,
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
        if self.recording_frames.get() {
            self.starts.borrow_mut().push(now);
        }
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

    /// Milliseconds between the starts of consecutive frames (their renders).
    ///
    /// A frame starts when the display link ticks, so these gaps are the
    /// display's cadence plus the scheduler's jitter. The gaps between paints
    /// ([`Self::paint_gaps_ms`]) add the frame's own cost variation on top: a
    /// frame that takes 7 ms followed by one that takes 10 ms puts their paints
    /// 3 ms further apart than a vsync, with no frame dropped.
    pub fn frame_gaps_ms(&self) -> Vec<f64> {
        gaps_ms(&self.starts.borrow())
    }

    /// Milliseconds between consecutive painted frames.
    pub fn paint_gaps_ms(&self) -> Vec<f64> {
        gaps_ms(&self.frames.borrow())
    }

    /// One more frame drawn by the benchmark itself.
    pub fn pumped(&self) {
        self.pumped.set(self.pumped.get() + 1);
    }

    pub fn pumped_frames(&self) -> usize {
        self.pumped.get()
    }
}

fn gaps_ms(times: &[Instant]) -> Vec<f64> {
    times
        .windows(2)
        .map(|w| (w[1] - w[0]).as_secs_f64() * 1000.0)
        .collect()
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

/// A note (printed to stderr) when the frames came slower than 30 Hz: a paced
/// run whose window is covered, on another Space or on a sleeping display is
/// not given frames at the display's rate (or at all).
pub fn scroll_note(gaps_ms: &[f64]) -> Option<String> {
    let mut sorted = gaps_ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    (percentile(&sorted, 0.50) > 33.0).then(|| {
        "BENCH note=frame gaps above 33 ms: the platform is not delivering frames at 30 Hz (a hidden window, a busy machine or a low-power display policy); see the CPU cost line for what the frames themselves take".to_owned()
    })
}

pub fn idle_line(renders: usize, seconds: u64) -> String {
    format!("BENCH idle renders={renders} seconds={seconds}")
}

/// Frames per second a 60 Hz display shows: `--seconds N` is `N * 60` frames.
pub const FRAMES_PER_SECOND: u64 = 60;

/// Drives the scroll benchmark: a short warm-up at the bottom, then a constant
/// number of rows per frame from the last row to the first (the same 21 rows
/// a frame that a 60 Hz run scrolls), so the work per frame does not depend on
/// how fast frames come.
pub struct ScrollBench {
    total_frames: u64,
    frame: u64,
    /// Frames come from the display link (`--paced`) rather than from the
    /// benchmark's own loop.
    pub paced: bool,
    warmup_until: Instant,
}

pub enum BenchPhase {
    Warmup,
    /// How far along, from 0.0 (bottom) to 1.0 (top).
    Running(f32),
    Done,
}

impl ScrollBench {
    pub fn new(seconds: u64, paced: bool) -> Self {
        Self::with_warmup(seconds, paced, Duration::from_millis(1500))
    }

    pub fn with_warmup(seconds: u64, paced: bool, warmup: Duration) -> Self {
        Self {
            total_frames: seconds * FRAMES_PER_SECOND,
            frame: 0,
            paced,
            warmup_until: Instant::now() + warmup,
        }
    }

    /// Called once per frame.
    pub fn tick(&mut self, now: Instant) -> BenchPhase {
        if now < self.warmup_until {
            return BenchPhase::Warmup;
        }
        if self.frame >= self.total_frames {
            return BenchPhase::Done;
        }
        let progress = self.frame as f32 / self.total_frames as f32;
        self.frame += 1;
        BenchPhase::Running(progress)
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
        assert!(!parse(&["--bench", "scroll"]).unwrap().paced);
        assert!(parse(&["--bench", "scroll", "--paced"]).unwrap().paced);
        assert_eq!(
            parse(&["--bench", "idle"]).unwrap().bench,
            Some((BenchKind::Idle, 8))
        );
        assert_eq!(
            parse(&["--bench", "video", "--seconds", "3"])
                .unwrap()
                .bench,
            Some((BenchKind::Video, 3))
        );
        assert_eq!(
            parse(&["--probe", "notify-gpui"]).unwrap().probe_notify,
            Some(NotifyBackend::Gpui)
        );
        assert_eq!(
            parse(&["--a11y-dump", "/tmp/a.json"]).unwrap().a11y_dump,
            Some(PathBuf::from("/tmp/a.json"))
        );
        assert!(parse(&["--probe", "x"]).is_err());
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
    fn the_scroll_bench_warms_up_then_runs_a_fixed_number_of_frames() {
        let mut bench = ScrollBench::new(2, false);
        let start = Instant::now();
        assert!(matches!(bench.tick(start), BenchPhase::Warmup));
        let after = start + Duration::from_millis(1600);
        let mut last = -1.0;
        for _ in 0..120 {
            match bench.tick(after) {
                BenchPhase::Running(p) => {
                    assert!(p > last && p < 1.0);
                    last = p;
                }
                _ => panic!("two seconds are 120 frames"),
            }
        }
        assert!(last > 0.99);
        assert!(matches!(bench.tick(after), BenchPhase::Done));
        assert!(matches!(bench.tick(after), BenchPhase::Done));
    }
}
