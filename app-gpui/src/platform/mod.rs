//! What ties the app to the desktop: the tray item and Dock badge, and
//! notifications. Everything is made under this app's own names (see
//! [`crate::identity`]): its own tray id, bundle identifier and badge, never
//! ZapFast's.
//!
//! Nothing here polls. The tray and the notification worker run on threads of
//! their own and wake a GPUI task through a channel only when something
//! happened (a click, a notification activated).

pub mod icon;
pub mod install;
pub mod notify;
pub mod tray;
