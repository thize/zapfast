//! The assets, all compiled in. Everything shared with ZapFast is referenced
//! by its path in the repository (`../../assets/...`), not copied; only the
//! few Lucide icons ZapFast takes from its `fastframe-icons` dependency, and
//! Inter from `fastframe-fonts`, live under this crate's own `assets/`.

use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

macro_rules! zapfast {
    ($name:literal) => {
        (
            concat!("icons/", $name),
            include_bytes!(concat!("../../assets/icons/", $name)).as_slice(),
        )
    };
}

macro_rules! local {
    ($name:literal) => {
        (
            concat!("icons/", $name),
            include_bytes!(concat!("../assets/icons/", $name)).as_slice(),
        )
    };
}

const FILES: &[(&str, &[u8])] = &[
    // ZapFast's own Lucide set (assets/icons).
    zapfast!("send.svg"),
    zapfast!("paperclip.svg"),
    zapfast!("smile.svg"),
    zapfast!("arrow-down.svg"),
    zapfast!("delivery-tick.svg"),
    zapfast!("delivery-ticks.svg"),
    zapfast!("message-circle.svg"),
    zapfast!("video.svg"),
    zapfast!("phone.svg"),
    zapfast!("reply.svg"),
    zapfast!("image.svg"),
    zapfast!("keyboard.svg"),
    zapfast!("timer.svg"),
    zapfast!("sticker.svg"),
    // The rest of the Lucide icons ZapFast ships through fastframe-icons.
    local!("search.svg"),
    local!("mic.svg"),
    local!("ellipsis.svg"),
    local!("plus.svg"),
    local!("check.svg"),
    local!("x.svg"),
    local!("chevron-down.svg"),
    // The chat wallpaper and the app logo.
    (
        "wallpaper.svg",
        include_bytes!("../../assets/wallpaper.svg").as_slice(),
    ),
    (
        "logo.svg",
        include_bytes!("../../packaging/icons/zapfast.svg").as_slice(),
    ),
];

/// Real pictures from ZapFast's demo assets (`assets/demo`, CC0 or public
/// domain, see its SOURCES.md), for the Media scene.
const DEMO: &[(&str, &[u8])] = &[
    (
        "demo/photos/launch.jpg",
        include_bytes!("../../assets/demo/photos/launch.jpg").as_slice(),
    ),
    (
        "demo/photos/spacewalk.jpg",
        include_bytes!("../../assets/demo/photos/spacewalk.jpg").as_slice(),
    ),
    // An animated WebP: 16 frames of 100 ms.
    (
        "demo/stickers/astronaut-float.webp",
        include_bytes!("../../assets/demo/stickers/astronaut-float.webp").as_slice(),
    ),
];

pub const INTER: &[u8] = include_bytes!("../assets/fonts/InterVariable.ttf");

/// Size of the wallpaper tile, in logical pixels (its SVG `width`/`height`).
pub const WALLPAPER_TILE: (f32, f32) = (374.0, 666.0);

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES
            .iter()
            .chain(DEMO)
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .chain(DEMO)
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_asset_is_present_and_looks_like_what_it_claims() {
        for (name, bytes) in FILES {
            assert!(!bytes.is_empty(), "{name}");
            let head = std::str::from_utf8(&bytes[..bytes.len().min(400)]).unwrap();
            assert!(head.contains("<svg"), "{name}");
        }
        assert!(Assets.load("icons/send.svg").unwrap().is_some());
        for (name, bytes) in DEMO {
            assert!(bytes.len() > 1000, "{name}");
            assert!(Assets.load(name).unwrap().is_some(), "{name}");
        }
        // JPEG, JPEG, and a RIFF/WEBP container with the animation chunk.
        assert_eq!(DEMO[0].1[..2], [0xFF, 0xD8]);
        assert_eq!(&DEMO[2].1[8..12], b"WEBP");
        assert!(DEMO[2].1.windows(4).any(|w| w == b"ANMF"));
        assert!(Assets.load("icons/nope.svg").unwrap().is_none());
        assert_eq!(INTER[..4], [0, 1, 0, 0]);
    }
}
