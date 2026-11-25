//! A surface for decoded video frames.
//!
//! The decoder runs on a thread of its own and sends [`VideoFrame`]s through a
//! channel. [`VideoSurface::attach`] awaits that channel on GPUI's executor (the
//! decoder's send wakes it: no polling, no timer) and shows the newest frame,
//! dropping the ones that piled up while the window was busy. Each frame is a
//! new `RenderImage`, and the previous one is released from the GPU atlas.

use crate::frames::render_image_from_bgra;
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    Context, IntoElement, ObjectFit, ParentElement as _, Render, RenderImage, Styled as _,
    StyledImage as _, Task, Window, canvas, div, img, px,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// One decoded frame, BGRA (GPUI's order), `width * height * 4` bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl VideoFrame {
    /// `None` when the bytes do not match the size.
    pub fn from_bgra(width: u32, height: u32, bgra: Vec<u8>) -> Option<Self> {
        (bgra.len() == width as usize * height as usize * 4).then_some(Self {
            width,
            height,
            bgra,
        })
    }

    /// From opaque RGBA (a decoder's usual output): swaps red and blue.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Self> {
        let mut bgra = rgba.to_vec();
        for pixel in bgra.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        Self::from_bgra(width, height, bgra)
    }
}

/// How a frame fills the surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VideoFit {
    /// The whole frame, letterboxed.
    #[default]
    Contain,
    /// The surface filled, the frame cropped (a round video message).
    Cover,
}

pub struct VideoSurface {
    image: Option<Arc<RenderImage>>,
    /// Frames shown since the surface was made; paint records a frame only
    /// when this moved.
    sequence: u64,
    recorded: Rc<Cell<u64>>,
    fit: VideoFit,
    /// Corner radius of the surface and its picture, in logical pixels.
    radius: f32,
    feed: Option<Task<()>>,
    stats: Option<Rc<VideoStats>>,
    /// Frames that arrived while a newer one was already waiting.
    coalesced: u64,
}

impl Default for VideoSurface {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoSurface {
    pub fn new() -> Self {
        Self {
            image: None,
            sequence: 0,
            recorded: Rc::default(),
            fit: VideoFit::Contain,
            radius: 0.0,
            feed: None,
            stats: None,
            coalesced: 0,
        }
    }

    /// Cover-crop instead of letterboxing (a round video message).
    pub fn fit(mut self, fit: VideoFit) -> Self {
        self.fit = fit;
        self
    }

    /// Rounds the corners (a round video message takes half its side).
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// Records every painted frame's render-to-paint cost.
    pub fn with_stats(mut self, stats: Rc<VideoStats>) -> Self {
        self.stats = Some(stats);
        self
    }

    pub fn has_frame(&self) -> bool {
        self.image.is_some()
    }

    pub fn is_attached(&self) -> bool {
        self.feed.is_some()
    }

    pub fn frames_shown(&self) -> u64 {
        self.sequence
    }

    /// Frames dropped because a newer one was already waiting.
    pub fn frames_coalesced(&self) -> u64 {
        self.coalesced
    }

    /// Shows frames from `receiver` until it is closed, [`detach`](Self::detach)ed
    /// or the surface is dropped. Replaces a previous feed.
    pub fn attach(&mut self, mut receiver: UnboundedReceiver<VideoFrame>, cx: &mut Context<Self>) {
        self.feed = Some(cx.spawn(async move |this, cx| {
            while let Some(mut frame) = receiver.next().await {
                // Only the newest frame is worth uploading.
                let mut skipped = 0;
                while let Ok(newer) = receiver.try_recv() {
                    frame = newer;
                    skipped += 1;
                }
                let shown = this.update(cx, |this, cx| {
                    this.coalesced += skipped;
                    this.push(frame, cx);
                });
                if shown.is_err() {
                    break;
                }
            }
            this.update(cx, |this, _| this.feed = None).ok();
        }));
    }

    /// Stops listening; the last frame stays until [`clear`](Self::clear).
    pub fn detach(&mut self) {
        self.feed = None;
    }

    /// Shows one frame now. A new image each time, so the GPU atlas takes it
    /// up on the next paint; the previous one is released.
    pub fn push(&mut self, frame: VideoFrame, cx: &mut Context<Self>) {
        if let Some(image) = render_image_from_bgra(frame.width, frame.height, frame.bgra) {
            self.show(image, cx);
        }
    }

    /// Shows a picture that is already a GPUI image.
    pub fn show(&mut self, image: Arc<RenderImage>, cx: &mut Context<Self>) {
        self.sequence += 1;
        if let Some(previous) = self.image.replace(image) {
            cx.drop_image(previous, None);
        }
        cx.notify();
    }

    /// Drops the picture (and its atlas entry): a paused or ended video.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if let Some(image) = self.image.take() {
            cx.drop_image(image, None);
            cx.notify();
        }
    }
}

impl Render for VideoSurface {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = Instant::now();
        let stats = self.stats.clone();
        let recorded = self.recorded.clone();
        let sequence = self.sequence;
        let fit = self.fit;
        let radius = px(self.radius);
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded(radius)
            .bg(gpui::black())
            .when_some(self.image.clone(), |surface, image| {
                surface.child(
                    img(image)
                        .size_full()
                        .rounded(radius)
                        .object_fit(match fit {
                            VideoFit::Contain => ObjectFit::Contain,
                            VideoFit::Cover => ObjectFit::Cover,
                        }),
                )
            })
            // Painted after the picture: the time from this render to the end
            // of its own paint is the frame's cost (render, layout, the atlas
            // upload inside `paint_image`, paint).
            .when_some(stats, |surface, stats| {
                surface.child(
                    canvas(
                        |_, _, _| (),
                        move |_, _, _, _| {
                            if sequence != 0 && recorded.replace(sequence) != sequence {
                                let now = Instant::now();
                                stats.painted(now, (now - render_started).as_secs_f64() * 1000.0);
                            }
                        },
                    )
                    .absolute()
                    .size(px(1.)),
                )
            })
    }
}

use gpui::prelude::FluentBuilder as _;

/// Measuring a video surface: how many frames reached the screen, at what
/// rate, and what each cost between `render` and the end of `paint`.
#[derive(Default)]
pub struct VideoStats {
    /// When each video frame was painted, and what its render-to-paint took.
    painted: RefCell<Vec<(Instant, f64)>>,
    /// What producing each frame (off the main thread) took, in ms; the
    /// producer reports it with [`generated`](Self::generated).
    generated_ms: RefCell<Vec<f64>>,
    pushed: Cell<u64>,
}

/// What a benchmark reports.
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

    /// Frames painted in the second before `now` and their mean cost.
    pub fn live(&self, now: Instant) -> (usize, f64) {
        let painted = self.painted.borrow();
        let recent: Vec<f64> = painted
            .iter()
            .rev()
            .take_while(|(at, _)| now.duration_since(*at) <= Duration::from_secs(1))
            .map(|(_, cost)| *cost)
            .collect();
        (recent.len(), mean(&recent))
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

/// The value at percentile `p` (0.0..=1.0) of ascending-sorted `sorted`, by
/// nearest rank.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// The line a benchmark prints.
pub fn bench_line(summary: &Summary) -> String {
    format!(
        "BENCH video frames={} fps={:.1} p95_ms={:.2}",
        summary.frames, summary.fps, summary.p95_ms
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::mpsc::unbounded;
    use gpui::{AppContext as _, TestAppContext};

    fn frame(shade: u8) -> VideoFrame {
        VideoFrame::from_bgra(4, 2, vec![shade; 4 * 2 * 4]).expect("a 4x2 frame")
    }

    #[test]
    fn frames_check_their_size_and_swap_rgba() {
        assert!(VideoFrame::from_bgra(2, 2, vec![0; 15]).is_none());
        let frame = VideoFrame::from_rgba(1, 1, &[1, 2, 3, 255]).expect("one pixel");
        assert_eq!(frame.bgra, vec![3, 2, 1, 255]);
    }

    #[gpui::test]
    fn the_surface_shows_pushed_frames_and_releases_them(cx: &mut TestAppContext) {
        let surface = cx.new(|_| VideoSurface::new());
        surface.update(cx, |s, cx| s.push(frame(10), cx));
        surface.update(cx, |s, cx| s.push(frame(20), cx));
        assert!(surface.read_with(cx, |s, _| s.has_frame()));
        assert_eq!(surface.read_with(cx, |s, _| s.frames_shown()), 2);
        surface.update(cx, |s, cx| s.clear(cx));
        assert!(!surface.read_with(cx, |s, _| s.has_frame()));
    }

    #[gpui::test]
    fn an_attached_feed_shows_the_newest_frame_and_ends_with_the_channel(cx: &mut TestAppContext) {
        let surface = cx.new(|_| VideoSurface::new());
        let (sender, receiver) = unbounded();
        surface.update(cx, |s, cx| s.attach(receiver, cx));
        assert!(surface.read_with(cx, |s, _| s.is_attached()));
        // Three frames queued before the executor runs: only the last is shown.
        for shade in [1, 2, 3] {
            sender
                .unbounded_send(frame(shade))
                .expect("the feed is open");
        }
        cx.run_until_parked();
        assert_eq!(surface.read_with(cx, |s, _| s.frames_shown()), 1);
        assert_eq!(surface.read_with(cx, |s, _| s.frames_coalesced()), 2);
        // A later frame is shown on its own, with no polling in between.
        sender.unbounded_send(frame(4)).expect("the feed is open");
        cx.run_until_parked();
        assert_eq!(surface.read_with(cx, |s, _| s.frames_shown()), 2);
        drop(sender);
        cx.run_until_parked();
        assert!(!surface.read_with(cx, |s, _| s.is_attached()));
        assert!(surface.read_with(cx, |s, _| s.has_frame()));
    }

    #[gpui::test]
    fn detaching_stops_the_feed(cx: &mut TestAppContext) {
        let surface = cx.new(|_| VideoSurface::new());
        let (sender, receiver) = unbounded();
        surface.update(cx, |s, cx| s.attach(receiver, cx));
        surface.update(cx, |s, _| s.detach());
        // The receiver went with the task: the producer learns the feed is gone.
        cx.run_until_parked();
        assert!(sender.unbounded_send(frame(1)).is_err());
        assert_eq!(surface.read_with(cx, |s, _| s.frames_shown()), 0);
    }

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
