//! Synthetic, deterministic data for `--demo`, snapshots and tests, delivered
//! through the same path as the real backend's events (see `bridge`).
//!
//! Nothing here is random and nothing reads the clock: [`NOW`] is a fixed
//! instant, every message is derived from its chat's index and its own, so two
//! runs draw the same pixels. [`Responder`] stands in for the worker: it takes
//! the commands the store sends and answers with the events the worker would
//! (a page of history, a delivery tick, a reaction), so the screens built on it
//! never learn they are not talking to WhatsApp.

use std::cell::RefCell;
use std::collections::HashMap;

use zapfast::backend::{Command, Event, LinkStatus};
use zapfast::model::{
    Chat, ChatId, Content, Delivery, LastMessage, Media, Message, Quoted, Reaction,
};

use crate::bridge::Injector;
use crate::state::CommandSink;

/// The demo's "now", Unix seconds: a Wednesday at 15:20 UTC.
pub const NOW: i64 = 1_760_023_200;

/// The linked account.
pub const ME: &str = "5511900000000@s.whatsapp.net";

/// Messages in a page of history.
pub const PAGE: usize = 30;

pub const ANA: &str = "5511911110001@s.whatsapp.net";
pub const FAMILY: &str = "120363000000000001@g.us";
pub const BRUNO: &str = "5511911110002@s.whatsapp.net";
pub const CAMILA: &str = "5511911110003@s.whatsapp.net";
pub const WORK: &str = "120363000000000002@g.us";
pub const STRANGER: &str = "5511911110004@s.whatsapp.net";

const LINES: [&str; 12] = [
    "Oi! Tudo bem?",
    "Cheguei agora, já te ligo.",
    "Pode ser amanhã às 10h?",
    "Mandei o documento por e-mail.",
    "Perfeito, obrigado! 🙏",
    "Você viu a notícia de hoje?",
    "Vou chegar um pouco atrasado.",
    "Combinado, até já.",
    "Que ótimo! Parabéns 🎉",
    "Preciso de uma ajuda com uma coisa.",
    "Pode me mandar o endereço?",
    "Depois te explico melhor, ok?",
];

struct Spec {
    id: &'static str,
    name: &'static str,
    messages: usize,
    unread: u32,
    pinned: bool,
    muted: bool,
    /// Seconds before [`NOW`] of the chat's last message.
    ago: i64,
}

const SPECS: [Spec; 5] = [
    Spec {
        id: ANA,
        name: "Ana Souza",
        messages: 64,
        unread: 2,
        pinned: false,
        muted: false,
        ago: 120,
    },
    Spec {
        id: FAMILY,
        name: "Família Souza",
        messages: 28,
        unread: 0,
        pinned: true,
        muted: false,
        ago: 3_600,
    },
    Spec {
        id: BRUNO,
        name: "Bruno Lima",
        messages: 12,
        unread: 0,
        pinned: false,
        muted: false,
        ago: 7_200,
    },
    Spec {
        id: CAMILA,
        name: "Dra. Camila",
        messages: 3,
        unread: 1,
        pinned: false,
        muted: false,
        ago: 90_000,
    },
    Spec {
        id: WORK,
        name: "Trabalho — Projeto Aurora",
        messages: 90,
        unread: 0,
        pinned: false,
        muted: true,
        ago: 200_000,
    },
];

fn is_group(id: &str) -> bool {
    id.ends_with("@g.us")
}

fn text(content: &str) -> Content {
    Content::Text {
        text: content.to_owned(),
        preview: None,
    }
}

fn media(mime: &str, size: u64) -> Media {
    Media {
        mime: mime.into(),
        size,
        width: None,
        height: None,
        album: None,
        path: None,
        state: Default::default(),
    }
}

/// The whole history of a chat, oldest first.
pub fn history(id: &str) -> Vec<Message> {
    let Some((index, spec)) = SPECS.iter().enumerate().find(|(_, spec)| spec.id == id) else {
        return Vec::new();
    };
    let mut messages: Vec<Message> = Vec::with_capacity(spec.messages);
    for n in 0..spec.messages {
        // The last message sits `ago` seconds back; the rest every 7 minutes
        // before it, a long gap now and then so day separators appear.
        let behind = (spec.messages - 1 - n) as i64;
        let timestamp = NOW - spec.ago - behind * 420 - (behind / 9) * 40_000;
        let from_me = (n + index) % 3 == 0;
        let (sender, sender_name) = match (from_me, is_group(id)) {
            (true, _) => (ME.to_owned(), None),
            (false, false) => (id.to_owned(), None),
            (false, true) => {
                let who = ["Ana Souza", "Bruno Lima", "Tia Rosa"][(n + index) % 3];
                (
                    format!("55119111100{:02}@s.whatsapp.net", n % 3 + 1),
                    Some(who.to_owned()),
                )
            }
        };
        let mut content = text(LINES[(n * 5 + index * 7) % LINES.len()]);
        if id == ANA && n == 20 {
            content = Content::Image {
                caption: Some("Olha que vista".into()),
                media: media("image/jpeg", 120_000),
                motion: None,
            };
        } else if id == ANA && n == 30 {
            content = Content::Audio {
                media: media("audio/ogg", 24_000),
                seconds: Some(37),
                voice_note: true,
                waveform: Vec::new(),
            };
        } else if id == ANA && n == 40 {
            content = Content::Document {
                media: media("application/pdf", 480_000),
                file_name: "contrato.pdf".into(),
                caption: None,
                pages: Some(4),
            };
        }
        let mut message = Message {
            id: format!("{index}-{n:03}"),
            chat: id.to_owned(),
            sender,
            sender_name,
            from_me,
            timestamp,
            history_order: None,
            content,
            status: if from_me {
                Delivery::Read
            } else {
                Delivery::None
            },
            delivered_at: None,
            read_at: None,
            quoted: None,
            reactions: Vec::new(),
            edited: false,
            mentions: Vec::new(),
            forwarded: false,
            thumbnail: None,
        };
        // Every seventh message quotes the one before it; every eleventh has a
        // reaction from the other side.
        if n % 7 == 6
            && let Some(previous) = messages.last()
        {
            message.quoted = Some(Quoted {
                chat: None,
                id: previous.id.clone(),
                sender: previous.sender.clone(),
                sender_name: previous.sender_name.clone(),
                summary: match &previous.content {
                    Content::Text { text, .. } => text.clone(),
                    other => other.summary(),
                },
                labelled: Some(!matches!(previous.content, Content::Text { .. })),
                mentions: Vec::new(),
            });
        }
        if n % 11 == 5 {
            message.reactions.push(Reaction {
                sender: if from_me {
                    id.to_owned()
                } else {
                    ME.to_owned()
                },
                from_me: !from_me,
                emoji: ["👍", "❤️", "😂"][n % 3].to_owned(),
            });
        }
        messages.push(message);
    }
    messages
}

fn last_of(messages: &[Message]) -> Option<LastMessage> {
    messages.last().map(|message| {
        let summary = match &message.content {
            Content::Text { text, .. } => text.clone(),
            other => other.summary(),
        };
        LastMessage {
            from_me: message.from_me,
            sender: message.sender.clone(),
            sender_name: message.sender_name.clone(),
            full: summary.clone(),
            summary,
            labelled: !matches!(message.content, Content::Text { .. }),
            status: message.status,
        }
    })
}

/// The chat list, newest first.
pub fn chats() -> Vec<Chat> {
    let mut chats: Vec<Chat> = SPECS
        .iter()
        .map(|spec| {
            let messages = history(spec.id);
            let mut chat = Chat::new(spec.id.to_owned(), spec.name.to_owned());
            chat.group_subject_known = is_group(spec.id);
            chat.last_activity = NOW - spec.ago;
            chat.unread = spec.unread;
            chat.pinned = spec.pinned;
            chat.pinned_at = if spec.pinned { NOW * 1000 } else { 0 };
            chat.muted_until = spec.muted.then_some(0);
            chat.last = last_of(&messages);
            chat
        })
        .collect();
    // A person never written to: not listed unless searched for.
    chats.push(Chat::new(STRANGER.to_owned(), "Fulano de Tal".to_owned()));
    chats.sort_by_key(|chat| std::cmp::Reverse(chat.last_activity));
    chats
}

/// The events a worker sends when the account is linked and connected.
pub fn start(injector: &Injector) {
    injector.send(Event::Link(LinkStatus::Connecting));
    injector.send(Event::Me {
        id: ME.to_owned(),
        lid: None,
        name: Some("Gigio".to_owned()),
        about: None,
    });
    injector.send(Event::Link(LinkStatus::Connected));
    injector.send(Event::Chats(chats()));
}

/// The events of the pairing screen: a code to scan.
pub fn unlinked(injector: &Injector) {
    injector.send(Event::Link(LinkStatus::Unlinked {
        qr: Some("2@DEMO-CODE-NOT-A-REAL-LOGIN,aGVsbG8gZnJvbSB0aGUgZGVtbw==,c3ludGhldGlj".into()),
        pair_code: None,
        pairing_phone: None,
    }));
}

/// Stands in for the worker: answers the commands the store sends.
pub struct Responder {
    injector: Injector,
    world: RefCell<World>,
}

struct World {
    chats: Vec<Chat>,
    messages: HashMap<ChatId, Vec<Message>>,
    sent: u32,
}

impl Responder {
    pub fn new(injector: Injector) -> Self {
        let chats = chats();
        let messages = SPECS
            .iter()
            .map(|spec| (spec.id.to_owned(), history(spec.id)))
            .collect();
        Self {
            injector,
            world: RefCell::new(World {
                chats,
                messages,
                sent: 0,
            }),
        }
    }

    fn page(&self, chat: &str, before: Option<(i64, String)>) {
        let world = self.world.borrow();
        let all = world.messages.get(chat).map(Vec::as_slice).unwrap_or(&[]);
        let end = match &before {
            Some((_, id)) => all.iter().position(|m| &m.id == id).unwrap_or(all.len()),
            None => all.len(),
        };
        let start = end.saturating_sub(PAGE);
        self.injector.send(Event::Messages {
            chat: chat.to_owned(),
            messages: all[start..end].to_vec(),
            older: before.is_some(),
            complete: start == 0,
        });
    }

    fn send_text(&self, chat: &str, body: &str, quoting: Option<zapfast::model::ReplyTarget>) {
        let (message, delivered, updated) = {
            let mut world = self.world.borrow_mut();
            world.sent += 1;
            let sent = world.sent;
            let messages = world.messages.entry(chat.to_owned()).or_default();
            let timestamp = messages.last().map_or(NOW, |m| m.timestamp.max(NOW)) + 60;
            let quoted = quoting.and_then(|target| {
                messages.iter().find(|m| m.id == target.id).map(|m| Quoted {
                    chat: None,
                    id: m.id.clone(),
                    sender: m.sender.clone(),
                    sender_name: m.sender_name.clone(),
                    summary: match &m.content {
                        Content::Text { text, .. } => text.clone(),
                        other => other.summary(),
                    },
                    labelled: Some(!matches!(m.content, Content::Text { .. })),
                    mentions: Vec::new(),
                })
            });
            let mut message = Message {
                id: format!("sent-{sent:03}"),
                chat: chat.to_owned(),
                sender: ME.to_owned(),
                sender_name: None,
                from_me: true,
                timestamp,
                history_order: None,
                content: text(body),
                status: Delivery::Pending,
                delivered_at: None,
                read_at: None,
                quoted,
                reactions: Vec::new(),
                edited: false,
                mentions: Vec::new(),
                forwarded: false,
                thumbnail: None,
            };
            let pending = message.clone();
            message.status = Delivery::Delivered;
            messages.push(message.clone());
            let chat_row = world.chats.iter_mut().find(|c| c.id == chat).map(|c| {
                c.last_activity = timestamp;
                c.last = last_of(std::slice::from_ref(&message));
                c.clone()
            });
            (pending, message, chat_row)
        };
        self.injector.send(Event::Messages {
            chat: chat.to_owned(),
            messages: vec![message],
            older: false,
            complete: false,
        });
        self.injector
            .send(Event::MessageUpdated(Box::new(delivered)));
        if let Some(chat) = updated {
            self.injector.send(Event::ChatUpdated(Box::new(chat)));
        }
    }

    fn react(&self, chat: &str, message: &str, emoji: &str) {
        let updated = {
            let mut world = self.world.borrow_mut();
            let Some(found) = world
                .messages
                .get_mut(chat)
                .and_then(|all| all.iter_mut().find(|m| m.id == message))
            else {
                return;
            };
            found.reactions.retain(|r| !r.from_me);
            if !emoji.is_empty() {
                found.reactions.push(Reaction {
                    sender: ME.to_owned(),
                    from_me: true,
                    emoji: emoji.to_owned(),
                });
            }
            found.clone()
        };
        self.injector.send(Event::MessageUpdated(Box::new(updated)));
    }

    fn read(&self, chat: &str) {
        let updated = {
            let mut world = self.world.borrow_mut();
            world.chats.iter_mut().find(|c| c.id == chat).map(|c| {
                c.unread = 0;
                c.marked_unread = false;
                c.clone()
            })
        };
        if let Some(chat) = updated {
            self.injector.send(Event::ChatUpdated(Box::new(chat)));
        }
    }
}

impl CommandSink for Responder {
    fn send(&self, command: Command) {
        match command {
            Command::LoadChat { chat, before } => self.page(&chat, before),
            Command::SendText {
                chat,
                text,
                quoting,
                ..
            } => self.send_text(&chat, &text, quoting),
            Command::React {
                chat,
                message,
                emoji,
            } => self.react(&chat, &message, &emoji),
            Command::MarkRead { chat, .. } => self.read(&chat),
            Command::FetchAvatar { id, full } => self.injector.send(Event::Avatar {
                id,
                full,
                path: None,
            }),
            // History from the phone, the updater and the rest: nothing to
            // answer in a demo.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::Bridge;
    use crate::state::Store;
    use gpui::{AppContext as _, TestAppContext};
    use std::rc::Rc;

    #[test]
    fn the_data_is_the_same_every_time() {
        assert_eq!(history(ANA), history(ANA));
        assert_eq!(chats(), chats());
        let ana = history(ANA);
        assert_eq!(ana.len(), 64);
        assert_eq!(ana.last().unwrap().timestamp, NOW - 120);
    }

    #[test]
    fn history_is_in_order_and_has_what_the_screens_draw() {
        for spec in &SPECS {
            let messages = history(spec.id);
            assert_eq!(messages.len(), spec.messages);
            assert!(messages.windows(2).all(|w| w[0].timestamp < w[1].timestamp));
            let ids: std::collections::HashSet<_> = messages.iter().map(|m| &m.id).collect();
            assert_eq!(ids.len(), messages.len(), "unique ids");
        }
        let ana = history(ANA);
        assert!(ana.iter().any(|m| m.quoted.is_some()));
        assert!(ana.iter().any(|m| !m.reactions.is_empty()));
        assert!(
            ana.iter()
                .any(|m| matches!(m.content, Content::Image { .. }))
        );
        assert!(
            ana.iter()
                .any(|m| matches!(m.content, Content::Audio { .. }))
        );
        assert!(
            ana.iter()
                .any(|m| matches!(m.content, Content::Document { .. }))
        );
        let group = history(FAMILY);
        assert!(group.iter().any(|m| m.sender_name.is_some()));
    }

    #[test]
    fn the_chat_list_has_a_pin_an_unread_a_muted_and_a_stranger() {
        let chats = chats();
        assert!(
            chats
                .windows(2)
                .all(|w| w[0].last_activity >= w[1].last_activity)
        );
        assert!(chats.iter().any(|c| c.pinned));
        assert!(chats.iter().any(|c| c.unread > 0));
        assert!(chats.iter().any(|c| c.muted_until.is_some()));
        assert!(chats.iter().any(|c| c.id == STRANGER && c.never_messaged()));
    }

    /// The whole loop the screens will run on: a store fed by the injector,
    /// answered by the responder, no WhatsApp anywhere.
    #[gpui::test]
    async fn the_demo_answers_the_store_through_the_bridge(cx: &mut TestAppContext) {
        let (mut bridge, injector) =
            Bridge::detached_with(|injector| Rc::new(Responder::new(injector)));
        let store = cx.new(|cx| Store::new(bridge.sink(), cx));
        let _pump = store.update(cx, |_, cx| bridge.pump(cx));
        start(&injector);
        cx.run_until_parked();
        store.read_with(cx, |store, cx| {
            assert!(store.session.read(cx).is_online());
            let list = store.chats.read(cx);
            assert_eq!(list.visible().len(), 5, "the stranger is not listed");
            assert_eq!(list.visible()[0].id, FAMILY, "the pinned chat is first");
            assert_eq!(list.unread_total(), 3);
        });

        // Opening a chat loads the newest page and reads the chat.
        store.update(cx, |store, cx| store.open_chat(Some(ANA.into()), cx));
        cx.run_until_parked();
        let ana = store.update(cx, |store, cx| store.conversation(ANA, cx));
        ana.read_with(cx, |c, _| {
            assert_eq!(c.messages.len(), PAGE);
            assert!(!c.complete);
            assert_eq!(c.messages.last().unwrap().id, "0-063");
        });
        store.read_with(cx, |store, cx| {
            assert_eq!(store.chats.read(cx).chat(ANA).unwrap().unread, 0);
            assert_eq!(store.chats.read(cx).unread_total(), 1);
        });

        // Scrolling to the top loads the page before it, then the rest.
        store.update(cx, |store, cx| store.load_older(ANA, cx));
        cx.run_until_parked();
        ana.read_with(cx, |c, _| assert_eq!(c.messages.len(), 2 * PAGE));
        store.update(cx, |store, cx| store.load_older(ANA, cx));
        cx.run_until_parked();
        ana.read_with(cx, |c, _| {
            assert_eq!(c.messages.len(), 64);
            assert!(c.complete);
            assert_eq!(c.messages[0].id, "0-000");
        });

        // Sending: the message appears, then its tick says delivered.
        store.update(cx, |store, cx| {
            assert!(store.send_text(ANA, "Oi, Ana!", None, cx))
        });
        cx.run_until_parked();
        ana.read_with(cx, |c, _| {
            let sent = c.messages.last().unwrap();
            assert_eq!(sent.id, "sent-001");
            assert!(sent.from_me);
            assert_eq!(sent.status, Delivery::Delivered);
        });
        store.read_with(cx, |store, cx| {
            let chat = store.chats.read(cx).chat(ANA).unwrap().clone();
            assert_eq!(chat.last.unwrap().summary, "Oi, Ana!");
        });

        // Replying quotes the message answered.
        store.update(cx, |store, cx| {
            assert!(store.send_text(ANA, "Pode ser!", Some("0-010".into()), cx))
        });
        cx.run_until_parked();
        ana.read_with(cx, |c, _| {
            let reply = c.messages.last().unwrap();
            let quoted = reply.quoted.as_ref().expect("a reply quotes");
            assert_eq!(quoted.id, "0-010");
            assert_eq!(c.message("0-010").map(|m| m.id.as_str()), Some("0-010"));
        });

        // Reacting, and taking the reaction back.
        store.update(cx, |store, cx| store.react(ANA, "0-063", "👍", cx));
        cx.run_until_parked();
        ana.read_with(cx, |c, _| {
            let reactions = &c.message("0-063").unwrap().reactions;
            assert!(reactions.iter().any(|r| r.from_me && r.emoji == "👍"));
        });
        store.update(cx, |store, cx| store.react(ANA, "0-063", "👍", cx));
        cx.run_until_parked();
        ana.read_with(cx, |c, _| {
            assert!(
                !c.message("0-063")
                    .unwrap()
                    .reactions
                    .iter()
                    .any(|r| r.from_me)
            );
        });
    }
}
