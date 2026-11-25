//! The screens: login with QR, the chat list and the conversation, drawn from
//! the entities in [`crate::state`]. Views read those entities and call the
//! store's intents (`open_chat`, `send_text`, `react`, `load_older`, ...);
//! they never see a backend event and never touch a protobuf. The root view is
//! [`crate::shell::Shell`].

pub mod avatar;
pub mod chat_list;
pub mod clip;
pub mod composer;
pub mod conversation;
pub mod copy;
pub mod login;
pub mod markup;
pub mod media;
pub mod msgview;
pub mod panel;
pub mod selectable;
pub mod selection;
pub mod timefmt;

#[cfg(test)]
pub(crate) mod testing;
