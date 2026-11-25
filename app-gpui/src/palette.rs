//! ZapFast's colors (src/theme.rs `Palette::light` / `Palette::dark`), as GPUI
//! colors. Only the roles this spike draws are kept.

use gpui::{Hsla, Rgba, WindowAppearance, rgb};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// App background behind the panels.
    pub window: Hsla,
    pub panel: Hsla,
    pub surface: Hsla,
    pub surface_hover: Hsla,
    pub surface_active: Hsla,
    pub outline: Hsla,
    pub text: Hsla,
    pub secondary: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
    /// Conversation background behind the bubbles.
    pub chat: Hsla,
    pub bubble_in: Hsla,
    pub bubble_out: Hsla,
    pub link: Hsla,
    /// Read-receipt blue.
    pub read: Hsla,
    /// Doodle tint over the chat background (`wallpaper::doodle_ink`).
    pub doodle: Hsla,
    pub selection: Hsla,
    pub highlight: Hsla,
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn with_alpha(color: Hsla, alpha: f32) -> Hsla {
    Hsla { a: alpha, ..color }
}

/// ZapFast's `wallpaper::doodle_ink`: near-black or white, whichever stands
/// further from the background, as opaque as it takes to differ by a fixed step.
fn doodle_ink(background: u32) -> Hsla {
    let Rgba { r, g, b, .. } = rgb(background);
    let luma = 0.2126 * r * 255.0 + 0.7152 * g * 255.0 + 0.0722 * b * 255.0;
    let (ink, step) = if luma - 30.0 > 255.0 - luma {
        (30.0, 23.2)
    } else {
        (255.0, 38.0)
    };
    let distance = (luma - ink).abs().max(1.0);
    let alpha = (step / distance).clamp(0.1, 0.4);
    Rgba {
        r: ink / 255.0,
        g: ink / 255.0,
        b: ink / 255.0,
        a: alpha,
    }
    .into()
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            dark: true,
            window: c(0x0b141a),
            panel: c(0x111b21),
            surface: c(0x202c33),
            surface_hover: c(0x2a3942),
            surface_active: c(0x35444d),
            outline: c(0x222d34),
            text: c(0xe9edef),
            secondary: c(0x97a5ae),
            accent: c(0x00a884),
            on_accent: c(0x0b141a),
            chat: c(0x0b141a),
            bubble_in: c(0x202c33),
            bubble_out: c(0x005c4b),
            link: c(0x53bdeb),
            read: c(0x53bdeb),
            doodle: doodle_ink(0x0b141a),
            selection: with_alpha(c(0x53bdeb), 0.38),
            highlight: with_alpha(c(0xffd279), 0.30),
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            window: c(0xf0f2f5),
            panel: c(0xffffff),
            surface: c(0xf0f2f5),
            surface_hover: c(0xe6e9ec),
            surface_active: c(0xd9dde1),
            outline: c(0xe9edef),
            text: c(0x111b21),
            secondary: c(0x515f67),
            accent: c(0x008069),
            on_accent: c(0xffffff),
            chat: c(0xefeae2),
            bubble_in: c(0xffffff),
            bubble_out: c(0xd9fdd3),
            link: c(0x027eb5),
            read: c(0x027eb5),
            doodle: doodle_ink(0xefeae2),
            selection: with_alpha(c(0x027eb5), 0.30),
            highlight: with_alpha(c(0xffb300), 0.35),
        }
    }

    pub fn for_appearance(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    pub fn for_window(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doodles_are_faint_in_both_themes() {
        for palette in [Palette::light(), Palette::dark()] {
            assert!((0.1..=0.4).contains(&palette.doodle.a));
        }
        // Dark ink on the light chat background, white ink on the dark one.
        assert!(Palette::light().doodle.l < 0.2);
        assert!(Palette::dark().doodle.l > 0.9);
    }
}
