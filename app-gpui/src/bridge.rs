//! The bridge between the backend's worker and GPUI's main thread.
//!
//! The backend is a Tokio runtime on its own threads. It reports through two
//! things: events it queues for [`Backend::poll`], and a [`Waker`] it calls
//! after queueing ("look again"). The bridge turns the waker into a message
//! on a `futures` channel; a GPUI foreground task awaits that channel and, on
//! each message, drains the queue and applies the events to the [`Store`].
//!
//! Nothing here polls: the task is parked on the channel until the worker
//! wakes it, so an idle app has no timers and no frames. `Waker::wake_after`
//! (used by the worker for a few short delays) is one thread that sleeps once
//! and then sends one message.
//!
//! The updater lives in the same worker, behind four commands. They are never
//! sent: [`GuardedBackend`] drops them if anything ever tries.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{Context, Task};
use zapfast::backend::{Backend, Command, Event, Waker};
use zapfast::paths::AccountDirs;

use crate::state::{CommandSink, Store};

/// Whether a command belongs to the updater, which this app never runs.
pub fn is_updater_command(command: &Command) -> bool {
    matches!(
        command,
        Command::CheckForUpdates
            | Command::InspectUpdate
            | Command::DownloadUpdate { .. }
            | Command::InstallUpdate { .. }
    )
}

/// A backend as a [`CommandSink`], without the updater's commands.
pub struct GuardedBackend(Rc<RefCell<Backend>>);

impl CommandSink for GuardedBackend {
    fn send(&self, command: Command) {
        if is_updater_command(&command) {
            log::warn!("the updater is not part of this app: a command was dropped");
            return;
        }
        self.0.borrow().send(command);
    }
}

/// The backend and the channel that tells the main thread to read it.
pub struct Bridge {
    backend: Rc<RefCell<Backend>>,
    sink: Rc<dyn CommandSink>,
    wakes: Option<UnboundedReceiver<()>>,
    /// Releases the worker's start, once a window is up (real backend only).
    startup: Option<Box<dyn FnOnce()>>,
}

/// What a `--demo` or a test uses to deliver events as the worker would: it
/// queues the event for the backend's `poll` and wakes the bridge, so the
/// event takes the same path as a real one.
#[derive(Clone)]
pub struct Injector {
    events: std::sync::mpsc::Sender<Event>,
    wake: UnboundedSender<()>,
}

impl Injector {
    pub fn send(&self, event: Event) {
        if self.events.send(event).is_ok() {
            let _ = self.wake.unbounded_send(());
        }
    }
}

/// A [`Waker`] that wakes `wake`, and does the delayed kind on a thread that
/// sleeps once.
fn waker(wake: UnboundedSender<()>) -> Waker {
    let later = wake.clone();
    Waker::new(
        move || {
            let _ = wake.unbounded_send(());
        },
        move |delay: Duration| {
            let later = later.clone();
            let slept = std::thread::Builder::new()
                .name("backend-wake-after".into())
                .spawn(move || {
                    std::thread::sleep(delay);
                    let _ = later.unbounded_send(());
                });
            if let Err(error) = slept {
                log::warn!("could not schedule a wake-up: {error}");
            }
        },
    )
}

impl Bridge {
    /// Starts the backend for one account. It does not start working until
    /// [`Bridge::release_startup`], after the first window is open.
    pub fn spawn(dirs: AccountDirs) -> Self {
        let (wake, wakes) = unbounded();
        let mut backend = Backend::spawn(dirs, waker(wake));
        let startup = backend.take_startup().map(|permit| {
            Box::new(move || {
                let _ = permit.send(());
            }) as Box<dyn FnOnce()>
        });
        let backend = Rc::new(RefCell::new(backend));
        Self {
            sink: Rc::new(GuardedBackend(backend.clone())),
            backend,
            wakes: Some(wakes),
            startup,
        }
    }

    /// A bridge with no worker behind it: events come from the returned
    /// [`Injector`], commands go to `sink` (a recorder, or a demo's
    /// responder that answers through the injector).
    pub fn detached(sink: Rc<dyn CommandSink>) -> (Self, Injector) {
        let (wake, wakes) = unbounded();
        let (backend, events) = Backend::detached();
        (
            Self {
                backend: Rc::new(RefCell::new(backend)),
                sink,
                wakes: Some(wakes),
                startup: None,
            },
            Injector { events, wake },
        )
    }

    /// Where the store sends its commands.
    pub fn sink(&self) -> Rc<dyn CommandSink> {
        self.sink.clone()
    }

    /// Lets the worker begin (open the archive, connect). Not before the
    /// first window is shown: the database migrations can take a while.
    pub fn release_startup(&mut self) {
        if let Some(startup) = self.startup.take() {
            startup();
        }
    }

    /// Starts the task that applies the backend's events to `store`, for as
    /// long as the returned task lives (and the store does).
    pub fn pump(&mut self, cx: &mut Context<Store>) -> Task<()> {
        let mut wakes = self.wakes.take().expect("the bridge is pumped once");
        let backend = self.backend.clone();
        cx.spawn(async move |store, cx| {
            loop {
                // Drain before waiting: events queued before the task started,
                // or between a drain and the wait, are never lost, because
                // each one also left a message in the channel.
                let events = backend.borrow().poll();
                if !events.is_empty() {
                    let applied = store.update(cx, |store, cx| {
                        for event in events {
                            store.apply_event(event, cx);
                        }
                    });
                    if applied.is_err() {
                        break;
                    }
                }
                if wakes.next().await.is_none() {
                    break;
                }
                // Several wake-ups for one batch are one drain.
                while wakes.try_recv().is_ok() {}
            }
        })
    }

    /// Asks the worker to stop and waits a little for it, as quitting does.
    pub fn shutdown(&self, wait: Duration) {
        let mut backend = self.backend.borrow_mut();
        backend.request_shutdown();
        backend.wait_for_shutdown(std::time::Instant::now() + wait);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::RecordingSink;
    use gpui::{AppContext as _, TestAppContext};
    use zapfast::backend::LinkStatus;

    #[test]
    fn the_updater_commands_are_the_ones_dropped() {
        assert!(is_updater_command(&Command::CheckForUpdates));
        assert!(is_updater_command(&Command::InspectUpdate));
        assert!(!is_updater_command(&Command::Resumed));
        assert!(!is_updater_command(&Command::Shutdown));
    }

    #[test]
    fn the_guard_does_not_pass_the_updater_commands_on() {
        let (backend, _events) = Backend::detached();
        let backend = Rc::new(RefCell::new(backend));
        let sink = GuardedBackend(backend);
        // A detached backend is offline: it would swallow every command, so
        // this checks only that the guard returns without sending to it.
        sink.send(Command::CheckForUpdates);
        sink.send(Command::InspectUpdate);
    }

    /// The waker the worker calls from its own threads: it only sends a
    /// message, and the delayed one sends it once the delay is over.
    #[test]
    fn the_waker_wakes_from_another_thread_and_after_a_delay() {
        let (wake, mut wakes) = unbounded();
        let waker = waker(wake);
        let from_thread = waker.clone();
        std::thread::spawn(move || from_thread.wake())
            .join()
            .unwrap();
        assert_eq!(futures::executor::block_on(wakes.next()), Some(()));
        assert!(wakes.try_recv().is_err(), "one call, one message");
        let started = std::time::Instant::now();
        waker.wake_after(Duration::from_millis(40));
        assert!(wakes.try_recv().is_err(), "not before the delay");
        assert_eq!(futures::executor::block_on(wakes.next()), Some(()));
        assert!(started.elapsed() >= Duration::from_millis(40));
    }

    /// An injected event reaches the store through the task, and nothing is
    /// applied while there is none.
    #[gpui::test]
    async fn injected_events_reach_the_store(cx: &mut TestAppContext) {
        let sink = Rc::new(RecordingSink::default());
        let (mut bridge, injector) = Bridge::detached(sink.clone());
        let store = cx.new(|cx| Store::new(bridge.sink(), cx));
        let task = store.update(cx, |_, cx| bridge.pump(cx));
        cx.run_until_parked();
        assert_eq!(store.read_with(cx, |store, _| store.applied), 0);

        injector.send(Event::Link(LinkStatus::Connecting));
        cx.run_until_parked();
        assert_eq!(store.read_with(cx, |store, _| store.applied), 1);

        // Two events, one wake-up each: both are applied.
        injector.send(Event::Syncing(true));
        injector.send(Event::SyncProgress(30));
        cx.run_until_parked();
        let (applied, syncing, progress) = store.read_with(cx, |store, cx| {
            let session = store.session.read(cx);
            (store.applied, session.syncing, session.sync_progress)
        });
        assert_eq!((applied, syncing, progress), (3, true, Some(30)));
        drop(task);
    }

    /// Dropping the store ends the task: it does not keep the app alive.
    #[gpui::test]
    async fn the_task_ends_with_the_store(cx: &mut TestAppContext) {
        let sink = Rc::new(RecordingSink::default());
        let (mut bridge, injector) = Bridge::detached(sink);
        let store = cx.new(|cx| Store::new(bridge.sink(), cx));
        let task = store.update(cx, |_, cx| bridge.pump(cx));
        cx.run_until_parked();
        drop(store);
        injector.send(Event::Syncing(true));
        cx.run_until_parked();
        drop(task);
    }
}
