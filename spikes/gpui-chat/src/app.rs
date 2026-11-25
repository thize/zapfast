//! Boots GPUI, opens the window, and runs the automated modes.

use crate::assets::{Assets, INTER};
use crate::bench::{BenchKind, Cli, SceneKind, idle_line};
use crate::chat::conversation::{ClearSelection, CopySelection, JumpNext};
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
    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| boot(cli, cx));
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
    let background = cli.bench.is_some() || cli.snapshot.is_some();
    let window_size = size(px(1200.), px(800.));
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
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("ZapFast GPUI chat spike".into()),
                    ..Default::default()
                }),
                focus: !background,
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

    match (&cli.bench, &cli.snapshot) {
        (Some((BenchKind::Idle, seconds)), _) => idle_bench(window, *seconds, started, cx),
        (_, Some(dir)) => snapshot(window, dir.clone(), cx),
        _ => {}
    }
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
        let steps: [(&str, SceneKind, bool); 4] = [
            ("chat", SceneKind::Chat, false),
            ("chat-selection", SceneKind::Chat, true),
            ("media", SceneKind::Media, false),
            ("platform", SceneKind::Platform, false),
        ];
        for (name, scene, with_selection) in steps {
            let shown = cx.update(|cx| {
                let shell = shell_of(window, cx);
                window
                    .update(cx, |_, window, cx| {
                        shell.update(cx, |shell, cx| {
                            shell.set_scene(scene, window, cx);
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
            // Give the window a few frames to lay out and paint.
            cx.background_executor()
                .timer(Duration::from_millis(900))
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
