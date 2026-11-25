//! The app's name, in one place. Change these two constants to rename the
//! app; everything that must differ from the installed ZapFast (its data
//! folder, keyring service, instance lock, login item) is derived from them,
//! see `identity`.

/// The name shown in the window title, menus and the About box.
pub const APP_NAME: &str = "ZapFast GPUI";

/// A short, lowercase, filesystem- and URL-safe name. It names the data
/// folders, the keyring service, the single-instance slot and the login item.
/// It must never equal ZapFast's own (`zapfast`, `fastsapp`, `fastwhatsapp`):
/// the tests in `identity` check that.
pub const APP_ID: &str = "zapfast-gpui";
