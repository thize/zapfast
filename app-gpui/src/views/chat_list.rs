//! The chat list: a search box over rows of avatar, name, preview, time and
//! unread badge, pinned chats first.

use crate::palette::Palette;
use crate::state::{ChatList, Store};
use crate::ui::{avatar_picture as avatar, icon, locale, palette};
use crate::views::avatar::Avatars;
use crate::views::msgview::preview;
use crate::views::timefmt;
use gpui::{
    AnyElement, AppContext as _, Context, CursorStyle, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, Role, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, div, prelude::FluentBuilder, px, uniform_list,
};
use gpui_component::input::{Input, InputEvent, InputState};
use zapfast::model::{Chat, ChatId, Delivery};

pub const ROW_HEIGHT: f32 = 72.0;

pub struct ChatListView {
    store: Entity<Store>,
    chats: Entity<ChatList>,
    avatars: Entity<Avatars>,
    search: Entity<InputState>,
    /// The chats the list shows, in order: the state's `visible()` with the
    /// search applied, recomputed when the state changes.
    visible: Vec<ChatId>,
    _subscriptions: Vec<Subscription>,
}

impl ChatListView {
    pub fn new(
        store: Entity<Store>,
        avatars: Entity<Avatars>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let chats = store.read(cx).chats.clone();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Pesquisar conversas"));
        let on_chats = cx.observe(&chats, |this, _, cx| {
            this.refresh(cx);
            cx.notify();
        });
        let on_avatars = cx.observe(&avatars, |_, _, cx| cx.notify());
        let on_search = cx.subscribe_in(
            &search,
            window,
            |this, search, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = search.read(cx).value().to_string();
                    this.set_query(query, cx);
                }
            },
        );
        let mut this = Self {
            store,
            chats,
            avatars,
            search,
            visible: Vec::new(),
            _subscriptions: vec![on_chats, on_avatars, on_search],
        };
        this.refresh(cx);
        this
    }

    /// Shows only the chats that match `query` (what the search box holds).
    pub fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        self.chats.update(cx, |list, cx| list.set_query(query, cx));
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.visible = self
            .chats
            .read(cx)
            .visible()
            .iter()
            .map(|chat| chat.id.clone())
            .collect();
    }

    /// The chats shown, in order (tests).
    pub fn visible_ids(&self) -> &[ChatId] {
        &self.visible
    }

    pub fn search_state(&self) -> &Entity<InputState> {
        &self.search
    }

    fn open(&mut self, id: ChatId, cx: &mut Context<Self>) {
        self.store
            .update(cx, |store, cx| store.open_chat(Some(id), cx));
    }

    /// The rows in `range`: each asks for its picture the first time it is
    /// drawn, and decodes the file once the backend has answered.
    fn rows(&mut self, range: std::ops::Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = palette(cx);
        let locale = locale(cx);
        let open = self.chats.read(cx).open.clone();
        let mut out = Vec::with_capacity(range.len());
        for index in range {
            let Some(id) = self.visible.get(index).cloned() else {
                continue;
            };
            let Some(chat) = self.chats.read(cx).chat(&id).cloned() else {
                continue;
            };
            let known = self.chats.read(cx).avatar(&id).cloned();
            if known.is_none() {
                self.store
                    .update(cx, |store, cx| store.request_avatar(&id, cx));
            }
            let picture = known.flatten().and_then(|path| {
                self.avatars
                    .update(cx, |avatars, cx| avatars.get(&path, cx))
            });
            let selected = open.as_deref() == Some(id.as_str());
            out.push(
                row(&chat, picture, selected, locale, p)
                    .on_click(cx.listener(move |this, _, _, cx| this.open(id.clone(), cx)))
                    .into_any_element(),
            );
        }
        out
    }
}

fn tick_for(status: Delivery, p: Palette) -> Option<(&'static str, gpui::Hsla)> {
    Some(match status {
        Delivery::None => return None,
        Delivery::Pending => ("icons/clock.svg", p.secondary),
        Delivery::Failed => ("icons/x.svg", gpui::hsla(0., 0.75, 0.55, 1.)),
        Delivery::Sent => ("icons/delivery-tick.svg", p.secondary),
        Delivery::Delivered => ("icons/delivery-ticks.svg", p.secondary),
        Delivery::Read | Delivery::Played => ("icons/delivery-ticks.svg", p.read),
    })
}

/// One row, from the state's chat.
fn row(
    chat: &Chat,
    picture: Option<std::sync::Arc<gpui::RenderImage>>,
    selected: bool,
    locale: zapfast::i18n::Locale,
    p: Palette,
) -> gpui::Stateful<gpui::Div> {
    let unread = chat.unread > 0;
    let flagged = unread || chat.marked_unread;
    let time = timefmt::list_label(chat.last_activity, timefmt::now());
    let preview = preview(chat, locale);
    let muted = chat.muted_until.is_some();
    let tick = chat
        .last
        .as_ref()
        .filter(|last| last.from_me)
        .and_then(|last| tick_for(last.status, p));
    let label = format!(
        "{}, {}{}",
        chat.name,
        preview,
        match chat.unread {
            0 if chat.marked_unread => ", marcada como não lida".to_owned(),
            0 => String::new(),
            1 => ", 1 mensagem não lida".to_owned(),
            n => format!(", {n} mensagens não lidas"),
        }
    );
    div()
        .id(SharedString::from(format!("chat-{}", chat.id)))
        .role(Role::ListItem)
        .aria_label(label)
        .aria_selected(selected)
        .w_full()
        .h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(14.))
        .px(px(14.))
        .cursor(CursorStyle::PointingHand)
        .when(selected, |row| row.bg(p.surface_active))
        .when(!selected, |row| row.hover(|s| s.bg(p.surface_hover)))
        .child(avatar(&chat.name, picture, 49., p.dark))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(3.))
                .border_b_1()
                .border_color(p.outline)
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
                                .text_size(px(16.5))
                                .text_color(p.text)
                                .child(SharedString::from(chat.name.clone())),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(if flagged { p.accent } else { p.secondary })
                                .child(SharedString::from(time)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .h(px(22.))
                        .when_some(tick, |line, (path, color)| {
                            line.child(icon(path, 16., color))
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(14.))
                                .text_color(p.secondary)
                                .child(SharedString::from(preview)),
                        )
                        .when(muted, |line| {
                            line.child(icon("icons/volume-x.svg", 16., p.secondary))
                        })
                        .when(chat.pinned && !flagged, |line| {
                            line.child(icon("icons/pin.svg", 16., p.secondary))
                        })
                        .when(flagged, |line| {
                            line.child(
                                div()
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .min_w(px(20.))
                                    .h(px(20.))
                                    .px(px(if chat.unread > 0 { 6. } else { 0. }))
                                    .rounded_full()
                                    .bg(if muted { p.secondary } else { p.accent })
                                    .text_color(p.on_accent)
                                    .text_size(px(12.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .when(chat.unread > 0, |badge| {
                                        badge.child(SharedString::from(if chat.unread > 99 {
                                            "99+".to_owned()
                                        } else {
                                            chat.unread.to_string()
                                        }))
                                    }),
                            )
                        }),
                ),
        )
}

impl Render for ChatListView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let count = self.visible.len();
        let empty = self.chats.read(cx).chats().is_empty();
        let searching = !self.chats.read(cx).query.trim().is_empty();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.panel)
            .child(
                div()
                    .flex_none()
                    .h(px(60.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(16.))
                    .child(
                        div()
                            .id("title")
                            .role(Role::Heading)
                            .text_size(px(22.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child("Conversas"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(header_button(
                                "new-chat",
                                "Nova conversa",
                                "icons/plus.svg",
                                p,
                            ))
                            .child(header_button("menu", "Menu", "icons/ellipsis.svg", p)),
                    ),
            )
            .child(
                div().flex_none().px(px(12.)).pb(px(8.)).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .h(px(36.))
                        .px(px(12.))
                        .rounded(px(10.))
                        .bg(p.surface)
                        .text_color(p.text)
                        .text_size(px(14.))
                        .child(icon("icons/search.svg", 18., p.secondary))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&self.search).appearance(false).bordered(false)),
                        ),
                ),
            )
            .child(
                div()
                    .id("chat-list")
                    .role(Role::List)
                    .aria_label("Conversas")
                    .flex_1()
                    .min_h_0()
                    .when(count == 0, |list| {
                        list.flex().justify_center().pt(px(32.)).child(
                            div()
                                .text_size(px(14.))
                                .text_color(p.secondary)
                                .child(if searching {
                                    "Nenhuma conversa encontrada"
                                } else if empty {
                                    "Carregando conversas…"
                                } else {
                                    "Nenhuma conversa"
                                }),
                        )
                    })
                    .when(count > 0, |list| {
                        list.child(
                            uniform_list(
                                "chats",
                                count,
                                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                    this.rows(range, cx)
                                }),
                            )
                            .size_full(),
                        )
                    }),
            )
    }
}

fn header_button(
    id: &'static str,
    label: &'static str,
    path: &'static str,
    p: Palette,
) -> impl IntoElement {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::{ANA, BRUNO, FAMILY};
    use crate::views::testing::{Demo, launch};
    use gpui::{TestAppContext, VisualTestContext};

    struct Fixture {
        demo: Demo,
        list: Entity<ChatListView>,
    }

    fn list(cx: &mut TestAppContext) -> (Fixture, &mut VisualTestContext) {
        let demo = launch(cx, true);
        let avatars = cx.new(|_| Avatars::default());
        let (list, cx) = cx.add_window_view({
            let store = demo.store.clone();
            move |window, cx| ChatListView::new(store, avatars, window, cx)
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        (Fixture { demo, list }, cx)
    }

    fn ids(fixture: &Fixture, cx: &mut VisualTestContext) -> Vec<ChatId> {
        fixture
            .list
            .read_with(cx, |list, _| list.visible_ids().to_vec())
    }

    #[gpui::test]
    fn the_list_shows_the_chats_pinned_first_and_hides_strangers(cx: &mut TestAppContext) {
        let (fixture, cx) = list(cx);
        let ids = ids(&fixture, cx);
        // Five chats; the person never written to is not listed.
        assert_eq!(ids.len(), 5);
        assert_eq!(ids[0], FAMILY, "the pinned chat leads");
        assert_eq!(ids[1], ANA, "then the latest activity");
    }

    #[gpui::test]
    fn the_search_filters_by_name_and_preview_without_accents(cx: &mut TestAppContext) {
        let (fixture, cx) = list(cx);
        let query = |cx: &mut VisualTestContext, text: &str| {
            fixture
                .list
                .update(cx, |list, cx| list.set_query(text.to_owned(), cx));
            ids(&fixture, cx)
        };
        assert_eq!(query(cx, "FAMILIA"), vec![FAMILY.to_owned()]);
        assert_eq!(query(cx, "bruno"), vec![BRUNO.to_owned()]);
        assert!(query(cx, "zzzz-nada").is_empty());
        assert_eq!(query(cx, "").len(), 5);
    }

    #[gpui::test]
    fn typing_in_the_search_box_filters_the_list(cx: &mut TestAppContext) {
        let (fixture, cx) = list(cx);
        let search = fixture
            .list
            .read_with(cx, |list, _| list.search_state().clone());
        cx.update(|window, cx| search.update(cx, |state, cx| state.focus(window, cx)));
        cx.simulate_input("aurora");
        cx.run_until_parked();
        assert_eq!(ids(&fixture, cx), vec![crate::demo::WORK.to_owned()]);
    }

    #[gpui::test]
    fn clicking_a_row_opens_that_chat_and_reads_it(cx: &mut TestAppContext) {
        let (fixture, cx) = list(cx);
        let unread = |cx: &mut VisualTestContext| {
            fixture.demo.store.read_with(cx, |store, cx| {
                store.chats.read(cx).chat(ANA).unwrap().unread
            })
        };
        assert!(unread(cx) > 0);
        // Title bar 60, search 44, then rows of 72 points: the second row.
        let at = gpui::point(px(120.), px(60. + 44. + ROW_HEIGHT * 1.5));
        cx.simulate_click(at, gpui::Modifiers::none());
        cx.run_until_parked();
        let open = fixture
            .demo
            .store
            .read_with(cx, |store, cx| store.chats.read(cx).open.clone());
        assert_eq!(open.as_deref(), Some(ANA));
        assert_eq!(unread(cx), 0);
    }

    #[gpui::test]
    fn rows_ask_for_their_pictures_once_and_the_backend_answers(cx: &mut TestAppContext) {
        let (fixture, cx) = list(cx);
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        fixture.demo.store.read_with(cx, |store, cx| {
            let chats = store.chats.read(cx);
            // Ana has a picture file; the group has none, and is not asked again.
            assert!(matches!(chats.avatar(ANA), Some(Some(_))));
            assert_eq!(chats.avatar(FAMILY), Some(&None));
        });
    }

    #[gpui::test]
    fn a_row_names_who_wrote_the_last_message_in_a_group(cx: &mut TestAppContext) {
        let (fixture, cx) = list(cx);
        let chat = fixture
            .demo
            .store
            .read_with(cx, |store, cx| store.chats.read(cx).chat(FAMILY).cloned())
            .unwrap();
        let line = preview(&chat, zapfast::i18n::Locale::PortugueseBrazil);
        assert!(!line.is_empty());
        assert!(
            line.contains(": ") || chat.last.as_ref().is_some_and(|l| !l.labelled && l.from_me),
            "{line}"
        );
    }
}
