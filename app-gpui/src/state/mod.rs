//! The app's state, as GPUI entities. See [`store`] for how events and
//! commands move through it.

pub mod chats;
pub mod conversation;
pub mod qr;
pub mod session;
pub mod settings;
pub mod store;

pub use chats::{ChatList, ChatListEvent};
pub use conversation::{Conversation, ConversationEvent};
pub use qr::QrModules;
pub use session::{Me, PairingMethod, Session, SessionEvent};
pub use settings::{AppSettings, SettingsEvent};
pub use store::{CommandSink, RecordingSink, Store, StoreEvent};
