//! ZapFast internals exposed for diagnostics and tests.
//!
//! The `egui-ui` feature, on by default, adds the egui interface: the window,
//! its views and everything that draws or lays out for it. Without it this is
//! the backend alone: the WhatsApp worker, the archive, the model, the
//! settings and the platform services, for another interface to build on.

#[cfg(feature = "egui-ui")]
pub mod a11y;
#[cfg(feature = "egui-ui")]
pub mod account;
pub mod animation;
#[cfg(feature = "egui-ui")]
pub mod announcer;
#[cfg(feature = "egui-ui")]
pub mod app;
pub mod app_lock;
#[cfg(target_os = "linux")]
pub mod appearance;
pub mod archive;
pub mod audio;
pub mod autostart;
#[cfg(feature = "egui-ui")]
pub mod backdrop;
pub mod backend;
#[cfg(feature = "egui-ui")]
pub mod bidi;
pub mod blocking;
pub mod call_audio;
pub mod call_diag;
pub mod call_tones;
pub mod calls;
pub mod camera;
#[cfg(all(feature = "egui-ui", target_os = "linux"))]
pub mod clipboard;
#[cfg(all(feature = "egui-ui", any(test, feature = "demo")))]
pub mod demo;
pub mod diagnostics;
#[cfg(feature = "egui-ui")]
pub mod emoji;
pub mod geo;
pub mod group_admin;
pub mod i18n;
#[cfg(feature = "egui-ui")]
pub mod image_cache;
#[cfg(feature = "egui-ui")]
pub mod image_preview;
pub mod lottie;
#[cfg(all(feature = "egui-ui", target_os = "macos"))]
pub mod macos;
pub mod markup;
pub mod media_pause;
pub mod mentions;
pub mod model;
#[cfg(feature = "egui-ui")]
pub mod motion;
pub mod nav;
pub mod new_group;
pub mod notify;
pub mod opener;
pub mod paths;
pub mod pixels;
pub mod privacy;
pub mod proxy;
#[cfg(feature = "egui-ui")]
pub mod qr;
pub mod safety;
pub mod settings;
pub mod single_instance;
pub mod sticker_meta;
pub mod sticker_search;
pub mod target;
#[cfg(feature = "egui-ui")]
pub mod theme;
pub mod timestretch;
#[cfg(feature = "egui-ui")]
pub mod transcript;
pub mod transport;
#[cfg(feature = "egui-ui")]
pub mod ui;
pub mod updates;
pub mod util;
pub mod video;
pub mod video_rate;
pub mod voice;
pub mod wallpaper;
