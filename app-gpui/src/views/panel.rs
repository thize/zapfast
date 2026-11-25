//! The right-hand pane for an open chat: header, messages, the reply bar and
//! the composer.

use super::composer::{Composer, Submit};
use super::conversation::{ConversationView, ConversationViewEvent, ReplyDraft};
use crate::bench::Stats;
use crate::state::{ChatList, Conversation, Store};
use crate::ui::{avatar_picture as avatar, icon, name_color, palette};
use crate::views::avatar::Avatars;
use gpui::{
    AppContext as _, Context, CursorStyle, Entity, FocusHandle, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Render, Role, SharedString, StatefulInteractiveElement,
    StyleRefinement, Styled, Subscription, Window, div, px,
};
use std::rc::Rc;
use zapfast::model::{ChatId, ChatKind, ReplyTarget};

pub struct ChatPanel {
    store: Entity<Store>,
    chats: Entity<ChatList>,
    avatars: Entity<Avatars>,
    chat: ChatId,
    conversation: Entity<ConversationView>,
    composer: Entity<Composer>,
    /// The message being answered, drawn as the bar above the composer.
    reply: Option<ReplyDraft>,
    _subscriptions: Vec<Subscription>,
}

impl ChatPanel {
    pub fn new(
        store: Entity<Store>,
        conversation: Entity<Conversation>,
        avatars: Entity<Avatars>,
        stats: Rc<Stats>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let chat = conversation.read(cx).chat.clone();
        let chats = store.read(cx).chats.clone();
        let view = {
            let store = store.clone();
            cx.new(|cx| ConversationView::new(store, conversation, stats, cx))
        };
        let composer = cx.new(|cx| Composer::new(window, cx));
        let on_submit = cx.subscribe(&composer, |this, _, Submit(text), cx| {
            this.send(text.clone(), cx);
        });
        // Choosing to reply puts the caret in the box; the bar itself is drawn
        // from the draft.
        let on_reply = cx.subscribe_in(
            &view,
            window,
            |this, _, event: &ConversationViewEvent, window, cx| match event {
                ConversationViewEvent::Reply(draft) => {
                    this.reply = Some(draft.clone());
                    this.composer
                        .update(cx, |composer, cx| composer.focus(window, cx));
                    cx.notify();
                }
                ConversationViewEvent::OpenMedia { .. } => {}
            },
        );
        let on_chats = cx.observe(&chats, |_, _, cx| cx.notify());
        let on_avatars = cx.observe(&avatars, |_, _, cx| cx.notify());
        Self {
            store,
            chats,
            avatars,
            chat,
            conversation: view,
            composer,
            reply: None,
            _subscriptions: vec![on_submit, on_reply, on_chats, on_avatars],
        }
    }

    /// Sends `text`, quoting the message being answered; the bar goes away
    /// once the store took the text.
    pub fn send(&mut self, text: String, cx: &mut Context<Self>) {
        let quoting = self
            .reply
            .as_ref()
            .map(|draft| ReplyTarget::from(draft.message.as_str()));
        let chat = self.chat.clone();
        let sent = self
            .store
            .update(cx, |store, cx| store.send_text(&chat, &text, quoting, cx));
        if sent && self.reply.take().is_some() {
            cx.notify();
        }
    }

    pub fn cancel_reply(&mut self, cx: &mut Context<Self>) {
        if self.reply.take().is_some() {
            cx.notify();
        }
    }

    pub fn reply(&self) -> Option<&ReplyDraft> {
        self.reply.as_ref()
    }

    pub fn chat(&self) -> &ChatId {
        &self.chat
    }

    pub fn conversation(&self) -> &Entity<ConversationView> {
        &self.conversation
    }

    pub fn composer(&self) -> &Entity<Composer> {
        &self.composer
    }

    pub fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        gpui::Focusable::focus_handle(self.conversation.read(cx), cx)
    }
}

impl Render for ChatPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let chat = self.chats.read(cx).chat(&self.chat).cloned();
        let name = chat.as_ref().map_or_else(String::new, |c| c.name.clone());
        let subtitle = chat.as_ref().and_then(|chat| match chat.kind {
            ChatKind::Group => Some(format!("{} participantes", chat.participants.len() + 1)),
            _ => None,
        });
        let picture = self
            .chats
            .read(cx)
            .avatar(&self.chat)
            .cloned()
            .flatten()
            .and_then(|path| {
                self.avatars
                    .update(cx, |avatars, cx| avatars.get(&path, cx))
            });
        let reply = self.reply.clone();
        let header_button = |id: &'static str, label: &'static str, path: &'static str| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .flex()
                .items_center()
                .justify_center()
                .size(px(40.))
                .rounded_full()
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.bg(p.surface_hover))
                .child(icon(path, 22., p.secondary))
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .key_context("Chat")
            .child(
                div()
                    .id("chat-header")
                    .role(Role::Banner)
                    .aria_label(format!("Conversa com {name}"))
                    .flex_none()
                    .h(px(60.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(16.))
                    .bg(p.panel)
                    .border_b_1()
                    .border_color(p.outline)
                    .child(avatar(&name, picture, 40., p.dark))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(16.))
                                    .text_color(p.text)
                                    .child(SharedString::from(name)),
                            )
                            .children(subtitle.map(|text| {
                                div()
                                    .truncate()
                                    .text_size(px(13.))
                                    .text_color(p.secondary)
                                    .child(SharedString::from(text))
                            })),
                    )
                    .child(header_button(
                        "call-video",
                        "Chamada de vídeo",
                        "icons/video.svg",
                    ))
                    .child(header_button("search", "Pesquisar", "icons/search.svg"))
                    .child(header_button("more", "Mais opções", "icons/ellipsis.svg")),
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
            .children(reply.map(|reply| {
                div()
                    .id("reply-bar")
                    .role(Role::Group)
                    .aria_label(format!("Respondendo a {}", reply.sender))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(14.))
                    .pt(px(8.))
                    .bg(p.panel)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .overflow_hidden()
                            .rounded(px(8.))
                            .bg(p.surface)
                            .child(div().w(px(4.)).flex_none().bg(p.accent))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .px(px(10.))
                                    .py(px(6.))
                                    .flex()
                                    .flex_col()
                                    .text_size(px(13.))
                                    .child(
                                        div()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(name_color(&reply.sender, p.dark))
                                            .child(SharedString::from(reply.sender.clone())),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_color(p.secondary)
                                            .child(SharedString::from(reply.summary.clone())),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("cancel-reply")
                            .role(Role::Button)
                            .aria_label("Cancelar resposta")
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(32.))
                            .rounded_full()
                            .cursor(CursorStyle::PointingHand)
                            .hover(|s| s.bg(p.surface_hover))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_reply(cx)))
                            .child(icon("icons/x.svg", 20., p.secondary)),
                    )
            }))
            .child(self.composer.clone())
    }
}
