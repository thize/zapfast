//! Boots GPUI, opens the window, and runs the automated modes.

use crate::assets::{Assets, INTER};
use crate::bench::{BenchKind, Cli, NotifyBackend, SceneKind, idle_line};
use crate::chat::conversation::{ClearSelection, CopySelection, JumpNext};
use crate::media::video;
use crate::palette::Palette;
use crate::selection::{Pos, Selection};
use crate::shell::{Shell, ShowChat, ShowMedia, ShowPlatform, apply_theme};
use crate::ui::ActivePalette;
use gpui::{
    ActivationPolicy, App, AppContext as _, Bounds, KeyBinding, TitlebarOptions, WindowBounds,
    WindowHandle, WindowOptions, px, size,
};
use std::borrow::Cow;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A keystroke with the platform's secondary modifier: `KeyBinding::new` only
/// takes explicit modifiers (the `secondary-` alias needs `KeyBinding::load`).
#[cfg(target_os = "macos")]
macro_rules! secondary {
    ($key:literal) => {
        concat!("cmd-", $key)
    };
}

#[cfg(not(target_os = "macos"))]
macro_rules! secondary {
    ($key:literal) => {
        concat!("ctrl-", $key)
    };
}

pub fn run(cli: Cli) {
    let app = gpui_platform::application().with_assets(Assets);
    // Automated runs must be an Accessory app before the run loop starts:
    // switching the policy inside `boot`, after launch, still lets macOS hand
    // the window focus. The badge probe keeps its Dock tile.
    let accessory = (cli.bench.is_some() || cli.snapshot.is_some() || cli.probe_notify.is_some())
        && !cli.probe_badge;
    let app = if accessory && cfg!(target_os = "macos") {
        app.with_windowing(gpui::WindowingRequest::Headless)
    } else {
        app
    };
    app.run(move |cx: &mut App| boot(cli, cx));
}

fn boot(cli: Cli, cx: &mut App) {
    if let Err(error) = cx.text_system().add_fonts(vec![Cow::Borrowed(INTER)]) {
        eprintln!("could not load Inter: {error:#}");
    }
    gpui_component::init(cx);
    let palette = match cli.dark {
        Some(dark) => Palette::for_appearance(dark),
        None => Palette::for_window(cx.window_appearance()),
    };
    cx.set_global(ActivePalette(palette));
    apply_theme(palette, cx);

    // The native menu bar (Edit menu included) and Cmd+Q.
    crate::platform::menus::install(cx);
    cx.bind_keys([
        // The secondary modifier is Cmd on macOS and Ctrl elsewhere.
        KeyBinding::new(secondary!("c"), CopySelection, Some("Conversation")),
        KeyBinding::new("escape", ClearSelection, Some("Conversation")),
        KeyBinding::new(secondary!("j"), JumpNext, Some("Chat")),
        KeyBinding::new(secondary!("1"), ShowChat, Some("Shell")),
        KeyBinding::new(secondary!("2"), ShowMedia, Some("Shell")),
        KeyBinding::new(secondary!("3"), ShowPlatform, Some("Shell")),
    ]);

    // Automated runs (--bench, --snapshot) never take focus from the user:
    // an Accessory app has no Dock icon and is never activated, and the window
    // opens unfocused in the bottom-right corner of the screen.
    let background = cli.bench.is_some()
        || cli.snapshot.is_some()
        || cli.probe_notify.is_some()
        || cli.probe_badge;
    let window_size = size(px(1200.), px(800.));
    let bounds = if background {
        // The badge probe needs a Dock tile, so it stays a regular app (never
        // activated, window unfocused).
        if !cli.probe_badge {
            cx.set_activation_policy(ActivationPolicy::Accessory);
        }
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
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("ZapFast GPUI chat spike".into()),
                    ..Default::default()
                }),
                focus: !background,
                // GPUI paints an unfocused window, or any window in Low Power
                // Mode, at most every 33 ms by default. A background run is
                // always unfocused and measures the app's own frame cost, so
                // those two OS-friendly caps are lifted for it (interactive
                // runs keep them).
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
            |window, cx| {
                let shell = cx.new(|cx| Shell::new(&cli, window, cx));
                cx.new(|cx| gpui_component::Root::new(shell, window, cx))
            },
        )
        .expect("the window opens");
    if !background {
        cx.activate(true);
    }
    if cli.minimize {
        window
            .update(cx, |_, window, _| window.minimize_window())
            .ok();
    }

    match (&cli.bench, &cli.snapshot) {
        (Some((BenchKind::Idle, seconds)), _) => idle_bench(window, *seconds, started, cx),
        (Some((BenchKind::Video, seconds)), _) => video_bench(window, *seconds, started, cx),
        (Some((BenchKind::Scroll, seconds)), _) => {
            scroll_bench(window, *seconds, cli.paced, started, cx)
        }
        (_, Some(dir)) => snapshot(window, dir.clone(), cx),
        _ if cli.probe_badge => badge_probe(window, cx),
        _ if cli.probe_notify.is_some() => {
            notify_probe(window, cli.probe_notify.expect("checked"), cx)
        }
        _ => {}
    }
}

/// Gives up on a benchmark that has produced nothing, so a run never hangs:
/// prints the failure on stdout (where the BENCH lines are) and exits 1.
fn bench_deadline(
    window: WindowHandle<gpui_component::Root>,
    name: &'static str,
    limit: Duration,
    started: Instant,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer((started + limit).saturating_duration_since(Instant::now()))
            .await;
        let visibility = cx.update(|cx| {
            window
                .update(cx, |_, window, _| format!("{:?}", window.visibility()))
                .unwrap_or_default()
        });
        println!(
            "BENCH {name} error=no result after {} s (window visibility {visibility}): the platform delivered no frames",
            limit.as_secs()
        );
        std::process::exit(1);
    })
    .detach();
}

/// Scrolls the conversation for `seconds * 60` frames, drawing each frame
/// itself, back to back, instead of waiting for the display link: a window
/// that is hidden, unfocused or throttled by the OS gets no frames (or a frame
/// every 33 ms), and a benchmark that waited for them measured the OS or hung.
/// A frame here is the whole of GPUI's work for it (render, layout, prepaint,
/// paint, atlas uploads) without presenting it to the GPU, which only the
/// display link's own tick can do (`--paced` runs that way instead).
fn scroll_bench(
    window: WindowHandle<gpui_component::Root>,
    seconds: u64,
    paced: bool,
    started: Instant,
    cx: &mut App,
) {
    // A paced run needs frames at the display's rate: allow for a slow start.
    let limit = Duration::from_secs(10 + seconds * if paced { 3 } else { 4 });
    bench_deadline(window, "scroll", limit, started, cx);
    if paced {
        return;
    }
    let shell = shell_of(window, cx);
    let conversation = shell.read(cx).chat().read(cx).conversation().clone();
    cx.spawn(async move |cx| {
        loop {
            let alive = cx.update(|cx| {
                conversation.update(cx, |_, cx| cx.notify());
                cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
                    .is_ok()
            });
            if !alive {
                break;
            }
            // Lets the run loop deliver events and the clock tick in between.
            cx.background_executor().timer(Duration::ZERO).await;
        }
    })
    .detach();
}

/// Draws frames for a hidden window, which the platform gives none: the
/// video's pictures are still pushed, uploaded and painted, as they would be
/// the moment the window is shown, and the run measures that instead of
/// reporting 0 fps because a window was covered.
fn draw_while_hidden(
    window: WindowHandle<gpui_component::Root>,
    stats: std::rc::Rc<crate::bench::Stats>,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(4))
                .await;
            let drew = cx.update(|cx| {
                cx.update_window(window.into(), |_, window, cx| {
                    if window.is_visible() {
                        return false;
                    }
                    window.draw(cx).clear(cx);
                    true
                })
                .unwrap_or(false)
            });
            if drew {
                stats.pumped();
            }
        }
    })
    .detach();
}

/// Runs the Media scene's call video alone for `seconds`, then prints how many
/// frames were painted, at what rate, and what each cost.
fn video_bench(
    window: WindowHandle<gpui_component::Root>,
    seconds: u64,
    started: Instant,
    cx: &mut App,
) {
    let shell = shell_of(window, cx);
    let stats = shell.read(cx).media().read(cx).video_stats();
    let scene_stats = shell.read(cx).stats();
    draw_while_hidden(window, scene_stats.clone(), cx);
    cx.spawn(async move |cx| {
        let end = started + Duration::from_secs(seconds);
        cx.background_executor()
            .timer(end.saturating_duration_since(Instant::now()))
            .await;
        // The first second warms the atlas and the pipeline up.
        if std::env::var_os("ZAPFAST_SPIKE_TRACE").is_some() {
            eprintln!("painted video frames (s): {:.2?}", stats.offsets(started));
        }
        let summary = stats.summary(started + Duration::from_secs(1), end);
        println!("{}", video::bench_line(&summary));
        eprintln!("{}", video::bench_detail(&summary));
        eprintln!("resident memory at the end: {}", resident_memory());
        let visibility = cx.update(|cx| {
            window
                .update(cx, |_, window, _| format!("{:?}", window.visibility()))
                .unwrap_or_default()
        });
        eprintln!("window visibility at the end: {visibility}");
        eprintln!(
            "frames the bench drew itself because the window was hidden: {}",
            scene_stats.pumped_frames()
        );
        cx.update(|cx| cx.quit());
    })
    .detach();
}

/// The process's resident set size, from `ps` (for the video's leak check).
fn resident_memory() -> String {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|text| text.trim().parse::<u64>().ok())
        .map_or_else(
            || "unknown".into(),
            |kib| format!("{:.0} MB", kib as f64 / 1024.0),
        )
}

/// Puts 3 on the Dock tile for ten seconds (so a Dock query can read it).
fn badge_probe(window: WindowHandle<gpui_component::Root>, cx: &mut App) {
    let shell = shell_of(window, cx);
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(1000))
            .await;
        cx.update(|cx| {
            let platform = shell.read(cx).platform().clone();
            platform.update(cx, |scene, cx| scene.set_badge(3, cx));
            println!("PROBE badge count=3");
        });
        cx.background_executor()
            .timer(Duration::from_secs(10))
            .await;
        cx.update(|cx| {
            let platform = shell.read(cx).platform().clone();
            platform.update(cx, |scene, cx| scene.set_badge(0, cx));
            cx.quit();
        });
    })
    .detach();
}

/// Posts a test notification through one backend and prints what it reported.
fn notify_probe(window: WindowHandle<gpui_component::Root>, backend: NotifyBackend, cx: &mut App) {
    let shell = shell_of(window, cx);
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(1200))
            .await;
        let (bundled, ()) = cx.update(|cx| {
            let platform = shell.read(cx).platform().clone();
            platform.update(cx, |scene, cx| match backend {
                NotifyBackend::Gpui => scene.notify_gpui(cx),
                NotifyBackend::Fastframe => scene.notify_fastframe(cx),
            });
            (crate::platform::is_bundled(), ())
        });
        // The permission prompt (bundled runs) waits for the person; give the
        // worker time to answer or fail.
        cx.background_executor().timer(Duration::from_secs(6)).await;
        cx.update(|cx| {
            println!("PROBE notify bundled={bundled}");
            let lines = shell.read(cx).platform().read(cx).log_lines();
            for line in lines.iter().rev() {
                println!("PROBE notify {line}");
            }
            cx.quit();
        });
    })
    .detach();
}

/// Waits `seconds`, then reports how often the conversation view rendered in
/// the last `seconds - 2` of them.
fn idle_bench(
    window: WindowHandle<gpui_component::Root>,
    seconds: u64,
    started: Instant,
    cx: &mut App,
) {
    let shell = shell_of(window, cx);
    let stats = shell.read(cx).stats();
    cx.spawn(async move |cx| {
        // Measured from launch, so the window is `seconds - 2` long as promised.
        cx.background_executor()
            .timer(
                (started + Duration::from_secs(seconds)).saturating_duration_since(Instant::now()),
            )
            .await;
        if std::env::var_os("ZAPFAST_SPIKE_TRACE").is_some() {
            eprintln!("render offsets (s): {:.2?}", stats.render_offsets(started));
        }
        let since = started + Duration::from_secs(2);
        println!("{}", idle_line(stats.renders_since(since), seconds));
        cx.update(|cx| cx.quit());
    })
    .detach();
}

fn shell_of(window: WindowHandle<gpui_component::Root>, cx: &mut App) -> gpui::Entity<Shell> {
    window
        .read_with(cx, |root, _| {
            root.view()
                .clone()
                .downcast::<Shell>()
                .expect("the root view is the shell")
        })
        .expect("the window is open")
}

/// Saves one PNG per scene, plus the chat with a selection across messages.
fn snapshot(window: WindowHandle<gpui_component::Root>, dir: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx| {
        let report = |message: String| println!("{message}");
        if let Err(error) = std::fs::create_dir_all(&dir) {
            report(format!(
                "SNAPSHOT unsupported: cannot create {}: {error}",
                dir.display()
            ));
            cx.update(|cx| cx.quit());
            return;
        }
        // The Media scene is taller than the window: three shots of it.
        let steps: [(&str, SceneKind, bool, f32); 6] = [
            ("chat", SceneKind::Chat, false, 0.0),
            ("chat-selection", SceneKind::Chat, true, 0.0),
            ("media", SceneKind::Media, false, 0.0),
            ("media-2", SceneKind::Media, false, 900.0),
            ("media-3", SceneKind::Media, false, 1800.0),
            ("platform", SceneKind::Platform, false, 0.0),
        ];
        for (name, scene, with_selection, scroll) in steps {
            let shown = cx.update(|cx| {
                let shell = shell_of(window, cx);
                window
                    .update(cx, |_, window, cx| {
                        shell.update(cx, |shell, cx| {
                            shell.set_scene(scene, window, cx);
                            shell
                                .media()
                                .update(cx, |media, cx| media.scroll_to(scroll, cx));
                            let chat = shell.chat().clone();
                            chat.update(cx, |chat, cx| {
                                chat.conversation().update(cx, |view, cx| {
                                    let last = view.message_count() - 1;
                                    let selection = with_selection.then(|| {
                                        Selection::new(
                                            Pos::new(last - 5, 8),
                                            Pos::new(last - 1, 20),
                                        )
                                    });
                                    view.set_selection(selection, cx);
                                });
                            });
                        });
                    })
                    .is_ok()
            });
            if !shown {
                break;
            }
            // Give the window a few frames to lay out and paint; the Media
            // scene also generates its pictures and renders its Lottie first.
            let wait = if scene == SceneKind::Media && scroll == 0.0 {
                2200
            } else {
                900
            };
            cx.background_executor()
                .timer(Duration::from_millis(wait))
                .await;
            let image = cx.update(|cx| {
                cx.update_window(window.into(), |_, window, cx| {
                    // Draw now rather than trust the frame loop, which pauses
                    // while the window is occluded.
                    window.draw(cx).clear(cx);
                    window.render_to_image()
                })
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()))
            });
            match image {
                Ok(image) => {
                    let path = dir.join(format!("{name}.png"));
                    match image.save(&path) {
                        Ok(()) => report(format!(
                            "SNAPSHOT {} {}x{}",
                            path.display(),
                            image.width(),
                            image.height()
                        )),
                        Err(error) => report(format!(
                            "SNAPSHOT unsupported: cannot write {}: {error}",
                            path.display()
                        )),
                    }
                }
                Err(error) => {
                    report(format!("SNAPSHOT unsupported: {error}"));
                    break;
                }
            }
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}
