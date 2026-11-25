//! The app's state root: one entity that owns the others and is the only
//! place a backend event is applied and a user's intent becomes a command.
//!
//! Event path: the backend's worker sends an [`Event`], the bridge hands it to
//! [`Store::apply_event`], which updates the entity it concerns
//! ([`Session`], [`ChatList`], one [`Conversation`]); those notify, and the
//! views observing them render. Nothing else writes the state.
//!
//! Command path: a view calls an intent (`open_chat`, `send_text`, `react`,
//! `load_older`, ...), which updates the state it can already know (a chat
//! read, a flag that a page was asked for) and sends the [`Command`] through
//! the [`CommandSink`]. The sink is the backend in a real run and a recorder
//! in `--demo` and in tests, so the same code runs in all of them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{AppContext as _, Context, Entity, EventEmitter, Subscription};
use zapfast::backend::{Command, Event, Refusal, Unsent};
use zapfast::model::{ChatId, MediaState, Message, ReplyTarget};

use super::chats::ChatList;
use super::conversation::Conversation;
use super::session::{Me, Session};
use super::settings::{AppSettings, SettingsEvent};

/// Where commands go. The real one is the backend (see `bridge`); a demo's
/// answers events through the same bridge.
pub trait CommandSink {
    fn send(&self, command: Command);
}

/// A sink that keeps what was sent, for tests and for `--demo`'s responder.
#[derive(Default)]
pub struct RecordingSink {
    sent: RefCell<Vec<Command>>,
}

impl RecordingSink {
    /// Takes what was sent so far.
    pub fn take(&self) -> Vec<Command> {
        std::mem::take(&mut *self.sent.borrow_mut())
    }
}

impl CommandSink for RecordingSink {
    fn send(&self, command: Command) {
        self.sent.borrow_mut().push(command);
    }
}

/// What the state says beyond a repaint, for whatever shows toasts, plays
/// sounds and posts notifications.
#[derive(Debug)]
pub enum StoreEvent {
    /// A live message arrived (not history): the notification's moment.
    Incoming { chat: ChatId, message: Box<Message> },
    /// A send was refused before anything left; it carries what was being
    /// sent so the composer can take it back.
    SendRefused {
        chat: ChatId,
        quoting: Option<ReplyTarget>,
        unsent: Unsent,
        reason: Refusal,
    },
    /// Something went wrong that the reader should see.
    Error(String),
    /// Something the reader should know.
    Info(String),
}

impl EventEmitter<StoreEvent> for Store {}

pub struct Store {
    pub session: Entity<Session>,
    pub chats: Entity<ChatList>,
    conversations: HashMap<ChatId, Entity<Conversation>>,
    sink: Rc<dyn CommandSink>,
    /// Whether the window is the one the reader is looking at: a chat read in
    /// a window nobody sees is not read.
    window_active: bool,
    /// Whether opening a chat tells the sender it was read (the account's
    /// read-receipts setting; on until settings are read in this app).
    pub send_read_receipts: bool,
    /// Whether pictures, clips and the like download as their rows come into
    /// view (the account's setting; on until settings are read in this app).
    /// Stickers always do.
    pub auto_download: bool,
    /// The reader's settings. The two switches above follow them.
    pub settings: Entity<AppSettings>,
    _settings: Subscription,
    /// Events applied, for tests and for the idle benchmark's accounting.
    pub applied: u64,
}

impl Store {
    pub fn new(sink: Rc<dyn CommandSink>, cx: &mut Context<Self>) -> Self {
        let settings = cx.new(|_| AppSettings::in_memory());
        let follow = cx.subscribe(&settings, |this, settings, _: &SettingsEvent, cx| {
            let settings = settings.read(cx).get();
            this.send_read_receipts = settings.send_read_receipts;
            this.auto_download = settings.auto_download;
        });
        Self {
            settings,
            _settings: follow,
            session: cx.new(|_| Session::default()),
            chats: cx.new(|_| ChatList::default()),
            conversations: HashMap::new(),
            sink,
            window_active: true,
            send_read_receipts: true,
            auto_download: true,
            applied: 0,
        }
    }

    /// The loaded history of a chat, created empty on first use.
    pub fn conversation(&mut self, chat: &str, cx: &mut Context<Self>) -> Entity<Conversation> {
        self.conversations
            .entry(chat.to_owned())
            .or_insert_with(|| cx.new(|_| Conversation::new(chat.to_owned())))
            .clone()
    }

    /// The conversation if it was ever opened or filled.
    pub fn existing_conversation(&self, chat: &str) -> Option<&Entity<Conversation>> {
        self.conversations.get(chat)
    }

    // ---- Events -----------------------------------------------------------

    /// Applies one event from the backend. Events the app has no use for yet
    /// (everything the later phases show) are dropped here, in one place.
    pub fn apply_event(&mut self, event: Event, cx: &mut Context<Self>) {
        self.applied += 1;
        match event {
            Event::Link(status) => {
                let connected = status.is_connected();
                self.session
                    .update(cx, |session, cx| session.set_link(status, cx));
                if connected && let Some(open) = self.chats.read(cx).open.clone() {
                    self.ensure_loaded(&open, cx);
                }
            }
            Event::Me {
                id,
                lid,
                name,
                about,
            } => self.session.update(cx, |session, cx| {
                session.set_me(
                    Me {
                        id,
                        lid,
                        name,
                        about,
                    },
                    cx,
                )
            }),
            Event::Syncing(syncing) => self
                .session
                .update(cx, |session, cx| session.set_syncing(syncing, cx)),
            Event::SyncProgress(percent) => self
                .session
                .update(cx, |session, cx| session.set_sync_progress(percent, cx)),
            Event::Chats(chats) => {
                self.chats.update(cx, |list, cx| list.set_chats(chats, cx));
                if let Some(open) = self.chats.read(cx).open.clone() {
                    self.ensure_loaded(&open, cx);
                }
            }
            Event::ChatUpdated(chat) => self.chat_updated(*chat, cx),
            Event::Messages {
                chat,
                messages,
                older,
                complete,
            } => {
                let conversation = self.conversation(&chat, cx);
                conversation.update(cx, |c, cx| c.merge(messages, older, complete, cx));
            }
            Event::MessageUpdated(message) => {
                let conversation = self.conversation(&message.chat, cx);
                conversation.update(cx, |c, cx| c.update(*message, cx));
            }
            Event::MessageDeleted { chat, id } => {
                if let Some(conversation) = self.conversations.get(&chat) {
                    conversation.update(cx, |c, cx| c.remove(&id, cx));
                }
            }
            Event::ChatRemoved { chat } => {
                self.conversations.remove(&chat);
                self.chats.update(cx, |list, cx| list.remove(&chat, cx));
            }
            Event::ChatCleared { chat, through } => {
                if let Some(conversation) = self.conversations.get(&chat) {
                    conversation.update(cx, |c, cx| c.clear_through(through, cx));
                }
            }
            Event::OlderFetched { chat, more, .. } => {
                if let Some(conversation) = self.conversations.get(&chat) {
                    conversation.update(cx, |c, cx| c.phone_answered(more, cx));
                }
            }
            Event::HistoryUnavailable { chat, error } => {
                if let Some(conversation) = self.conversations.get(&chat) {
                    conversation.update(cx, |c, cx| {
                        c.set_loading_older(false, cx);
                        c.phone_answered(false, cx);
                    });
                }
                cx.emit(StoreEvent::Error(error));
            }
            Event::Typing {
                chat,
                sender,
                activity,
            } => self
                .chats
                .update(cx, |list, cx| list.set_typing(chat, sender, activity, cx)),
            Event::Avatar { id, full, path } => {
                if !full {
                    self.chats
                        .update(cx, |list, cx| list.set_avatar(id, path, cx));
                }
            }
            Event::Incoming { chat, message } => {
                cx.emit(StoreEvent::Incoming { chat, message });
            }
            Event::SendRefused {
                chat,
                quoting,
                unsent,
                reason,
            } => cx.emit(StoreEvent::SendRefused {
                chat,
                quoting,
                unsent,
                reason,
            }),
            Event::Media {
                card: None,
                chat,
                message,
                result,
            } => {
                if let Some(conversation) = self.conversations.get(&chat) {
                    conversation.update(cx, |c, cx| c.set_media(&message, result, cx));
                }
            }
            Event::DownloadProgress {
                chat,
                message,
                percent,
            } => {
                if let Some(conversation) = self.conversations.get(&chat) {
                    conversation.update(cx, |c, cx| c.set_progress(&message, percent, cx));
                }
            }
            Event::Error(message) => cx.emit(StoreEvent::Error(message)),
            Event::Info(message) => cx.emit(StoreEvent::Info(message)),
            _ => {}
        }
    }

    /// A chat that changed: when it is the one the reader has open and is
    /// looking at, what arrived in it is read.
    fn chat_updated(&mut self, mut chat: zapfast::model::Chat, cx: &mut Context<Self>) {
        let looking = self.window_active
            && !chat.locked
            && chat.unread > 0
            && self.chats.read(cx).open.as_deref() == Some(chat.id.as_str());
        if looking {
            chat.unread = 0;
            chat.marked_unread = false;
            chat.unread_mention = None;
            self.send_mark_read(&chat.id);
        }
        self.chats.update(cx, |list, cx| list.upsert(chat, cx));
    }

    // ---- Intents ----------------------------------------------------------

    /// Whether the window is the one in front. A window that comes back to
    /// the front reads the chat it shows.
    pub fn set_window_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.window_active == active {
            return;
        }
        self.window_active = active;
        if active && let Some(open) = self.chats.read(cx).open.clone() {
            self.mark_read(&open, cx);
        }
    }

    /// Opens a chat (or closes the pane with `None`): its first page is asked
    /// for once, and what is unread in it is read.
    pub fn open_chat(&mut self, id: Option<ChatId>, cx: &mut Context<Self>) {
        self.chats.update(cx, |list, cx| list.open(id.clone(), cx));
        if let Some(id) = id {
            self.ensure_loaded(&id, cx);
            self.mark_read(&id, cx);
        }
    }

    /// Asks the archive for a chat's newest page, once.
    pub fn ensure_loaded(&mut self, chat: &str, cx: &mut Context<Self>) {
        let conversation = self.conversation(chat, cx);
        let ask = conversation.update(cx, |c, _| !std::mem::replace(&mut c.requested, true));
        if ask {
            self.sink.send(Command::LoadChat {
                chat: chat.to_owned(),
                before: None,
            });
        }
    }

    /// The reader scrolled to the top: the page before the oldest message,
    /// from the archive, and when the archive has none, from the phone.
    pub fn load_older(&mut self, chat: &str, cx: &mut Context<Self>) {
        let Some(conversation) = self.conversations.get(chat).cloned() else {
            return;
        };
        let (loading, complete, before) = conversation.read_with(cx, |c, _| {
            (
                c.loading_older,
                c.complete,
                c.oldest().map(|m| (m.timestamp, m.id.clone())),
            )
        });
        if loading {
            return;
        }
        let Some(before) = before else {
            return;
        };
        if complete {
            self.fetch_older(chat, true, cx);
            return;
        }
        conversation.update(cx, |c, cx| c.set_loading_older(true, cx));
        self.sink.send(Command::LoadChat {
            chat: chat.to_owned(),
            before: Some(before),
        });
    }

    /// Asks the phone for history older than the archive has. `explicit`: the
    /// reader scrolled to the top, so a silent phone is worth saying.
    pub fn fetch_older(&mut self, chat: &str, explicit: bool, cx: &mut Context<Self>) {
        let Some(conversation) = self.conversations.get(chat).cloned() else {
            return;
        };
        if !self.session.read(cx).is_online() {
            return;
        }
        let ask = conversation.update(cx, |c, _| {
            if c.phone_exhausted || c.fetching_phone {
                false
            } else {
                c.fetching_phone = true;
                true
            }
        });
        if ask {
            self.sink.send(Command::FetchOlder {
                chat: chat.to_owned(),
                explicit,
            });
        }
    }

    /// Marks a chat read, here at once and on the backend. Only when the
    /// window is in front: a chat open behind other windows stays unread.
    pub fn mark_read(&mut self, chat: &str, cx: &mut Context<Self>) {
        if !self.window_active {
            return;
        }
        let unread = self
            .chats
            .read(cx)
            .chat(chat)
            .is_some_and(|chat| chat.unread > 0 || chat.marked_unread);
        if unread {
            self.chats
                .update(cx, |list, cx| list.mark_read_locally(chat, cx));
            self.send_mark_read(chat);
        }
    }

    fn send_mark_read(&self, chat: &str) {
        self.sink.send(Command::MarkRead {
            chat: chat.to_owned(),
            receipts: self.send_read_receipts,
        });
    }

    /// Sends a text, optionally as a reply. False when there was nothing to
    /// send: an empty or whitespace-only text, or a chat that cannot be
    /// written to.
    pub fn send_text(
        &mut self,
        chat: &str,
        text: &str,
        quoting: Option<ReplyTarget>,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = text.trim();
        let writable = self.chats.read(cx).chat(chat).is_none_or(|c| c.can_send());
        if text.is_empty() || !writable {
            return false;
        }
        self.sink.send(Command::SendText {
            chat: chat.to_owned(),
            text: text.to_owned(),
            quoting,
            mentions: Vec::new(),
            preview: None,
        });
        true
    }

    /// Reacts to a message with `emoji`; reacting again with the same emoji
    /// takes the reaction back (an empty emoji, as the backend reads it).
    pub fn react(&mut self, chat: &str, message: &str, emoji: &str, cx: &mut Context<Self>) {
        let ours = self.conversations.get(chat).and_then(|c| {
            c.read(cx).message(message).and_then(|m| {
                m.reactions
                    .iter()
                    .find(|r| r.from_me)
                    .map(|r| r.emoji.clone())
            })
        });
        let emoji = if ours.as_deref() == Some(emoji) {
            String::new()
        } else {
            emoji.to_owned()
        };
        self.sink.send(Command::React {
            chat: chat.to_owned(),
            message: message.to_owned(),
            emoji,
        });
    }

    /// Downloads the attachment of `message`. `manual` is the reader asking
    /// (no size limit); the automatic kind refuses what is past the limit. The
    /// row shows the download from here on, and asking twice sends one command.
    pub fn download(&mut self, chat: &str, message: &str, manual: bool, cx: &mut Context<Self>) {
        let Some(conversation) = self.conversations.get(chat).cloned() else {
            return;
        };
        let wanted = conversation
            .read(cx)
            .message(message)
            .and_then(|m| m.content.media())
            .is_some_and(|media| {
                media.path.is_none()
                    && !matches!(media.state, MediaState::Downloading)
                    && (manual || media.is_within_download_limit())
            });
        if !wanted {
            return;
        }
        conversation.update(cx, |c, cx| {
            c.set_media_state(message, MediaState::Downloading, cx)
        });
        self.sink.send(Command::Download {
            manual,
            card: None,
            chat: chat.to_owned(),
            message: message.to_owned(),
        });
    }

    /// Asks for a pairing code to type on the phone instead of scanning.
    pub fn pair_with_phone(&mut self, phone: &str) {
        let digits: String = phone.chars().filter(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            self.sink.send(Command::PairWithPhone(digits));
        }
    }

    /// Asks for an avatar picture once; the answer is `Event::Avatar`.
    pub fn request_avatar(&mut self, id: &str, cx: &mut Context<Self>) {
        if self
            .chats
            .update(cx, |list, _| list.should_request_avatar(id))
        {
            self.sink.send(Command::FetchAvatar {
                id: id.to_owned(),
                full: false,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::chats::fixtures::chat;
    use crate::state::conversation::fixtures::{CHAT, message};
    use gpui::TestAppContext;
    use zapfast::backend::LinkStatus;
    use zapfast::model::{Delivery, Reaction};

    fn setup(cx: &mut TestAppContext) -> (Entity<Store>, Rc<RecordingSink>) {
        let sink = Rc::new(RecordingSink::default());
        let store = cx.new(|cx| Store::new(sink.clone(), cx));
        (store, sink)
    }

    fn apply(store: &Entity<Store>, event: Event, cx: &mut TestAppContext) {
        store.update(cx, |store, cx| store.apply_event(event, cx));
    }

    fn connected(store: &Entity<Store>, cx: &mut TestAppContext) {
        apply(store, Event::Link(LinkStatus::Connected), cx);
    }

    fn unread_chat(unread: u32) -> zapfast::model::Chat {
        let mut c = chat(CHAT, "Ana", 100);
        c.unread = unread;
        c
    }

    #[gpui::test]
    fn events_become_session_chats_and_messages(cx: &mut TestAppContext) {
        let (store, sink) = setup(cx);
        apply(&store, Event::Link(LinkStatus::Connecting), cx);
        apply(
            &store,
            Event::Me {
                id: "5511@s.whatsapp.net".into(),
                lid: None,
                name: Some("Gigio".into()),
                about: None,
            },
            cx,
        );
        apply(&store, Event::Syncing(true), cx);
        apply(&store, Event::SyncProgress(55), cx);
        apply(&store, Event::Chats(vec![chat(CHAT, "Ana", 100)]), cx);
        apply(
            &store,
            Event::Messages {
                chat: CHAT.into(),
                messages: vec![message("m1", 10), message("m2", 20)],
                older: false,
                complete: true,
            },
            cx,
        );
        store.update(cx, |store, cx| {
            let session = store.session.read(cx);
            assert_eq!(session.link, LinkStatus::Connecting);
            assert_eq!(session.me.as_ref().unwrap().name.as_deref(), Some("Gigio"));
            assert!(session.syncing);
            assert_eq!(session.sync_progress, Some(55));
            assert_eq!(store.chats.read(cx).chats().len(), 1);
            let conversation = store.conversation(CHAT, cx);
            assert_eq!(conversation.read(cx).messages.len(), 2);
            assert!(conversation.read(cx).complete);
            assert_eq!(store.applied, 6);
        });
        assert!(sink.take().is_empty(), "applying events sends nothing");
    }

    #[gpui::test]
    fn events_the_app_does_not_use_yet_change_nothing(cx: &mut TestAppContext) {
        let (store, sink) = setup(cx);
        apply(&store, Event::Labels(Vec::new()), cx);
        apply(&store, Event::LinkPreviewsDisabled(true), cx);
        assert!(sink.take().is_empty());
        store.read_with(cx, |store, cx| {
            assert_eq!(store.session.read(cx).link, LinkStatus::Starting);
            assert!(store.chats.read(cx).chats().is_empty());
        });
    }

    #[gpui::test]
    fn opening_a_chat_loads_its_first_page_once_and_reads_it(cx: &mut TestAppContext) {
        let (store, sink) = setup(cx);
        apply(&store, Event::Chats(vec![unread_chat(3)]), cx);
        store.update(cx, |store, cx| store.open_chat(Some(CHAT.into()), cx));
        let sent = sink.take();
        assert!(matches!(
            &sent[..],
            [
                Command::LoadChat { chat, before: None },
                Command::MarkRead { chat: read, receipts: true },
            ] if chat == CHAT && read == CHAT
        ));
        store.read_with(cx, |store, cx| {
            let list = store.chats.read(cx);
            assert_eq!(list.open.as_deref(), Some(CHAT));
            assert_eq!(list.chat(CHAT).unwrap().unread, 0);
        });
        // Opening it again, or the chat list arriving again, asks for nothing.
        store.update(cx, |store, cx| store.open_chat(Some(CHAT.into()), cx));
        apply(&store, Event::Chats(vec![chat(CHAT, "Ana", 100)]), cx);
        connected(&store, cx);
        assert!(sink.take().is_empty());
    }

    #[gpui::test]
    fn a_message_that_arrives_in_the_open_chat_is_read_while_looking(cx: &mut TestAppContext) {
        let (store, sink) = setup(cx);
        apply(&store, Event::Chats(vec![chat(CHAT, "Ana", 100)]), cx);
        store.update(cx, |store, cx| store.open_chat(Some(CHAT.into()), cx));
        sink.take();
        apply(&store, Event::ChatUpdated(Box::new(unread_chat(1))), cx);
        assert!(matches!(
            &sink.take()[..],
            [Command::MarkRead { receipts: true, .. }]
        ));
        store.read_with(cx, |store, cx| {
            assert_eq!(store.chats.read(cx).chat(CHAT).unwrap().unread, 0);
        });
        // The window behind others: the chat stays unread, and is read when
        // it comes back.
        store.update(cx, |store, cx| store.set_window_active(false, cx));
        apply(&store, Event::ChatUpdated(Box::new(unread_chat(2))), cx);
        assert!(sink.take().is_empty());
        store.read_with(cx, |store, cx| {
            assert_eq!(store.chats.read(cx).chat(CHAT).unwrap().unread, 2);
        });
        store.update(cx, |store, cx| store.set_window_active(true, cx));
        assert!(matches!(&sink.take()[..], [Command::MarkRead { .. }]));
    }

    #[gpui::test]
    fn the_read_receipts_setting_goes_with_the_command(cx: &mut TestAppContext) {
        let (store, sink) = setup(cx);
        apply(&store, Event::Chats(vec![unread_chat(1)]), cx);
        store.update(cx, |store, cx| {
            store.send_read_receipts = false;
            store.mark_read(CHAT, cx);
        });
        assert!(matches!(
            &sink.take()[..],
            [Command::MarkRead {
                receipts: false,
                ..
            }]
        ));
    }

    #[gpui::test]
    fn sending_text_a_reply_and_nothing(cx: &mut TestAppContext) {
        let (store, sink) = setup(cx);
        apply(&store, Event::Chats(vec![chat(CHAT, "Ana", 100)]), cx);
        let sent = store.update(cx, |store, cx| {
            [
                store.send_text(CHAT, "  olá  ", None, cx),
                store.send_text(CHAT, "resposta", Some("m1".into()), cx),
                store.send_text(CHAT, "   \n ", None, cx),
            ]
        });
        assert_eq!(sent, [true, true, false]);
        let commands = sink.take();
        assert_eq!(commands.len(), 2);
        assert!(matches!(
            &commands[0],
            Command::SendText { chat, text, quoting: None, .. } if chat == CHAT && text == "olá"
        ));
        assert!(matches!(
            &commands[1],
            Command::SendText { quoting: Some(target), text, .. }
                if target.id == "m1" && target.chat.is_none() && text == "resposta"
        ));
    }

    #[gpui::test]
    fn a_chat_that_cannot_be_written_to_takes_no_text(cx: &mut TestAppContext) {
        let (store, sink) = setup(cx);
        let mut read_only = chat(CHAT, "Avisos", 1);
        read_only.read_only = true;
        apply(&store, Event::Chats(vec![read_only]), cx);
        assert!(!store.update(cx, |store, cx| store.send_text(CHAT, "oi", None, cx)));
        assert!(sink.take().is_empty());
    }

    #[gpui::test]
    fn reacting_twice_with_the_same_emoji_takes_the_reaction_back(cx: &mut TestAppContext) {
        let (store, sink) = setup(cx);
        apply(
            &store,
            Event::Messages {
                chat: CHAT.into(),
                messages: vec![message("m1", 10)],
                older: false,
                complete: true,
            },
            cx,
        );
        let react = |store: &Entity<Store>, emoji: &str, cx: &mut TestAppContext| {
            store.update(cx, |store, cx| store.react(CHAT, "m1", emoji, cx));
        };
        react(&store, "👍", cx);
        let mut reacted = message("m1", 10);
        reacted.reactions.push(Reaction {
            sender: "me".into(),
            from_me: true,
            emoji: "👍".into(),
        });
        apply(&store, Event::MessageUpdated(Box::new(reacted)), cx);
        react(&store, "👍", cx);
        react(&store, "❤️", cx);
        let emojis: Vec<String> = sink
            .take()
            .into_iter()
            .map(|command| match command {
                Command::React { emoji, message, .. } => {
                    assert_eq!(message, "m1");
                    emoji
                }
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(emojis, ["👍", "", "❤️"]);
    }

    #[gpui::test]
    fn scrolling_to_the_top_pages_the_archive_then_the_phone(cx: &mut TestAppContext) {
        let (store, sink) = setup(cx);
        connected(&store, cx);
        apply(
            &store,
            Event::Messages {
                chat: CHAT.into(),
                messages: vec![message("m5", 50), message("m6", 60)],
                older: false,
                complete: false,
            },
            cx,
        );
        store.update(cx, |store, cx| {
            store.load_older(CHAT, cx);
            store.load_older(CHAT, cx); // already loading: nothing more
        });
        let sent = sink.take();
        assert_eq!(sent.len(), 1);
        assert!(matches!(
            &sent[0],
            Command::LoadChat { before: Some((50, id)), .. } if id == "m5"
        ));
        // The page arrives and is the last the archive has.
        apply(
            &store,
            Event::Messages {
                chat: CHAT.into(),
                messages: vec![message("m3", 30)],
                older: true,
                complete: true,
            },
            cx,
        );
        store.update(cx, |store, cx| store.load_older(CHAT, cx));
        assert!(matches!(
            &sink.take()[..],
            [Command::FetchOlder { explicit: true, .. }]
        ));
        // Asked once; the phone's "nothing more" ends it for good.
        store.update(cx, |store, cx| store.load_older(CHAT, cx));
        assert!(sink.take().is_empty());
        apply(
            &store,
            Event::OlderFetched {
                chat: CHAT.into(),
                more: false,
                silent: false,
            },
            cx,
        );
        store.update(cx, |store, cx| store.load_older(CHAT, cx));
        assert!(sink.take().is_empty());
    }

    #[gpui::test]
    fn the_phone_is_not_asked_while_offline(cx: &mut TestAppContext) {
        let (store, sink) = setup(cx);
        apply(
            &store,
            Event::Messages {
                chat: CHAT.into(),
                messages: vec![message("m1", 10)],
                older: false,
                complete: true,
            },
            cx,
        );
        store.update(cx, |store, cx| store.load_older(CHAT, cx));
        assert!(sink.take().is_empty());
    }

    #[gpui::test]
    fn pairing_by_phone_sends_only_the_digits(cx: &mut TestAppContext) {
        let (store, sink) = setup(cx);
        store.update(cx, |store, _| {
            store.pair_with_phone("+55 (11) 99999-0000");
            store.pair_with_phone("abc");
        });
        assert!(matches!(
            &sink.take()[..],
            [Command::PairWithPhone(phone)] if phone == "5511999990000"
        ));
    }

    #[gpui::test]
    fn an_avatar_is_asked_for_once_and_answered(cx: &mut TestAppContext) {
        let (store, sink) = setup(cx);
        store.update(cx, |store, cx| {
            store.request_avatar("a@s.whatsapp.net", cx);
            store.request_avatar("a@s.whatsapp.net", cx);
        });
        assert!(matches!(
            &sink.take()[..],
            [Command::FetchAvatar { full: false, .. }]
        ));
        apply(
            &store,
            Event::Avatar {
                id: "a@s.whatsapp.net".into(),
                full: false,
                path: Some("/tmp/a.jpg".into()),
            },
            cx,
        );
        store.read_with(cx, |store, cx| {
            assert!(store.chats.read(cx).avatar("a@s.whatsapp.net").is_some());
        });
    }

    #[gpui::test]
    fn removed_and_cleared_chats(cx: &mut TestAppContext) {
        let (store, _) = setup(cx);
        apply(&store, Event::Chats(vec![chat(CHAT, "Ana", 100)]), cx);
        apply(
            &store,
            Event::Messages {
                chat: CHAT.into(),
                messages: vec![message("m1", 10), message("m2", 20)],
                older: false,
                complete: true,
            },
            cx,
        );
        apply(
            &store,
            Event::ChatCleared {
                chat: CHAT.into(),
                through: 10,
            },
            cx,
        );
        store.update(cx, |store, cx| {
            assert_eq!(store.conversation(CHAT, cx).read(cx).messages.len(), 1);
        });
        apply(
            &store,
            Event::MessageDeleted {
                chat: CHAT.into(),
                id: "m2".into(),
            },
            cx,
        );
        store.update(cx, |store, cx| {
            assert!(store.conversation(CHAT, cx).read(cx).messages.is_empty());
        });
        apply(&store, Event::ChatRemoved { chat: CHAT.into() }, cx);
        store.read_with(cx, |store, cx| {
            assert!(store.chats.read(cx).chats().is_empty());
            assert!(store.existing_conversation(CHAT).is_none());
        });
    }

    #[gpui::test]
    fn what_the_reader_should_hear_about_is_emitted(cx: &mut TestAppContext) {
        let (store, _) = setup(cx);
        let seen = Rc::new(RefCell::new(Vec::<String>::new()));
        let log = seen.clone();
        cx.update(|cx| {
            cx.subscribe(&store, move |_, event: &StoreEvent, _| {
                log.borrow_mut().push(match event {
                    StoreEvent::Incoming { chat, message } => {
                        format!("incoming {chat} {}", message.id)
                    }
                    StoreEvent::SendRefused { reason, .. } => format!("refused {reason:?}"),
                    StoreEvent::Error(text) => format!("error {text}"),
                    StoreEvent::Info(text) => format!("info {text}"),
                })
            })
            .detach()
        });
        apply(
            &store,
            Event::Incoming {
                chat: CHAT.into(),
                message: Box::new(message("m9", 9)),
            },
            cx,
        );
        apply(&store, Event::Error("boom".into()), cx);
        apply(&store, Event::Info("fyi".into()), cx);
        apply(
            &store,
            Event::SendRefused {
                chat: CHAT.into(),
                quoting: None,
                unsent: Unsent::Text("oi".into()),
                reason: Refusal::Offline,
            },
            cx,
        );
        assert_eq!(
            *seen.borrow(),
            [
                format!("incoming {CHAT} m9"),
                "error boom".to_owned(),
                "info fyi".to_owned(),
                "refused Offline".to_owned(),
            ]
        );
    }

    #[gpui::test]
    fn our_message_goes_from_pending_to_delivered(cx: &mut TestAppContext) {
        let (store, _) = setup(cx);
        let mut sent = message("mine", 10);
        sent.from_me = true;
        sent.status = Delivery::Pending;
        apply(
            &store,
            Event::Messages {
                chat: CHAT.into(),
                messages: vec![sent.clone()],
                older: false,
                complete: true,
            },
            cx,
        );
        sent.status = Delivery::Read;
        apply(&store, Event::MessageUpdated(Box::new(sent)), cx);
        store.update(cx, |store, cx| {
            let conversation = store.conversation(CHAT, cx);
            assert_eq!(
                conversation.read(cx).message("mine").unwrap().status,
                Delivery::Read
            );
        });
    }
}
