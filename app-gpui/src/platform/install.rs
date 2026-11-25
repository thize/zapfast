//! Starts the tray, the badge and the notifications and keeps them in step
//! with the state. Called once, for a window a person is going to use (not the
//! benchmarks and snapshots, which must leave nothing on the desktop).

use super::notify::{Notifier, Presence, note_for};
use super::tray::{Hub, TrayAction, action_for};
use crate::state::{ChatList, Store, StoreEvent};
use crate::ui::locale;
use crate::views::timefmt;
use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui::{App, Entity, WindowHandle};
use std::cell::RefCell;
use std::rc::Rc;

/// Brings the window to the front, as a tray click or a clicked notification
/// asks.
fn bring_forward(window: WindowHandle<gpui_component::Root>, cx: &mut App) {
    cx.activate(true);
    window
        .update(cx, |_, window, _| window.activate_window())
        .ok();
}

fn sync_badge(hub: &RefCell<Hub>, chats: &Entity<ChatList>, cx: &App) {
    let count = chats.read(cx).unread_chat_count(timefmt::now());
    hub.borrow_mut().set_count(count);
}

pub fn install(window: WindowHandle<gpui_component::Root>, store: Entity<Store>, cx: &mut App) {
    // The tray item and the Dock badge.
    let (hub, wake) = Hub::start();
    let hub = Rc::new(RefCell::new(hub));
    let chats = store.read(cx).chats.clone();
    sync_badge(&hub, &chats, cx);
    {
        let hub = hub.clone();
        cx.observe(&chats, move |chats, cx| sync_badge(&hub, &chats, cx))
            .detach();
    }
    {
        let hub = hub.clone();
        let mut wake = wake;
        cx.spawn(async move |cx| {
            while wake.next().await.is_some() {
                let events = hub.borrow().events();
                for event in events {
                    match action_for(event) {
                        Some(TrayAction::Open) => cx.update(|cx| bring_forward(window, cx)),
                        Some(TrayAction::Quit) => cx.update(|cx| cx.quit()),
                        None => {}
                    }
                }
            }
        })
        .detach();
    }

    // Notifications for messages that arrive while the reader is elsewhere,
    // and a click on one opening its chat.
    let (clicks, mut clicked) = unbounded();
    let notifier = Rc::new(RefCell::new(Notifier::new(clicks)));
    cx.subscribe(&store, {
        let store = store.clone();
        move |_, event: &StoreEvent, cx| {
            let StoreEvent::Incoming { chat, message } = event else {
                return;
            };
            let state = store.read(cx);
            let list = state.chats.read(cx);
            let Some(chat) = list.chat(chat) else {
                return;
            };
            let settings = state.settings.read(cx).get();
            let presence = Presence {
                window_active: state.is_window_active(),
                open_chat: list.open.as_deref(),
                now: timefmt::now(),
            };
            if let Some(note) = note_for(chat, message, settings, presence, locale(cx)) {
                notifier.borrow_mut().show(note);
            }
        }
    })
    .detach();
    cx.spawn(async move |cx| {
        while let Some(chat) = clicked.next().await {
            let store = store.clone();
            cx.update(|cx| {
                store.update(cx, |store, cx| store.open_chat(Some(chat), cx));
                bring_forward(window, cx);
            });
        }
    })
    .detach();
}
