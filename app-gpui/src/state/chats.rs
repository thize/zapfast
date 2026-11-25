//! The chat list: the chats the backend reports, the one open, the local
//! search, who is typing, and the pictures found for the avatars.
//!
//! The backend sends the chats newest first; what the list shows is this
//! module's [`ChatList::visible`]: archived and locked chats are out, a person
//! never written to is out unless the reader kept them, pinned chats go first
//! (most recently pinned on top), the rest by latest activity, as the egui
//! list does.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::{Context, EventEmitter};
use zapfast::model::{Chat, ChatActivity, ChatId, ChatKind};
use zapfast::util::search_key;

/// What changed in the list, beyond a repaint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatListEvent {
    /// The chat on screen changed (or closed).
    Opened(Option<ChatId>),
    /// The chat on screen is no longer in the list: deleted, locked or left.
    OpenChatGone(ChatId),
}

#[derive(Debug, Default)]
pub struct ChatList {
    chats: Vec<Chat>,
    /// The chat the conversation pane shows.
    pub open: Option<ChatId>,
    /// The search box's text, as typed.
    pub query: String,
    /// Who is typing or recording, per chat: `(sender, activity)`.
    pub typing: HashMap<ChatId, Vec<(String, ChatActivity)>>,
    /// The picture file found for an avatar id, or `None` when it has none.
    avatars: HashMap<String, Option<PathBuf>>,
    /// Avatars asked for and not answered yet.
    avatar_requested: HashSet<String>,
}

impl EventEmitter<ChatListEvent> for ChatList {}

impl ChatList {
    pub fn chats(&self) -> &[Chat] {
        &self.chats
    }

    pub fn chat(&self, id: &str) -> Option<&Chat> {
        self.chats.iter().find(|chat| chat.id == id)
    }

    /// The full list from the backend.
    pub fn set_chats(&mut self, chats: Vec<Chat>, cx: &mut Context<Self>) {
        self.chats = chats;
        self.check_open(cx);
        cx.notify();
    }

    /// One chat that changed or appeared.
    pub fn upsert(&mut self, chat: Chat, cx: &mut Context<Self>) {
        match self.chats.iter_mut().find(|known| known.id == chat.id) {
            Some(known) => *known = chat,
            None => self.chats.push(chat),
        }
        self.check_open(cx);
        cx.notify();
    }

    pub fn remove(&mut self, id: &str, cx: &mut Context<Self>) {
        self.chats.retain(|chat| chat.id != id);
        self.typing.remove(id);
        self.check_open(cx);
        cx.notify();
    }

    /// Marks a chat read in the list, before the backend confirms it.
    pub fn mark_read_locally(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == id)
            && (chat.unread > 0 || chat.marked_unread || chat.unread_mention.is_some())
        {
            chat.unread = 0;
            chat.marked_unread = false;
            chat.unread_mention = None;
            cx.notify();
        }
    }

    /// The chat the reader opened, or `None` to close it.
    pub fn open(&mut self, id: Option<ChatId>, cx: &mut Context<Self>) {
        if self.open == id {
            return;
        }
        self.open = id.clone();
        cx.emit(ChatListEvent::Opened(id));
        cx.notify();
    }

    /// A chat that is gone, or that may not be shown, closes the pane.
    fn check_open(&mut self, cx: &mut Context<Self>) {
        let Some(open) = self.open.clone() else {
            return;
        };
        if self.chat(&open).is_none_or(|chat| chat.locked) {
            self.open = None;
            cx.emit(ChatListEvent::OpenChatGone(open));
            cx.emit(ChatListEvent::Opened(None));
        }
    }

    pub fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        if self.query != query {
            self.query = query;
            cx.notify();
        }
    }

    /// Someone started or stopped typing in a chat.
    pub fn set_typing(
        &mut self,
        chat: ChatId,
        sender: String,
        activity: Option<ChatActivity>,
        cx: &mut Context<Self>,
    ) {
        let typers = self.typing.entry(chat).or_default();
        let before = typers.clone();
        typers.retain(|(who, _)| *who != sender);
        if let Some(activity) = activity {
            typers.push((sender, activity));
        }
        if *typers != before {
            cx.notify();
        }
    }

    /// The picture file of an avatar, once the backend found it.
    pub fn set_avatar(&mut self, id: String, path: Option<PathBuf>, cx: &mut Context<Self>) {
        self.avatar_requested.remove(&id);
        if self.avatars.insert(id, path.clone()) != Some(path) {
            cx.notify();
        }
    }

    /// The avatar's picture: `Some(Some(path))` found, `Some(None)` none,
    /// `None` not asked for yet.
    pub fn avatar(&self, id: &str) -> Option<&Option<PathBuf>> {
        self.avatars.get(id)
    }

    /// Whether `id`'s avatar still has to be asked for; true once per id until
    /// the answer comes.
    pub fn should_request_avatar(&mut self, id: &str) -> bool {
        !self.avatars.contains_key(id) && self.avatar_requested.insert(id.to_owned())
    }

    /// The Dock and tray count: unarchived, unmuted, unlocked chats that look
    /// unread. WhatsApp counts chats here, not the messages inside them.
    pub fn unread_chat_count(&self, now: i64) -> u32 {
        self.chats
            .iter()
            .filter(|chat| {
                !chat.archived && !chat.locked && !chat.muted(now) && chat.looks_unread()
            })
            .count() as u32
    }

    /// The chats the list shows, in order.
    pub fn visible(&self) -> Vec<&Chat> {
        let needle = search_key(self.query.trim());
        let mut chats: Vec<&Chat> = self
            .chats
            .iter()
            .filter(|chat| !chat.locked)
            .filter(|chat| !needle.is_empty() || !chat.archived)
            .filter(|chat| {
                !needle.is_empty()
                    || self.open.as_deref() == Some(chat.id.as_str())
                    || !unlisted_stranger(chat)
            })
            .filter(|chat| needle.is_empty() || matches_query(chat, &needle))
            .collect();
        chats.sort_by(|a, b| {
            b.pinned.cmp(&a.pinned).then_with(|| {
                if a.pinned && b.pinned {
                    b.pinned_at.cmp(&a.pinned_at).then(a.id.cmp(&b.id))
                } else {
                    b.last_activity.cmp(&a.last_activity).then(a.id.cmp(&b.id))
                }
            })
        });
        chats
    }

    /// Unread chats and the messages in them, for the Dock badge and tray.
    pub fn unread_total(&self) -> u32 {
        self.chats
            .iter()
            .filter(|chat| !chat.archived && !chat.locked)
            .map(|chat| chat.unread)
            .sum()
    }
}

/// A person never written to, left out as on the phone, unless the reader kept
/// them: pinned, a favorite, labeled or unread.
fn unlisted_stranger(chat: &Chat) -> bool {
    chat.kind == ChatKind::Direct
        && chat.never_messaged()
        && !chat.pinned
        && !chat.favorite
        && !chat.looks_unread()
        && chat.labels.is_empty()
}

fn matches_query(chat: &Chat, needle: &str) -> bool {
    search_key(&chat.name).contains(needle)
        || chat.phone().is_some_and(|phone| phone.contains(needle))
        || chat
            .last
            .as_ref()
            .is_some_and(|last| search_key(&last.summary).contains(needle))
}

#[cfg(test)]
pub(crate) mod fixtures {
    use zapfast::model::{Chat, Delivery, LastMessage};

    /// A chat with a last message at `activity`.
    pub fn chat(id: &str, name: &str, activity: i64) -> Chat {
        let mut chat = Chat::new(id.into(), name.into());
        chat.last_activity = activity;
        chat.last = Some(LastMessage {
            from_me: false,
            sender: id.into(),
            sender_name: None,
            summary: format!("last words of {name}"),
            full: format!("last words of {name}"),
            labelled: false,
            status: Delivery::None,
        });
        chat
    }
}

#[cfg(test)]
mod count_tests {
    use super::*;
    use fixtures::chat;

    #[test]
    fn the_badge_counts_unread_unmuted_unarchived_chats() {
        let mut list = ChatList::default();
        let mut unread = chat("a@s.whatsapp.net", "A", 3);
        unread.unread = 4;
        let mut marked = chat("b@s.whatsapp.net", "B", 2);
        marked.marked_unread = true;
        let mut muted = chat("c@s.whatsapp.net", "C", 1);
        muted.unread = 2;
        muted.muted_until = Some(0);
        let mut archived = chat("d@s.whatsapp.net", "D", 1);
        archived.unread = 2;
        archived.archived = true;
        let read = chat("e@s.whatsapp.net", "E", 1);
        for chat in [unread, marked, muted, archived, read] {
            list.chats.push(chat);
        }
        // Two chats, not six messages.
        assert_eq!(list.unread_chat_count(100), 2);
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::chat;
    use super::*;
    use gpui::{AppContext as _, TestAppContext};

    fn names(list: &gpui::Entity<ChatList>, cx: &TestAppContext) -> Vec<String> {
        list.read_with(cx, |list, _| {
            list.visible()
                .iter()
                .map(|chat| chat.name.clone())
                .collect()
        })
    }

    #[gpui::test]
    fn pinned_first_then_by_latest_activity(cx: &mut TestAppContext) {
        let list = cx.new(|_| ChatList::default());
        let mut pinned_old = chat("1@s.whatsapp.net", "Pinned old", 5);
        pinned_old.pinned = true;
        pinned_old.pinned_at = 10;
        let mut pinned_new = chat("2@s.whatsapp.net", "Pinned new", 1);
        pinned_new.pinned = true;
        pinned_new.pinned_at = 20;
        list.update(cx, |list, cx| {
            list.set_chats(
                vec![
                    chat("3@s.whatsapp.net", "Older", 50),
                    pinned_old,
                    chat("4@s.whatsapp.net", "Newer", 90),
                    pinned_new,
                ],
                cx,
            )
        });
        assert_eq!(
            names(&list, cx),
            ["Pinned new", "Pinned old", "Newer", "Older"]
        );
    }

    #[gpui::test]
    fn archived_locked_and_strangers_stay_out(cx: &mut TestAppContext) {
        let list = cx.new(|_| ChatList::default());
        let mut archived = chat("1@s.whatsapp.net", "Archived", 9);
        archived.archived = true;
        let mut locked = chat("2@s.whatsapp.net", "Locked", 9);
        locked.locked = true;
        let mut stranger = Chat::new("3@s.whatsapp.net".into(), "Stranger".into());
        stranger.last = None;
        list.update(cx, |list, cx| {
            list.set_chats(
                vec![
                    archived,
                    locked,
                    stranger,
                    chat("4@s.whatsapp.net", "Kept", 9),
                ],
                cx,
            )
        });
        assert_eq!(names(&list, cx), ["Kept"]);
        // Searching finds the archived chat and the stranger, not the locked one.
        list.update(cx, |list, cx| list.set_query("a".into(), cx));
        let found = names(&list, cx);
        assert!(found.contains(&"Archived".to_owned()));
        assert!(found.contains(&"Stranger".to_owned()));
        assert!(!found.contains(&"Locked".to_owned()));
    }

    #[gpui::test]
    fn the_search_ignores_case_and_accents_and_reads_the_preview(cx: &mut TestAppContext) {
        let list = cx.new(|_| ChatList::default());
        let mut with_phone = chat("5511987654321@s.whatsapp.net", "Maria José", 3);
        with_phone.last.as_mut().unwrap().summary = "Foto".into();
        list.update(cx, |list, cx| {
            list.set_chats(vec![with_phone, chat("2@s.whatsapp.net", "Bruno", 2)], cx)
        });
        for (query, expected) in [
            ("jose", vec!["Maria José"]),
            ("JOSÉ", vec!["Maria José"]),
            ("987654", vec!["Maria José"]),
            ("foto", vec!["Maria José"]),
            ("last words of bruno", vec!["Bruno"]),
            ("zzz", vec![]),
            ("", vec!["Maria José", "Bruno"]),
        ] {
            list.update(cx, |list, cx| list.set_query(query.into(), cx));
            assert_eq!(names(&list, cx), expected, "{query:?}");
        }
    }

    #[gpui::test]
    fn the_open_chat_closes_when_it_goes_away(cx: &mut TestAppContext) {
        let list = cx.new(|_| ChatList::default());
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let log = events.clone();
        cx.update(|cx| {
            cx.subscribe(&list, move |_, event: &ChatListEvent, _| {
                log.borrow_mut().push(event.clone())
            })
            .detach()
        });
        let id = "1@s.whatsapp.net";
        list.update(cx, |list, cx| {
            list.set_chats(vec![chat(id, "A", 1)], cx);
            list.open(Some(id.into()), cx);
        });
        let mut locked = chat(id, "A", 1);
        locked.locked = true;
        list.update(cx, |list, cx| list.upsert(locked, cx));
        list.read_with(cx, |list, _| assert_eq!(list.open, None));
        assert_eq!(
            *events.borrow(),
            [
                ChatListEvent::Opened(Some(id.into())),
                ChatListEvent::OpenChatGone(id.into()),
                ChatListEvent::Opened(None),
            ]
        );
    }

    #[gpui::test]
    fn unread_and_read_locally(cx: &mut TestAppContext) {
        let list = cx.new(|_| ChatList::default());
        let mut a = chat("1@s.whatsapp.net", "A", 1);
        a.unread = 3;
        let mut b = chat("2@s.whatsapp.net", "B", 2);
        b.unread = 2;
        b.archived = true;
        list.update(cx, |list, cx| list.set_chats(vec![a, b], cx));
        list.read_with(cx, |list, _| assert_eq!(list.unread_total(), 3));
        list.update(cx, |list, cx| {
            list.mark_read_locally("1@s.whatsapp.net", cx)
        });
        list.read_with(cx, |list, _| assert_eq!(list.unread_total(), 0));
    }

    #[gpui::test]
    fn typing_and_avatars(cx: &mut TestAppContext) {
        let list = cx.new(|_| ChatList::default());
        list.update(cx, |list, cx| {
            list.set_typing("c".into(), "a".into(), Some(ChatActivity::Typing), cx);
            list.set_typing(
                "c".into(),
                "a".into(),
                Some(ChatActivity::RecordingAudio),
                cx,
            );
            list.set_typing("c".into(), "b".into(), Some(ChatActivity::Typing), cx);
            list.set_typing("c".into(), "b".into(), None, cx);
        });
        list.read_with(cx, |list, _| {
            assert_eq!(
                list.typing["c"],
                [("a".to_owned(), ChatActivity::RecordingAudio)]
            );
        });
        list.update(cx, |list, cx| {
            assert!(list.should_request_avatar("x"));
            assert!(!list.should_request_avatar("x"), "asked once");
            assert_eq!(list.avatar("x"), None);
            list.set_avatar("x".into(), Some("/p.jpg".into()), cx);
            assert!(!list.should_request_avatar("x"));
            assert_eq!(list.avatar("x"), Some(&Some(PathBuf::from("/p.jpg"))));
        });
    }
}
