//! The window's root view for phase 2: a plain skeleton that shows the state
//! is alive (the link, the chats, the open conversation's size). The screens
//! replace it; what stays is how a view holds the [`Store`] and observes only
//! the entities it draws, so an idle app renders nothing.

use std::rc::Rc;

use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, px,
};
use gpui_component::{Theme, ThemeMode};
use zapfast::backend::LinkStatus;

use crate::bench::Stats;
use crate::brand::APP_NAME;
use crate::palette::Palette;
use crate::state::Store;
use crate::ui::{ActivePalette, avatar, palette};

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
    stats: Rc<Stats>,
    _subscriptions: Vec<gpui::Subscription>,
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
            cx.observe(&chats, |_, _, cx| cx.notify()),
            appearance,
            activation,
        ];
        Self {
            store,
            stats,
            _subscriptions: subscriptions,
        }
    }

    pub fn stats(&self) -> Rc<Stats> {
        self.stats.clone()
    }

    pub fn store(&self) -> &Entity<Store> {
        &self.store
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.stats.render();
        let p = palette(cx);
        let store = self.store.read(cx);
        let session = store.session.read(cx);
        let status = match &session.link {
            LinkStatus::Starting => "Iniciando…".to_owned(),
            LinkStatus::Connecting => "Conectando…".to_owned(),
            LinkStatus::Connected => "Conectado".to_owned(),
            LinkStatus::Unlinked { .. } => match &session.qr {
                Some(qr) => format!("Aguardando leitura do QR ({0}×{0} módulos)", qr.width),
                None => "Aguardando pareamento".to_owned(),
            },
            LinkStatus::Disconnected { reason } => format!("Desconectado: {reason}"),
            LinkStatus::LoggedOut => "Aparelho desvinculado".to_owned(),
            LinkStatus::Failed(error) => format!("Falhou: {error}"),
        };
        let rows: Vec<_> = store
            .chats
            .read(cx)
            .visible()
            .into_iter()
            .map(|chat| {
                let preview = chat
                    .last
                    .as_ref()
                    .map(|last| last.summary.clone())
                    .unwrap_or_default();
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .child(avatar(&chat.name, 40.0, p.dark))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().child(SharedString::from(chat.name.clone())))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(p.secondary)
                                    .child(SharedString::from(preview)),
                            ),
                    )
            })
            .collect();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.window)
            .text_color(p.text)
            .child(
                div()
                    .px_3()
                    .py_2()
                    .bg(p.panel)
                    .child(SharedString::from(format!("{APP_NAME} — {status}"))),
            )
            .child(div().w(px(360.0)).flex().flex_col().children(rows))
    }
}
