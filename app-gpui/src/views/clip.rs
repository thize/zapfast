//! A video file shown in a bubble: its first frame as the poster, and, while
//! it plays, the frames the core's decoder produces, paced to their own times.
//!
//! The decoder runs on its own thread (`zapfast::video::decode_in_background`)
//! and a second thread holds each frame back until it is due and hands it to
//! the entity over a channel its task awaits: while a clip is paused there is
//! no thread, no timer and no task, and while it plays the only wake-ups are
//! the frames themselves. Sound is not played yet (the clip is muted), which
//! is how GIFs are anyway.

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::AppContext as _;
use gpui::{
    Context, Entity, IntoElement, ParentElement as _, Render, RenderImage, Styled as _, Task,
    Window, div, px,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use zapfast::backend::Waker;
use zapfast::pixels::PixelImage;
use zapfast::video::{Delivery, decode_in_background};
use zapfast_gpui_media::{
    MediaPicture, MediaSource, VideoFit, VideoFrame, VideoSurface, render_image_from_bgra,
};

/// Longest side, in pixels, the frames are decoded at: twice a bubble's width,
/// for a sharp picture on a 2x display.
pub const SIDE: u32 = 600;

/// How a clip stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipState {
    /// Showing its poster.
    Paused,
    Playing,
    /// The file is not an H.264 MP4 the decoder reads.
    Unsupported,
}

/// What the pacing thread tells the entity.
enum Signal {
    Frame(VideoFrame),
    /// The last frame was shown and the clip does not loop.
    Ended,
    Unsupported,
}

/// The pacing thread of a playing clip. Dropping it stops the thread.
struct Pacer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Pacer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }
}

pub struct Clip {
    path: PathBuf,
    size: (f32, f32),
    surface: Entity<VideoSurface>,
    poster: Option<Arc<RenderImage>>,
    state: ClipState,
    looped: bool,
    pacer: Option<Pacer>,
    feed: Option<Task<()>>,
    loading_poster: Option<Task<()>>,
    frames_shown: u64,
}

impl Clip {
    /// A clip of the video at `path`, drawn in a box of `size` logical pixels
    /// with corners `radius` wide. The poster starts loading at once.
    pub fn new(
        path: impl Into<PathBuf>,
        size: (f32, f32),
        radius: f32,
        looped: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let surface = cx.new(|_| VideoSurface::new().fit(VideoFit::Cover).radius(radius));
        let mut clip = Self {
            path: path.into(),
            size,
            surface,
            poster: None,
            state: ClipState::Paused,
            looped,
            pacer: None,
            feed: None,
            loading_poster: None,
            frames_shown: 0,
        };
        clip.load_poster(cx);
        clip
    }

    pub fn state(&self) -> &ClipState {
        &self.state
    }

    /// Whether the clip plays again when it ends (a GIF).
    pub fn is_looped(&self) -> bool {
        self.looped
    }

    pub fn is_playing(&self) -> bool {
        self.state == ClipState::Playing
    }

    pub fn has_poster(&self) -> bool {
        self.poster.is_some()
    }

    /// Frames that reached the surface since the clip was made.
    pub fn frames_shown(&self) -> u64 {
        self.frames_shown
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Decodes the first frame off the main thread.
    fn load_poster(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        self.loading_poster = Some(cx.spawn(async move |this, cx| {
            let poster = cx
                .background_executor()
                .spawn(async move { first_frame(&path) })
                .await;
            this.update(cx, |this, cx| {
                this.loading_poster = None;
                if this.state == ClipState::Paused {
                    this.poster = poster;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.is_playing() {
            self.pause(cx);
        } else {
            self.play(cx);
        }
    }

    /// Plays from the start (and again from the start after it ended).
    pub fn play(&mut self, cx: &mut Context<Self>) {
        if self.is_playing() || self.state == ClipState::Unsupported {
            return;
        }
        let (sender, receiver) = unbounded();
        self.pacer = Some(spawn_pacer(self.path.clone(), self.looped, sender));
        self.feed = Some(self.listen(receiver, cx));
        self.state = ClipState::Playing;
        cx.notify();
    }

    /// Stops the decoder and goes back to the poster.
    pub fn pause(&mut self, cx: &mut Context<Self>) {
        if self.state != ClipState::Playing {
            return;
        }
        self.stop(cx);
        self.state = ClipState::Paused;
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.pacer = None;
        self.feed = None;
        self.surface.update(cx, |surface, cx| surface.clear(cx));
    }

    fn listen(&self, mut receiver: UnboundedReceiver<Signal>, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            while let Some(signal) = receiver.next().await {
                if this.update(cx, |this, cx| this.apply(signal, cx)).is_err() {
                    break;
                }
            }
        })
    }

    fn apply(&mut self, signal: Signal, cx: &mut Context<Self>) {
        match signal {
            Signal::Frame(frame) => {
                self.frames_shown += 1;
                self.surface
                    .update(cx, |surface, cx| surface.push(frame, cx));
            }
            Signal::Ended => {
                self.stop(cx);
                self.state = ClipState::Paused;
                cx.notify();
            }
            Signal::Unsupported => {
                self.stop(cx);
                self.state = ClipState::Unsupported;
                cx.notify();
            }
        }
    }
}

impl Render for Clip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let (w, h) = self.size;
        let body = div().relative().w(px(w)).h(px(h)).flex_none();
        match (&self.state, &self.poster) {
            (ClipState::Playing, _) => body.child(self.surface.clone()),
            (_, Some(poster)) => body.child(
                MediaPicture::new("clip-poster", MediaSource::Image(poster.clone()), w, h)
                    .radius(0.0),
            ),
            _ => body,
        }
    }
}

/// The first frame of the video at `path`, as a GPUI image.
pub fn first_frame(path: &Path) -> Option<Arc<RenderImage>> {
    let receiver = decode_in_background(path, Duration::ZERO, SIDE, Waker::default());
    loop {
        match receiver.recv().ok()? {
            Delivery::Frame(_, image) => {
                let frame = frame_of(&image)?;
                return render_image_from_bgra(frame.width, frame.height, frame.bgra);
            }
            Delivery::Length(_) => {}
            Delivery::End | Delivery::Unsupported(_) => return None,
        }
    }
}

/// A decoded picture as the surface takes it. Video is opaque, so the
/// premultiplied bytes are the straight ones.
fn frame_of(image: &PixelImage) -> Option<VideoFrame> {
    let bgra: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|pixel| {
            let [r, g, b, a] = pixel.to_array();
            [b, g, r, a]
        })
        .collect();
    VideoFrame::from_bgra(image.width() as u32, image.height() as u32, bgra)
}

/// Starts the decoder and a thread that releases each frame at its time.
fn spawn_pacer(path: PathBuf, looped: bool, signals: UnboundedSender<Signal>) -> Pacer {
    let stop = Arc::new(AtomicBool::new(false));
    // GPUI's test scheduler is deterministic and panics when another thread
    // wakes one of its tasks, so the entity's tests run without the thread and
    // feed `apply` themselves; `pace` is tested on its own.
    #[cfg(test)]
    if !tests::THREADS.with(std::cell::Cell::get) {
        return Pacer { stop, thread: None };
    }
    let flag = stop.clone();
    let thread = std::thread::Builder::new()
        .name("clip-pace".into())
        .spawn(move || pace(&path, looped, &flag, &signals))
        .ok();
    Pacer { stop, thread }
}

fn pace(path: &Path, looped: bool, stop: &AtomicBool, signals: &UnboundedSender<Signal>) {
    loop {
        let started = Instant::now();
        let deliveries = decode_in_background(path, Duration::ZERO, SIDE, Waker::default());
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            match deliveries.recv() {
                Ok(Delivery::Frame(at, image)) => {
                    let due = started + at;
                    while let Some(left) = due.checked_duration_since(Instant::now()) {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::park_timeout(left);
                    }
                    let Some(frame) = frame_of(&image) else {
                        continue;
                    };
                    if signals.unbounded_send(Signal::Frame(frame)).is_err() {
                        return;
                    }
                }
                Ok(Delivery::Length(_)) => {}
                Ok(Delivery::End) => break,
                Ok(Delivery::Unsupported(_)) | Err(_) => {
                    signals.unbounded_send(Signal::Unsupported).ok();
                    return;
                }
            }
        }
        if !looped {
            signals.unbounded_send(Signal::Ended).ok();
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use gpui::TestAppContext;

    thread_local! {
        /// Whether `play` starts the real threads (see `spawn_pacer`).
        pub(super) static THREADS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    fn sample() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/video/sample.mp4")
    }

    fn frame() -> VideoFrame {
        VideoFrame::from_bgra(2, 2, vec![0; 16]).expect("2x2")
    }

    #[test]
    fn the_first_frame_of_a_video_is_a_picture_and_a_missing_file_has_none() {
        assert!(first_frame(&sample()).is_some());
        assert!(first_frame(Path::new("/nonexistent.mp4")).is_none());
    }

    #[test]
    fn the_pacer_releases_frames_at_their_times_and_says_when_it_ended() {
        let (sender, mut receiver) = unbounded();
        let stop = AtomicBool::new(false);
        let started = Instant::now();
        pace(&sample(), false, &stop, &sender);
        drop(sender);
        let mut frames = 0;
        let mut last = None;
        while let Some(signal) = block_on(receiver.next()) {
            match signal {
                Signal::Frame(_) => frames += 1,
                other => last = Some(other),
            }
        }
        assert!(frames > 1, "frames: {frames}");
        assert!(matches!(last, Some(Signal::Ended)));
        // Paced to the video's own time, not decoded as fast as possible.
        assert!(started.elapsed() > Duration::from_millis(100));
    }

    #[test]
    fn the_pacer_stops_when_told_and_reports_a_file_it_cannot_play() {
        let (sender, mut receiver) = unbounded();
        let stop = AtomicBool::new(true);
        pace(&sample(), true, &stop, &sender);
        drop(sender);
        assert!(
            block_on(receiver.next()).is_none(),
            "stopped before a frame"
        );

        let (sender, mut receiver) = unbounded();
        pace(
            Path::new("/nonexistent.mp4"),
            false,
            &AtomicBool::new(false),
            &sender,
        );
        drop(sender);
        assert!(matches!(
            block_on(receiver.next()),
            Some(Signal::Unsupported)
        ));
    }

    #[gpui::test]
    fn a_clip_shows_its_first_frame_as_the_poster(cx: &mut TestAppContext) {
        let clip = cx.new(|cx| Clip::new(sample(), (200., 120.), 0., false, cx));
        cx.run_until_parked();
        assert!(clip.read_with(cx, |clip, _| clip.has_poster()));
        assert_eq!(
            clip.read_with(cx, |c, _| c.state().clone()),
            ClipState::Paused
        );
    }

    #[gpui::test]
    fn frames_reach_the_surface_and_the_end_returns_to_the_poster(cx: &mut TestAppContext) {
        let clip = cx.new(|cx| Clip::new(sample(), (200., 120.), 0., false, cx));
        cx.run_until_parked();
        clip.update(cx, |clip, cx| clip.play(cx));
        assert!(clip.read_with(cx, |c, _| c.is_playing()));
        clip.update(cx, |clip, cx| clip.apply(Signal::Frame(frame()), cx));
        assert_eq!(clip.read_with(cx, |c, _| c.frames_shown()), 1);
        clip.update(cx, |clip, cx| clip.apply(Signal::Ended, cx));
        assert_eq!(
            clip.read_with(cx, |c, _| c.state().clone()),
            ClipState::Paused
        );
        // It plays again from the start.
        clip.update(cx, |clip, cx| clip.toggle(cx));
        assert!(clip.read_with(cx, |c, _| c.is_playing()));
        clip.update(cx, |clip, cx| clip.toggle(cx));
        assert!(!clip.read_with(cx, |c, _| c.is_playing()));
    }

    #[gpui::test]
    fn a_file_the_decoder_refuses_stays_unsupported(cx: &mut TestAppContext) {
        let clip = cx.new(|cx| Clip::new("/nonexistent.mp4", (200., 120.), 0., false, cx));
        clip.update(cx, |clip, cx| clip.play(cx));
        clip.update(cx, |clip, cx| clip.apply(Signal::Unsupported, cx));
        clip.update(cx, |clip, cx| clip.play(cx));
        assert_eq!(
            clip.read_with(cx, |c, _| c.state().clone()),
            ClipState::Unsupported
        );
    }
}
