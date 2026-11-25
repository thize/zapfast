//! The window's root view: the login screen until the account is linked, then
//! the chat list beside the open chat. It holds the [`Store`] and observes only
//! the entities it draws, so an idle app renders nothing.

use std::rc::Rc;

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement,
    ParentElement, Render, Role, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, div, img, px,
};
use gpui_component::{Theme, ThemeMode};

use crate::bench::Stats;
use crate::palette::Palette;
use crate::state::{ChatList, ChatListEvent, Session, Store};
use crate::ui::{ActivePalette, palette};
use crate::views::avatar::Avatars;
use crate::views::chat_list::ChatListView;
use crate::views::login::LoginView;
use crate::views::panel::{ChatPanel, PanelEvent};
use crate::views::viewer::{Viewer, ViewerEvent, ViewerItem};

pub const SIDEBAR_WIDTH: f32 = 400.0;

/// Makes gpui-component's theme (used by the text area) match the palette.
pub fn apply_theme(palette: Palette, cx: &mut gpui::App) {
    Theme::change(
        if palette.dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
    Theme::update(cx, |theme| {
        theme.colors.foreground = palette.text;
        theme.colors.background = palette.surface;
        theme.colors.muted_foreground = palette.secondary;
        theme.colors.caret = palette.text;
        theme.colors.selection = palette.selection;
        theme.colors.input = palette.outline;
        theme.cursor_blink = caret_policy(true);
    });
}

/// A caret that blinks for a few seconds after typing, then rests; steady (no
/// timer at all) while the window is not the active one. gpui-component only
/// restarts the blink on activation and never stops it on deactivation
/// (gpui-base input/base/state.rs:972-985), so the policy does it.
pub fn caret_policy(window_active: bool) -> gpui_component::input::CursorBlink {
    use gpui_component::input::CursorBlink;
    if window_active {
        CursorBlink::blinking(std::time::Duration::from_millis(530))
            .with_idle_timeout(std::time::Duration::from_secs(4))
    } else {
        CursorBlink::steady()
    }
}

pub struct Shell {
    store: Entity<Store>,
    session: Entity<Session>,
    chats: Entity<ChatList>,
    avatars: Entity<Avatars>,
    login: Entity<LoginView>,
    list: Entity<ChatListView>,
    panel: Option<Entity<ChatPanel>>,
    /// The picture viewer, over the whole window while it is open.
    viewer: Option<Entity<Viewer>>,
    /// What listens to the open panel and the viewer.
    _panel_events: Option<Subscription>,
    _viewer_events: Option<Subscription>,
    stats: Rc<Stats>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(
        store: Entity<Store>,
        stats: Rc<Stats>,
        forced_dark: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (session, chats) = {
            let store = store.read(cx);
            (store.session.clone(), store.chats.clone())
        };
        let avatars = cx.new(|_| Avatars::default());
        let login = cx.new(|cx| LoginView::new(store.clone(), window, cx));
        let list = cx.new(|cx| ChatListView::new(store.clone(), avatars.clone(), window, cx));
        let on_open = cx.subscribe_in(
            &chats,
            window,
            |this, _, event: &ChatListEvent, window, cx| {
                if let ChatListEvent::Opened(open) = event {
                    this.show_chat(open.clone(), window, cx);
                }
            },
        );
        let appearance = window.observe_window_appearance(move |window, cx| {
            let palette = match forced_dark {
                Some(dark) => Palette::for_appearance(dark),
                None => Palette::for_window(window.appearance()),
            };
            cx.set_global(ActivePalette(palette));
            apply_theme(palette, cx);
            window.refresh();
        });
        // GPUI redraws every view when the window gains or loses focus; the
        // idle benchmark discounts that, and the store reads or leaves a chat
        // unread by whether the window is the one in front.
        let activation_stats = stats.clone();
        let activation_store = store.clone();
        let activation = cx.observe_window_activation(window, move |_, window, cx| {
            activation_stats.activation_changed();
            let active = window.is_window_active();
            Theme::update(cx, |theme| theme.cursor_blink = caret_policy(active));
            activation_store.update(cx, |store, cx| store.set_window_active(active, cx));
        });
        let subscriptions = vec![
            cx.observe(&session, |_, _, cx| cx.notify()),
            on_open,
            appearance,
            activation,
        ];
        let mut this = Self {
            store,
            session,
            chats,
            avatars,
            login,
            list,
            panel: None,
            viewer: None,
            _panel_events: None,
            _viewer_events: None,
            stats,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        let open = this.chats.read(cx).open.clone();
        this.show_chat(open, window, cx);
        this
    }

    /// Opens the pane of `chat`, or closes it with `None`.
    fn show_chat(&mut self, chat: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = chat.map(|chat| {
            let conversation = self
                .store
                .update(cx, |store, cx| store.conversation(&chat, cx));
            let (store, avatars, stats) =
                (self.store.clone(), self.avatars.clone(), self.stats.clone());
            cx.new(|cx| ChatPanel::new(store, conversation, avatars, stats, window, cx))
        });
        self.viewer = None;
        self._viewer_events = None;
        self._panel_events = self.panel.as_ref().map(|panel| {
            cx.subscribe_in(panel, window, |this, _, event: &PanelEvent, window, cx| {
                let PanelEvent::OpenViewer { items, index } = event;
                this.open_viewer(items.clone(), *index, window, cx);
            })
        });
        if let Some(panel) = &self.panel {
            let handle = panel.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
        cx.notify();
    }

    /// Opens the picture viewer over the window.
    pub fn open_viewer(
        &mut self,
        items: Vec<ViewerItem>,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let viewer = cx.new(|cx| Viewer::new(items, index, cx));
        self._viewer_events = Some(cx.subscribe_in(
            &viewer,
            window,
            |this, _, ViewerEvent::Close, window, cx| this.close_viewer(window, cx),
        ));
        window.focus(&viewer.read(cx).focus_handle(cx), cx);
        self.viewer = Some(viewer);
        cx.notify();
    }

    /// Closes the picture viewer, and the chat has the keys again.
    pub fn close_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.viewer.take().is_none() {
            return;
        }
        self._viewer_events = None;
        if let Some(panel) = &self.panel {
            let handle = panel.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
        cx.notify();
    }

    pub fn viewer(&self) -> Option<&Entity<Viewer>> {
        self.viewer.as_ref()
    }

    pub fn stats(&self) -> Rc<Stats> {
        self.stats.clone()
    }

    pub fn store(&self) -> &Entity<Store> {
        &self.store
    }

    pub fn login(&self) -> &Entity<LoginView> {
        &self.login
    }

    pub fn chat_list(&self) -> &Entity<ChatListView> {
        &self.list
    }

    pub fn panel(&self) -> Option<&Entity<ChatPanel>> {
        self.panel.as_ref()
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.stats.render();
        let p = palette(cx);
        if self.session.read(cx).needs_login() {
            return div()
                .size_full()
                .key_context("Shell")
                .track_focus(&self.focus)
                .child(self.login.clone())
                .into_any_element();
        }
        let right = match &self.panel {
            Some(panel) => div().size_full().child(panel.clone()).into_any_element(),
            None => div()
                .id("no-chat")
                .role(Role::Group)
                .aria_label("Nenhuma conversa aberta")
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(14.))
                .bg(p.window)
                .child(img("logo.svg").size(px(72.)))
                .child(div().text_size(px(28.)).text_color(p.text).child("ZapFast"))
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(p.secondary)
                        .child(SharedString::from("Escolha uma conversa para começar.")),
                )
                .into_any_element(),
        };
        div()
            .size_full()
            .relative()
            .flex()
            .key_context("Shell")
            .track_focus(&self.focus)
            .bg(p.window)
            .child(
                div()
                    .flex_none()
                    .w(px(SIDEBAR_WIDTH))
                    .h_full()
                    .border_r_1()
                    .border_color(p.outline)
                    .child(self.list.clone()),
            )
            .child(div().flex_1().min_w_0().h_full().child(right))
            .children(self.viewer.clone())
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::{self, ANA, PAGE};
    use crate::views::testing::launch;
    use gpui::TestAppContext;

    #[gpui::test]
    fn a_picture_opens_in_the_viewer_zooms_and_escape_closes_it(cx: &mut TestAppContext) {
        let demo = launch(cx, true);
        let store = demo.store.clone();
        let (shell, cx) = cx.add_window_view({
            let store = store.clone();
            move |window, cx| Shell::new(store, Stats::new(), Some(true), window, cx)
        });
        store.update(cx, |store, cx| store.open_chat(Some(ANA.to_owned()), cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let view = shell
            .read_with(cx, |shell, cx| {
                shell
                    .panel()
                    .map(|panel| panel.read(cx).conversation().clone())
            })
            .expect("the panel opened");
        let photo = view
            .read_with(cx, |view, _| {
                (0..view.message_count()).find(|&i| {
                    matches!(
                        view.message(i).message.content,
                        zapfast::model::Content::Image { .. }
                    )
                })
            })
            .expect("a photo in the first page");
        // The first click downloads the picture, the second opens it.
        view.update(cx, |view, cx| view.media_clicked(photo, cx));
        cx.run_until_parked();
        assert!(shell.read_with(cx, |shell, _| shell.viewer().is_none()));
        view.update(cx, |view, cx| view.media_clicked(photo, cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let viewer = shell
            .read_with(cx, |shell, _| shell.viewer().cloned())
            .expect("the viewer opened");
        assert_eq!(viewer.read_with(cx, |v, _| v.count()), 1);

        cx.update(|window, cx| {
            viewer.update(cx, |viewer, cx| {
                viewer.zoom_by(crate::views::viewer::ZOOM_STEP, None, window, cx)
            })
        });
        assert_eq!(
            viewer.read_with(cx, |v, _| v.zoom().scale),
            crate::views::viewer::ZOOM_STEP
        );
        cx.update(|window, cx| window.draw(cx).clear(cx));

        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(shell.read_with(cx, |shell, _| shell.viewer().is_none()));
    }

    #[gpui::test]
    fn login_gives_way_to_the_chats_once_linked_and_a_chat_opens_and_closes(
        cx: &mut TestAppContext,
    ) {
        let demo = launch(cx, false);
        let store = demo.store.clone();
        let (shell, cx) = cx.add_window_view({
            let store = store.clone();
            move |window, cx| Shell::new(store, Stats::new(), Some(true), window, cx)
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(store.read_with(cx, |s, cx| s.session.read(cx).needs_login()));
        assert!(shell.read_with(cx, |shell, _| shell.panel().is_none()));

        // The phone accepts the link: the chat list takes the window.
        demo::start(&demo.injector);
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(!store.read_with(cx, |s, cx| s.session.read(cx).needs_login()));
        assert_eq!(
            shell.read_with(cx, |shell, cx| shell
                .chat_list()
                .read(cx)
                .visible_ids()
                .len()),
            5
        );
        assert!(shell.read_with(cx, |shell, _| shell.panel().is_none()));

        // Opening a chat shows its conversation, loaded from the backend.
        store.update(cx, |store, cx| store.open_chat(Some(ANA.to_owned()), cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let panel = shell
            .read_with(cx, |shell, _| shell.panel().cloned())
            .expect("the panel opened");
        assert_eq!(panel.read_with(cx, |panel, _| panel.chat().clone()), ANA);
        assert_eq!(
            panel.read_with(cx, |panel, cx| panel
                .conversation()
                .read(cx)
                .message_count()),
            PAGE
        );

        // Closing it goes back to the empty pane.
        store.update(cx, |store, cx| store.open_chat(None, cx));
        cx.run_until_parked();
        assert!(shell.read_with(cx, |shell, _| shell.panel().is_none()));
    }

    #[gpui::test]
    fn a_window_in_the_background_does_not_read_the_open_chat(cx: &mut TestAppContext) {
        let demo = launch(cx, true);
        let store = demo.store.clone();
        let unread = |cx: &mut TestAppContext| {
            store.read_with(cx, |s, cx| s.chats.read(cx).chat(ANA).unwrap().unread)
        };
        store.update(cx, |store, cx| store.set_window_active(false, cx));
        store.update(cx, |store, cx| store.open_chat(Some(ANA.to_owned()), cx));
        cx.run_until_parked();
        assert!(
            unread(cx) > 0,
            "an open chat behind other windows stays unread"
        );
        store.update(cx, |store, cx| store.set_window_active(true, cx));
        cx.run_until_parked();
        assert_eq!(unread(cx), 0, "coming back to the front reads it");
    }
}
