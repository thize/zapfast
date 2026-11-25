//! Placeholder for the Platform scene (tray, notifications, drag and drop,
//! accessibility): part 2.

use crate::ui::palette;
use gpui::{Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, px};

pub struct PlatformScene;

impl Render for PlatformScene {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .bg(p.chat)
            .text_color(p.secondary)
            .child(
                div()
                    .text_size(px(22.))
                    .text_color(p.text)
                    .child(SharedString::from("Platform")),
            )
            .child(SharedString::from(
                "Platform integration arrives in part 2 of the spike.",
            ))
    }
}
