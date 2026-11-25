//! What the Platform scene exercises: the menu-bar tray item and the Dock badge
//! (ZapFast's `fastframe-tray` and `fastframe-dock`), native notifications, and
//! the application menu bar.

pub mod icon;
pub mod menus;
pub mod notify;
pub mod tray;

/// The bundle identifier of the spike's `.app` (see `scripts/bundle-macos.sh`);
/// macOS notifications work only from a bundle that has one.
pub const BUNDLE_ID: &str = "dev.zapfast.gpui-spike";
pub const APP_NAME: &str = "ZapFast GPUI spike";

/// Whether the executable runs from inside a `.app` bundle. `App::app_path`
/// answers the same on a real platform but is `unimplemented!()` in GPUI's
/// test platform (gpui-pre `platform/test/platform.rs:772`), so views that
/// render it could not be tested.
pub fn is_bundled() -> bool {
    std::env::current_exe().is_ok_and(|exe| {
        exe.ancestors()
            .any(|dir| dir.extension().is_some_and(|ext| ext == "app"))
    })
}
