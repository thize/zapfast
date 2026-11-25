//! The Chat scene: conversation header, the 10,000-message list, composer.

use crate::bench::{ScrollBench, Stats};
use crate::chat::composer::{Composer, Submit};
use crate::chat::conversation::ConversationView;
use crate::data::{self, Conversation};
use crate::ui::{avatar, icon, palette};
use gpui::{
    AppContext as _, Context, Entity, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StyleRefinement, Styled, Subscription, Window, div, px,
};
use std::rc::Rc;

pub const CHAT_TITLE: &str = "Família Souza";

pub struct ChatScene {
    conversation: Entity<ConversationView>,
    composer: Entity<Composer>,
    _submit: Subscription,
}

impl ChatScene {
    pub fn new(
        convo: Conversation,
        stats: Rc<Stats>,
        bench: Option<ScrollBench>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let conversation = cx.new(|cx| ConversationView::new(convo, stats, bench, cx));
        let composer = cx.new(|cx| Composer::new(window, cx));
        let submit = cx.subscribe(&composer, |this, _, Submit(text), cx| {
            let text = text.clone();
            this.conversation.update(cx, |view, cx| view.send(text, cx));
        });
        Self {
            conversation,
            composer,
            _submit: submit,
        }
    }

    pub fn conversation(&self) -> &Entity<ConversationView> {
        &self.conversation
    }

    pub fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        gpui::Focusable::focus_handle(self.conversation.read(cx), cx)
    }
}

impl Render for ChatScene {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let header_button = |path: &'static str| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(40.))
                .rounded_full()
                .child(icon(path, 22., p.secondary))
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .key_context("Chat")
            .child(
                div()
                    .flex_none()
                    .h(px(60.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(16.))
                    .bg(p.panel)
                    .border_b_1()
                    .border_color(p.outline)
                    .child(avatar(CHAT_TITLE, 40., p.dark))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .text_color(p.text)
                                    .child(SharedString::from(CHAT_TITLE)),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(p.secondary)
                                    .truncate()
                                    .child(SharedString::from(
                                        data::PARTICIPANTS
                                            .iter()
                                            .map(|n| n.split(' ').next().unwrap_or(n))
                                            .chain(["Você"])
                                            .collect::<Vec<_>>()
                                            .join(", "),
                                    )),
                            ),
                    )
                    .child(header_button("icons/video.svg"))
                    .child(header_button("icons/search.svg"))
                    .child(header_button("icons/ellipsis.svg")),
            )
            .child(
                div().flex_1().min_h_0().child(
                    // Cached: the view re-renders only when it is notified, so
                    // the composer's caret blinking never re-renders the list.
                    self.conversation
                        .clone()
                        .cached(StyleRefinement::default().size_full()),
                ),
            )
            .child(self.composer.clone())
    }
}
