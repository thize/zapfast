//! A window with one of each media component, and two automated runs that
//! check the rules the components exist to keep:
//!
//! ```text
//! cargo run --example gallery                          # interactive, everything playing
//! cargo run --example gallery -- --bench idle --seconds 12
//! cargo run --example gallery -- --bench active --seconds 12
//! ```
//!
//! `--bench idle` leaves every component paused or inactive and prints the
//! process's CPU use over the last `seconds - 2`: it must read 0%. `--bench
//! active` plays all of them (a 30 fps video fed by a decoder-like thread) and
//! prints the CPU use and the video's frame rate and cost.
//!
//! Automated runs never take focus: on macOS they start as an Accessory app,
//! and the window opens unfocused in the corner of the screen. The pictures are
//! ZapFast's demo assets (`assets/demo`, CC0 or public domain).

use futures::channel::mpsc::unbounded;
use gpui::{
    ActivationPolicy, App, AppContext as _, AssetSource, Bounds, Context, Entity, IntoElement,
    ParentElement as _, Render, Result, SharedString, Styled as _, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions, div, hsla, px, size,
};
use std::borrow::Cow;
use std::rc::Rc;
use std::time::{Duration, Instant};
use zapfast_gpui_media::video::bench_line;
use zapfast_gpui_media::{
    AnimatedPicture, LottiePlayer, LottieSource, MediaPicture, MediaSource, VideoFrame, VideoStats,
    VideoSurface, cached,
};

const ASSETS: &[(&str, &[u8])] = &[
    (
        "demo/photos/launch.jpg",
        include_bytes!("../../assets/demo/photos/launch.jpg"),
    ),
    (
        "demo/stickers/astronaut-float.webp",
        include_bytes!("../../assets/demo/stickers/astronaut-float.webp"),
    ),
];
const BOUNCE: &str = include_str!("../testdata/bounce.json");

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ASSETS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ASSETS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

const STICKER: (f32, f32) = (180.0, 180.0);
const VIDEO: (f32, f32) = (480.0, 270.0);

struct Gallery {
    sticker: Entity<AnimatedPicture>,
    lottie: Entity<LottiePlayer>,
    video: Entity<VideoSurface>,
    stats: Rc<VideoStats>,
}

impl Gallery {
    fn new(cx: &mut Context<Self>) -> Self {
        let stats = VideoStats::new();
        let sticker = cx.new(|_| {
            AnimatedPicture::new(
                "sticker",
                MediaSource::asset("demo/stickers/astronaut-float.webp"),
                STICKER,
            )
            .sticker()
            .label("An astronaut floating")
        });
        let lottie = cx.new(|cx| {
            let mut player = LottiePlayer::new();
            player.load(LottieSource::Json(BOUNCE.into()), cx);
            player
        });
        let video = cx.new(|_| VideoSurface::new().with_stats(stats.clone()));
        Self {
            sticker,
            lottie,
            video,
            stats,
        }
    }

    /// Plays or stops everything that moves.
    fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        self.sticker.update(cx, |s, cx| s.set_playing(active, cx));
        self.lottie.update(cx, |l, cx| l.set_active(active, cx));
        if active {
            self.start_video(cx);
        } else {
            self.video.update(cx, |v, cx| {
                v.detach();
                v.clear(cx);
            });
        }
    }

    /// A thread that makes a 640x360 frame every 1/30 s, as a decoder would,
    /// and a surface awaiting its channel.
    fn start_video(&mut self, cx: &mut Context<Self>) {
        let (sender, receiver) = unbounded();
        self.video.update(cx, |v, cx| v.attach(receiver, cx));
        std::thread::spawn(move || {
            let started = Instant::now();
            for n in 0u64.. {
                let due = started + Duration::from_secs_f64(n as f64 / 30.0);
                std::thread::sleep(due.saturating_duration_since(Instant::now()));
                let frame = synthetic_frame(n);
                if sender.unbounded_send(frame).is_err() {
                    break;
                }
            }
        });
    }
}

/// A scrolling diagonal gradient with a bouncing box: opaque BGRA.
fn synthetic_frame(n: u64) -> VideoFrame {
    let (w, h) = (640usize, 360usize);
    let mut bgra = vec![255u8; w * h * 4];
    let phase = (n % 256) as usize;
    for y in 0..h {
        for x in 0..w {
            let v = ((x + y + phase * 4) & 0xFF) as u8;
            let at = (y * w + x) * 4;
            bgra[at] = v;
            bgra[at + 1] = 255 - v;
            bgra[at + 2] = v / 2;
        }
    }
    let bx = ((n as f32 * 0.07).sin() * 0.5 + 0.5) * (w - 80) as f32;
    for y in 100..180 {
        for x in bx as usize..bx as usize + 80 {
            let at = (y * w + x) * 4;
            bgra[at..at + 3].copy_from_slice(&[255, 255, 255]);
        }
    }
    VideoFrame::from_bgra(w as u32, h as u32, bgra).expect("a 640x360 frame")
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(hsla(0.5, 0.15, 0.12, 1.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.))
            .p(px(12.))
            .child(
                MediaPicture::bubble(
                    "photo",
                    MediaSource::asset("demo/photos/launch.jpg"),
                    1024,
                    683,
                )
                .backdrop(hsla(0., 0., 0.2, 1.))
                .label("A rocket launch"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(cached(&self.sticker, STICKER))
                    .child(cached(&self.lottie, STICKER)),
            )
            .child(cached(&self.video, VIDEO))
    }
}

// ----- automated runs ----------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Bench {
    Idle,
    Active,
}

fn parse() -> (Option<(Bench, u64)>, bool) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let seconds = value("--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(12u64)
        .max(4);
    let bench = match value("--bench").as_deref() {
        Some("idle") => Some((Bench::Idle, seconds)),
        Some("active") => Some((Bench::Active, seconds)),
        Some(other) => {
            eprintln!("unknown --bench {other}; use idle or active");
            std::process::exit(2);
        }
        None => None,
    };
    (bench, args.iter().any(|a| a == "--minimize"))
}

/// The process's user plus system CPU time so far.
fn cpu_time() -> Duration {
    // SAFETY: `getrusage` fills the struct it is given and reads nothing.
    let usage = unsafe {
        let mut usage = std::mem::zeroed::<libc::rusage>();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        usage
    };
    let of = |t: libc::timeval| Duration::new(t.tv_sec as u64, t.tv_usec as u32 * 1000);
    of(usage.ru_utime) + of(usage.ru_stime)
}

fn main() {
    let (bench, minimize) = parse();
    let app = gpui_platform::application().with_assets(Assets);
    // Automated runs must be an Accessory app before the run loop starts.
    let app = if bench.is_some() && cfg!(target_os = "macos") {
        app.with_windowing(gpui::WindowingRequest::Headless)
    } else {
        app
    };
    app.run(move |cx: &mut App| boot(bench, minimize, cx));
}

fn boot(bench: Option<(Bench, u64)>, minimize: bool, cx: &mut App) {
    let background = bench.is_some();
    let window_size = size(px(520.), px(900.));
    let bounds = if background {
        cx.set_activation_policy(ActivationPolicy::Accessory);
        match cx.primary_display() {
            Some(display) => {
                let screen = display.bounds();
                Bounds::new(
                    gpui::point(
                        screen.right() - window_size.width,
                        screen.bottom() - window_size.height,
                    ),
                    window_size,
                )
            }
            None => Bounds::centered(None, window_size, cx),
        }
    } else {
        Bounds::centered(None, window_size, cx)
    };
    let started = Instant::now();
    let window: WindowHandle<Gallery> = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("zapfast-gpui-media gallery".into()),
                    ..Default::default()
                }),
                focus: !background,
                // Automated runs measure the components' own cost: lift the
                // OS-friendly caps on unfocused windows and Low Power Mode.
                inactive_frame_interval: if background {
                    None
                } else {
                    WindowOptions::default().inactive_frame_interval
                },
                low_power_frame_interval: if background {
                    None
                } else {
                    WindowOptions::default().low_power_frame_interval
                },
                ..Default::default()
            },
            |_, cx| cx.new(Gallery::new),
        )
        .expect("the window opens");
    if !background {
        window
            .update(cx, |gallery, _, cx| gallery.set_active(true, cx))
            .ok();
        cx.activate(true);
    }
    if minimize {
        window
            .update(cx, |_, window, _| window.minimize_window())
            .ok();
    }
    let Some((kind, seconds)) = bench else {
        return;
    };
    if kind == Bench::Active {
        window
            .update(cx, |gallery, _, cx| gallery.set_active(true, cx))
            .ok();
        draw_while_hidden(window, cx);
    }
    cx.spawn(async move |cx| {
        // The first two seconds warm up: poster decoding, the atlas, the pipeline.
        let warm = started + Duration::from_secs(2);
        cx.background_executor()
            .timer(warm.saturating_duration_since(Instant::now()))
            .await;
        let cpu_before = cpu_time();
        let wall_before = Instant::now();
        let end = started + Duration::from_secs(seconds);
        cx.background_executor()
            .timer(end.saturating_duration_since(Instant::now()))
            .await;
        let cpu = cpu_time() - cpu_before;
        let wall = wall_before.elapsed();
        let percent = 100.0 * cpu.as_secs_f64() / wall.as_secs_f64();
        let name = if kind == Bench::Idle {
            "idle"
        } else {
            "active"
        };
        println!(
            "BENCH {name} cpu_percent={percent:.2} cpu_ms={:.0} seconds={:.1}",
            cpu.as_secs_f64() * 1000.0,
            wall.as_secs_f64()
        );
        if kind == Bench::Active {
            let stats = cx.update(|cx| {
                window
                    .read_with(cx, |gallery, _| gallery.stats.clone())
                    .ok()
            });
            if let Some(stats) = stats {
                let summary = stats.summary(wall_before, Instant::now());
                println!("{}", bench_line(&summary));
            }
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

/// Draws frames for a hidden window, which the platform gives none, so the
/// run measures the components and not whether the OS delivered frames.
fn draw_while_hidden(window: WindowHandle<Gallery>, cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(4))
                .await;
            cx.update(|cx| {
                cx.update_window(window.into(), |_, window, cx| {
                    if !window.is_visible() {
                        window.draw(cx).clear(cx);
                    }
                })
                .ok();
            });
        }
    })
    .detach();
}
