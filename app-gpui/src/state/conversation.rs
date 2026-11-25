//! One chat's loaded history and paging state, as a GPUI entity.
//!
//! The merge rules are the egui app's (`Conversation::merge` in `src/app.rs`,
//! which belongs to that UI and is not shared): rows that arrive again replace
//! the copy held, keeping a download's path and state that a freshly
//! classified copy lacks; older pages go in front; the order is the
//! timestamp, then the phone's own order.
//!
//! A view does not diff the vector. Each change that reaches it also arrives
//! as a [`ConversationEvent`] saying what moved, which is what a virtualized
//! list needs to splice itself and keep its scroll position.

use gpui::{Context, EventEmitter};
use zapfast::model::{ChatId, Message};

/// What a change to the messages did to the rows, in terms a list can apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConversationEvent {
    /// The rows are not the same ones as before: re-measure everything.
    Reset,
    /// `count` rows were added in front of the earlier ones.
    Prepended { count: usize },
    /// `count` rows were added after the last one.
    Appended { count: usize },
    /// The row at `index` has new content, or its height may differ.
    Updated { index: usize },
    /// The row that was at `index` is gone.
    Removed { index: usize },
    /// The "loading older messages" flag changed.
    Loading,
}

/// Loaded history of one chat.
#[derive(Debug)]
pub struct Conversation {
    pub chat: ChatId,
    /// In display order, oldest first.
    pub messages: Vec<Message>,
    /// Whether the local archive has no earlier messages.
    pub complete: bool,
    /// Whether an older page was asked of the archive and has not come yet.
    pub loading_older: bool,
    /// Whether the first page was asked for.
    pub requested: bool,
    /// Whether a request to the phone for older history is active.
    pub fetching_phone: bool,
    /// Whether the phone has nothing more to give, or cannot be asked.
    pub phone_exhausted: bool,
    /// Counts every change, so a cached view can tell it must render again
    /// without looking at the messages.
    pub revision: u64,
}

impl EventEmitter<ConversationEvent> for Conversation {}

impl Conversation {
    pub fn new(chat: ChatId) -> Self {
        Self {
            chat,
            messages: Vec::new(),
            complete: false,
            loading_older: false,
            requested: false,
            fetching_phone: false,
            phone_exhausted: false,
            revision: 0,
        }
    }

    pub fn message(&self, id: &str) -> Option<&Message> {
        self.messages.iter().find(|message| message.id == id)
    }

    pub fn position(&self, id: &str) -> Option<usize> {
        self.messages.iter().position(|message| message.id == id)
    }

    /// The oldest message held, the key to page before.
    pub fn oldest(&self) -> Option<&Message> {
        self.messages.first()
    }

    /// A page of messages from the archive or a live message. `older` marks a
    /// page that goes in front; `complete` that the archive has no earlier
    /// rows.
    pub fn merge(
        &mut self,
        incoming: Vec<Message>,
        older: bool,
        complete: bool,
        cx: &mut Context<Self>,
    ) {
        let was_empty = self.messages.is_empty();
        if older {
            self.loading_older = false;
            self.complete = complete;
        } else if was_empty {
            self.complete = complete;
        }
        let before: Vec<String> = self.messages.iter().map(|m| m.id.clone()).collect();
        let mut updated = Vec::new();
        let mut added = 0usize;
        for mut message in incoming {
            match self.position(&message.id) {
                Some(index) => {
                    keep_local_media(&self.messages[index], &mut message);
                    if self.messages[index] != message {
                        self.messages[index] = message;
                        updated.push(self.messages[index].id.clone());
                    }
                }
                None => {
                    self.messages.push(message);
                    added += 1;
                }
            }
        }
        sort(&mut self.messages);
        self.revision += 1;
        if older || added > 0 || !updated.is_empty() {
            self.announce(&before, added, &updated, cx);
        }
        if older {
            cx.emit(ConversationEvent::Loading);
        }
        cx.notify();
    }

    /// Works out what the merge did to the rows and says so.
    fn announce(
        &mut self,
        before: &[String],
        added: usize,
        updated: &[String],
        cx: &mut Context<Self>,
    ) {
        let kept_order = self
            .messages
            .iter()
            .filter(|message| before.contains(&message.id))
            .map(|message| &message.id)
            .eq(before.iter());
        if !kept_order {
            cx.emit(ConversationEvent::Reset);
            return;
        }
        if before.is_empty() {
            cx.emit(ConversationEvent::Reset);
            return;
        }
        if added > 0 {
            let in_front = self.messages[..added]
                .iter()
                .all(|message| !before.contains(&message.id));
            let behind = self.messages[self.messages.len() - added..]
                .iter()
                .all(|message| !before.contains(&message.id));
            if in_front {
                cx.emit(ConversationEvent::Prepended { count: added });
            } else if behind {
                cx.emit(ConversationEvent::Appended { count: added });
            } else {
                cx.emit(ConversationEvent::Reset);
                return;
            }
        }
        for id in updated {
            if let Some(index) = self.position(id) {
                cx.emit(ConversationEvent::Updated { index });
            }
        }
    }

    /// A message the worker changed: a delivery tick, an edit, a reaction, a
    /// revoke. A new timestamp (our own message restamped with the server's)
    /// moves the row.
    pub fn update(&mut self, mut message: Message, cx: &mut Context<Self>) {
        let Some(index) = self.position(&message.id) else {
            return;
        };
        keep_local_media(&self.messages[index], &mut message);
        if self.messages[index] == message {
            return;
        }
        let moved = self.messages[index].timestamp != message.timestamp;
        self.messages[index] = message;
        self.revision += 1;
        if moved {
            sort(&mut self.messages);
            cx.emit(ConversationEvent::Reset);
        } else {
            cx.emit(ConversationEvent::Updated { index });
        }
        cx.notify();
    }

    /// A message deleted for good, here or on another device.
    pub fn remove(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.position(id) else {
            return;
        };
        self.messages.remove(index);
        self.revision += 1;
        cx.emit(ConversationEvent::Removed { index });
        cx.notify();
    }

    /// The chat was cleared through `through` (Unix seconds); it stays.
    pub fn clear_through(&mut self, through: i64, cx: &mut Context<Self>) {
        let before = self.messages.len();
        self.messages.retain(|message| message.timestamp > through);
        if self.messages.len() != before {
            self.revision += 1;
            cx.emit(ConversationEvent::Reset);
            cx.notify();
        }
    }

    /// An older page was asked of the archive.
    pub fn set_loading_older(&mut self, loading: bool, cx: &mut Context<Self>) {
        if self.loading_older != loading {
            self.loading_older = loading;
            cx.emit(ConversationEvent::Loading);
            cx.notify();
        }
    }

    /// The phone answered a request for older history, or could not.
    pub fn phone_answered(&mut self, more: bool, cx: &mut Context<Self>) {
        self.fetching_phone = false;
        self.phone_exhausted = !more;
        self.revision += 1;
        cx.notify();
    }
}

fn sort(messages: &mut [Message]) {
    messages.sort_by_key(|message| (message.timestamp, message.history_order.unwrap_or(i64::MAX)));
}

/// A freshly classified copy has no downloaded file and no transfer state of
/// its own: it keeps the ones the copy it replaces had.
fn keep_local_media(existing: &Message, incoming: &mut Message) {
    let Some(old) = existing.content.media() else {
        return;
    };
    if let Some(media) = incoming.content.media_mut()
        && media.path.is_none()
    {
        media.state = old.state.clone();
        media.path = old.path.clone();
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use zapfast::model::{Content, Delivery, Message};

    pub const CHAT: &str = "5511999990000@s.whatsapp.net";

    /// A text message at `timestamp`, id `id`.
    pub fn message(id: &str, timestamp: i64) -> Message {
        Message {
            id: id.into(),
            chat: CHAT.into(),
            sender: CHAT.into(),
            sender_name: None,
            from_me: false,
            timestamp,
            history_order: None,
            content: Content::Text {
                text: format!("text of {id}"),
                preview: None,
            },
            status: Delivery::None,
            delivered_at: None,
            read_at: None,
            quoted: None,
            reactions: Vec::new(),
            edited: false,
            mentions: Vec::new(),
            forwarded: false,
            thumbnail: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{CHAT, message};
    use super::*;
    use gpui::{AppContext as _, TestAppContext};
    use std::{cell::RefCell, rc::Rc};
    use zapfast::model::{Content, Delivery, Reaction};

    fn new(
        cx: &mut TestAppContext,
    ) -> (
        gpui::Entity<Conversation>,
        Rc<RefCell<Vec<ConversationEvent>>>,
    ) {
        let conversation = cx.new(|_| Conversation::new(CHAT.into()));
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        cx.update(|cx| {
            cx.subscribe(&conversation, move |_, event: &ConversationEvent, _| {
                log.borrow_mut().push(event.clone());
            })
            .detach();
        });
        (conversation, seen)
    }

    fn ids(conversation: &gpui::Entity<Conversation>, cx: &TestAppContext) -> Vec<String> {
        conversation.read_with(cx, |c, _| c.messages.iter().map(|m| m.id.clone()).collect())
    }

    #[gpui::test]
    fn a_first_page_then_a_live_message_then_an_older_page(cx: &mut TestAppContext) {
        let (conversation, seen) = new(cx);
        conversation.update(cx, |c, cx| {
            c.merge(vec![message("c", 30), message("b", 20)], false, false, cx)
        });
        assert_eq!(ids(&conversation, cx), ["b", "c"]);
        conversation.update(cx, |c, cx| {
            c.merge(vec![message("d", 40)], false, false, cx)
        });
        conversation.update(cx, |c, cx| {
            c.loading_older = true;
            c.merge(vec![message("a", 10)], true, true, cx)
        });
        assert_eq!(ids(&conversation, cx), ["a", "b", "c", "d"]);
        conversation.read_with(cx, |c, _| {
            assert!(c.complete && !c.loading_older);
            assert!(c.revision >= 3);
        });
        let seen = seen.borrow();
        assert!(seen.contains(&ConversationEvent::Appended { count: 1 }));
        assert!(seen.contains(&ConversationEvent::Prepended { count: 1 }));
    }

    #[gpui::test]
    fn a_message_that_arrives_again_is_updated_not_duplicated(cx: &mut TestAppContext) {
        let (conversation, seen) = new(cx);
        conversation.update(cx, |c, cx| {
            c.merge(vec![message("a", 10)], false, false, cx)
        });
        let mut again = message("a", 10);
        again.edited = true;
        conversation.update(cx, |c, cx| c.merge(vec![again], false, false, cx));
        assert_eq!(ids(&conversation, cx), ["a"]);
        assert!(
            seen.borrow()
                .contains(&ConversationEvent::Updated { index: 0 })
        );
        // The same copy again changes nothing and says nothing.
        let before = seen.borrow().len();
        let mut same = message("a", 10);
        same.edited = true;
        conversation.update(cx, |c, cx| c.merge(vec![same], false, false, cx));
        assert_eq!(seen.borrow().len(), before);
    }

    #[gpui::test]
    fn ticks_reactions_and_revokes_update_in_place(cx: &mut TestAppContext) {
        let (conversation, seen) = new(cx);
        let mut sent = message("m", 10);
        sent.from_me = true;
        sent.status = Delivery::Pending;
        conversation.update(cx, |c, cx| c.merge(vec![sent.clone()], false, false, cx));
        sent.status = Delivery::Delivered;
        sent.reactions.push(Reaction {
            sender: CHAT.into(),
            from_me: false,
            emoji: "👍".into(),
        });
        conversation.update(cx, |c, cx| c.update(sent, cx));
        conversation.read_with(cx, |c, _| {
            let m = c.message("m").unwrap();
            assert_eq!(m.status, Delivery::Delivered);
            assert_eq!(m.reactions.len(), 1);
        });
        assert!(
            seen.borrow()
                .contains(&ConversationEvent::Updated { index: 0 })
        );
        let mut revoked = message("m", 10);
        revoked.content = Content::Revoked;
        conversation.update(cx, |c, cx| c.update(revoked, cx));
        conversation.read_with(cx, |c, _| {
            assert!(matches!(c.message("m").unwrap().content, Content::Revoked));
        });
        // An update for a message that is not here is ignored.
        conversation.update(cx, |c, cx| c.update(message("zzz", 99), cx));
        assert_eq!(ids(&conversation, cx), ["m"]);
    }

    #[gpui::test]
    fn our_message_restamped_by_the_server_moves(cx: &mut TestAppContext) {
        let (conversation, seen) = new(cx);
        conversation.update(cx, |c, cx| {
            c.merge(
                vec![message("mine", 100), message("theirs", 101)],
                false,
                false,
                cx,
            )
        });
        conversation.update(cx, |c, cx| c.update(message("mine", 105), cx));
        assert_eq!(ids(&conversation, cx), ["theirs", "mine"]);
        assert!(seen.borrow().contains(&ConversationEvent::Reset));
    }

    #[gpui::test]
    fn removal_and_clearing(cx: &mut TestAppContext) {
        let (conversation, seen) = new(cx);
        conversation.update(cx, |c, cx| {
            c.merge(
                vec![message("a", 10), message("b", 20), message("c", 30)],
                false,
                true,
                cx,
            )
        });
        conversation.update(cx, |c, cx| c.remove("b", cx));
        assert_eq!(ids(&conversation, cx), ["a", "c"]);
        assert!(
            seen.borrow()
                .contains(&ConversationEvent::Removed { index: 1 })
        );
        conversation.update(cx, |c, cx| c.clear_through(10, cx));
        assert_eq!(ids(&conversation, cx), ["c"]);
        conversation.update(cx, |c, cx| c.remove("missing", cx));
        assert_eq!(ids(&conversation, cx), ["c"]);
    }

    #[gpui::test]
    fn the_phones_answer_ends_the_request(cx: &mut TestAppContext) {
        let (conversation, _) = new(cx);
        conversation.update(cx, |c, cx| {
            c.fetching_phone = true;
            c.phone_answered(false, cx);
        });
        conversation.read_with(cx, |c, _| {
            assert!(!c.fetching_phone && c.phone_exhausted);
        });
    }
}
