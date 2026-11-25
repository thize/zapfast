//! The message list of the open chat: virtualized rows, bubbles with their
//! quote, reactions and ticks, the hover actions and the message menu, loading
//! older messages at the top, and the pointer handling that turns drags into a
//! selection across messages.

use super::media::{self, BubbleMedia};
use super::selectable::{self, Registry, SelectableText, TextColors};
use super::viewer::ViewerItem;
use crate::assets::WALLPAPER_TILE;
use crate::bench::Stats;
use crate::palette::Palette;
use crate::state::{Conversation, ConversationEvent, Store};
use crate::ui::{icon, locale, name_color, palette};
use crate::views::copy::{MessageView, format_selection};
use crate::views::msgview::{MsgView, quoted_sender};
use crate::views::selection::{Pos, Selection, Unit, extend_by_unit, snap_to_boundary, unit_range};
use crate::views::timefmt;
use gpui::{
    AnyElement, App, ClipboardItem, Context, CursorStyle, DispatchPhase, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement, IntoElement, ListAlignment, ListOffset, ListState,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point,
    Render, Role, SharedString, StatefulInteractiveElement, Styled, Subscription, Task, WeakEntity,
    Window, actions, canvas, div, list, point, prelude::FluentBuilder, px, relative, svg,
};
use gpui_component::input;
use std::rc::Rc;
use std::time::Duration;
use zapfast::model::{ChatId, Delivery};

actions!(
    conversation,
    [CopySelection, ClearSelection, ScrollToBottom, CloseMenu]
);

/// The reactions the menu offers.
pub const QUICK_REACTIONS: [&str; 6] = ["👍", "❤️", "😂", "😮", "😢", "🙏"];

const OVERDRAW: Pixels = px(600.);
/// Distance from the top or bottom edge at which a drag starts scrolling.
const EDGE: f32 = 48.0;
const FLASH: Duration = Duration::from_millis(1400);
const TICK: Duration = Duration::from_millis(16);

/// One row of the virtualized list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// A day separator, by day number.
    Day(i64),
    /// The unread divider, with the count of messages below it.
    Unread(usize),
    Message(usize),
}

/// The rows of `views`, with a day separator at every new day and the unread
/// divider before message `first_unread`.
pub fn build_rows(views: &[MsgView], first_unread: Option<usize>) -> (Vec<Row>, Vec<usize>) {
    let mut rows = Vec::with_capacity(views.len() + 8);
    let mut message_rows = Vec::with_capacity(views.len());
    let mut last_day = None;
    for (index, view) in views.iter().enumerate() {
        let day = timefmt::day_of(view.timestamp());
        if last_day != Some(day) {
            rows.push(Row::Day(day));
            last_day = Some(day);
        }
        if first_unread == Some(index) {
            rows.push(Row::Unread(views.len() - index));
        }
        message_rows.push(rows.len());
        rows.push(Row::Message(index));
    }
    (rows, message_rows)
}

struct Drag {
    unit: Unit,
    /// The unit the gesture started on (a word or a message; empty for chars).
    initial: (Pos, Pos),
}

/// What the message menu shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    /// Reply, react, copy: opened by a right click.
    Actions,
    /// The six quick reactions: opened by the hover button.
    Reactions,
}

#[derive(Clone, Copy, Debug)]
struct Menu {
    message: usize,
    kind: MenuKind,
    /// Window position of the click.
    at: Point<Pixels>,
}

/// A message being answered, as the reply bar quotes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplyDraft {
    pub chat: ChatId,
    pub message: String,
    pub sender: String,
    pub summary: String,
}

/// What the view tells the pane around it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConversationViewEvent {
    /// The reader chose to answer a message.
    Reply(ReplyDraft),
    /// The reader clicked the picture or clip of a message that has its file:
    /// the viewer opens on it.
    OpenMedia { chat: ChatId, message: String },
}

pub struct ConversationView {
    store: Entity<Store>,
    conversation: Entity<Conversation>,
    chat: ChatId,
    views: Vec<MsgView>,
    rows: Vec<Row>,
    /// Row index of each message.
    message_rows: Vec<usize>,
    list: ListState,
    selection: Option<Selection>,
    drag: Option<Drag>,
    registry: Registry,
    focus: FocusHandle,
    pointer: Point<Pixels>,
    /// Pixels per tick the drag scrolls by (negative scrolls up).
    scroll_speed: f32,
    autoscroll: Option<Task<()>>,
    flash: Option<usize>,
    flash_task: Option<Task<()>>,
    menu: Option<Menu>,
    /// The conversation's revision an older page was last asked for at, so a
    /// view at the top asks once per change of the state.
    asked_at: Option<u64>,
    stats: Rc<Stats>,
    /// What the bubbles' pictures, stickers and clips hold between frames.
    media: BubbleMedia,
    _conversation: Subscription,
}

impl EventEmitter<ConversationViewEvent> for ConversationView {}

impl Focusable for ConversationView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ConversationView {
    pub fn new(
        store: Entity<Store>,
        conversation: Entity<Conversation>,
        stats: Rc<Stats>,
        cx: &mut Context<Self>,
    ) -> Self {
        let chat = conversation.read(cx).chat.clone();
        let subscription = cx.subscribe(&conversation, |this, _, event: &ConversationEvent, cx| {
            this.apply_change(event, cx);
        });
        let mut this = Self {
            store,
            conversation,
            chat,
            views: Vec::new(),
            rows: Vec::new(),
            message_rows: Vec::new(),
            list: ListState::new(0, ListAlignment::Bottom, OVERDRAW),
            selection: None,
            drag: None,
            registry: Registry::default(),
            focus: cx.focus_handle(),
            pointer: Point::default(),
            scroll_speed: 0.0,
            autoscroll: None,
            flash: None,
            flash_task: None,
            menu: None,
            asked_at: None,
            stats,
            media: BubbleMedia::default(),
            _conversation: subscription,
        };
        this.rebuild(cx);
        this
    }

    // ----- following the state ---------------------------------------------

    fn unread_of(&self, cx: &App) -> usize {
        self.store
            .read(cx)
            .chats
            .read(cx)
            .chat(&self.chat)
            .map_or(0, |chat| chat.unread as usize)
    }

    /// The unread divider's place, as the message it sits before.
    fn divider_before(&self) -> Option<usize> {
        let at = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Unread(_)))?;
        match self.rows.get(at + 1) {
            Some(Row::Message(index)) => Some(*index),
            _ => None,
        }
    }

    /// Builds every row again from the conversation's messages (the first
    /// page, or a change the list cannot splice).
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let locale = locale(cx);
        let unread = self.unread_of(cx);
        let conversation = self.conversation.clone();
        self.views = conversation
            .read(cx)
            .messages
            .iter()
            .cloned()
            .map(|message| MsgView::new(message, locale))
            .collect();
        let unread = unread.min(self.views.len());
        let first_unread = (unread > 0).then(|| self.views.len() - unread);
        let (rows, message_rows) = build_rows(&self.views, first_unread);
        self.rows = rows;
        self.message_rows = message_rows;
        let views = &self.views;
        self.media
            .retain(|id| views.iter().any(|view| view.id() == id));
        self.list.reset(self.rows.len());
        self.list.scroll_to_end();
        self.selection = None;
        self.menu = None;
        cx.notify();
    }

    /// Follows one change of the conversation. A change the view cannot place
    /// (its messages and the conversation's disagree in number) rebuilds
    /// everything instead.
    fn apply_change(&mut self, event: &ConversationEvent, cx: &mut Context<Self>) {
        let locale = locale(cx);
        let conversation = self.conversation.clone();
        let total = conversation.read(cx).messages.len();
        match *event {
            ConversationEvent::Loading | ConversationEvent::Progress => {}
            ConversationEvent::Reset | ConversationEvent::Removed { .. } => self.rebuild(cx),
            ConversationEvent::Updated { index } => {
                if total != self.views.len() || index >= total {
                    return self.rebuild(cx);
                }
                let message = conversation.read(cx).messages[index].clone();
                self.views[index] = MsgView::new(message, locale);
                // The list measures the row again: its height may differ.
                let row = self.message_rows[index];
                self.list.splice(row..row + 1, 1);
            }
            ConversationEvent::Appended { count } => {
                if total != self.views.len() + count {
                    return self.rebuild(cx);
                }
                let at_end = self.at_bottom();
                let before = self.views.len();
                let fresh: Vec<MsgView> = conversation.read(cx).messages[before..]
                    .iter()
                    .cloned()
                    .map(|message| MsgView::new(message, locale))
                    .collect();
                let mine = fresh.iter().any(|view| view.outgoing());
                self.views.extend(fresh);
                let first_unread = self.divider_before();
                let (rows, message_rows) = build_rows(&self.views, first_unread);
                let old_rows = self.rows.len();
                let added = rows.len() - old_rows;
                self.rows = rows;
                self.message_rows = message_rows;
                self.list.splice(old_rows..old_rows, added);
                // Our own messages, and anything new while the reader is at the
                // end, keep the end in view.
                if mine || at_end {
                    self.list.scroll_to_end();
                }
            }
            ConversationEvent::Prepended { count } => {
                if total != self.views.len() + count || self.rows.is_empty() {
                    return self.rebuild(cx);
                }
                // Keep the message at the top of the view where it was.
                let top = self.list.logical_scroll_top();
                let anchor = self.rows[top.item_ix.min(self.rows.len() - 1)..]
                    .iter()
                    .find_map(|row| match row {
                        Row::Message(index) => Some(*index),
                        _ => None,
                    });
                let at_row = matches!(self.rows.get(top.item_ix), Some(Row::Message(_)));
                let divider = self.rows.iter().find_map(|row| match row {
                    Row::Unread(count) => Some(*count),
                    _ => None,
                });
                let old_rows = self.rows.len();
                let mut views: Vec<MsgView> = conversation.read(cx).messages[..count]
                    .iter()
                    .cloned()
                    .map(|message| MsgView::new(message, locale))
                    .collect();
                views.append(&mut self.views);
                self.views = views;
                let first_unread = divider.map(|n| self.views.len() - n);
                let (rows, message_rows) = build_rows(&self.views, first_unread);
                // Everything after the old first row is unchanged: the old
                // leading day separator is the only row that can go.
                let new_head = rows.len() - (old_rows - 1);
                self.rows = rows;
                self.message_rows = message_rows;
                self.list.splice(0..1, new_head);
                if let Some(anchor) = anchor {
                    let row = self.message_rows[anchor + count];
                    self.list.scroll_to(ListOffset {
                        item_ix: row,
                        offset_in_item: if at_row { top.offset_in_item } else { px(0.) },
                    });
                }
                if let Some(selection) = self.selection.as_mut() {
                    selection.shift(count);
                }
                if let Some(menu) = self.menu.as_mut() {
                    menu.message += count;
                }
            }
        }
        cx.notify();
    }

    #[cfg(test)]
    pub fn message(&self, index: usize) -> &MsgView {
        &self.views[index]
    }

    /// The first row the list shows (tests).
    #[cfg(test)]
    pub fn list_top_item(&self) -> usize {
        self.list.logical_scroll_top().item_ix
    }

    pub fn message_count(&self) -> usize {
        self.views.len()
    }

    pub fn set_selection(&mut self, selection: Option<Selection>, cx: &mut Context<Self>) {
        self.selection = selection;
        cx.notify();
    }

    /// Whether the list is showing its end (new messages stay in view).
    fn at_bottom(&self) -> bool {
        self.list.logical_scroll_top().item_ix >= self.list.item_count()
    }

    // ----- paging ------------------------------------------------------------

    /// Asks for older messages once the top of the list is on screen. The
    /// store sends one request at a time, from the archive and then the
    /// phone; this asks once per change of the conversation.
    fn request_older_if_at_top(&mut self, cx: &mut Context<Self>) {
        let top = self.list.logical_scroll_top();
        if top.item_ix != 0 || self.rows.is_empty() {
            return;
        }
        let conversation = self.conversation.read(cx);
        if !conversation.requested || conversation.loading_older {
            return;
        }
        if self.asked_at == Some(conversation.revision) {
            return;
        }
        self.asked_at = Some(conversation.revision);
        let (chat, store) = (self.chat.clone(), self.store.clone());
        // Not inside `render`: the store's answer notifies this very view.
        cx.spawn(async move |_, cx| {
            store.update(cx, |store, cx| store.load_older(&chat, cx));
        })
        .detach();
    }

    fn loading_older(&self, cx: &App) -> bool {
        self.conversation.read(cx).loading_older
    }

    // ----- sending, replying, reacting ---------------------------------------

    pub fn scroll_to_bottom(&mut self, cx: &mut Context<Self>) {
        self.list.scroll_to_end();
        cx.notify();
    }

    /// Puts message `index` at the top of the viewport and flashes it.
    pub fn jump_to(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(&row) = self.message_rows.get(index) else {
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

    /// Jumps to the message a quote points at, if it is loaded.
    fn jump_to_id(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(index) = self.views.iter().position(|view| view.id() == id) {
            self.jump_to(index, cx);
        }
    }

    /// Starts answering message `index`: the pane around shows the bar.
    pub fn reply_to(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(view) = self.views.get(index) else {
            return;
        };
        let draft = ReplyDraft {
            chat: self.chat.clone(),
            message: view.id().to_owned(),
            sender: view.sender.clone(),
            summary: reply_summary(view),
        };
        self.menu = None;
        cx.emit(ConversationViewEvent::Reply(draft));
        cx.notify();
    }

    /// Reacts to message `index`, or takes the reaction back.
    pub fn react_to(&mut self, index: usize, emoji: &str, cx: &mut Context<Self>) {
        let Some(view) = self.views.get(index) else {
            return;
        };
        let (chat, id) = (self.chat.clone(), view.id().to_owned());
        self.menu = None;
        self.store
            .update(cx, |store, cx| store.react(&chat, &id, emoji, cx));
        cx.notify();
    }

    fn open_menu(
        &mut self,
        message: usize,
        kind: MenuKind,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.menu = Some(Menu { message, kind, at });
        cx.notify();
    }

    /// Shows the menu of message `message` beside the message's own text
    /// (snapshots and tests; a click opens it where the pointer is).
    pub fn show_menu(&mut self, message: usize, kind: MenuKind, cx: &mut Context<Self>) {
        let viewport = self.list.viewport_bounds();
        let at = self
            .registry
            .visible(viewport)
            .into_iter()
            .find(|(index, ..)| *index == message)
            .map_or(
                point(viewport.left() + px(60.), viewport.top() + px(160.)),
                |(_, _, bounds)| point(bounds.left(), bounds.top() - px(48.)),
            );
        self.open_menu(message, kind, at, cx);
    }

    fn copy_message(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(view) = self.views.get(index) {
            let text = if view.text().is_empty() {
                view.chip.clone().unwrap_or_default()
            } else {
                view.text().to_owned()
            };
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        self.menu = None;
        cx.notify();
    }

    // ----- selection ---------------------------------------------------------

    fn text_of(&self, message: usize) -> &str {
        self.views[message].text()
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
        // A click anywhere outside the menu closes it and does nothing else.
        if self.menu.take().is_some() {
            cx.notify();
            return;
        }
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

    /// The selection as copied: one message's text as is, several as
    /// `[HH:MM, D/M/YYYY] Name: text`.
    pub fn selected_text(&self) -> Option<String> {
        let selection = self.selection?;
        let text = format_selection(&selection, |index| {
            let view = &self.views[index];
            MessageView {
                sender: &view.sender,
                timestamp: view.timestamp(),
                text: view.text(),
            }
        });
        (!text.is_empty()).then_some(text)
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Selects every message, first character to last (Edit > Select All, and
    /// Cmd+A while the list has focus).
    fn select_all(&mut self, cx: &mut Context<Self>) {
        let Some(last) = self.views.len().checked_sub(1) else {
            return;
        };
        let end = self.views[last].text().len();
        self.selection = Some(Selection::new(Pos::new(0, 0), Pos::new(last, end)));
        cx.notify();
    }

    // ----- media -------------------------------------------------------------

    /// The first picture of the chat that is loaded, for snapshots.
    pub fn first_picture(&self) -> Option<usize> {
        self.views
            .iter()
            .position(|view| matches!(view.message.content, zapfast::model::Content::Image { .. }))
    }

    /// Closes the message menu, for snapshots.
    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// The chat's pictures that have their file, oldest first, with the place
    /// of message `message` among them (`None` when it is not one).
    pub fn pictures(&self, message: &str) -> (Vec<ViewerItem>, Option<usize>) {
        let mut at = None;
        let items = self
            .views
            .iter()
            .filter_map(|view| {
                let zapfast::model::Content::Image { caption, media, .. } = &view.message.content
                else {
                    return None;
                };
                let path = media.path.clone()?;
                Some((view, caption.clone(), path))
            })
            .enumerate()
            .map(|(place, (view, caption, path))| {
                if view.id() == message {
                    at = Some(place);
                }
                ViewerItem {
                    message: view.id().to_owned(),
                    path,
                    caption: caption.filter(|c| !c.is_empty()),
                    sender: view.sender.clone(),
                    timestamp: view.timestamp(),
                }
            })
            .collect();
        (items, at)
    }

    /// Works out which message rows are on screen, from the layout just
    /// painted: their media plays, and what still has no file is asked for.
    /// Work happens only when the set of rows changes.
    fn sync_media(&mut self, cx: &mut Context<Self>) {
        let shown = self.media.painted.visible(self.list.viewport_bounds());
        let fresh: Vec<String> = shown
            .iter()
            .filter(|id| !self.media_shown(id))
            .cloned()
            .collect();
        self.media.set_shown(shown, cx);
        if fresh.is_empty() {
            return;
        }
        let auto = self.store.read(cx).auto_download;
        let wanted: Vec<String> = fresh
            .into_iter()
            .filter(|id| {
                self.conversation
                    .read(cx)
                    .message(id)
                    .and_then(|m| Some((media::Kind::of(&m.content)?, m.content.media()?)))
                    .is_some_and(|(kind, media)| media::wants_download(kind, media, auto))
            })
            .collect();
        if wanted.is_empty() {
            return;
        }
        let (store, chat) = (self.store.clone(), self.chat.clone());
        cx.defer(move |cx| {
            store.update(cx, |store, cx| {
                for id in &wanted {
                    store.download(&chat, id, false, cx);
                }
            });
        });
    }

    fn media_shown(&self, id: &str) -> bool {
        self.media.is_shown(id)
    }

    /// A click on a message's picture or clip: the file opens in the viewer,
    /// a missing one is asked for (a failed one again, with no size limit).
    pub fn media_clicked(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(view) = self.views.get(index) else {
            return;
        };
        let Some(media) = view.message.content.media() else {
            return;
        };
        let id = view.id().to_owned();
        if media.path.is_some() && self.media.toggle_clip(&id, cx) {
            return;
        }
        if media.path.is_some() {
            cx.emit(ConversationViewEvent::OpenMedia {
                chat: self.chat.clone(),
                message: id,
            });
        } else if !matches!(media.state, zapfast::model::MediaState::Downloading) {
            let chat = self.chat.clone();
            self.store
                .update(cx, |store, cx| store.download(&chat, &id, true, cx));
        }
    }

    // ----- rows --------------------------------------------------------------

    fn render_row(&mut self, ix: usize, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        match self.rows[ix] {
            Row::Day(day) => {
                let label = timefmt::long_date(day);
                div()
                    .id(("day", ix))
                    .role(Role::Heading)
                    .aria_level(2)
                    .aria_label(label.clone())
                    .child(pill(label, palette))
                    .into_any_element()
            }
            Row::Unread(count) => {
                let label = if count == 1 {
                    "1 mensagem não lida".to_owned()
                } else {
                    format!("{count} mensagens não lidas")
                };
                div()
                    .id(("unread", ix))
                    .role(Role::Label)
                    .aria_label(label.clone())
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
            Row::Message(index) => self.render_message(index, palette, cx),
        }
    }

    fn render_message(
        &mut self,
        index: usize,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let progress = self
            .conversation
            .read(cx)
            .progress
            .get(self.views[index].id())
            .copied();
        let picture = self
            .media
            .element(index, &self.views[index], progress, palette, cx);
        let view = &self.views[index];
        let outgoing = view.outgoing();
        let group = self.is_group(cx);
        let selected = self
            .selection
            .and_then(|s| s.range_in(index, view.text().len()));
        let flashing = self.flash == Some(index);

        let mut bubble = div()
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

        if !outgoing && group {
            bubble = bubble.child(
                div()
                    .text_size(px(12.5))
                    .line_height(px(17.))
                    .whitespace_nowrap()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(name_color(&view.sender, palette.dark))
                    .child(SharedString::from(view.sender.clone())),
            );
        }
        if let Some(quoted) = &view.message.quoted {
            let locale = locale(cx);
            let target = quoted.id.clone();
            let sender = quoted_sender(quoted);
            bubble =
                bubble.child(
                    div()
                        .id(("quote", index))
                        .mt(px(2.))
                        .mb(px(4.))
                        .flex()
                        .overflow_hidden()
                        .rounded(px(6.))
                        .cursor(CursorStyle::PointingHand)
                        .bg(if palette.dark {
                            gpui::hsla(0., 0., 0., 0.22)
                        } else {
                            gpui::hsla(0., 0., 0., 0.06)
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, _, cx| this.jump_to_id(&target, cx)))
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
                                        .text_color(name_color(&sender, palette.dark))
                                        .child(SharedString::from(sender)),
                                )
                                .child(div().text_color(palette.secondary).line_clamp(2).child(
                                    SharedString::from(quoted.summary_in(locale).into_owned()),
                                )),
                        ),
                );
        }
        if let Some(picture) = picture {
            bubble = bubble.child(div().mt(px(2.)).mb(px(3.)).child(picture));
        } else if let Some(chip) = &view.chip {
            bubble = bubble.child(
                div()
                    .mt(px(2.))
                    .mb(px(2.))
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(6.))
                    .bg(if palette.dark {
                        gpui::hsla(0., 0., 0., 0.18)
                    } else {
                        gpui::hsla(0., 0., 0., 0.05)
                    })
                    .text_size(px(13.5))
                    .text_color(palette.text)
                    .whitespace_nowrap()
                    .child(SharedString::from(chip.clone())),
            );
        }
        if !view.text().is_empty() {
            let colors = TextColors {
                link: palette.link,
                selection: palette.selection,
            };
            let highlights = selectable::highlights(&view.parsed.runs, selected.as_ref(), colors);
            bubble = bubble.child(SelectableText::new(
                index,
                view.shared.clone(),
                highlights,
                self.registry.clone(),
            ));
        }
        bubble = bubble.child(
            div()
                .flex()
                .justify_end()
                .items_center()
                .gap(px(3.))
                .h(px(15.))
                .text_size(px(11.))
                .text_color(palette.secondary)
                .when(view.message.edited, |footer| footer.child("editada"))
                .child(SharedString::from(timefmt::clock(view.timestamp())))
                .when(outgoing, |footer| {
                    let (path, color) = match view.message.status {
                        Delivery::Pending => ("icons/clock.svg", palette.secondary),
                        Delivery::Failed => ("icons/x.svg", gpui::hsla(0., 0.75, 0.55, 1.)),
                        Delivery::Sent | Delivery::None => {
                            ("icons/delivery-tick.svg", palette.secondary)
                        }
                        Delivery::Delivered => ("icons/delivery-ticks.svg", palette.secondary),
                        Delivery::Read | Delivery::Played => {
                            ("icons/delivery-ticks.svg", palette.read)
                        }
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
        let reactions = reaction_counts(&view.message.reactions);
        if !reactions.is_empty() {
            let pill_bg = if palette.dark {
                palette.surface_hover
            } else {
                palette.panel
            };
            column = column.child(
                div()
                    .mt(px(-6.))
                    .mx(px(8.))
                    .when(outgoing, |pill| pill.self_end())
                    .when(!outgoing, |pill| pill.self_start())
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
                    .children(reactions.iter().map(|(emoji, count, _)| {
                        SharedString::from(if *count > 1 {
                            format!("{emoji} {count}")
                        } else {
                            emoji.clone()
                        })
                    })),
            );
        }

        // Hover actions, beside the bubble on the side facing the list's
        // middle: react and reply.
        let group_name = SharedString::from(format!("message-{index}"));
        let action = |id: &'static str, label: &'static str, path: &'static str| {
            div()
                .id((id, index))
                .role(Role::Button)
                .aria_label(label)
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded_full()
                .bg(palette.surface)
                .border_1()
                .border_color(palette.outline)
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.bg(palette.surface_hover))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(icon(path, 16., palette.secondary))
        };
        let toolbar = |cx: &mut Context<Self>| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .px(px(6.))
                .invisible()
                .group_hover(group_name.clone(), |s| s.visible())
                .child(
                    action("react", "Reagir", "icons/smile.svg").on_click(cx.listener(
                        move |this, event: &gpui::ClickEvent, _, cx| {
                            this.open_menu(index, MenuKind::Reactions, event.position(), cx)
                        },
                    )),
                )
                .child(
                    action("reply", "Responder", "icons/reply.svg")
                        .on_click(cx.listener(move |this, _, _, cx| this.reply_to(index, cx))),
                )
        };

        // The row is one accessibility node: its label carries the sender, the
        // time and the text (the text itself is a custom `StyledText` element,
        // which exposes nothing), and the description what surrounds it.
        let label = message_label(&view.sender, view.timestamp(), &view_body(view));
        let description = message_description(view);
        let mut row = div()
            .id(("message", index))
            .group(group_name.clone())
            .role(Role::ListItem)
            .aria_label(label)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(self.views.len());
        if let Some(description) = description {
            row = row.aria_description(description);
        }
        row.w_full()
            .flex()
            .items_center()
            .px(px(14.))
            .py(px(2.))
            .when(outgoing, |row| row.justify_end())
            .when(flashing, |row| row.bg(palette.highlight))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.open_menu(index, MenuKind::Actions, event.position, cx)
                }),
            )
            .when(outgoing, |row| row.child(toolbar(cx)))
            .child(column)
            .when(!outgoing, |row| row.child(toolbar(cx)))
            .into_any_element()
    }

    fn is_group(&self, cx: &App) -> bool {
        self.store
            .read(cx)
            .chats
            .read(cx)
            .chat(&self.chat)
            .is_some_and(|chat| chat.kind != zapfast::model::ChatKind::Direct)
    }

    /// The message menu, over the list at the click.
    fn render_menu(&self, menu: Menu, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let origin = self.list.viewport_bounds().origin;
        let local = point(menu.at.x - origin.x, menu.at.y - origin.y);
        let index = menu.message;
        let mine = self
            .views
            .get(index)
            .and_then(|v| v.message.reactions.iter().find(|r| r.from_me))
            .map(|r| r.emoji.clone());
        let panel = div()
            .absolute()
            .left(local.x)
            .top(local.y)
            .bg(p.surface)
            .border_1()
            .border_color(p.outline)
            .rounded(px(12.))
            .shadow_md()
            .text_color(p.text)
            .text_size(px(14.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation());
        match menu.kind {
            MenuKind::Reactions => panel
                .id("reaction-bar")
                .role(Role::Menu)
                .aria_label("Reagir")
                .flex()
                .gap(px(2.))
                .p(px(6.))
                .children(QUICK_REACTIONS.iter().map(|emoji| {
                    let emoji = *emoji;
                    div()
                        .id(SharedString::from(format!("react-{emoji}")))
                        .role(Role::MenuItem)
                        .aria_label(emoji)
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(38.))
                        .rounded_full()
                        .text_size(px(24.))
                        .cursor(CursorStyle::PointingHand)
                        .when(mine.as_deref() == Some(emoji), |s| s.bg(p.surface_active))
                        .hover(|s| s.bg(p.surface_hover))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.react_to(index, emoji, cx)),
                        )
                        .child(emoji)
                }))
                .into_any_element(),
            MenuKind::Actions => {
                let item = |id: &'static str, label: &'static str, path: &'static str| {
                    div()
                        .id(id)
                        .role(Role::MenuItem)
                        .aria_label(label)
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(14.))
                        .h(px(38.))
                        .min_w(px(180.))
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.bg(p.surface_hover))
                        .child(icon(path, 18., p.secondary))
                        .child(label)
                };
                panel
                    .id("message-menu")
                    .role(Role::Menu)
                    .aria_label("Menu da mensagem")
                    .py(px(6.))
                    .flex()
                    .flex_col()
                    .child(
                        item("menu-reply", "Responder", "icons/reply.svg")
                            .on_click(cx.listener(move |this, _, _, cx| this.reply_to(index, cx))),
                    )
                    .child(
                        item("menu-react", "Reagir", "icons/smile.svg").on_click(cx.listener(
                            move |this, event: &gpui::ClickEvent, _, cx| {
                                this.open_menu(index, MenuKind::Reactions, event.position(), cx)
                            },
                        )),
                    )
                    .child(
                        item("menu-copy", "Copiar", "icons/copy.svg").on_click(
                            cx.listener(move |this, _, _, cx| this.copy_message(index, cx)),
                        ),
                    )
                    .into_any_element()
            }
        }
    }
}

/// The text a screen reader reads for a message: its text, or its chip.
fn view_body(view: &MsgView) -> String {
    match (&view.chip, view.text().is_empty()) {
        (Some(chip), true) => chip.clone(),
        (Some(chip), false) => format!("{chip}. {}", view.text()),
        (None, _) => view.text().to_owned(),
    }
}

/// The line the composer's reply bar quotes.
pub fn reply_summary(view: &MsgView) -> String {
    match (&view.chip, view.text().is_empty()) {
        (Some(chip), true) => chip.clone(),
        (Some(chip), false) => format!("{chip} {}", view.text().lines().next().unwrap_or("")),
        (None, _) => view.text().lines().next().unwrap_or("").to_owned(),
    }
}

/// Reactions grouped by emoji in order of first use: `(emoji, count, mine)`.
pub fn reaction_counts(reactions: &[zapfast::model::Reaction]) -> Vec<(String, usize, bool)> {
    let mut counts: Vec<(String, usize, bool)> = Vec::new();
    for reaction in reactions {
        match counts
            .iter_mut()
            .find(|(emoji, ..)| *emoji == reaction.emoji)
        {
            Some(entry) => {
                entry.1 += 1;
                entry.2 |= reaction.from_me;
            }
            None => counts.push((reaction.emoji.clone(), 1, reaction.from_me)),
        }
    }
    counts
}

/// What a screen reader says for a message: `Ada, 10:41: the text`.
pub fn message_label(sender: &str, timestamp: i64, text: &str) -> String {
    format!("{sender}, {}: {text}", timefmt::clock(timestamp))
}

/// The supporting detail read after the label: the message it answers,
/// reactions, and for outgoing messages whether it was delivered or read.
pub fn message_description(view: &MsgView) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(quoted) = &view.message.quoted {
        parts.push(format!(
            "Em resposta a {}: {}",
            quoted_sender(quoted),
            quoted.summary
        ));
    }
    let reactions = reaction_counts(&view.message.reactions);
    if !reactions.is_empty() {
        let reactions: Vec<String> = reactions
            .iter()
            .map(|(emoji, count, _)| {
                if *count > 1 {
                    format!("{emoji} {count}")
                } else {
                    emoji.clone()
                }
            })
            .collect();
        parts.push(format!("Reações: {}", reactions.join(", ")));
    }
    if view.outgoing() {
        parts.push(
            match view.message.status {
                Delivery::Pending => "Enviando",
                Delivery::Failed => "Falhou",
                Delivery::None | Delivery::Sent => "Enviada",
                Delivery::Delivered => "Entregue",
                Delivery::Read | Delivery::Played => "Lida",
            }
            .to_owned(),
        );
    }
    (!parts.is_empty()).then(|| parts.join(". "))
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
pub fn wallpaper(palette: Palette) -> impl IntoElement {
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
fn pointer_driver(view: WeakEntity<ConversationView>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
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

/// After the list is laid out, tells the view which rows are on screen, so
/// the media in them plays (and downloads) and the rest rests.
fn media_driver(view: WeakEntity<ConversationView>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, _, cx| {
            view.update(cx, |this, cx| this.sync_media(cx)).ok();
        },
    )
    .absolute()
    .inset_0()
}

impl Render for ConversationView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.stats.render();
        self.media.painted.next_frame();
        self.request_older_if_at_top(cx);
        let p = palette(cx);
        let show_button = !self.at_bottom();
        let weak = cx.weak_entity();
        let title = self
            .store
            .read(cx)
            .chats
            .read(cx)
            .chat(&self.chat)
            .map_or_else(String::new, |chat| chat.name.clone());
        let loading = self.loading_older(cx);
        let menu = self.menu;

        div()
            .id("conversation")
            .role(Role::List)
            .aria_label(format!("Mensagens de {title}"))
            .key_context("Conversation")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &CopySelection, _, cx| this.copy(cx)))
            // Edit > Copy and Select All send gpui-component's input actions,
            // the ones the composer handles; the message selection answers
            // them too when the list has focus.
            .on_action(cx.listener(|this, _: &input::Copy, _, cx| this.copy(cx)))
            .on_action(cx.listener(|this, _: &input::SelectAll, _, cx| this.select_all(cx)))
            .on_action(cx.listener(|this, _: &ClearSelection, _, cx| {
                this.selection = None;
                this.menu = None;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ScrollToBottom, _, cx| this.scroll_to_bottom(cx)))
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(p.chat)
            .child(wallpaper(p))
            .child(
                list(
                    self.list.clone(),
                    cx.processor(move |this, ix, _window, cx| this.render_row(ix, p, cx)),
                )
                .size_full(),
            )
            .child(pointer_driver(weak.clone()))
            .child(media_driver(weak))
            .when(loading, |root| {
                root.child(
                    div()
                        .id("loading-older")
                        .role(Role::Status)
                        .aria_label("Carregando mensagens anteriores")
                        .absolute()
                        .top(px(10.))
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .px(px(14.))
                                .py(px(6.))
                                .rounded_full()
                                .bg(p.surface)
                                .border_1()
                                .border_color(p.outline)
                                .text_size(px(12.5))
                                .text_color(p.secondary)
                                .child("Carregando mensagens anteriores…"),
                        ),
                )
            })
            .when(show_button, |root| {
                root.child(
                    div()
                        .id("scroll-to-bottom")
                        .role(Role::Button)
                        .aria_label("Ir para a última mensagem")
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
            .when_some(menu, |root, menu| root.child(self.render_menu(menu, p, cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::{ANA, FAMILY, PAGE};
    use crate::views::avatar::Avatars;
    use crate::views::panel::ChatPanel;
    use crate::views::testing::{Demo, launch};
    use gpui::{AppContext as _, TestAppContext, VisualTestContext};

    struct Open {
        demo: Demo,
        panel: Entity<ChatPanel>,
        view: Entity<ConversationView>,
    }

    /// A window with `chat` open, as the app shows it.
    fn open<'a>(cx: &'a mut TestAppContext, chat: &str) -> (Open, &'a mut VisualTestContext) {
        let demo = launch(cx, true);
        demo.store
            .update(cx, |store, cx| store.open_chat(Some(chat.to_owned()), cx));
        cx.run_until_parked();
        let conversation = demo
            .store
            .update(cx, |store, cx| store.conversation(chat, cx));
        let avatars = cx.new(|_| Avatars::default());
        let (panel, cx) = cx.add_window_view({
            let store = demo.store.clone();
            move |window, cx| {
                ChatPanel::new(
                    store,
                    conversation,
                    avatars,
                    crate::bench::Stats::new(),
                    window,
                    cx,
                )
            }
        });
        draw(cx);
        let view = panel.read_with(cx, |panel, _| panel.conversation().clone());
        (Open { demo, panel, view }, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn settle(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        draw(cx);
    }

    fn visible_text_boxes(
        view: &Entity<ConversationView>,
        cx: &mut VisualTestContext,
    ) -> Vec<(usize, gpui::Bounds<Pixels>)> {
        view.read_with(cx, |view, _| {
            view.registry
                .visible(view.list.viewport_bounds())
                .into_iter()
                .map(|(message, _, bounds)| (message, bounds))
                .collect()
        })
    }

    fn scroll_to_top(open: &Open, cx: &mut VisualTestContext) {
        open.view.update(cx, |view, cx| {
            view.list.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        settle(cx);
        settle(cx);
    }

    fn last_text_index(view: &Entity<ConversationView>, cx: &mut VisualTestContext) -> usize {
        view.read_with(cx, |view, _| {
            view.views
                .iter()
                .rposition(|v| !v.text().is_empty())
                .expect("a text message")
        })
    }

    fn unread(open: &Open, chat: &str, cx: &mut VisualTestContext) -> u32 {
        open.demo
            .store
            .read_with(cx, |s, cx| s.chats.read(cx).chat(chat).unwrap().unread)
    }

    #[gpui::test]
    fn the_first_page_loads_at_the_bottom_and_reads_the_chat(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        let count = open.view.read_with(cx, |view, _| view.message_count());
        assert_eq!(count, PAGE);
        assert!(open.view.read_with(cx, |view, _| view.at_bottom()));
        let painted = visible_text_boxes(&open.view, cx);
        assert!(
            painted.iter().any(|(message, _)| *message >= count - 3),
            "the newest messages are not painted: {painted:?}"
        );
        // Only the visible rows were laid out.
        assert!(painted.len() < count);
        assert_eq!(unread(&open, ANA, cx), 0, "opening a chat reads it");
    }

    #[gpui::test]
    fn scrolling_to_the_top_loads_older_messages_and_keeps_the_view(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        let first_before = open
            .view
            .read_with(cx, |view, _| view.message(0).id().to_owned());
        scroll_to_top(&open, cx);
        let (count, top_message) = open.view.read_with(cx, |view, _| {
            let top = view.list.logical_scroll_top();
            let first_message = view.rows[top.item_ix..]
                .iter()
                .find_map(|row| match row {
                    Row::Message(index) => Some(*index),
                    _ => None,
                })
                .expect("a message at or below the top");
            (
                view.message_count(),
                view.message(first_message).id().to_owned(),
            )
        });
        assert_eq!(count, PAGE * 2, "one older page arrived");
        assert_eq!(top_message, first_before, "the view stayed where it was");
        // Row bookkeeping stayed consistent: every message has one row.
        open.view.read_with(cx, |view, _| {
            assert_eq!(view.message_rows.len(), view.views.len());
            for (index, &row) in view.message_rows.iter().enumerate() {
                assert_eq!(view.rows[row], Row::Message(index));
            }
            assert_eq!(view.list.item_count(), view.rows.len());
        });
    }

    #[gpui::test]
    fn the_whole_history_can_be_paged_back_to_its_start(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        for _ in 0..4 {
            scroll_to_top(&open, cx);
        }
        assert_eq!(open.view.read_with(cx, |view, _| view.message_count()), 64);
        let complete = open
            .demo
            .store
            .update(cx, |s, cx| s.conversation(ANA, cx).read(cx).complete);
        assert!(complete, "the start of the chat is known");
    }

    #[gpui::test]
    fn sending_shows_the_message_at_the_end_with_its_receipts(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        let before = open.view.read_with(cx, |view, _| view.message_count());
        let composer = open
            .panel
            .read_with(cx, |panel, cx| panel.composer().read(cx).state().clone());
        cx.update(|window, cx| composer.update(cx, |state, cx| state.focus(window, cx)));
        cx.simulate_input("olá, tudo bem?");
        cx.simulate_keystrokes("enter");
        settle(cx);
        open.view.read_with(cx, |view, _| {
            assert_eq!(view.message_count(), before + 1);
            let sent = view.message(before);
            assert_eq!(sent.text(), "olá, tudo bem?");
            assert!(sent.outgoing());
            assert_eq!(sent.message.status, zapfast::model::Delivery::Delivered);
            assert!(view.at_bottom(), "the end stays in view");
        });
        assert_eq!(
            composer.read_with(cx, |state, _| state.value().to_string()),
            ""
        );
        // The chat list shows it as the latest message.
        let preview = open.demo.store.read_with(cx, |s, cx| {
            s.chats
                .read(cx)
                .chat(ANA)
                .and_then(|c| c.last.as_ref().map(|l| l.summary.clone()))
        });
        assert_eq!(preview.as_deref(), Some("olá, tudo bem?"));
        // With nothing in flight, time passing renders nothing.
        let stats = open.view.read_with(cx, |view, _| view.stats.clone());
        let settled = stats.total_renders();
        cx.background_executor
            .advance_clock(Duration::from_secs(10));
        settle(cx);
        assert_eq!(stats.total_renders(), settled);
    }

    #[gpui::test]
    fn replying_quotes_the_message_in_the_one_sent(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        let target = last_text_index(&open.view, cx);
        let target_id = open
            .view
            .read_with(cx, |view, _| view.message(target).id().to_owned());
        open.view.update(cx, |view, cx| view.reply_to(target, cx));
        settle(cx);
        let draft = open
            .panel
            .read_with(cx, |panel, _| panel.reply().cloned())
            .expect("a reply is being composed");
        assert_eq!(draft.message, target_id);

        let composer = open
            .panel
            .read_with(cx, |panel, cx| panel.composer().read(cx).state().clone());
        cx.update(|window, cx| composer.update(cx, |state, cx| state.focus(window, cx)));
        cx.simulate_input("concordo");
        cx.simulate_keystrokes("enter");
        settle(cx);
        open.view.read_with(cx, |view, _| {
            let sent = view.message(view.message_count() - 1);
            assert_eq!(sent.text(), "concordo");
            let quoted = sent.message.quoted.as_ref().expect("quotes the target");
            assert_eq!(quoted.id, target_id);
        });
        assert!(
            open.panel.read_with(cx, |panel, _| panel.reply().is_none()),
            "the reply is spent"
        );
    }

    #[gpui::test]
    fn cancelling_a_reply_sends_a_plain_message(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        open.view.update(cx, |view, cx| view.reply_to(3, cx));
        open.panel.update(cx, |panel, cx| panel.cancel_reply(cx));
        open.panel
            .update(cx, |panel, cx| panel.send("oi".into(), cx));
        settle(cx);
        open.view.read_with(cx, |view, _| {
            let sent = view.message(view.message_count() - 1);
            assert_eq!(sent.text(), "oi");
            assert!(sent.message.quoted.is_none());
        });
    }

    #[gpui::test]
    fn reacting_adds_a_pill_and_the_same_emoji_takes_it_back(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        let target = open.view.read_with(cx, |view, _| {
            view.views
                .iter()
                .rposition(|v| !v.message.reactions.iter().any(|r| r.from_me))
                .unwrap()
        });
        let mine = |cx: &mut VisualTestContext| {
            open.view.read_with(cx, |view, _| {
                view.message(target)
                    .message
                    .reactions
                    .iter()
                    .filter(|r| r.from_me)
                    .map(|r| r.emoji.clone())
                    .collect::<Vec<_>>()
            })
        };
        open.view
            .update(cx, |view, cx| view.react_to(target, "❤️", cx));
        settle(cx);
        assert_eq!(mine(cx), ["❤️"]);
        // Another emoji replaces it, as on WhatsApp.
        open.view
            .update(cx, |view, cx| view.react_to(target, "👍", cx));
        settle(cx);
        assert_eq!(mine(cx), ["👍"]);
        // The same one again takes it back.
        open.view
            .update(cx, |view, cx| view.react_to(target, "👍", cx));
        settle(cx);
        assert!(mine(cx).is_empty());
    }

    #[gpui::test]
    fn a_right_click_opens_the_menu_and_a_click_elsewhere_closes_it(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        let boxes = visible_text_boxes(&open.view, cx);
        let (message, bounds) = boxes[boxes.len() / 2];
        let at = point(bounds.left() + px(6.), bounds.top() + px(4.));
        cx.simulate_mouse_down(at, MouseButton::Right, gpui::Modifiers::none());
        cx.simulate_mouse_up(at, MouseButton::Right, gpui::Modifiers::none());
        settle(cx);
        let menu = open
            .view
            .read_with(cx, |view, _| view.menu)
            .expect("a menu");
        assert_eq!(menu.message, message);
        assert_eq!(menu.kind, MenuKind::Actions);
        // A click on the list closes it without starting a selection.
        let viewport = open
            .view
            .read_with(cx, |view, _| view.list.viewport_bounds());
        // The middle of the list is wallpaper: bubbles hug its sides.
        let away = point(viewport.center().x, bounds.top() + px(4.));
        cx.simulate_mouse_down(away, MouseButton::Left, gpui::Modifiers::none());
        cx.simulate_mouse_up(away, MouseButton::Left, gpui::Modifiers::none());
        settle(cx);
        open.view.read_with(cx, |view, _| {
            assert!(view.menu.is_none());
            assert!(view.selection.is_none());
        });
    }

    #[gpui::test]
    fn the_reaction_bar_offers_six_emoji_and_choosing_one_reacts(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        let target = PAGE - 3;
        open.view.update(cx, |view, cx| {
            view.show_menu(target, MenuKind::Reactions, cx)
        });
        settle(cx);
        assert_eq!(QUICK_REACTIONS.len(), 6);
        assert_eq!(
            open.view.read_with(cx, |view, _| view.menu.map(|m| m.kind)),
            Some(MenuKind::Reactions)
        );
        open.view
            .update(cx, |view, cx| view.react_to(target, QUICK_REACTIONS[2], cx));
        settle(cx);
        open.view.read_with(cx, |view, _| {
            assert!(view.menu.is_none());
            assert!(
                view.message(target)
                    .message
                    .reactions
                    .iter()
                    .any(|r| r.from_me && r.emoji == QUICK_REACTIONS[2])
            );
        });
    }

    #[gpui::test]
    fn dragging_across_bubbles_selects_and_copies_in_whatsapp_format(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, FAMILY);
        let boxes = visible_text_boxes(&open.view, cx);
        assert!(boxes.len() >= 4, "only {} bubbles painted", boxes.len());
        // Skip the edge bubbles: they may be cut off by the viewport.
        let (first, first_box) = boxes[1];
        let (last, last_box) = boxes[boxes.len() - 2];
        let start = point(first_box.left() + px(4.), first_box.top() + px(2.));
        let end = point(last_box.left() + px(30.), last_box.top() + px(2.));
        let none = gpui::Modifiers::none();

        cx.simulate_mouse_down(start, MouseButton::Left, none);
        draw(cx);
        cx.simulate_mouse_move(end, MouseButton::Left, none);
        draw(cx);
        cx.simulate_mouse_up(end, MouseButton::Left, none);

        let selection = open
            .view
            .read_with(cx, |view, _| view.selection)
            .expect("a selection");
        let (from, to) = selection.ordered();
        assert_eq!(from.msg, first);
        assert_eq!(to.msg, last);

        let copied = open
            .view
            .read_with(cx, |view, _| view.selected_text())
            .expect("text to copy");
        let lines: Vec<_> = copied.lines().filter(|l| l.starts_with('[')).collect();
        assert!(lines.len() >= 2, "{copied}");
        assert!(
            lines
                .iter()
                .all(|l| l.contains(", ") && l.contains("] ") && l.contains(": ")),
            "{copied}"
        );

        // The real action path puts it on the clipboard; Esc clears it.
        cx.dispatch_action(CopySelection);
        let clip = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .expect("something on the clipboard");
        assert_eq!(clip, copied);
        cx.dispatch_action(ClearSelection);
        assert!(open.view.read_with(cx, |view, _| view.selection.is_none()));
    }

    #[gpui::test]
    fn double_click_selects_a_word_and_shift_click_extends(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, FAMILY);
        let boxes = visible_text_boxes(&open.view, cx);
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
        draw(cx);
        let (text, selection) = open.view.read_with(cx, |view, _| {
            (
                view.message(message).text().to_owned(),
                view.selection.expect("selected"),
            )
        });
        let (from, to) = selection.ordered();
        assert_eq!((from.msg, to.msg), (message, message));
        let word = &text[from.offset..to.offset];
        assert!(!word.is_empty() && !word.contains(' '), "{word:?}");

        let (later, later_box) = *boxes.last().unwrap();
        let far = point(later_box.left() + px(40.), later_box.top() + px(2.));
        let shift = gpui::Modifiers {
            shift: true,
            ..gpui::Modifiers::none()
        };
        cx.simulate_mouse_down(far, MouseButton::Left, shift);
        cx.simulate_mouse_up(far, MouseButton::Left, shift);
        let extended = open
            .view
            .read_with(cx, |view, _| view.selection)
            .expect("still selected");
        assert_eq!(extended.anchor, selection.anchor);
        assert_eq!(extended.focus.msg, later);
    }

    /// Edit > Select All and Edit > Copy send gpui-component's input actions;
    /// with the list focused they act on the message selection.
    #[gpui::test]
    fn the_edit_menu_actions_select_and_copy_messages(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, FAMILY);
        cx.update(|window, cx| {
            let focus = open.view.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.dispatch_action(input::SelectAll);
        let selection = open
            .view
            .read_with(cx, |view, _| view.selection)
            .expect("Select All selects");
        assert_eq!(selection.ordered().0, Pos::new(0, 0));
        cx.dispatch_action(input::Copy);
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .expect("Copy puts text on the clipboard");
        assert!(copied.lines().filter(|l| l.starts_with('[')).count() >= 10);
    }

    #[gpui::test]
    fn an_idle_conversation_never_re_renders(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        settle(cx);
        let stats = open.view.read_with(cx, |view, _| view.stats.clone());
        let settled = stats.total_renders();
        assert!(settled >= 1);
        cx.background_executor
            .advance_clock(Duration::from_secs(10));
        settle(cx);
        assert_eq!(stats.total_renders(), settled);
        // With the caret blinking in the composer the composer redraws, the
        // (cached) conversation does not.
        let composer = open
            .panel
            .read_with(cx, |panel, cx| panel.composer().read(cx).state().clone());
        cx.update(|window, cx| composer.update(cx, |state, cx| state.focus(window, cx)));
        draw(cx);
        let focused = stats.total_renders();
        for _ in 0..6 {
            cx.background_executor
                .advance_clock(Duration::from_millis(600));
            settle(cx);
        }
        assert_eq!(
            stats.total_renders(),
            focused,
            "the caret made the list render"
        );
    }

    #[test]
    fn reactions_are_counted_by_emoji() {
        let reaction = |emoji: &str, from_me: bool| zapfast::model::Reaction {
            sender: "x".into(),
            from_me,
            emoji: emoji.into(),
        };
        let counts = reaction_counts(&[
            reaction("👍", false),
            reaction("❤️", false),
            reaction("👍", true),
        ]);
        assert_eq!(
            counts,
            vec![("👍".to_owned(), 2, true), ("❤️".to_owned(), 1, false)]
        );
    }

    #[test]
    fn screen_readers_get_sender_time_and_text() {
        let locale = zapfast::i18n::Locale::PortugueseBrazil;
        let mut message = crate::demo::history(ANA)
            .into_iter()
            .find(|m| !m.from_me && matches!(m.content, zapfast::model::Content::Text { .. }))
            .expect("an incoming text");
        message.sender_name = Some("Ana Souza".into());
        let view = MsgView::new(message, locale);
        let label = message_label(&view.sender, view.timestamp(), &view_body(&view));
        assert!(label.starts_with("Ana Souza, "), "{label}");
        // `Name, HH:MM: text`
        assert!(label.matches(':').count() >= 2, "{label}");
        let mut mine = view.clone();
        mine.message.from_me = true;
        mine.message.status = Delivery::Read;
        assert!(message_description(&mine).unwrap().ends_with("Lida"));
    }

    /// The indexes of the messages that carry a picture, sticker or clip.
    fn media_messages(view: &Entity<ConversationView>, cx: &mut VisualTestContext) -> Vec<usize> {
        view.read_with(cx, |view, _| {
            (0..view.message_count())
                .filter(|&i| media::Kind::of(&view.message(i).message.content).is_some())
                .collect()
        })
    }

    fn file_of(view: &Entity<ConversationView>, index: usize, cx: &mut VisualTestContext) -> bool {
        view.read_with(cx, |view, _| {
            view.message(index)
                .message
                .content
                .media()
                .is_some_and(|m| m.path.is_some())
        })
    }

    #[gpui::test]
    fn media_on_screen_downloads_and_the_rest_waits(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        settle(cx);
        let all = media_messages(&open.view, cx);
        assert_eq!(all.len(), 5, "the demo's pictures, stickers and clips");
        let shown: Vec<bool> = open.view.read_with(cx, |view, _| {
            all.iter()
                .map(|&i| view.media.is_shown(view.message(i).id()))
                .collect()
        });
        assert!(shown.iter().any(|&on| on), "some media is on screen");
        for (&index, &on) in all.iter().zip(&shown) {
            // Stickers always download; photos and clips follow the setting,
            // which is on: so what is on screen has its file, the rest not.
            assert_eq!(file_of(&open.view, index, cx), on, "message {index}");
        }
    }

    #[gpui::test]
    fn a_click_downloads_what_is_missing_and_opens_what_is_there(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        settle(cx);
        let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = opened.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&open.view, move |_, event: &ConversationViewEvent, _| {
                if let ConversationViewEvent::OpenMedia { message, .. } = event {
                    seen.borrow_mut().push(message.clone());
                }
            })
        });
        let all = media_messages(&open.view, cx);
        let missing = all
            .iter()
            .copied()
            .find(|&i| !file_of(&open.view, i, cx))
            .expect("a message whose file was not asked for");
        open.view
            .update(cx, |view, cx| view.media_clicked(missing, cx));
        settle(cx);
        assert!(file_of(&open.view, missing, cx), "the click downloaded it");
        assert!(opened.borrow().is_empty(), "and did not open it yet");
        open.view
            .update(cx, |view, cx| view.media_clicked(missing, cx));
        let id = open
            .view
            .read_with(cx, |view, _| view.message(missing).id().to_owned());
        assert_eq!(*opened.borrow(), vec![id], "the second click opens it");
    }

    #[gpui::test]
    fn a_gif_plays_while_shown_and_a_video_waits_for_a_click(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        settle(cx);
        settle(cx);
        // (message index, gif) of the clips whose file is there and row shown.
        let clips: Vec<(usize, bool)> = open.view.read_with(cx, |view, _| {
            (0..view.message_count())
                .filter_map(|i| {
                    let m = &view.message(i).message;
                    match m.content {
                        zapfast::model::Content::Video { gif, .. }
                            if view.media.is_shown(&m.id) && view.media.clip(&m.id).is_some() =>
                        {
                            Some((i, gif))
                        }
                        _ => None,
                    }
                })
                .collect()
        });
        let playing = |cx: &mut VisualTestContext, i: usize| {
            open.view.read_with(cx, |view, cx| {
                let id = view.message(i).id();
                view.media.clip(id).unwrap().read(cx).is_playing()
            })
        };
        assert!(clips.iter().any(|&(_, gif)| gif), "a GIF is on screen");
        assert!(clips.iter().any(|&(_, gif)| !gif), "a video is on screen");
        for &(i, gif) in &clips {
            assert_eq!(playing(cx, i), gif, "message {i}: only the GIF autoplays");
        }
        let video = clips.iter().find(|(_, gif)| !gif).unwrap().0;
        open.view
            .update(cx, |view, cx| view.media_clicked(video, cx));
        assert!(playing(cx, video), "a click plays the video");
        open.view
            .update(cx, |view, cx| view.media_clicked(video, cx));
        assert!(!playing(cx, video), "and the next one pauses it");
    }

    #[gpui::test]
    fn playing_media_does_not_wake_the_list(cx: &mut TestAppContext) {
        let (open, cx) = open(cx, ANA);
        settle(cx);
        let stats = open.view.read_with(cx, |view, _| view.stats.clone());
        let settled = stats.total_renders();
        for _ in 0..10 {
            cx.background_executor.advance_clock(Duration::from_secs(1));
            settle(cx);
        }
        assert_eq!(
            stats.total_renders(),
            settled,
            "an animated sticker redraws itself, never the list"
        );
    }

    #[test]
    fn rows_have_a_separator_per_day_and_one_unread_divider() {
        let views: Vec<MsgView> = crate::demo::history(ANA)
            .into_iter()
            .rev()
            .take(30)
            .rev()
            .map(|m| MsgView::new(m, zapfast::i18n::Locale::PortugueseBrazil))
            .collect();
        let (rows, message_rows) = build_rows(&views, Some(25));
        assert_eq!(message_rows.len(), 30);
        assert_eq!(
            rows.iter().filter(|r| matches!(r, Row::Unread(5))).count(),
            1
        );
        let days = rows.iter().filter(|r| matches!(r, Row::Day(_))).count();
        assert!(days >= 1);
        assert!(matches!(rows[0], Row::Day(_)));
        assert_eq!(rows.len(), 30 + days + 1);
    }
}
