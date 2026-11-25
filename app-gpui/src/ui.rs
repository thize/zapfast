//! Small pieces shared by every scene: the palette global, icons, avatars.

use crate::palette::Palette;
use gpui::{
    AnyElement, App, Global, Hsla, IntoElement, ObjectFit, ParentElement, RenderImage,
    SharedString, Styled, StyledImage as _, Svg, div, hsla, img, px, svg,
};
use std::sync::Arc;

/// The active palette, kept in a global so every view reads the same one and
/// a change of appearance reaches all of them with one window refresh.
pub struct ActivePalette(pub Palette);

impl Global for ActivePalette {}

/// The language the screens write content labels in.
pub struct ActiveLocale(pub zapfast::i18n::Locale);

impl Global for ActiveLocale {}

/// The active language, the system's when none was set.
pub fn locale(cx: &App) -> zapfast::i18n::Locale {
    cx.try_global::<ActiveLocale>()
        .map_or_else(zapfast::i18n::detect, |active| active.0)
}

pub fn palette(cx: &App) -> Palette {
    cx.global::<ActivePalette>().0
}

/// A Lucide icon from the embedded assets, tinted with `color`.
pub fn icon(path: &'static str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(path)
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// Up to two initials of `name`.
pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

/// A stable color per name, readable on both themes.
pub fn name_color(name: &str, dark: bool) -> Hsla {
    let hash = name.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    });
    let hue = (hash % 360) as f32 / 360.0;
    hsla(hue, 0.55, if dark { 0.68 } else { 0.38 }, 1.0)
}

pub fn avatar(name: &str, size: f32, dark: bool) -> impl IntoElement {
    let fill = name_color(name, dark);
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded_full()
        .bg(Hsla { a: 0.22, ..fill })
        .text_color(fill)
        .text_size(px(size * 0.38))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(SharedString::from(initials(name)))
}

/// A round avatar with the contact's picture when there is one.
pub fn avatar_picture(
    name: &str,
    picture: Option<Arc<RenderImage>>,
    size: f32,
    dark: bool,
) -> AnyElement {
    match picture {
        Some(picture) => img(picture)
            .size(px(size))
            .flex_none()
            .rounded_full()
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => avatar(name, size, dark).into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_take_two_letters() {
        assert_eq!(initials("Ana Souza"), "AS");
        assert_eq!(initials("Trabalho — Projeto Aurora"), "TP");
        assert_eq!(initials("é"), "É");
        assert_eq!(initials("Família"), "F");
        assert_eq!(initials(""), "");
    }

    #[test]
    fn name_colors_are_stable() {
        assert_eq!(name_color("Ana", true), name_color("Ana", true));
        assert_ne!(name_color("Ana", true), name_color("Bruno", true));
    }
}
