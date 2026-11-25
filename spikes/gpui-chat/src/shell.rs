//! The window: the chat list on the left and the active scene on the right.

use crate::bench::{Cli, SceneKind, ScrollBench, Stats};
use crate::data::{self, ChatSummary};
use crate::palette::Palette;
use crate::scenes::chat::ChatScene;
use crate::scenes::media::MediaScene;
use crate::scenes::platform::PlatformScene;
use crate::ui::{ActivePalette, avatar, icon, palette};
use gpui::{
    AppContext as _, Context, CursorStyle, FocusHandle, Focusable, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, actions, div, img, prelude::FluentBuilder, px,
};
use gpui_component::theme::{Theme, ThemeMode};
use std::rc::Rc;

actions!(chat_spike, [ShowChat, ShowMedia, ShowPlatform]);

pub const SIDEBAR_WIDTH: f32 = 380.0;

pub struct Shell {
    scene: SceneKind,
    chat: gpui::Entity<ChatScene>,
    media: gpui::Entity<MediaScene>,
    platform: gpui::Entity<PlatformScene>,
    chats: Vec<ChatSummary>,
    focus: FocusHandle,
    stats: Rc<Stats>,
    _appearance: Subscription,
    _activation: Subscription,
}

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
        // A caret that blinks for a few seconds after typing, then rests:
        // nothing repaints while idle.
        theme.cursor_blink =
            gpui_component::input::CursorBlink::blinking(std::time::Duration::from_millis(530))
                .with_idle_timeout(std::time::Duration::from_secs(4));
    });
}

impl Shell {
    pub fn new(cli: &Cli, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let stats = Stats::new();
        let bench = cli
            .bench
            .filter(|(kind, _)| *kind == crate::bench::BenchKind::Scroll)
            .map(|(_, seconds)| ScrollBench::new(seconds));
        let convo = data::default_conversation();
        let chat_stats = stats.clone();
        let chat = cx.new(|cx| ChatScene::new(convo, chat_stats, bench, window, cx));
        let media = cx.new(|_| MediaScene);
        let platform = cx.new(|_| PlatformScene);

        let appearance = window.observe_window_appearance(|window, cx| {
            let palette = Palette::for_window(window.appearance());
            cx.set_global(ActivePalette(palette));
            apply_theme(palette, cx);
            window.refresh();
        });

        // GPUI redraws every view when the window gains or loses focus; that
        // is the user's doing, not idle repainting, so the bench discounts it.
        let activation_stats = stats.clone();
        let activation = cx.observe_window_activation(window, move |_, _, _| {
            activation_stats.activation_changed();
        });
        let shell = Self {
            scene: cli.scene,
            chat,
            media,
            platform,
            chats: data::chat_list(),
            focus: cx.focus_handle(),
            stats,
            _appearance: appearance,
            _activation: activation,
        };
        if let Some(index) = cli.jump {
            let chat = shell.chat.read(cx).conversation().clone();
            chat.update(cx, |view, cx| view.jump_to(index, cx));
        }
        shell.focus_scene(window, cx);
        shell
    }

    pub fn stats(&self) -> Rc<Stats> {
        self.stats.clone()
    }

    pub fn chat(&self) -> &gpui::Entity<ChatScene> {
        &self.chat
    }

    pub fn set_scene(&mut self, scene: SceneKind, window: &mut Window, cx: &mut Context<Self>) {
        self.scene = scene;
        self.focus_scene(window, cx);
        cx.notify();
    }

    /// Chat keeps focus in the list (so Cmd+C, Cmd+J and Esc work at once);
    /// the other scenes park it on the shell so Cmd+1..3 keep working.
    fn focus_scene(&self, window: &mut Window, cx: &mut gpui::App) {
        match self.scene {
            SceneKind::Chat => {
                let handle = self.chat.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }
            _ => window.focus(&self.focus, cx),
        }
    }

    fn switcher(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .p(px(2.))
            .gap(px(2.))
            .rounded(px(8.))
            .bg(p.surface)
            .children(SceneKind::ALL.into_iter().enumerate().map(|(i, kind)| {
                let active = kind == self.scene;
                div()
                    .id(("scene", i))
                    .px(px(10.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .text_size(px(12.5))
                    .cursor(CursorStyle::PointingHand)
                    .when(active, |s| {
                        s.bg(p.panel)
                            .text_color(p.text)
                            .font_weight(FontWeight::SEMIBOLD)
                    })
                    .when(!active, |s| {
                        s.text_color(p.secondary).hover(|s| s.bg(p.surface_hover))
                    })
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.set_scene(kind, window, cx)),
                    )
                    .child(SharedString::from(kind.label()))
            }))
    }

    fn chat_row(&self, index: usize, chat: &ChatSummary, p: Palette) -> impl IntoElement {
        let selected = index == 0;
        div()
            .id(("chat", index))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(14.))
            .h(px(72.))
            .px(px(14.))
            .cursor(CursorStyle::PointingHand)
            .when(selected, |row| row.bg(p.surface_active))
            .when(!selected, |row| row.hover(|row| row.bg(p.surface_hover)))
            .child(avatar(chat.name, 49., p.dark))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(16.))
                                    .text_color(p.text)
                                    .child(SharedString::from(chat.name)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(12.))
                                    .text_color(if chat.unread > 0 {
                                        p.accent
                                    } else {
                                        p.secondary
                                    })
                                    .child(SharedString::from(chat.time)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(14.))
                                    .text_color(p.secondary)
                                    .child(SharedString::from(chat.preview)),
                            )
                            .when(chat.unread > 0, |row| {
                                row.child(
                                    div()
                                        .flex()
                                        .flex_none()
                                        .items_center()
                                        .justify_center()
                                        .min_w(px(20.))
                                        .h(px(20.))
                                        .px(px(6.))
                                        .rounded_full()
                                        .text_size(px(11.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .when(chat.muted, |b| b.bg(p.secondary).text_color(p.panel))
                                        .when(!chat.muted, |b| {
                                            b.bg(p.accent).text_color(p.on_accent)
                                        })
                                        .child(SharedString::from(chat.unread.to_string())),
                                )
                            }),
                    ),
            )
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let icon_button = |path: &'static str| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(40.))
                .rounded_full()
                .child(icon(path, 22., p.secondary))
        };
        let sidebar =
            div()
                .flex_none()
                .w(px(SIDEBAR_WIDTH))
                .h_full()
                .flex()
                .flex_col()
                .bg(p.panel)
                .border_r_1()
                .border_color(p.outline)
                .child(
                    div()
                        .flex_none()
                        .h(px(60.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(16.))
                        .child(img("logo.svg").size(px(36.)).flex_none().rounded_full())
                        .child(div().flex_1())
                        .child(self.switcher(p, cx))
                        .child(icon_button("icons/plus.svg"))
                        .child(icon_button("icons/ellipsis.svg")),
                )
                .child(
                    div().flex_none().px(px(12.)).py(px(8.)).child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .h(px(36.))
                            .px(px(12.))
                            .rounded(px(10.))
                            .bg(p.surface)
                            .child(icon("icons/search.svg", 18., p.secondary))
                            .child(div().text_size(px(14.)).text_color(p.secondary).child(
                                SharedString::from("Pesquisar ou começar uma nova conversa"),
                            )),
                    ),
                )
                .child(
                    div()
                        .id("chat-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .children(
                            self.chats
                                .iter()
                                .enumerate()
                                .map(|(i, chat)| self.chat_row(i, chat, p)),
                        ),
                );

        let scene = match self.scene {
            SceneKind::Chat => self.chat.clone().into_any_element(),
            SceneKind::Media => self.media.clone().into_any_element(),
            SceneKind::Platform => self.platform.clone().into_any_element(),
        };

        div()
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ShowChat, window, cx| {
                this.set_scene(SceneKind::Chat, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowMedia, window, cx| {
                this.set_scene(SceneKind::Media, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowPlatform, window, cx| {
                this.set_scene(SceneKind::Platform, window, cx)
            }))
            .size_full()
            .flex()
            .bg(p.window)
            .text_color(p.text)
            .font_family("Inter Variable")
            .text_size(px(15.))
            .child(sidebar)
            .child(div().flex_1().min_w_0().h_full().child(scene))
    }
}
