//! What a message looks like on screen, worked out once when it arrives: its
//! text with the markup interpreted, the chip that stands in for content this
//! phase does not draw (photo, voice message, ...), and who to name as sender.
//! Media itself is Phase 3.

use crate::views::markup::{self, Parsed};
use gpui::SharedString;
use zapfast::i18n::Locale;
use zapfast::model::{Chat, Content, Message, localized_summary, phone_of};

/// How the screens name the account's owner.
pub const SELF_NAME: &str = "Você";

/// A message prepared for drawing and selecting.
#[derive(Clone, Debug)]
pub struct MsgView {
    pub message: Message,
    /// The text as shown, markers removed.
    pub parsed: Parsed,
    /// `parsed.plain`, shared so each frame can hand it to the text element.
    pub shared: SharedString,
    /// A label such as `📷 Foto` for content that is not text.
    pub chip: Option<String>,
    /// The sender's name as the bubble and a copied transcript show it.
    pub sender: String,
}

impl MsgView {
    pub fn new(message: Message, locale: Locale) -> Self {
        let parsed = markup::parse(&text_of(&message.content));
        let shared = SharedString::from(parsed.plain.clone());
        let chip = chip(&message.content, locale);
        let sender = sender_name(&message);
        Self {
            message,
            parsed,
            shared,
            chip,
            sender,
        }
    }

    pub fn text(&self) -> &str {
        &self.parsed.plain
    }

    pub fn id(&self) -> &str {
        &self.message.id
    }

    pub fn timestamp(&self) -> i64 {
        self.message.timestamp
    }

    pub fn outgoing(&self) -> bool {
        self.message.from_me
    }
}

/// The name shown for the person who sent `message`.
pub fn sender_name(message: &Message) -> String {
    if message.from_me {
        return SELF_NAME.to_owned();
    }
    message
        .sender_name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| phone_of(&message.sender).map(|phone| format!("+{phone}")))
        .unwrap_or_else(|| message.sender.clone())
}

/// The name a quote shows for who wrote the quoted message.
pub fn quoted_sender(quoted: &zapfast::model::Quoted) -> String {
    quoted
        .sender_name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| phone_of(&quoted.sender).map(|phone| format!("+{phone}")))
        .unwrap_or_else(|| quoted.sender.clone())
}

/// The text of a message, or of its caption.
pub fn text_of(content: &Content) -> String {
    match content {
        Content::Text { text, .. } | Content::Interactive { text, .. } => text.clone(),
        Content::Image { caption, .. }
        | Content::Video { caption, .. }
        | Content::Document { caption, .. } => caption.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// The emoji that heads a content label, by the label's English text (the one
/// the model's summaries start with).
pub fn emoji_for_label(summary: &str) -> Option<&'static str> {
    const LABELS: [(&str, &str); 14] = [
        ("Photo", "📷"),
        ("GIF", "🎞️"),
        ("Video message", "⏺️"),
        ("Video", "🎥"),
        ("Voice message", "🎤"),
        ("Audio", "🎵"),
        ("Document", "📄"),
        ("Sticker", "🙂"),
        ("Location", "📍"),
        ("Live location", "📍"),
        ("Contact", "👤"),
        ("Poll", "📊"),
        ("Missed", "📞"),
        ("This message was deleted", "🚫"),
    ];
    LABELS
        .into_iter()
        .filter(|(label, _)| summary.starts_with(label))
        .max_by_key(|(label, _)| label.len())
        .map(|(_, emoji)| emoji)
}

fn label(locale: Locale, english: &str) -> String {
    localized_summary(locale, english).into_owned()
}

/// The chip for content that is not text, `None` for text.
pub fn chip(content: &Content, locale: Locale) -> Option<String> {
    let (emoji, text) = match content {
        Content::Text { .. } | Content::Interactive { .. } => return None,
        Content::Image { .. } => ("📷", label(locale, "Photo")),
        Content::Video { gif: true, .. } => ("🎞️", label(locale, "GIF")),
        Content::Video { note: true, .. } => ("⏺️", label(locale, "Video message")),
        Content::Video { .. } => ("🎥", label(locale, "Video")),
        Content::Audio {
            voice_note: true, ..
        } => ("🎤", content.summary_in(locale)),
        Content::Audio { .. } => ("🎵", content.summary_in(locale)),
        Content::Document { file_name, .. } => {
            ("📄", format!("{}: {file_name}", label(locale, "Document")))
        }
        Content::Sticker { .. } => ("🙂", label(locale, "Sticker")),
        Content::Location { .. } | Content::LiveLocation { .. } => {
            ("📍", content.summary_in(locale))
        }
        Content::Contact { .. } => ("👤", content.summary_in(locale)),
        Content::Poll { .. } => ("📊", content.summary_in(locale)),
        Content::Call { .. } => ("📞", content.summary_in(locale)),
        Content::Revoked => ("🚫", content.summary_in(locale)),
        _ => ("📎", content.summary_in(locale)),
    };
    Some(format!("{emoji} {text}"))
}

/// The line under a chat's name in the list: who wrote it in a group, and the
/// message (a chip for anything that is not text).
pub fn preview(chat: &Chat, locale: Locale) -> String {
    let Some(last) = &chat.last else {
        return String::new();
    };
    let body = if last.labelled {
        let text = localized_summary(locale, &last.summary).into_owned();
        match emoji_for_label(&last.summary) {
            Some(emoji) => format!("{emoji} {text}"),
            None => text,
        }
    } else {
        last.summary.clone()
    };
    if last.from_me && chat.kind == zapfast::model::ChatKind::Group {
        format!("{SELF_NAME}: {body}")
    } else if !last.from_me && chat.kind == zapfast::model::ChatKind::Group {
        match &last.sender_name {
            Some(name) if !name.is_empty() => {
                format!("{}: {body}", name.split(' ').next().unwrap_or(name))
            }
            _ => body,
        }
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zapfast::model::{Delivery, Media, MediaState};

    fn message(content: Content) -> Message {
        Message {
            id: "m1".into(),
            chat: "5511@s.whatsapp.net".into(),
            sender: "5511987654321@s.whatsapp.net".into(),
            sender_name: None,
            from_me: false,
            timestamp: 0,
            history_order: None,
            content,
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

    fn media() -> Media {
        Media {
            mime: "image/jpeg".into(),
            size: 1,
            width: None,
            height: None,
            album: None,
            path: None,
            state: MediaState::default(),
        }
    }

    #[test]
    fn text_is_parsed_and_has_no_chip() {
        let view = MsgView::new(
            message(Content::Text {
                text: "oi *tudo* bem".into(),
                preview: None,
            }),
            Locale::PortugueseBrazil,
        );
        assert_eq!(view.text(), "oi tudo bem");
        assert!(view.chip.is_none());
        assert_eq!(view.sender, "+5511987654321");
    }

    #[test]
    fn media_becomes_a_chip_and_keeps_its_caption_as_text() {
        let view = MsgView::new(
            message(Content::Image {
                caption: Some("na praia".into()),
                media: media(),
                motion: None,
            }),
            Locale::PortugueseBrazil,
        );
        assert_eq!(view.chip.as_deref(), Some("📷 Foto"));
        assert_eq!(view.text(), "na praia");
    }

    #[test]
    fn voice_messages_and_documents_have_their_own_chips() {
        let voice = chip(
            &Content::Audio {
                media: media(),
                seconds: Some(12),
                voice_note: true,
                waveform: Vec::new(),
            },
            Locale::PortugueseBrazil,
        )
        .unwrap();
        assert!(voice.starts_with("🎤 "), "{voice}");
        assert!(voice.contains("0:12"), "{voice}");
        let document = chip(
            &Content::Document {
                media: media(),
                file_name: "contrato.pdf".into(),
                caption: None,
                pages: None,
            },
            Locale::PortugueseBrazil,
        )
        .unwrap();
        assert!(document.starts_with("📄 "), "{document}");
        assert!(document.ends_with("contrato.pdf"), "{document}");
    }

    #[test]
    fn own_messages_are_named_voce() {
        let mut own = message(Content::Revoked);
        own.from_me = true;
        assert_eq!(sender_name(&own), SELF_NAME);
    }

    #[test]
    fn labels_map_to_emoji() {
        assert_eq!(emoji_for_label("Photo: legenda"), Some("📷"));
        assert_eq!(emoji_for_label("Voice message (0:03)"), Some("🎤"));
        assert_eq!(emoji_for_label("Video message"), Some("⏺️"));
        assert_eq!(emoji_for_label("Olá"), None);
    }
}
