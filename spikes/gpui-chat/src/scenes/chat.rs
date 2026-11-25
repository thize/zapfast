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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{SEED, generate};
    use crate::palette::Palette;
    use crate::ui::ActivePalette;
    use gpui::TestAppContext;

    #[gpui::test]
    fn enter_sends_and_shift_enter_adds_a_line(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(ActivePalette(Palette::dark()));
            gpui_component::init(cx);
        });
        let (scene, cx) = cx.add_window_view(|window, cx| {
            ChatScene::new(generate(SEED, 100), Stats::new(), None, window, cx)
        });
        let state = scene.read_with(cx, |scene, cx| scene.composer.read(cx).state().clone());
        cx.update(|window, cx| state.update(cx, |state, cx| state.focus(window, cx)));
        let count = |cx: &mut gpui::VisualTestContext| {
            scene.read_with(cx, |scene, cx| scene.conversation.read(cx).message_count())
        };
        let before = count(cx);

        cx.simulate_input("olá, ação");
        cx.simulate_keystrokes("shift-enter");
        cx.simulate_input("segunda linha");
        assert_eq!(count(cx), before, "Shift+Enter must not send");
        assert_eq!(
            state.read_with(cx, |state, _| state.value().to_string()),
            "olá, ação\nsegunda linha"
        );

        cx.simulate_keystrokes("enter");
        assert_eq!(count(cx), before + 1, "Enter sends");
        assert_eq!(
            state.read_with(cx, |state, _| state.value().to_string()),
            "",
            "the box is cleared"
        );
        let sent = scene.read_with(cx, |scene, cx| {
            scene
                .conversation
                .read(cx)
                .message(before)
                .text()
                .to_owned()
        });
        assert_eq!(sent, "olá, ação\nsegunda linha");

        // An empty box sends nothing.
        cx.simulate_keystrokes("enter");
        assert_eq!(count(cx), before + 1);
    }

    fn draw(cx: &mut gpui::VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    #[gpui::test]
    fn an_idle_conversation_never_re_renders(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(ActivePalette(Palette::dark()));
            gpui_component::init(cx);
        });
        let stats = Stats::new();
        let scene_stats = stats.clone();
        let (scene, cx) = cx.add_window_view(move |window, cx| {
            ChatScene::new(generate(SEED, 300), scene_stats, None, window, cx)
        });
        draw(cx);
        let settled =
            stats.renders_since(std::time::Instant::now() - std::time::Duration::from_secs(3600));
        assert!(settled >= 1);

        // Nothing is focused: ten seconds of nothing, and no further render.
        cx.background_executor
            .advance_clock(std::time::Duration::from_secs(10));
        cx.run_until_parked();
        draw(cx);
        let count = |stats: &Stats| {
            stats.renders_since(std::time::Instant::now() - std::time::Duration::from_secs(3600))
        };
        assert_eq!(count(&stats), settled);

        // With the caret blinking in the composer, the composer redraws but
        // the (cached) conversation does not.
        let state = scene.read_with(cx, |scene, cx| scene.composer.read(cx).state().clone());
        cx.update(|window, cx| state.update(cx, |state, cx| state.focus(window, cx)));
        draw(cx);
        // Focusing may re-render once; blinking must not re-render at all.
        let focused = count(&stats);
        for _ in 0..6 {
            cx.background_executor
                .advance_clock(std::time::Duration::from_millis(600));
            cx.run_until_parked();
            draw(cx);
        }
        assert_eq!(count(&stats), focused, "the caret made the list render");
    }

    /// The scroll benchmark must not wait for the platform's frames: a hidden
    /// window gets none (the run used to hang), and the benchmark's own loop
    /// draws them. Here nothing but the test draws, as in a hidden window.
    #[gpui::test]
    fn the_scroll_bench_finishes_without_platform_frames(cx: &mut TestAppContext) {
        use crate::bench::ScrollBench;
        cx.update(|cx| {
            cx.set_global(ActivePalette(Palette::dark()));
            gpui_component::init(cx);
        });
        let stats = Stats::new();
        let scene_stats = stats.clone();
        let (scene, cx) = cx.add_window_view(move |window, cx| {
            let bench = ScrollBench::with_warmup(1, false, std::time::Duration::ZERO);
            ChatScene::new(generate(SEED, 2000), scene_stats, Some(bench), window, cx)
        });
        let view = scene.read_with(cx, |scene, _| scene.conversation.clone());
        let mut frames = 0;
        while !view.read_with(cx, |view, _| view.bench_finished()) {
            assert!(frames < 100, "a one second run is 60 frames, not {frames}");
            view.update(cx, |_, cx| cx.notify());
            draw(cx);
            frames += 1;
        }
        // 60 running frames and the one that reports; the window's own first
        // draw was the run's first frame.
        assert_eq!(frames, 60);
        // 60 frames from the second one on are recorded (the first turns
        // recording on after its render began), the reporting one included.
        assert_eq!(stats.frame_gaps_ms().len(), 59);
        let top = view.read_with(cx, |view, _| view.list_top_item());
        assert!(top < 100, "the run ends at the top, not at row {top}");
    }

    /// What Edit > Copy, Paste and Select All send reaches the composer.
    #[gpui::test]
    fn the_edit_menu_actions_work_in_the_composer(cx: &mut TestAppContext) {
        use gpui_component::input;
        cx.update(|cx| {
            cx.set_global(ActivePalette(Palette::dark()));
            gpui_component::init(cx);
        });
        let (scene, cx) = cx.add_window_view(|window, cx| {
            ChatScene::new(generate(SEED, 20), Stats::new(), None, window, cx)
        });
        let state = scene.read_with(cx, |scene, cx| scene.composer.read(cx).state().clone());
        cx.update(|window, cx| state.update(cx, |state, cx| state.focus(window, cx)));
        cx.simulate_input("copiar isto");
        cx.dispatch_action(input::SelectAll);
        cx.dispatch_action(input::Copy);
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some("copiar isto")
        );
        cx.dispatch_action(input::Cut);
        assert_eq!(
            state.read_with(cx, |state, _| state.value().to_string()),
            ""
        );
        cx.dispatch_action(input::Paste);
        cx.dispatch_action(input::Paste);
        assert_eq!(
            state.read_with(cx, |state, _| state.value().to_string()),
            "copiar istocopiar isto"
        );
        cx.dispatch_action(input::Undo);
        assert_ne!(
            state.read_with(cx, |state, _| state.value().to_string()),
            "copiar istocopiar isto"
        );
    }
}
