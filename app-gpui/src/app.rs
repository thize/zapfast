//! Boots GPUI, wires the backend (or the demo) to the store, opens the window
//! and runs the automated modes.

use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use gpui::{
    ActivationPolicy, App, AppContext as _, Bounds, Entity, KeyBinding, Menu, MenuItem,
    SystemMenuType, TitlebarOptions, WindowBounds, WindowHandle, WindowOptions, actions, px, size,
};

use crate::assets::{Assets, INTER};
use crate::bench::{Stats, idle_line};
use crate::brand::APP_NAME;
use crate::bridge::{Bridge, Injector};
use crate::cli::{BenchKind, Cli};
use crate::identity::Identity;
use crate::instance::{self, Launch, Request};
use crate::palette::Palette;
use crate::shell::{Shell, apply_theme};
use crate::state::Store;
use crate::ui::{ActiveLocale, ActivePalette};
use crate::views::conversation::{ClearSelection, CopySelection, MenuKind};
use crate::{demo, identity};

actions!(zapfast_gpui, [Quit]);

/// Why a launch ended before a window opened.
#[derive(Debug, PartialEq, Eq)]
pub enum Exit {
    /// Another copy of this app runs and was asked to show its window.
    AlreadyRunning,
    Failed(String),
}

pub fn run(cli: Cli) -> Result<(), Exit> {
    // A real run owns an account: it files its keys under its own keyring
    // service and takes the instance slot before anything else. A demo, a
    // benchmark or a snapshot touches neither (and never the disk).
    let real = if cli.synthetic() {
        None
    } else {
        let identity = Identity::discover();
        identity
            .apply_keyring_service()
            .map_err(|error| Exit::Failed(format!("{error:#}")))?;
        let (guard, requests) = match instance::claim(&identity) {
            Launch::First(guard, requests) => (guard, requests),
            Launch::AlreadyRunning => return Err(Exit::AlreadyRunning),
            Launch::Unanswered => {
                return Err(Exit::Failed(format!(
                    "another copy of {APP_NAME} holds the instance lock and did not answer"
                )));
            }
        };
        identity
            .account()
            .ensure()
            .map_err(|error| Exit::Failed(format!("cannot create the data folders: {error}")))?;
        Some((identity, guard, requests))
    };

    let app = gpui_platform::application().with_assets(Assets);
    // Automated runs must be an Accessory app before the run loop starts:
    // switching the policy after launch still lets macOS hand the window
    // focus.
    let app = if cli.automated() && cfg!(target_os = "macos") {
        app.with_windowing(gpui::WindowingRequest::Headless)
    } else {
        app
    };
    app.run(move |cx: &mut App| boot(cli, real, cx));
    Ok(())
}

type Real = (
    Identity,
    fastframe_instance::Guard,
    futures::channel::mpsc::UnboundedReceiver<Request>,
);

/// The menu bar: the application menu with Quit (Cmd+Q), and Edit.
fn menus() -> Vec<Menu> {
    use gpui::OsAction;
    use gpui_component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
    vec![
        Menu::new(APP_NAME).items([
            MenuItem::os_submenu("Serviços", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(format!("Sair de {APP_NAME}"), Quit),
        ]),
        Menu::new("Editar").items([
            MenuItem::os_action("Desfazer", Undo, OsAction::Undo),
            MenuItem::os_action("Refazer", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Recortar", Cut, OsAction::Cut),
            MenuItem::os_action("Copiar", Copy, OsAction::Copy),
            MenuItem::os_action("Colar", Paste, OsAction::Paste),
            MenuItem::separator(),
            MenuItem::os_action("Selecionar tudo", SelectAll, OsAction::SelectAll),
        ]),
    ]
}

fn boot(cli: Cli, real: Option<Real>, cx: &mut App) {
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
    // Content labels ("Foto", "Mensagem de voz") follow the system's language;
    // a synthetic run is always in Portuguese, so its pictures do not vary.
    cx.set_global(ActiveLocale(if cli.synthetic() {
        zapfast::i18n::Locale::PortugueseBrazil
    } else {
        zapfast::i18n::detect()
    }));
    cx.set_app_identity(&identity::bundle_id(), APP_NAME);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([KeyBinding::new(
        if cfg!(target_os = "macos") {
            "cmd-q"
        } else {
            "ctrl-q"
        },
        Quit,
        None,
    )]);
    cx.set_menus(menus());
    cx.bind_keys([
        // The secondary modifier is Cmd on macOS and Ctrl elsewhere.
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-c"
            } else {
                "ctrl-c"
            },
            CopySelection,
            Some("Conversation"),
        ),
        KeyBinding::new("escape", ClearSelection, Some("Conversation")),
    ]);
    crate::views::viewer::bind_keys(cx);

    // The state, fed by the backend, or by the demo through the same path.
    let (mut bridge, injector, instance) = match real {
        Some((identity, guard, requests)) => {
            let bridge = Bridge::spawn(identity.account());
            (bridge, None, Some((guard, requests)))
        }
        None => {
            let (bridge, injector) =
                Bridge::detached_with(|injector| Rc::new(demo::Responder::new(injector)));
            (bridge, Some(injector), None)
        }
    };
    let store = cx.new(|cx| Store::new(bridge.sink(), cx));
    store.update(cx, |_, cx| bridge.pump(cx)).detach();

    let background = cli.automated();
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
    let stats = Stats::new();
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(APP_NAME.into()),
                    ..Default::default()
                }),
                focus: !background,
                // GPUI paints an unfocused window at most every 33 ms by
                // default; a background run measures the app's own frame cost.
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
            {
                let (store, stats, dark) = (store.clone(), stats.clone(), cli.dark);
                move |window, cx| {
                    let shell = cx.new(|cx| Shell::new(store, stats, dark, window, cx));
                    cx.new(|cx| gpui_component::Root::new(shell, window, cx))
                }
            },
        )
        .expect("the window opens");
    if !background {
        cx.activate(true);
    }

    // The worker starts once a window is up.
    bridge.release_startup();
    let bridge = Rc::new(RefCell::new(bridge));
    {
        let bridge = bridge.clone();
        cx.on_app_quit(move |_| {
            bridge.borrow().shutdown(Duration::from_secs(2));
            async {}
        })
        .detach();
    }
    if let Some((guard, requests)) = instance {
        show_on_request(window, guard, requests, cx);
    }
    if let Some(injector) = &injector {
        if cli.snapshot.is_some() {
            demo::unlinked(injector);
        } else {
            demo::start(injector);
        }
    }

    match (&cli.bench, &cli.snapshot, injector) {
        (Some((BenchKind::Idle, seconds)), _, _) => idle_bench(stats, *seconds, started, cx),
        (_, Some(dir), Some(injector)) => snapshot(window, injector, dir.clone(), cx),
        _ => {}
    }
}

/// Brings the window forward whenever another launch asks, for as long as the
/// app runs. The guard is held by this task, so the slot stays taken.
fn show_on_request(
    window: WindowHandle<gpui_component::Root>,
    guard: fastframe_instance::Guard,
    mut requests: futures::channel::mpsc::UnboundedReceiver<Request>,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        let _guard = guard;
        while let Some(Request::Show) = requests.next().await {
            cx.update(|cx| {
                cx.activate(true);
                window
                    .update(cx, |_, window, _| window.activate_window())
                    .ok();
            });
        }
    })
    .detach();
}

/// Waits `seconds` from launch, prints how many times the root view rendered
/// after the first two (the start-up), and quits. Zero is the pass.
fn idle_bench(stats: Rc<Stats>, seconds: u64, started: Instant, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(
                (started + Duration::from_secs(seconds)).saturating_duration_since(Instant::now()),
            )
            .await;
        let since = started + Duration::from_secs(2);
        println!("{}", idle_line(stats.renders_since(since), seconds));
        cx.update(|cx| cx.quit());
    })
    .detach();
}

/// The screens `--snapshot` saves, in order: what the state looks like at the
/// login, then with the chats loaded. The screens add their own.
fn snapshot(
    window: WindowHandle<gpui_component::Root>,
    injector: Injector,
    dir: PathBuf,
    cx: &mut App,
) {
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
        for name in [
            "login",
            "login-phone",
            "chats",
            "chat",
            "chat-selection",
            "chat-reply",
            "chat-reactions",
            "chat-photo",
            "viewer",
            "viewer-zoom",
        ] {
            cx.update(|cx| snapshot_step(name, window, &injector, cx));
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

/// Puts the app in the state of the screen `name` (see [`snapshot`]).
fn snapshot_step(
    name: &str,
    window: WindowHandle<gpui_component::Root>,
    injector: &Injector,
    cx: &mut App,
) {
    use crate::state::PairingMethod;
    use crate::views::selection::{Pos, Selection};
    let shell = shell_of(window, cx);
    let store = shell.read(cx).store().clone();
    let panel = shell.read(cx).panel().cloned();
    let conversation = panel.map(|panel| panel.read(cx).conversation().clone());
    match name {
        "login-phone" => {
            store.update(cx, |store, cx| {
                store.session.update(cx, |session, cx| {
                    session.method = PairingMethod::PhoneNumber;
                    cx.notify();
                });
                store.pair_with_phone("5511911110001");
            });
        }
        "chats" => demo::start(injector),
        "chat" => store.update(cx, |store, cx| {
            store.open_chat(Some(demo::ANA.to_owned()), cx)
        }),
        "chat-selection" => {
            if let Some(view) = conversation {
                view.update(cx, |view, cx| {
                    let last = view.message_count().saturating_sub(1);
                    view.set_selection(
                        Some(Selection::new(
                            Pos::new(last - 5, 4),
                            Pos::new(last - 1, 12),
                        )),
                        cx,
                    );
                });
            }
        }
        "chat-reply" => {
            if let Some(view) = conversation {
                view.update(cx, |view, cx| {
                    view.set_selection(None, cx);
                    let last = view.message_count().saturating_sub(2);
                    view.reply_to(last, cx);
                });
            }
        }
        "chat-reactions" => {
            if let Some(view) = conversation {
                view.update(cx, |view, cx| {
                    let target = view.message_count().saturating_sub(3);
                    view.show_menu(target, MenuKind::Reactions, cx);
                });
            }
        }
        // The photo is asked for (its file arrives while the shot waits), then
        // opened in the viewer, then zoomed in on the middle.
        "chat-photo" => {
            if let Some(view) = conversation {
                view.update(cx, |view, cx| {
                    view.set_selection(None, cx);
                    view.close_menu(cx);
                    if let Some(photo) = view.first_picture() {
                        view.media_clicked(photo, cx);
                    }
                });
            }
        }
        "viewer" => {
            if let Some(view) = conversation {
                view.update(cx, |view, cx| {
                    if let Some(photo) = view.first_picture() {
                        view.media_clicked(photo, cx);
                    }
                });
            }
        }
        "viewer-zoom" => {
            if let Some(viewer) = shell.read(cx).viewer().cloned() {
                window
                    .update(cx, |_, window, cx| {
                        viewer.update(cx, |viewer, cx| viewer.zoom_by(3.0, None, window, cx));
                    })
                    .ok();
            }
        }
        _ => {}
    }
}

/// The shell of an open window, for the automated modes and tests.
pub fn shell_of(window: WindowHandle<gpui_component::Root>, cx: &mut App) -> Entity<Shell> {
    window
        .read_with(cx, |root, _| {
            root.view()
                .clone()
                .downcast::<Shell>()
                .expect("the root view is the shell")
        })
        .expect("the window is open")
}
