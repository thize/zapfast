//! Desktop notifications for messages that arrive while the reader is not
//! looking at the chat, through ZapFast's `fastframe-notifications`, under
//! this app's own identity (bundle identifier and name), so Notification
//! Center files them apart from ZapFast's.
//!
//! [`note_for`] decides, with no I/O, whether a message deserves one and what
//! it says; [`Notifier`] hands it to the system on a thread of its own (the
//! permission request waits for the reader's answer) and reports a click as the
//! chat to open.

use crate::brand::APP_NAME;
use crate::identity::bundle_id;
use fastframe_notifications::{App as Identity, Client, Error, Event, Handle, Notification};
use futures::channel::mpsc::UnboundedSender;
use std::sync::{Arc, Mutex};
use zapfast::i18n::Locale;
use zapfast::model::{Chat, ChatId, ChatKind, Message};
use zapfast::settings::Settings;

/// Notifications that may still wait for a click, as the root keeps them: the
/// oldest is let go past this, so a long absence does not pile up handles.
const MAX_HANDLES: usize = 32;

/// What a notification says and which chat a click on it opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub chat: ChatId,
    pub title: String,
    pub body: String,
}

/// The reader's situation when a message arrives.
#[derive(Clone, Copy, Debug)]
pub struct Presence<'a> {
    /// Whether the window is the one in front.
    pub window_active: bool,
    /// The chat on screen.
    pub open_chat: Option<&'a str>,
    /// Unix seconds, for mutes that end.
    pub now: i64,
}

/// The notification a live message from someone else deserves, or `None`:
/// notifications off, our own message, a muted chat, or a reader who is
/// looking at that very chat.
pub fn note_for(
    chat: &Chat,
    message: &Message,
    settings: &Settings,
    presence: Presence<'_>,
    locale: Locale,
) -> Option<Note> {
    if !settings.notifications || message.from_me || chat.muted(presence.now) {
        return None;
    }
    if presence.window_active && presence.open_chat == Some(chat.id.as_str()) {
        return None;
    }
    let (title, body) = if settings.notification_content {
        let sender = crate::views::msgview::sender_name(message);
        zapfast::notify::lines(
            &chat.name,
            chat.kind == ChatKind::Group,
            &sender,
            &message.content.summary_in(locale),
            None,
        )
    } else {
        let (_, body) = zapfast::notify::locked_lines(locale);
        (APP_NAME.to_owned(), body)
    };
    Some(Note {
        chat: chat.id.clone(),
        title,
        body,
    })
}

/// Hands notes to the system, and tells which chat a click opened.
pub struct Notifier {
    client: Option<Result<Client, Error>>,
    clicks: UnboundedSender<ChatId>,
    /// Kept while a click matters (dropping the last handle releases the
    /// listener).
    handles: Arc<Mutex<Vec<Handle>>>,
}

impl Notifier {
    pub fn new(clicks: UnboundedSender<ChatId>) -> Self {
        Self {
            client: None,
            clicks,
            handles: Arc::default(),
        }
    }

    fn client(&mut self) -> &Result<Client, Error> {
        self.client.get_or_insert_with(|| {
            Client::new(
                Identity::new(bundle_id(), APP_NAME)
                    // ZapFast's logo and brand green, as the root draws them.
                    .logo(
                        include_bytes!("../../../packaging/macos/icon-1024.png"),
                        [0x00, 0xa8, 0x84],
                    ),
            )
        })
    }

    /// Shows `note`. Permission is asked once by the system (this may prompt)
    /// and everything after happens on the worker's thread: a refusal or a
    /// missing bundle is logged and the message simply goes unannounced.
    pub fn show(&mut self, note: Note) {
        let client = match self.client() {
            Ok(client) => client.clone(),
            Err(error) => {
                log::warn!("notifications unavailable: {error}");
                return;
            }
        };
        let clicks = self.clicks.clone();
        let handles = self.handles.clone();
        std::thread::spawn(move || {
            if let Err(error) = client.request_permission_blocking() {
                log::warn!("notification permission: {error}");
                return;
            }
            let chat = note.chat.clone();
            let shown = client.show_with(
                Notification::new(note.title)
                    .body(note.body)
                    // One per chat: a newer message replaces the older one.
                    .key(note.chat)
                    .category("im.received"),
                move |event| {
                    if let Event::Activated = event {
                        let _ = clicks.unbounded_send(chat.clone());
                    }
                },
            );
            match shown {
                Ok(handle) => {
                    let mut kept = handles.lock().unwrap_or_else(|e| e.into_inner());
                    kept.push(handle);
                    if kept.len() > MAX_HANDLES {
                        kept.remove(0);
                    }
                }
                Err(error) => log::warn!("notification not shown: {error}"),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::{ANA, FAMILY};
    use zapfast::model::{Content, Delivery};

    fn chat(id: &str, name: &str, kind: ChatKind) -> Chat {
        let mut chat = crate::state::chats::fixtures::chat(id, name, 0);
        chat.kind = kind;
        chat
    }

    fn message(from_me: bool) -> Message {
        Message {
            id: "m1".into(),
            chat: ANA.into(),
            sender: ANA.into(),
            sender_name: Some("Ana Souza".into()),
            from_me,
            timestamp: 1,
            history_order: None,
            content: Content::Text {
                text: "Chego em dez minutos".into(),
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

    const AWAY: Presence<'static> = Presence {
        window_active: false,
        open_chat: None,
        now: 100,
    };

    fn pt() -> Locale {
        Locale::PortugueseBrazil
    }

    #[test]
    fn a_message_from_someone_else_notifies_with_the_chat_and_the_text() {
        let note = note_for(
            &chat(ANA, "Ana Souza", ChatKind::Direct),
            &message(false),
            &Settings::default(),
            AWAY,
            pt(),
        )
        .expect("a notification");
        assert_eq!(note.title, "Ana Souza");
        assert_eq!(note.body, "Chego em dez minutos");
        assert_eq!(note.chat, ANA);
    }

    #[test]
    fn a_group_message_names_who_wrote_it() {
        let note = note_for(
            &chat(FAMILY, "Família Souza", ChatKind::Group),
            &message(false),
            &Settings::default(),
            AWAY,
            pt(),
        )
        .unwrap();
        assert_eq!(note.body, "Ana Souza: Chego em dez minutos");
    }

    #[test]
    fn hiding_the_content_says_only_that_a_message_came() {
        let mut settings = Settings::default();
        settings.notification_content = false;
        let note = note_for(
            &chat(ANA, "Ana Souza", ChatKind::Direct),
            &message(false),
            &settings,
            AWAY,
            pt(),
        )
        .unwrap();
        assert_eq!(note.title, APP_NAME);
        assert!(!note.body.contains("Chego"), "{}", note.body);
        assert!(!note.title.contains("Ana"));
    }

    #[test]
    fn no_notification_for_our_own_muted_or_looked_at_chats_or_when_off() {
        let ana = chat(ANA, "Ana Souza", ChatKind::Direct);
        let on = Settings::default();
        assert!(note_for(&ana, &message(true), &on, AWAY, pt()).is_none());

        let mut muted = ana.clone();
        muted.muted_until = Some(0);
        assert!(note_for(&muted, &message(false), &on, AWAY, pt()).is_none());
        // A mute that ended no longer counts.
        muted.muted_until = Some(50);
        assert!(note_for(&muted, &message(false), &on, AWAY, pt()).is_some());

        let mut off = Settings::default();
        off.notifications = false;
        assert!(note_for(&ana, &message(false), &off, AWAY, pt()).is_none());

        // Looking at that chat: nothing. Window in front on another chat, or
        // the window away from that chat: notified.
        let looking = Presence {
            window_active: true,
            open_chat: Some(ANA),
            now: 100,
        };
        assert!(note_for(&ana, &message(false), &on, looking, pt()).is_none());
        let elsewhere = Presence {
            open_chat: Some(FAMILY),
            ..looking
        };
        assert!(note_for(&ana, &message(false), &on, elsewhere, pt()).is_some());
        let behind = Presence {
            window_active: false,
            ..looking
        };
        assert!(note_for(&ana, &message(false), &on, behind, pt()).is_some());
    }

    #[test]
    fn the_identity_is_this_apps_bundle_and_not_zapfasts() {
        assert!(bundle_id().ends_with(crate::brand::APP_ID));
        assert_ne!(bundle_id(), "me.paolino.fastsapp");
    }
}
