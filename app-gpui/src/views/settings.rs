//! The settings screen: a page over the window with the switches that matter
//! in this phase. Each control changes [`AppSettings`] at once; the rest of
//! the app follows the entity (the theme, notifications, downloads, read
//! receipts), and the file is written a moment later.

use crate::brand::{APP_ID, APP_NAME};
use crate::palette::Palette;
use crate::state::AppSettings;
use crate::ui::{icon, palette};
use gpui::{
    Context, CursorStyle, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyBinding, MouseButton, ParentElement as _, Render, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, actions, div, px,
};
use zapfast::settings::{Settings, ThemeChoice};

actions!(settings, [CloseSettings]);

pub fn bind_keys(cx: &mut gpui::App) {
    cx.bind_keys([KeyBinding::new("escape", CloseSettings, Some("Settings"))]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsViewEvent {
    Close,
}

/// One on/off setting: what it is called, what it does, how to read and flip it.
struct Toggle {
    id: &'static str,
    title: &'static str,
    detail: &'static str,
    get: fn(&Settings) -> bool,
    set: fn(&mut Settings, bool),
}

const NOTIFICATIONS: [Toggle; 2] = [
    Toggle {
        id: "notifications",
        title: "Notificações",
        detail: "Avisar de mensagens novas quando a conversa não está aberta.",
        get: |s| s.notifications,
        set: |s, on| s.notifications = on,
    },
    Toggle {
        id: "notification-content",
        title: "Mostrar o conteúdo",
        detail: "Desligado, o aviso diz só “Nova mensagem”.",
        get: |s| s.notification_content,
        set: |s, on| s.notification_content = on,
    },
];

const CHATS: [Toggle; 2] = [
    Toggle {
        id: "auto-download",
        title: "Baixar mídia automaticamente",
        detail: "Fotos, vídeos e GIFs baixam quando aparecem na conversa.",
        get: |s| s.auto_download,
        set: |s, on| s.auto_download = on,
    },
    Toggle {
        id: "read-receipts",
        title: "Confirmação de leitura",
        detail: "Quem escreveu vê que você leu, se a sua conta permite.",
        get: |s| s.send_read_receipts,
        set: |s, on| s.send_read_receipts = on,
    },
];

const THEMES: [(ThemeChoice, &str); 3] = [
    (ThemeChoice::System, "Sistema"),
    (ThemeChoice::Light, "Claro"),
    (ThemeChoice::Dark, "Escuro"),
];

pub struct SettingsView {
    settings: Entity<AppSettings>,
    focus: FocusHandle,
    _observe: Subscription,
}

impl EventEmitter<SettingsViewEvent> for SettingsView {}

impl Focusable for SettingsView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SettingsView {
    pub fn new(settings: Entity<AppSettings>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&settings, |_, _, cx| cx.notify());
        Self {
            settings,
            focus: cx.focus_handle(),
            _observe: observe,
        }
    }

    fn toggle(&mut self, toggle: &'static Toggle, cx: &mut Context<Self>) {
        let on = !(toggle.get)(self.settings.read(cx).get());
        self.settings.update(cx, |settings, cx| {
            settings.update(|s| (toggle.set)(s, on), cx)
        });
    }

    fn choose_theme(&mut self, theme: ThemeChoice, cx: &mut Context<Self>) {
        self.settings
            .update(cx, |settings, cx| settings.update(|s| s.theme = theme, cx));
    }
}

fn section(title: &'static str, p: Palette) -> gpui::Div {
    div().flex().flex_col().gap(px(2.)).child(
        div()
            .px(px(4.))
            .pb(px(6.))
            .text_size(px(13.))
            .text_color(p.accent)
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(title),
    )
}

fn switch(on: bool, p: Palette) -> gpui::Div {
    div()
        .flex_none()
        .w(px(40.))
        .h(px(22.))
        .rounded_full()
        .bg(if on { p.accent } else { p.surface_active })
        .flex()
        .items_center()
        .px(px(2.))
        .when_else(on, |s| s.justify_end(), |s| s.justify_start())
        .child(
            div()
                .size(px(18.))
                .rounded_full()
                .bg(gpui::hsla(0., 0., 1., 1.)),
        )
}

use gpui::prelude::FluentBuilder as _;

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let settings = self.settings.read(cx).get().clone();
        let rows = |toggles: &'static [Toggle], cx: &mut Context<Self>| {
            toggles
                .iter()
                .map(|toggle| {
                    let on = (toggle.get)(&settings);
                    div()
                        .id(toggle.id)
                        .role(Role::Switch)
                        .aria_label(toggle.title)
                        .aria_toggled(if on {
                            gpui::Toggled::True
                        } else {
                            gpui::Toggled::False
                        })
                        .flex()
                        .items_center()
                        .gap(px(14.))
                        .px(px(12.))
                        .py(px(10.))
                        .rounded(px(8.))
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.bg(p.surface_hover))
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle(toggle, cx)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .text_color(p.text)
                                        .child(toggle.title),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(p.secondary)
                                        .child(toggle.detail),
                                ),
                        )
                        .child(switch(on, p))
                })
                .collect::<Vec<_>>()
        };
        let notification_rows = rows(&NOTIFICATIONS, cx);
        let chat_rows = rows(&CHATS, cx);
        let theme = settings.theme;

        div()
            .id("settings")
            .role(Role::Dialog)
            .aria_label("Configurações")
            .key_context("Settings")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseSettings, _, cx| {
                cx.emit(SettingsViewEvent::Close)
            }))
            .absolute()
            .inset_0()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.window)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex_none()
                    .h(px(60.))
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .bg(p.panel)
                    .border_b_1()
                    .border_color(p.outline)
                    .child(
                        div()
                            .id("close-settings")
                            .role(Role::Button)
                            .aria_label("Fechar configurações")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(40.))
                            .rounded_full()
                            .cursor(CursorStyle::PointingHand)
                            .hover(|s| s.bg(p.surface_hover))
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(SettingsViewEvent::Close)
                            }))
                            .child(icon("icons/x.svg", 22., p.secondary)),
                    )
                    .child(
                        div()
                            .id("settings-title")
                            .role(Role::Heading)
                            .text_size(px(20.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child("Configurações"),
                    ),
            )
            .child(
                div().flex_1().min_h_0().flex().justify_center().child(
                    div()
                        .id("settings-body")
                        .w_full()
                        .max_w(px(640.))
                        .overflow_y_scroll()
                        .px(px(16.))
                        .py(px(20.))
                        .flex()
                        .flex_col()
                        .gap(px(22.))
                        .child(
                            section("Aparência", p).child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(14.))
                                    .px(px(12.))
                                    .py(px(10.))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(15.))
                                            .text_color(p.text)
                                            .child("Tema"),
                                    )
                                    .child(div().flex().gap(px(4.)).children(THEMES.map(
                                        |(choice, label)| {
                                            let chosen = choice == theme;
                                            div()
                                                .id(label)
                                                .role(Role::RadioButton)
                                                .aria_label(label)
                                                .px(px(14.))
                                                .py(px(6.))
                                                .rounded_full()
                                                .cursor(CursorStyle::PointingHand)
                                                .text_size(px(14.))
                                                .when_else(
                                                    chosen,
                                                    |s| s.bg(p.accent).text_color(p.on_accent),
                                                    |s| {
                                                        s.bg(p.surface)
                                                            .text_color(p.text)
                                                            .hover(|s| s.bg(p.surface_hover))
                                                    },
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.choose_theme(choice, cx)
                                                }))
                                                .child(label)
                                        },
                                    ))),
                            ),
                        )
                        .child(section("Notificações", p).children(notification_rows))
                        .child(section("Conversas", p).children(chat_rows))
                        .child(
                            section("Sobre", p).child(
                                div()
                                    .px(px(12.))
                                    .text_size(px(13.))
                                    .text_color(p.secondary)
                                    .child(SharedString::from(format!(
                                        "{APP_NAME} ({APP_ID}). Seus dados, a chave do arquivo, o bloqueio de instância e o item de login são só deste aplicativo; o ZapFast instalado não é lido nem alterado. Este aplicativo não busca atualizações."
                                    ))),
                            ),
                        ),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};

    #[test]
    fn every_toggle_reads_and_writes_its_own_field() {
        let mut settings = Settings::default();
        for toggle in NOTIFICATIONS.iter().chain(CHATS.iter()) {
            let before = (toggle.get)(&settings);
            (toggle.set)(&mut settings, !before);
            assert_eq!((toggle.get)(&settings), !before, "{}", toggle.id);
        }
        // Four different fields: the defaults are all flipped by now.
        assert_eq!(
            NOTIFICATIONS
                .iter()
                .chain(CHATS.iter())
                .map(|t| t.id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            4
        );
    }

    #[gpui::test]
    fn choosing_and_toggling_change_the_settings(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(crate::ui::ActivePalette(Palette::dark()));
        });
        let settings = cx.new(|_| AppSettings::in_memory());
        let view = cx.new(|cx| SettingsView::new(settings.clone(), cx));
        view.update(cx, |view, cx| view.choose_theme(ThemeChoice::Light, cx));
        assert_eq!(
            settings.read_with(cx, |s, _| s.get().theme),
            ThemeChoice::Light
        );
        view.update(cx, |view, cx| view.toggle(&CHATS[0], cx));
        assert!(!settings.read_with(cx, |s, _| s.get().auto_download));
        view.update(cx, |view, cx| view.toggle(&CHATS[0], cx));
        assert!(settings.read_with(cx, |s, _| s.get().auto_download));
    }
}
