//! Lottie stickers: rasterized once on a background thread with ZapFast's
//! rasterizer (`rasterlottie` 0.2.4, `images` and `text` only, as in the root
//! package's `src/lottie.rs`), then played by one timer that moves a frame
//! index at the animation's own rate. The timer exists only while the player is
//! active; an inactive player shows its first frame and schedules nothing.
//!
//! WhatsApp sends these as ZIP-based `.was` archives holding the animation's
//! JSON, never extracted to disk, with the same size caps as the root package's
//! reader ([`LottieLimits`]).

use crate::frames::{premultiplied_rgba_to_bgra, render_image};
use gpui::{
    Bounds, Context, Corners, IntoElement, ParentElement as _, Render, RenderImage, SharedString,
    Styled as _, Task, Window, canvas, div, point, px, size,
};
use image::RgbaImage;
use rasterlottie::{Animation, RenderConfig, Renderer, Rgba8};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What a sticker may cost. The defaults are the root package's caps, with a
/// frame rate and a frame budget a little above its egui player's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LottieLimits {
    pub max_archive_bytes: u64,
    pub max_json_bytes: u64,
    pub max_canvas_side: u32,
    pub max_seconds: f32,
    pub max_fps: f32,
    pub max_frames: usize,
    /// The longest side of a rendered frame, in pixels (twice a 180 pt sticker).
    pub max_output_side: f32,
}

impl Default for LottieLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 4 * 1024 * 1024,
            max_json_bytes: 2 * 1024 * 1024,
            max_canvas_side: 1024,
            max_seconds: 15.0,
            max_fps: 30.0,
            max_frames: 90,
            max_output_side: 360.0,
        }
    }
}

/// Where the animation comes from.
#[derive(Clone, Debug)]
pub enum LottieSource {
    /// The animation's JSON.
    Json(Arc<str>),
    /// A WhatsApp `.was` archive on disk.
    Was(PathBuf),
}

/// A rendered clip: straight-alpha BGRA frames, as GPUI paints them.
pub struct Clip {
    pub frames: Vec<RgbaImage>,
    /// How long each frame is shown.
    pub frame_interval: Duration,
    /// Whether these are all the animation's frames (not only its poster).
    pub complete: bool,
}

/// Reads just the animation JSON out of a `.was` archive: ignores metadata and
/// trust tokens, never extracts a member onto disk, and the size caps also
/// bound the decompression.
pub fn animation_json_from_was(path: &Path, limits: &LottieLimits) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > limits.max_archive_bytes {
        return None;
    }
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut fallback = None;
    let mut preferred = None;
    for (index, name) in archive.file_names().enumerate() {
        // A name zip cannot read is no animation: its neighbours still count.
        let Ok(name) = name else { continue };
        if name == "animation.json" || name.ends_with("/animation.json") {
            preferred = Some(index);
            break;
        }
        if name.ends_with(".json") && !name.contains("metadata") && !name.contains("trust_token") {
            fallback.get_or_insert(index);
        }
    }
    let entry = archive.by_index(preferred.or(fallback)?).ok()?;
    if entry.size() > limits.max_json_bytes {
        return None;
    }
    let mut data = Vec::with_capacity(entry.size() as usize);
    entry
        .take(limits.max_json_bytes + 1)
        .read_to_end(&mut data)
        .ok()?;
    if data.len() as u64 > limits.max_json_bytes {
        return None;
    }
    String::from_utf8(data).ok()
}

/// Rasterizes at most `requested` frames of `json`, spread evenly over the
/// animation. One requested frame is the poster; the rest are rendered only for
/// playback.
pub fn render(json: &str, limits: &LottieLimits, requested: usize) -> Result<Clip, String> {
    if requested == 0 {
        return Err("no frames requested".into());
    }
    let animation = Animation::from_json_str(json).map_err(|e| e.to_string())?;
    if animation.width == 0
        || animation.height == 0
        || animation.width > limits.max_canvas_side
        || animation.height > limits.max_canvas_side
        || !animation.frame_rate.is_finite()
        || animation.frame_rate <= 0.0
        || !animation.in_point.is_finite()
        || !animation.out_point.is_finite()
    {
        return Err("the animation's size or frame rate is out of bounds".into());
    }
    let seconds = animation.duration_seconds();
    if !seconds.is_finite() || seconds <= 0.0 || seconds > limits.max_seconds {
        return Err("the animation's duration is out of bounds".into());
    }
    // The prepared renderer caches tessellation and reusable layer geometry:
    // repreparing each frame would be expensive for a complicated sticker.
    let prepared = Renderer::default()
        .prepare(&animation)
        .map_err(|e| e.to_string())?;
    let longest = animation.width.max(animation.height) as f32;
    let scale = (limits.max_output_side / longest).clamp(0.01, 4.0);
    let config = RenderConfig::new(Rgba8::TRANSPARENT, scale);
    let fps = animation.frame_rate.min(limits.max_fps);
    let total = ((seconds * fps).ceil() as usize).clamp(1, limits.max_frames);
    let count = total.min(requested);
    let mut frames = Vec::with_capacity(count);
    for index in 0..count {
        let at = animation.in_point + index as f32 * animation.duration_frames() / total as f32;
        let raster = prepared
            .render_frame(at, config)
            .map_err(|e| e.to_string())?;
        let bgra = premultiplied_rgba_to_bgra(&raster.pixels);
        frames.push(
            RgbaImage::from_raw(raster.width, raster.height, bgra)
                .ok_or("the raster has the wrong size")?,
        );
    }
    Ok(Clip {
        frames,
        frame_interval: Duration::from_secs_f32(seconds / total as f32)
            .max(Duration::from_millis(20)),
        complete: count == total,
    })
}

fn decode(source: &LottieSource, limits: &LottieLimits, requested: usize) -> Result<Clip, String> {
    match source {
        LottieSource::Json(json) => {
            if json.len() as u64 > limits.max_json_bytes {
                return Err("the animation is too large".into());
            }
            render(json, limits, requested)
        }
        LottieSource::Was(path) => {
            let json = animation_json_from_was(path, limits)
                .ok_or("the archive holds no readable animation")?;
            render(&json, limits, requested)
        }
    }
}

/// Where a `w` x `h` picture goes in a box of `avail_w` x `avail_h` to be
/// contained: `(x, y, width, height)`, centred.
pub fn contain(avail_w: f32, avail_h: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    if w <= 0.0 || h <= 0.0 {
        return (0.0, 0.0, avail_w, avail_h);
    }
    let scale = (avail_w / w).min(avail_h / h);
    let (fw, fh) = (w * scale, h * scale);
    ((avail_w - fw) / 2.0, (avail_h - fh) / 2.0, fw, fh)
}

/// What the player has to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LottieState {
    /// Nothing loaded yet.
    Empty,
    Loading,
    Ready,
    Failed(String),
}

pub struct LottiePlayer {
    source: Option<LottieSource>,
    limits: LottieLimits,
    clip: Option<Arc<RenderImage>>,
    /// Whether `clip` holds every frame, not only the poster.
    complete: bool,
    interval: Duration,
    index: usize,
    loading: Option<Task<()>>,
    timer: Option<Task<()>>,
    active: bool,
    error: Option<String>,
}

impl Default for LottiePlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl LottiePlayer {
    pub fn new() -> Self {
        Self::with_limits(LottieLimits::default())
    }

    pub fn with_limits(limits: LottieLimits) -> Self {
        Self {
            source: None,
            limits,
            clip: None,
            complete: false,
            interval: Duration::from_millis(33),
            index: 0,
            loading: None,
            timer: None,
            active: false,
            error: None,
        }
    }

    pub fn state(&self) -> LottieState {
        if let Some(error) = &self.error {
            LottieState::Failed(error.clone())
        } else if self.clip.is_some() {
            LottieState::Ready
        } else if self.loading.is_some() {
            LottieState::Loading
        } else {
            LottieState::Empty
        }
    }

    /// Whether the frame timer is running (the idle tests ask).
    pub fn is_running(&self) -> bool {
        self.timer.is_some()
    }

    pub fn frame_index(&self) -> usize {
        self.index
    }

    pub fn frame_count(&self) -> usize {
        self.clip.as_ref().map_or(0, |clip| clip.frame_count())
    }

    /// Whether every frame is rendered (the poster alone is not).
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Loads a sticker, replacing the previous one. While inactive only its
    /// first frame is rendered.
    pub fn load(&mut self, source: LottieSource, cx: &mut Context<Self>) {
        self.source = Some(source);
        self.clip = None;
        self.complete = false;
        self.index = 0;
        self.error = None;
        self.timer = None;
        self.loading = None;
        self.begin(cx);
        cx.notify();
    }

    /// Starts or stops the animation (a sticker scrolls into view or out of it,
    /// the window is covered). Stopping leaves the current frame showing.
    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        if !active {
            self.timer = None;
            return;
        }
        if self.clip.is_some() && self.complete {
            if self.timer.is_none() {
                self.start(cx);
            }
        } else {
            self.begin(cx);
        }
    }

    /// Renders the clip off the main thread: every frame when active, the
    /// poster otherwise.
    fn begin(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.source.clone() else {
            return;
        };
        if self.loading.is_some() {
            return;
        }
        let requested = if self.active {
            self.limits.max_frames
        } else {
            1
        };
        let limits = self.limits;
        self.loading = Some(cx.spawn(async move |this, cx| {
            let rendered = cx
                .background_executor()
                .spawn(async move { decode(&source, &limits, requested) })
                .await;
            this.update(cx, |this, cx| {
                this.loading = None;
                match rendered {
                    Ok(clip) => {
                        let interval = clip.frame_interval;
                        this.interval = interval;
                        this.complete = clip.complete;
                        this.index = 0;
                        this.error = None;
                        this.clip = Some(render_image(
                            clip.frames.into_iter().map(|f| (f, interval)).collect(),
                        ));
                        if this.active {
                            if this.complete {
                                this.start(cx);
                            } else {
                                // Activated while the poster was rendering.
                                this.begin(cx);
                            }
                        }
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Advances the frame at the animation's rate. The index follows the clock,
    /// so a late tick skips frames instead of falling behind.
    fn start(&mut self, cx: &mut Context<Self>) {
        let Some(clip) = self.clip.clone() else {
            return;
        };
        let count = clip.frame_count();
        if count <= 1 {
            return;
        }
        let fps = 1.0 / self.interval.as_secs_f64();
        self.timer = Some(cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let mut last = usize::MAX;
            loop {
                let next = (started.elapsed().as_secs_f64() * fps).floor() as u64 + 1;
                let due = started + Duration::from_secs_f64(next as f64 / fps);
                cx.background_executor()
                    .timer(due.saturating_duration_since(Instant::now()))
                    .await;
                let index = (next as usize) % count;
                if index == last {
                    continue;
                }
                last = index;
                if this
                    .update(cx, |this, cx| {
                        this.index = index;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
}

impl Render for LottiePlayer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self.clip.clone().map(|clip| (clip, self.index));
        let tile = div().relative().size_full();
        match frame {
            Some((clip, index)) => tile
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            let picture = clip.size(index.min(clip.frame_count() - 1));
                            let (x, y, w, h) = contain(
                                f32::from(bounds.size.width),
                                f32::from(bounds.size.height),
                                picture.width.0 as f32,
                                picture.height.0 as f32,
                            );
                            let target = Bounds::new(
                                point(bounds.origin.x + px(x), bounds.origin.y + px(y)),
                                size(px(w), px(h)),
                            );
                            let _ = window.paint_image(
                                target,
                                target,
                                Corners::default(),
                                clip.clone(),
                                index.min(clip.frame_count() - 1),
                                false,
                            );
                        },
                    )
                    .size_full(),
                )
                .into_any_element(),
            None => tile
                .when_some(self.error.clone(), |tile, error| {
                    tile.child(SharedString::from(error))
                })
                .into_any_element(),
        }
    }
}

use gpui::prelude::FluentBuilder as _;

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};
    use std::io::Write as _;

    const BOUNCE: &str = include_str!("../testdata/bounce.json");

    fn archive(path: &Path, entries: &[(&str, &str)]) {
        let file = std::fs::File::create(path).expect("archive file");
        let mut zip = zip::ZipWriter::new(file);
        for (name, body) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("entry");
            zip.write_all(body.as_bytes()).expect("write");
        }
        zip.finish().expect("finish");
    }

    /// Lets the background render finish: the test executor does not run real
    /// threads by itself.
    fn settle(
        cx: &mut TestAppContext,
        player: &gpui::Entity<LottiePlayer>,
        until: impl Fn(&LottiePlayer) -> bool,
    ) {
        cx.executor().allow_parking();
        for _ in 0..400 {
            cx.executor().advance_clock(Duration::from_millis(50));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(5));
            if player.read_with(cx, |p, _| until(p)) {
                break;
            }
        }
    }

    #[test]
    fn the_bounce_renders_distinct_looping_frames_in_gpuis_pixel_order() {
        let limits = LottieLimits::default();
        let clip = render(BOUNCE, &limits, 1000).expect("the animation renders");
        // 2 s at 30 fps.
        assert_eq!(clip.frames.len(), 60);
        assert!(clip.complete);
        assert_eq!(clip.frame_interval, Duration::from_secs_f32(1.0 / 30.0));
        let (w, h) = clip.frames[0].dimensions();
        assert_eq!((w, h), (360, 360), "scaled to the output cap");
        // A transparent corner and an opaque ball: it is a sticker.
        assert_eq!(clip.frames[0].get_pixel(0, 0).0[3], 0);
        let opaque = clip.frames[30].pixels().filter(|p| p.0[3] == 255).count();
        assert!(opaque > 1000, "{opaque} opaque pixels");
        // The ball moves, and the clip loops.
        assert_ne!(clip.frames[0].as_raw(), clip.frames[15].as_raw());
        assert_ne!(clip.frames[15].as_raw(), clip.frames[30].as_raw());
        let diff = clip.frames[0]
            .as_raw()
            .iter()
            .zip(clip.frames[59].as_raw())
            .filter(|(a, b)| a != b)
            .count();
        assert!(diff < 60_000, "{diff} bytes differ between first and last");
        // BGRA: the green ball (0, 168, 132) has blue 132 and red 0.
        let ball = clip.frames[0]
            .pixels()
            .find(|p| p.0[3] == 255 && p.0[1] > 100)
            .expect("an opaque green pixel");
        assert!(ball.0[0] > ball.0[2], "BGRA puts blue first: {ball:?}");
    }

    #[test]
    fn a_poster_is_one_frame_and_the_limits_apply() {
        let limits = LottieLimits::default();
        let poster = render(BOUNCE, &limits, 1).expect("poster");
        assert_eq!(poster.frames.len(), 1);
        assert!(!poster.complete);
        let few = LottieLimits {
            max_frames: 10,
            ..limits
        };
        let clip = render(BOUNCE, &few, 1000).expect("frames");
        assert_eq!(clip.frames.len(), 10);
        assert!(
            clip.frame_interval.abs_diff(Duration::from_millis(200)) < Duration::from_millis(1)
        );
        let small = LottieLimits {
            max_canvas_side: 64,
            ..limits
        };
        assert!(render(BOUNCE, &small, 1).is_err());
        let short = LottieLimits {
            max_seconds: 1.0,
            ..limits
        };
        assert!(render(BOUNCE, &short, 1).is_err());
        assert!(render("not json", &limits, 1).is_err());
        assert!(render(BOUNCE, &limits, 0).is_err());
    }

    #[test]
    fn was_archives_are_read_without_extraction_and_capped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let limits = LottieLimits::default();
        let plain = dir.path().join("a.was");
        archive(&plain, &[("animation.json", BOUNCE)]);
        assert_eq!(
            animation_json_from_was(&plain, &limits).as_deref(),
            Some(BOUNCE)
        );
        // Metadata and trust tokens are skipped; a nested animation is found.
        let nested = dir.path().join("b.was");
        archive(
            &nested,
            &[
                ("metadata.json", "{}"),
                ("trust_token.json", "{}"),
                ("sticker/animation.json", BOUNCE),
            ],
        );
        assert_eq!(
            animation_json_from_was(&nested, &limits).as_deref(),
            Some(BOUNCE)
        );
        let none = dir.path().join("c.was");
        archive(&none, &[("metadata.json", "{}")]);
        assert!(animation_json_from_was(&none, &limits).is_none());
        let tight = LottieLimits {
            max_json_bytes: 100,
            ..limits
        };
        assert!(animation_json_from_was(&plain, &tight).is_none());
        let small = LottieLimits {
            max_archive_bytes: 100,
            ..limits
        };
        assert!(animation_json_from_was(&plain, &small).is_none());
        assert!(animation_json_from_was(&dir.path().join("missing.was"), &limits).is_none());
    }

    #[test]
    fn contain_centres_and_keeps_the_aspect_ratio() {
        assert_eq!(contain(100.0, 100.0, 50.0, 50.0), (0.0, 0.0, 100.0, 100.0));
        assert_eq!(contain(200.0, 100.0, 50.0, 50.0), (50.0, 0.0, 100.0, 100.0));
        assert_eq!(contain(100.0, 200.0, 100.0, 50.0), (0.0, 75.0, 100.0, 50.0));
        assert_eq!(contain(80.0, 60.0, 0.0, 0.0), (0.0, 0.0, 80.0, 60.0));
    }

    /// The idle rule: nothing runs until the sticker is active, and nothing
    /// runs once it is not.
    #[gpui::test]
    fn the_timer_runs_only_while_the_sticker_is_active(cx: &mut TestAppContext) {
        let player = cx.new(|_| LottiePlayer::new());
        assert_eq!(player.read_with(cx, |p, _| p.state()), LottieState::Empty);
        // Loading while inactive renders the poster only, with no timer.
        player.update(cx, |p, cx| {
            p.load(LottieSource::Json(BOUNCE.into()), cx);
        });
        assert_eq!(player.read_with(cx, |p, _| p.state()), LottieState::Loading);
        settle(cx, &player, |p| p.frame_count() > 0);
        assert_eq!(player.read_with(cx, |p, _| p.state()), LottieState::Ready);
        assert_eq!(player.read_with(cx, |p, _| p.frame_count()), 1);
        assert!(!player.read_with(cx, |p, _| p.is_running()));
        // Activating renders the rest, then starts the timer.
        player.update(cx, |p, cx| p.set_active(true, cx));
        settle(cx, &player, |p| p.is_running());
        assert_eq!(player.read_with(cx, |p, _| p.frame_count()), 60);
        assert!(player.read_with(cx, |p, _| p.is_running()));
        // The frame index moves with the clock.
        settle(cx, &player, |p| p.frame_index() != 0);
        assert_ne!(player.read_with(cx, |p, _| p.frame_index()), 0);
        // Deactivating stops it at once and keeps the frames.
        player.update(cx, |p, cx| p.set_active(false, cx));
        cx.run_until_parked();
        assert!(!player.read_with(cx, |p, _| p.is_running()));
        assert_eq!(player.read_with(cx, |p, _| p.frame_count()), 60);
        // Reactivating needs no new render.
        player.update(cx, |p, cx| p.set_active(true, cx));
        assert!(player.read_with(cx, |p, _| p.is_running()));
        player.update(cx, |p, cx| p.set_active(false, cx));
        assert!(!player.read_with(cx, |p, _| p.is_running()));
    }

    #[gpui::test]
    fn a_broken_sticker_fails_without_a_timer(cx: &mut TestAppContext) {
        let player = cx.new(|_| LottiePlayer::new());
        player.update(cx, |p, cx| {
            p.set_active(true, cx);
            p.load(LottieSource::Json("nope".into()), cx);
        });
        settle(cx, &player, |p| matches!(p.state(), LottieState::Failed(_)));
        assert!(matches!(
            player.read_with(cx, |p, _| p.state()),
            LottieState::Failed(_)
        ));
        assert!(!player.read_with(cx, |p, _| p.is_running()));
    }

    #[gpui::test]
    fn a_was_archive_loads_through_the_same_path(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sticker.was");
        archive(&path, &[("animation.json", BOUNCE)]);
        let player = cx.new(|_| LottiePlayer::new());
        player.update(cx, |p, cx| p.load(LottieSource::Was(path), cx));
        settle(cx, &player, |p| p.frame_count() > 0);
        assert_eq!(player.read_with(cx, |p, _| p.state()), LottieState::Ready);
    }
}
