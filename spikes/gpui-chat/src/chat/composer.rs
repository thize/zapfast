//! The message box: a gpui-component multi-line text area (which brings IME and
//! marked-text handling) between the emoji/attach buttons and send/mic.

use crate::ui::{icon, palette};
use gpui::{
    AppContext as _, Context, CursorStyle, Entity, EventEmitter, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Render, StatefulInteractiveElement, Styled, Subscription, Window,
    div, px,
};
use gpui_component::input::{InputEvent, Textarea, TextareaState};

pub const MAX_ROWS: usize = 6;

/// Emitted when the user presses Enter on a non-empty message.
pub struct Submit(pub String);

pub struct Composer {
    state: Entity<TextareaState>,
    _subscription: Subscription,
}

impl EventEmitter<Submit> for Composer {}

impl Composer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, MAX_ROWS)
                .submit_on_enter(true)
                .placeholder("Digite uma mensagem")
        });
        let subscription = cx.subscribe_in(
            &state,
            window,
            |this, state, event: &InputEvent, window, cx| {
                match event {
                    // Enter submits; Shift+Enter inserts a newline and also lands
                    // here, with `shift` set.
                    InputEvent::PressEnter { shift: false, .. } => this.submit(state, window, cx),
                    // The send/mic button follows whether there is text.
                    InputEvent::Change => cx.notify(),
                    _ => {}
                }
            },
        );
        Self {
            state,
            _subscription: subscription,
        }
    }

    #[cfg(test)]
    pub fn state(&self) -> &Entity<TextareaState> {
        &self.state
    }

    fn submit(
        &mut self,
        state: &Entity<TextareaState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = state.read(cx).value().to_string();
        let text = text.trim_end().to_owned();
        if text.trim().is_empty() {
            return;
        }
        state.update(cx, |state, cx| state.set_value("", window, cx));
        cx.emit(Submit(text));
        cx.notify();
    }
}

impl Render for Composer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let has_text = !self.state.read(cx).value().trim().is_empty();
        let state = self.state.clone();
        let button = |id: &'static str, path: &'static str| {
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(40.))
                .rounded_full()
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.bg(p.surface_hover))
                .child(icon(path, 24., p.secondary))
        };
        div()
            .w_full()
            .flex_none()
            .flex()
            .items_end()
            .gap(px(6.))
            .px(px(14.))
            .py(px(10.))
            .bg(p.panel)
            .border_t_1()
            .border_color(p.outline)
            .child(button("emoji", "icons/smile.svg"))
            .child(button("attach", "icons/paperclip.svg"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px(px(12.))
                    .py(px(5.))
                    .rounded(px(10.))
                    .bg(p.surface)
                    .text_color(p.text)
                    .text_size(px(15.))
                    .child(Textarea::new(&state).appearance(false).bordered(false)),
            )
            .child(
                button(
                    "send",
                    if has_text {
                        "icons/send.svg"
                    } else {
                        "icons/mic.svg"
                    },
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    let state = this.state.clone();
                    this.submit(&state, window, cx);
                })),
            )
    }
}
