//! The message list: virtualized rows, bubbles, jumping, and the pointer
//! handling that turns drags into a selection across messages.

use super::selectable::{self, Registry, SelectableText, TextColors};
use crate::assets::WALLPAPER_TILE;
use crate::bench::{BenchPhase, ScrollBench, Stats, scroll_line, scroll_note};
use crate::copy::{MessageView, format_selection};
use crate::data::{Conversation, Delivery, Message, Row, SELF_NAME};
use crate::palette::Palette;
use crate::selection::{Pos, Selection, Unit, extend_by_unit, snap_to_boundary, unit_range};
use crate::timefmt;
use crate::ui::{icon, name_color, palette};
use gpui::{
    AnyElement, App, ClipboardItem, Context, CursorStyle, DispatchPhase, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ListAlignment, ListOffset, ListState, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render,
    SharedString, StatefulInteractiveElement, Styled, Task, WeakEntity, Window, actions, canvas,
    div, list, point, prelude::FluentBuilder, px, relative, svg,
};
use std::rc::Rc;
use std::time::{Duration, Instant};

actions!(
    chat_spike,
    [JumpNext, CopySelection, ClearSelection, ScrollToBottom]
);

/// The messages Cmd+J cycles through.
pub const JUMP_TARGETS: [usize; 3] = [2500, 5000, 7500];
const OVERDRAW: Pixels = px(600.);
/// Distance from the top or bottom edge at which a drag starts scrolling.
const EDGE: f32 = 48.0;
const FLASH: Duration = Duration::from_millis(1400);
const TICK: Duration = Duration::from_millis(16);

struct Drag {
    unit: Unit,
    /// The unit the gesture started on (a word or a message; empty for chars).
    initial: (Pos, Pos),
}

pub struct ConversationView {
    convo: Conversation,
    list: ListState,
    selection: Option<Selection>,
    drag: Option<Drag>,
    registry: Registry,
    focus: FocusHandle,
    pointer: Point<Pixels>,
    /// Pixels per tick the drag scrolls by (negative scrolls up).
    scroll_speed: f32,
    autoscroll: Option<Task<()>>,
    jump_step: usize,
    flash: Option<usize>,
    flash_task: Option<Task<()>>,
    stats: Rc<Stats>,
    bench: Option<ScrollBench>,
}

impl Focusable for ConversationView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ConversationView {
    pub fn new(
        convo: Conversation,
        stats: Rc<Stats>,
        bench: Option<ScrollBench>,
        cx: &mut Context<Self>,
    ) -> Self {
        let list = ListState::new(convo.rows.len(), ListAlignment::Bottom, OVERDRAW);
        Self {
            convo,
            list,
            selection: None,
            drag: None,
            registry: Registry::default(),
            focus: cx.focus_handle(),
            pointer: Point::default(),
            scroll_speed: 0.0,
            autoscroll: None,
            jump_step: 0,
            flash: None,
            flash_task: None,
            stats,
            bench,
        }
    }

    #[cfg(test)]
    pub fn message(&self, index: usize) -> &Message {
        &self.convo.messages[index]
    }

    pub fn message_count(&self) -> usize {
        self.convo.messages.len()
    }

    pub fn set_selection(&mut self, selection: Option<Selection>, cx: &mut Context<Self>) {
        self.selection = selection;
        cx.notify();
    }

    /// Whether the list is showing its end (new messages stay in view).
    fn at_bottom(&self) -> bool {
        self.list.logical_scroll_top().item_ix >= self.list.item_count()
    }

    // ----- sending and jumping ---------------------------------------------

    /// Appends an outgoing message and scrolls to it.
    pub fn send(&mut self, text: String, cx: &mut Context<Self>) {
        let timestamp = self.convo.messages.last().map_or(0, |m| m.timestamp) + 60;
        let message = Message::new(true, SELF_NAME, timestamp, text);
        let before = self.convo.rows.len();
        let added = self.convo.push(message);
        self.list.splice(before..before, added);
        self.list.scroll_to_end();
        cx.notify();
    }

    pub fn scroll_to_bottom(&mut self, cx: &mut Context<Self>) {
        self.list.scroll_to_end();
        cx.notify();
    }

    /// Puts message `index` at the top of the viewport and flashes it.
    pub fn jump_to(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(&row) = self.convo.message_rows.get(index) else {
            return;
        };
        self.list.scroll_to(ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        });
        self.flash = Some(index);
        // One-shot: a single timer that clears the highlight, no polling.
        self.flash_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FLASH).await;
            this.update(cx, |this, cx| {
                this.flash = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn jump_next(&mut self, cx: &mut Context<Self>) {
        let target = JUMP_TARGETS[self.jump_step % JUMP_TARGETS.len()];
        self.jump_step += 1;
        self.jump_to(target, cx);
    }

    // ----- selection ---------------------------------------------------------

    fn text_of(&self, message: usize) -> &str {
        self.convo.messages[message].text()
    }

    /// The text position under `at`, from the layouts painted last.
    fn hit(&self, at: Point<Pixels>) -> Option<Pos> {
        let viewport = self.list.viewport_bounds();
        let entries = self.registry.visible(viewport);
        let (message, layout, bounds) = selectable::nearest(&entries, at.y)?;
        let text = self.text_of(*message);
        // Clamp into the text box so a pointer left or right of the bubble
        // still lands on the line it is level with.
        let inside = point(
            at.x.clamp(bounds.left(), bounds.right()),
            at.y.clamp(bounds.top(), bounds.bottom() - px(1.)),
        );
        let raw = match layout.index_for_position(inside) {
            Ok(index) | Err(index) => index,
        };
        Some(Pos::new(*message, snap_to_boundary(text, raw)))
    }

    fn unit_at(&self, unit: Unit, pos: Pos) -> (Pos, Pos) {
        let range = unit_range(unit, self.text_of(pos.msg), pos.offset);
        (Pos::new(pos.msg, range.start), Pos::new(pos.msg, range.end))
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.pointer = event.position;
        let Some(pos) = self.hit(event.position) else {
            self.selection = None;
            cx.notify();
            return;
        };
        if event.modifiers.shift
            && let Some(selection) = self.selection.as_mut()
        {
            selection.extend_to(pos);
            self.drag = Some(Drag {
                unit: Unit::Char,
                initial: (pos, pos),
            });
        } else {
            let unit = match event.click_count {
                0 | 1 => Unit::Char,
                2 => Unit::Word,
                _ => Unit::Message,
            };
            let initial = self.unit_at(unit, pos);
            self.selection = Some(Selection::new(initial.0, initial.1));
            self.drag = Some(Drag { unit, initial });
        }
        cx.notify();
    }

    fn mouse_drag(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.pointer = at;
        self.follow_pointer();
        self.update_autoscroll(cx);
        cx.notify();
    }

    /// Moves the focus end of the selection to the pointer.
    fn follow_pointer(&mut self) {
        let Some(pos) = self.hit(self.pointer) else {
            return;
        };
        let Some(drag) = self.drag.as_ref() else {
            return;
        };
        let (unit, initial) = (drag.unit, drag.initial);
        match unit {
            Unit::Char => {
                if let Some(selection) = self.selection.as_mut() {
                    selection.extend_to(pos);
                }
            }
            Unit::Word | Unit::Message => {
                let under = self.unit_at(unit, pos);
                self.selection = Some(extend_by_unit(initial, under));
            }
        }
    }

    fn mouse_up(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_none() {
            return;
        }
        self.scroll_speed = 0.0;
        if self.selection.is_some_and(|s| s.is_collapsed()) {
            self.selection = None;
        }
        cx.notify();
    }

    /// Starts or stops the timer that scrolls while the pointer is held near
    /// the top or bottom edge. It only exists during such a drag.
    fn update_autoscroll(&mut self, cx: &mut Context<Self>) {
        let viewport = self.list.viewport_bounds();
        let y = self.pointer.y;
        let speed = if y < viewport.top() + px(EDGE) {
            -((viewport.top() + px(EDGE) - y).as_f32() / 3.0).clamp(2.0, 48.0)
        } else if y > viewport.bottom() - px(EDGE) {
            ((y - (viewport.bottom() - px(EDGE))).as_f32() / 3.0).clamp(2.0, 48.0)
        } else {
            0.0
        };
        self.scroll_speed = speed;
        if speed != 0.0 && self.autoscroll.is_none() {
            self.autoscroll = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(TICK).await;
                    let keep = this
                        .update(cx, |this, cx| this.autoscroll_tick(cx))
                        .unwrap_or(false);
                    if !keep {
                        break;
                    }
                }
            }));
        }
    }

    fn autoscroll_tick(&mut self, cx: &mut Context<Self>) -> bool {
        if self.drag.is_none() || self.scroll_speed == 0.0 {
            // Detach rather than drop: this runs inside the task itself.
            if let Some(task) = self.autoscroll.take() {
                task.detach();
            }
            return false;
        }
        // The text under a stationary pointer changes as the list scrolls, so
        // re-resolve it from the layouts painted since the last tick.
        self.follow_pointer();
        self.scroll_list_by(self.scroll_speed);
        cx.notify();
        true
    }

    /// Scrolls by `dy` pixels (positive is down). `ListState::scroll_by` cannot
    /// be used here: in a bottom-aligned list that sits at its end it measures
    /// from the end of the content rather than from the top of the viewport,
    /// so a small step up leaves less than a viewport below the new anchor and
    /// the next layout snaps the list straight back to the bottom
    /// (gpui-pre list.rs:554-583 against :1173-1185). The scrollbar API works
    /// in viewport-top offsets, which is what is needed.
    fn scroll_list_by(&self, dy: f32) {
        let top = -self.list.scroll_px_offset_for_scrollbar().y;
        self.list
            .set_offset_from_scrollbar(point(px(0.), -(top + px(dy))));
    }

    fn selected_text(&self) -> Option<String> {
        let selection = self.selection?;
        let text = format_selection(&selection, |index| {
            let message = &self.convo.messages[index];
            MessageView {
                sender: &message.sender,
                timestamp: message.timestamp,
                text: message.text(),
            }
        });
        (!text.is_empty()).then_some(text)
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    // ----- rows --------------------------------------------------------------

    fn render_row(&mut self, ix: usize, palette: Palette) -> AnyElement {
        match self.convo.rows[ix] {
            Row::Day(day) => pill(timefmt::long_date(day), palette).into_any_element(),
            Row::Unread(count) => {
                let label = if count == 1 {
                    "1 mensagem não lida".to_owned()
                } else {
                    format!("{count} mensagens não lidas")
                };
                div()
                    .w_full()
                    .flex()
                    .justify_center()
                    .py(px(6.))
                    .child(
                        div()
                            .w_full()
                            .py(px(5.))
                            .bg(palette.surface)
                            .text_color(palette.secondary)
                            .text_size(px(12.5))
                            .flex()
                            .justify_center()
                            .child(SharedString::from(label)),
                    )
                    .into_any_element()
            }
            Row::Message(index) => self.render_message(index, palette),
        }
    }

    fn render_message(&self, index: usize, palette: Palette) -> AnyElement {
        let message = &self.convo.messages[index];
        let selected = self
            .selection
            .and_then(|s| s.range_in(index, message.text().len()));
        let colors = TextColors {
            link: palette.link,
            selection: palette.selection,
        };
        let highlights = selectable::highlights(&message.parsed.runs, selected.as_ref(), colors);
        let body = SelectableText::new(
            index,
            message.shared.clone(),
            highlights,
            self.registry.clone(),
        );
        let outgoing = message.outgoing;
        let flashing = self.flash == Some(index);

        let mut bubble = div()
            .max_w(relative(0.65))
            .min_w(px(72.))
            .flex()
            .flex_col()
            .px(px(9.))
            .pt(px(5.))
            .pb(px(4.))
            .rounded(px(10.))
            .bg(if outgoing {
                palette.bubble_out
            } else {
                palette.bubble_in
            })
            .text_color(palette.text)
            .text_size(px(14.5))
            .line_height(px(20.))
            .cursor(CursorStyle::IBeam);

        if !outgoing {
            bubble = bubble.child(
                div()
                    .text_size(px(12.5))
                    .line_height(px(17.))
                    .whitespace_nowrap()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(name_color(&message.sender, palette.dark))
                    .child(SharedString::from(message.sender.clone())),
            );
        }
        if let Some(quote) = &message.quote {
            bubble = bubble.child(
                div()
                    .mt(px(2.))
                    .mb(px(4.))
                    .flex()
                    .overflow_hidden()
                    .rounded(px(6.))
                    .bg(if palette.dark {
                        gpui::hsla(0., 0., 0., 0.22)
                    } else {
                        gpui::hsla(0., 0., 0., 0.06)
                    })
                    .child(div().w(px(4.)).flex_none().bg(palette.accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(8.))
                            .py(px(4.))
                            .flex()
                            .flex_col()
                            .text_size(px(12.5))
                            .line_height(px(17.))
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(name_color(&quote.sender, palette.dark))
                                    .child(SharedString::from(quote.sender.clone())),
                            )
                            .child(
                                div()
                                    .text_color(palette.secondary)
                                    .line_clamp(2)
                                    .child(SharedString::from(quote.text.clone())),
                            ),
                    ),
            );
        }
        bubble = bubble.child(body).child(
            div()
                .flex()
                .justify_end()
                .items_center()
                .gap(px(3.))
                .h(px(15.))
                .text_size(px(11.))
                .text_color(palette.secondary)
                .child(SharedString::from(timefmt::clock(message.timestamp)))
                .when(outgoing, |footer| {
                    let (path, color) = match message.delivery {
                        Delivery::Sent => ("icons/delivery-tick.svg", palette.secondary),
                        Delivery::Delivered => ("icons/delivery-ticks.svg", palette.secondary),
                        Delivery::Read => ("icons/delivery-ticks.svg", palette.read),
                    };
                    footer.child(icon(path, 14., color))
                }),
        );

        let mut column = div()
            .flex()
            .flex_col()
            .max_w(relative(0.65))
            .when(outgoing, |c| c.items_end())
            .child(bubble);
        if !message.reactions.is_empty() {
            let pill_bg = if palette.dark {
                palette.surface_hover
            } else {
                palette.panel
            };
            column = column.child(
                div()
                    .mt(px(-6.))
                    .mx(px(8.))
                    .px(px(6.))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .rounded_full()
                    .bg(pill_bg)
                    .border_1()
                    .border_color(palette.outline)
                    .text_size(px(12.))
                    .text_color(palette.secondary)
                    .children(message.reactions.iter().map(|r| {
                        SharedString::from(if r.count > 1 {
                            format!("{} {}", r.emoji, r.count)
                        } else {
                            r.emoji.to_owned()
                        })
                    })),
            );
        }

        div()
            .w_full()
            .flex()
            .px(px(14.))
            .py(px(2.))
            .when(outgoing, |row| row.justify_end())
            .when(flashing, |row| row.bg(palette.highlight))
            .child(column)
            .into_any_element()
    }

    fn after_frame_bench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bench) = self.bench.as_mut() else {
            return;
        };
        let rows = self.convo.rows.len();
        match bench.tick(Instant::now()) {
            BenchPhase::Warmup => {}
            BenchPhase::Running(progress) => {
                self.stats.record_frames(true);
                let row = (((1.0 - progress) * rows as f32) as usize).min(rows - 1);
                self.list.scroll_to(ListOffset {
                    item_ix: row,
                    offset_in_item: px(0.),
                });
            }
            BenchPhase::Done => {
                self.stats.record_frames(false);
                let gaps = self.stats.frame_gaps_ms();
                println!("{}", scroll_line(&gaps));
                let (p50, p95, max) = self.stats.work_summary_ms();
                eprintln!(
                    "frame CPU cost (render to paint): p50={p50:.2} ms p95={p95:.2} ms max={max:.2} ms"
                );
                if let Some(note) = scroll_note(&gaps) {
                    // stderr: stdout carries only the PID and BENCH lines.
                    eprintln!("{note}");
                }
                cx.quit();
                return;
            }
        }
        window.request_animation_frame();
    }
}

fn pill(label: String, palette: Palette) -> impl IntoElement {
    div().w_full().flex().justify_center().py(px(8.)).child(
        div()
            .px(px(12.))
            .py(px(4.))
            .rounded(px(8.))
            .bg(palette.surface)
            .text_color(palette.secondary)
            .text_size(px(12.5))
            .child(SharedString::from(label)),
    )
}

/// Wallpaper tiles, in the doodle tint, behind the list.
fn wallpaper(palette: Palette) -> impl IntoElement {
    let (width, height) = WALLPAPER_TILE;
    div()
        .absolute()
        .inset_0()
        .overflow_hidden()
        .children((0..3).flat_map(move |row| {
            (0..4).map(move |col| {
                svg()
                    .path("wallpaper.svg")
                    .absolute()
                    .left(px(col as f32 * width))
                    .top(px(row as f32 * height))
                    .w(px(width))
                    .h(px(height))
                    .text_color(palette.doodle)
            })
        }))
}

/// Registers the window-level mouse listeners that drive selection. Window
/// level, not element level, so a drag keeps working when the pointer leaves
/// the list. It is painted before the floating button so the button, whose
/// handlers run first in the bubble phase, can stop the event.
fn pointer_driver(view: WeakEntity<ConversationView>, stats: Rc<Stats>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            stats.frame();
            let down = view.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble
                    && event.button == MouseButton::Left
                    && bounds.contains(&event.position)
                {
                    down.update(cx, |this, cx| this.mouse_down(event, window, cx))
                        .ok();
                }
            });
            let moved = view.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && event.pressed_button == Some(MouseButton::Left)
                {
                    moved
                        .update(cx, |this, cx| {
                            if this.drag.is_some() {
                                this.mouse_drag(event.position, cx);
                            }
                        })
                        .ok();
                }
            });
            let up = view.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                    up.update(cx, |this, cx| this.mouse_up(cx)).ok();
                }
            });
        },
    )
    .absolute()
    .inset_0()
}

impl Render for ConversationView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.stats.render();
        self.after_frame_bench(window, cx);
        let p = palette(cx);
        let show_button = !self.at_bottom();
        let weak = cx.weak_entity();

        div()
            .id("conversation")
            .key_context("Conversation")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &CopySelection, _, cx| this.copy(cx)))
            .on_action(cx.listener(|this, _: &ClearSelection, _, cx| {
                this.selection = None;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &JumpNext, _, cx| this.jump_next(cx)))
            .on_action(cx.listener(|this, _: &ScrollToBottom, _, cx| this.scroll_to_bottom(cx)))
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(p.chat)
            .child(wallpaper(p))
            .child(
                list(
                    self.list.clone(),
                    cx.processor(move |this, ix, _window, _cx| this.render_row(ix, p)),
                )
                .size_full(),
            )
            .child(pointer_driver(weak, self.stats.clone()))
            .when(show_button, |root| {
                root.child(
                    div()
                        .id("scroll-to-bottom")
                        .absolute()
                        .right(px(18.))
                        .bottom(px(14.))
                        .size(px(42.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(p.surface)
                        .border_1()
                        .border_color(p.outline)
                        .shadow_md()
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.bg(p.surface_hover))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, _, cx| this.scroll_to_bottom(cx)))
                        .child(icon("icons/arrow-down.svg", 22., p.secondary)),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{self, SEED};
    use crate::ui::ActivePalette;
    use gpui::{Entity, TestAppContext};

    fn open(
        cx: &mut TestAppContext,
        messages: usize,
    ) -> (Entity<ConversationView>, &mut gpui::VisualTestContext) {
        cx.update(|cx| cx.set_global(ActivePalette(Palette::dark())));
        let convo = data::generate(SEED, messages);
        let (view, cx) =
            cx.add_window_view(|_, cx| ConversationView::new(convo, Stats::new(), None, cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        (view, cx)
    }

    fn rows_with_bounds(
        view: &Entity<ConversationView>,
        cx: &mut gpui::VisualTestContext,
    ) -> usize {
        // Rows whose text was laid out and painted this frame. (A
        // bottom-aligned list answers `bounds_for_item` with `None` for every
        // row above its end anchor, so that API cannot count them.)
        view.read_with(cx, |view, _| {
            view.registry.visible(view.list.viewport_bounds()).len()
        })
    }

    #[gpui::test]
    fn the_list_measures_only_the_rows_it_shows(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, 2000);
        let total = view.read_with(cx, |view, _| view.convo.rows.len());
        assert!(total > 2000);
        let measured = rows_with_bounds(&view, cx);
        assert!(measured > 0, "the list laid out no rows");
        assert!(measured < 100, "{measured} of {total} rows laid out");
        // It starts at the bottom: the newest message is the one on screen.
        let newest = view.read_with(cx, |view, _| view.message_count() - 1);
        let painted = view.read_with(cx, |view, _| {
            view.registry.visible(view.list.viewport_bounds())
        });
        assert!(painted.iter().any(|(message, _, _)| *message == newest));
        assert!(view.read_with(cx, |view, _| view.at_bottom()));
    }

    #[gpui::test]
    fn jumping_puts_the_message_at_the_top_of_the_viewport(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, 3000);
        view.update(cx, |view, cx| view.jump_to(1500, cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let (top, row_bounds, viewport) = view.read_with(cx, |view, _| {
            let row = view.convo.message_rows[1500];
            (
                view.list.logical_scroll_top(),
                view.list.bounds_for_item(row),
                view.list.viewport_bounds(),
            )
        });
        let row = view.read_with(cx, |view, _| view.convo.message_rows[1500]);
        assert_eq!(top.item_ix, row);
        assert_eq!(top.offset_in_item, px(0.));
        assert_eq!(row_bounds.expect("laid out").top(), viewport.top());
        assert!(!view.read_with(cx, |view, _| view.at_bottom()));
        // Scrolling back to the bottom restores it.
        view.update(cx, |view, cx| view.scroll_to_bottom(cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(view.read_with(cx, |view, _| view.at_bottom()));
    }

    #[gpui::test]
    fn sending_appends_a_message_and_stays_at_the_bottom(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, 200);
        let before = view.read_with(cx, |view, _| view.message_count());
        view.update(cx, |view, cx| view.send("olá, tudo bem?".into(), cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        view.read_with(cx, |view, _| {
            assert_eq!(view.message_count(), before + 1);
            assert_eq!(view.message(before).text(), "olá, tudo bem?");
            assert!(view.message(before).outgoing);
            assert!(view.at_bottom());
            let painted = view.registry.visible(view.list.viewport_bounds());
            assert!(painted.iter().any(|(message, _, _)| *message == before));
        });
    }

    fn redraw(cx: &mut gpui::VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn visible_text_boxes(
        view: &Entity<ConversationView>,
        cx: &mut gpui::VisualTestContext,
    ) -> Vec<(usize, gpui::Bounds<Pixels>)> {
        view.read_with(cx, |view, _| {
            view.registry
                .visible(view.list.viewport_bounds())
                .into_iter()
                .map(|(message, _, bounds)| (message, bounds))
                .collect()
        })
    }

    #[gpui::test]
    fn dragging_across_bubbles_selects_and_copies_in_whatsapp_format(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, 300);
        let boxes = visible_text_boxes(&view, cx);
        assert!(boxes.len() >= 4, "only {} bubbles painted", boxes.len());
        // Skip the edge bubbles: they may be cut off by the viewport.
        let (first, first_box) = boxes[1];
        let (last, last_box) = boxes[boxes.len() - 2];
        let start = point(first_box.left() + px(4.), first_box.top() + px(2.));
        let end = point(last_box.left() + px(30.), last_box.top() + px(2.));
        let none = gpui::Modifiers::none();

        cx.simulate_mouse_down(start, MouseButton::Left, none);
        redraw(cx);
        cx.simulate_mouse_move(end, MouseButton::Left, none);
        redraw(cx);
        cx.simulate_mouse_up(end, MouseButton::Left, none);

        let selection = view
            .read_with(cx, |view, _| view.selection)
            .expect("a selection");
        let (from, to) = selection.ordered();
        assert_eq!(from.msg, first);
        assert_eq!(to.msg, last);
        assert!(to.offset > 0);

        let copied = view
            .read_with(cx, |view, _| view.selected_text())
            .expect("text to copy");
        let lines: Vec<_> = copied.lines().filter(|l| l.starts_with('[')).collect();
        assert!(lines.len() >= 3, "{copied}");
        assert!(lines[0].starts_with('['), "{copied}");
        assert!(
            lines
                .iter()
                .all(|l| l.contains(", ") && l.contains("] ") && l.contains(": "))
        );

        // Esc clears it.
        view.update(cx, |view, cx| {
            view.selection = None;
            cx.notify();
        });
        assert!(view.read_with(cx, |view, _| view.selected_text()).is_none());
    }

    #[gpui::test]
    fn double_click_selects_a_word_and_shift_click_extends(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, 300);
        let boxes = visible_text_boxes(&view, cx);
        let (message, bounds) = boxes[boxes.len() / 2];
        let at = point(bounds.left() + px(3.), bounds.top() + px(2.));
        cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: gpui::Modifiers::none(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: gpui::Modifiers::none(),
            button: MouseButton::Left,
            click_count: 2,
        });
        redraw(cx);
        let (text, selection) = view.read_with(cx, |view, _| {
            (
                view.message(message).text().to_owned(),
                view.selection.expect("selected"),
            )
        });
        let (from, to) = selection.ordered();
        assert_eq!((from.msg, to.msg), (message, message));
        let word = &text[from.offset..to.offset];
        assert!(!word.is_empty() && !word.contains(' '), "{word:?}");

        // Shift-click further down grows it across messages.
        let (later, later_box) = *boxes.last().unwrap();
        let far = point(later_box.left() + px(40.), later_box.top() + px(2.));
        let shift = gpui::Modifiers {
            shift: true,
            ..gpui::Modifiers::none()
        };
        cx.simulate_mouse_down(far, MouseButton::Left, shift);
        cx.simulate_mouse_up(far, MouseButton::Left, shift);
        let extended = view
            .read_with(cx, |view, _| view.selection)
            .expect("still selected");
        assert_eq!(extended.anchor, selection.anchor);
        assert_eq!(extended.focus.msg, later);
    }

    #[gpui::test]
    fn triple_click_selects_the_whole_message_and_a_plain_click_clears(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, 300);
        let boxes = visible_text_boxes(&view, cx);
        let (message, bounds) = boxes[1];
        let at = point(bounds.left() + px(3.), bounds.top() + px(2.));
        cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: gpui::Modifiers::none(),
            button: MouseButton::Left,
            click_count: 3,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: gpui::Modifiers::none(),
            button: MouseButton::Left,
            click_count: 3,
        });
        let (len, selection) = view.read_with(cx, |view, _| {
            (
                view.message(message).text().len(),
                view.selection.expect("selected"),
            )
        });
        assert_eq!(
            selection.ordered(),
            (Pos::new(message, 0), Pos::new(message, len))
        );
        redraw(cx);
        // A single click without a drag leaves nothing selected.
        cx.simulate_click(at, gpui::Modifiers::none());
        assert!(view.read_with(cx, |view, _| view.selection).is_none());
    }

    #[gpui::test]
    fn holding_the_pointer_at_the_top_edge_scrolls_and_extends_the_selection(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = open(cx, 600);
        let boxes = visible_text_boxes(&view, cx);
        let (anchor_msg, anchor_box) = boxes[boxes.len() - 2];
        let viewport = view.read_with(cx, |view, _| view.list.viewport_bounds());
        let none = gpui::Modifiers::none();
        let start = point(anchor_box.left() + px(4.), anchor_box.top() + px(2.));
        cx.simulate_mouse_down(start, MouseButton::Left, none);
        redraw(cx);
        // Park the pointer inside the top edge zone, then let time pass with
        // the pointer still: only the autoscroll timer moves anything.
        let edge = point(viewport.left() + px(60.), viewport.top() + px(6.));
        cx.simulate_mouse_move(edge, MouseButton::Left, none);
        for _ in 0..40 {
            cx.background_executor.advance_clock(TICK);
            cx.run_until_parked();
            redraw(cx);
        }
        redraw(cx);
        let (scrolled_up, selection) = view.read_with(cx, |view, _| {
            (!view.at_bottom(), view.selection.expect("a selection"))
        });
        assert!(scrolled_up, "the list did not scroll");
        assert_eq!(selection.anchor.msg, anchor_msg);
        assert!(
            selection.focus.msg + 3 < anchor_msg,
            "focus {} did not move well above anchor {anchor_msg}",
            selection.focus.msg
        );

        // Releasing stops the timer: nothing moves afterwards.
        cx.simulate_mouse_up(edge, MouseButton::Left, none);
        let before = view.read_with(cx, |view, _| view.list.logical_scroll_top());
        for _ in 0..10 {
            cx.background_executor.advance_clock(TICK);
            cx.run_until_parked();
        }
        let after = view.read_with(cx, |view, _| view.list.logical_scroll_top());
        assert_eq!(before, after);
        assert!(view.read_with(cx, |view, _| view.autoscroll.is_none()));

        // Copy through the real action path puts the WhatsApp format on the clipboard.
        cx.dispatch_action(CopySelection);
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .expect("something on the clipboard");
        assert!(
            copied.lines().filter(|l| l.starts_with('[')).count() >= 3,
            "{copied}"
        );

        // Esc clears the selection.
        cx.dispatch_action(ClearSelection);
        assert!(view.read_with(cx, |view, _| view.selection.is_none()));
    }

    #[gpui::test]
    fn the_cmd_j_targets_cycle(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, 8000);
        let mut seen = Vec::new();
        for _ in 0..4 {
            view.update(cx, |view, cx| {
                view.jump_next(cx);
                seen.push(view.flash);
            });
        }
        assert_eq!(seen, [Some(2500), Some(5000), Some(7500), Some(2500)]);
    }
}
