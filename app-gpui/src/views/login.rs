//! The screen of an account that is not linked yet: the pairing QR code with
//! the steps to scan it, or, by phone number, the pairing code to type on the
//! phone.

use crate::palette::Palette;
use crate::state::{PairingMethod, QrModules, Session, Store};
use crate::ui::{icon, palette};
use gpui::{
    AppContext as _, Bounds, Context, CursorStyle, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, Role, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, canvas, div, fill, img, point, prelude::FluentBuilder, px, rgb, size,
};
use gpui_component::input::{Input, InputEvent, InputState};
use zapfast::backend::LinkStatus;

/// The code's side on screen, in points.
pub const QR_SIDE: f32 = 264.0;

/// The pairing code as it is typed on the phone: `ABCD-EFGH`.
pub fn group_code(code: &str) -> String {
    let code: String = code.chars().filter(|c| c.is_alphanumeric()).collect();
    match code.len() {
        8 => format!("{}-{}", &code[..4], &code[4..]),
        _ => code,
    }
}

/// Digits only, as the phone number is sent.
pub fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

pub struct LoginView {
    store: Entity<Store>,
    session: Entity<Session>,
    phone: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl LoginView {
    pub fn new(store: Entity<Store>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let session = store.read(cx).session.clone();
        let phone = cx
            .new(|cx| InputState::new(window, cx).placeholder("Número com DDI e DDD, só dígitos"));
        let on_session = cx.observe(&session, |_, _, cx| cx.notify());
        let on_phone =
            cx.subscribe_in(
                &phone,
                window,
                |this, _, event: &InputEvent, _, cx| match event {
                    InputEvent::PressEnter { .. } => this.request_code(cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            );
        Self {
            store,
            session,
            phone,
            _subscriptions: vec![on_session, on_phone],
        }
    }

    pub fn method(&self, cx: &gpui::App) -> PairingMethod {
        self.session.read(cx).method
    }

    pub fn set_method(&mut self, method: PairingMethod, cx: &mut Context<Self>) {
        self.session.update(cx, |session, cx| {
            session.method = method;
            cx.notify();
        });
    }

    pub fn phone_state(&self) -> &Entity<InputState> {
        &self.phone
    }

    /// Asks the backend for a pairing code for the number typed.
    pub fn request_code(&mut self, cx: &mut Context<Self>) {
        let phone = digits(self.phone.read(cx).value().as_ref());
        if phone.len() < 8 {
            return;
        }
        self.store
            .update(cx, |store, _| store.pair_with_phone(&phone));
    }
}

fn qr_view(modules: QrModules, p: Palette) -> impl IntoElement {
    div()
        .id("qr")
        .role(Role::Image)
        .aria_label("Código QR para conectar o ZapFast ao WhatsApp do celular")
        .flex_none()
        .size(px(QR_SIDE))
        .p(px(12.))
        .rounded(px(12.))
        // The code stays black on white in both themes: scanners need it.
        .bg(rgb(0xffffff))
        .border_1()
        .border_color(p.outline)
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let total = modules.width_with_quiet_zone() as f32;
                    // Whole pixels per module, so the edges stay sharp.
                    let cell = (bounds.size.width.as_f32().min(bounds.size.height.as_f32())
                        / total)
                        .floor()
                        .max(1.0);
                    let extent = cell * total;
                    let origin = point(
                        bounds.origin.x + (bounds.size.width - px(extent)) / 2.,
                        bounds.origin.y + (bounds.size.height - px(extent)) / 2.,
                    );
                    let quiet = crate::state::qr::QUIET_ZONE as f32;
                    for (x, y) in modules.dark_modules() {
                        let square = Bounds::new(
                            point(
                                origin.x + px((x as f32 + quiet) * cell),
                                origin.y + px((y as f32 + quiet) * cell),
                            ),
                            size(px(cell), px(cell)),
                        );
                        window.paint_quad(fill(square, rgb(0x000000)));
                    }
                },
            )
            .size_full(),
        )
}

fn step(number: usize, text: &'static str, p: Palette) -> impl IntoElement {
    div()
        .flex()
        .gap(px(12.))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(26.))
                .rounded_full()
                .bg(p.surface)
                .text_size(px(13.))
                .text_color(p.secondary)
                .child(SharedString::from(number.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pt(px(2.))
                .text_size(px(16.))
                .line_height(px(22.))
                .text_color(p.text)
                .child(text),
        )
}

impl Render for LoginView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let session = self.session.read(cx);
        let qr = session.qr.clone();
        let pair_code = session.pair_code.clone();
        let mode = session.method;
        let failed = match &session.link {
            LinkStatus::Failed(reason) => Some(reason.clone()),
            _ => None,
        };
        let phone_ready = digits(self.phone.read(cx).value().as_ref()).len() >= 8;

        let right: gpui::AnyElement = match mode {
            PairingMethod::Qr => match qr {
                Some(modules) => qr_view(modules, p).into_any_element(),
                None => div()
                    .id("qr-waiting")
                    .role(Role::Status)
                    .aria_label("Preparando o código QR")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(QR_SIDE + 24.))
                    .rounded(px(12.))
                    .bg(p.surface)
                    .text_size(px(14.))
                    .text_color(p.secondary)
                    .child(match &failed {
                        Some(reason) => SharedString::from(format!("Falhou: {reason}")),
                        None => SharedString::from("Preparando o código QR…"),
                    })
                    .into_any_element(),
            },
            PairingMethod::PhoneNumber => div()
                .id("phone-pairing")
                .flex()
                .flex_col()
                .flex_none()
                .w(px(QR_SIDE + 24.))
                .gap(px(14.))
                .child(
                    div()
                        .text_size(px(15.))
                        .text_color(p.text)
                        .child("Digite o número do celular com o código do país e o DDD."),
                )
                .child(
                    div()
                        .h(px(40.))
                        .px(px(12.))
                        .flex()
                        .items_center()
                        .rounded(px(10.))
                        .bg(p.surface)
                        .text_color(p.text)
                        .text_size(px(15.))
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&self.phone).appearance(false).bordered(false)),
                        ),
                )
                .child(
                    div()
                        .id("request-code")
                        .role(Role::Button)
                        .aria_label("Gerar código de pareamento")
                        .flex()
                        .items_center()
                        .justify_center()
                        .h(px(40.))
                        .rounded_full()
                        .bg(if phone_ready { p.accent } else { p.surface })
                        .text_color(if phone_ready { p.on_accent } else { p.secondary })
                        .text_size(px(15.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .when(phone_ready, |b| b.cursor(CursorStyle::PointingHand))
                        .on_click(cx.listener(|this, _, _, cx| this.request_code(cx)))
                        .child("Gerar código"),
                )
                .children(pair_code.map(|code| {
                    div()
                        .id("pair-code")
                        .role(Role::Status)
                        .aria_label(format!("Código de pareamento {}", group_code(&code)))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(6.))
                        .py(px(14.))
                        .rounded(px(12.))
                        .bg(p.surface)
                        .child(
                            div()
                                .text_size(px(30.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(p.text)
                                .child(SharedString::from(group_code(&code))),
                        )
                        .child(
                            div()
                                .px(px(14.))
                                .text_size(px(13.))
                                .text_color(p.secondary)
                                .child("No celular: Dispositivos conectados, Conectar dispositivo, Conectar com número de telefone."),
                        )
                }))
                .into_any_element(),
        };

        let toggle_label = match mode {
            PairingMethod::Qr => "Conectar com número de telefone",
            PairingMethod::PhoneNumber => "Conectar com código QR",
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(p.window)
            .child(
                div()
                    .id("login-card")
                    .role(Role::Main)
                    .aria_label("Conectar o ZapFast")
                    .flex()
                    .gap(px(56.))
                    .px(px(56.))
                    .py(px(48.))
                    .rounded(px(16.))
                    .bg(p.panel)
                    .border_1()
                    .border_color(p.outline)
                    .child(
                        div()
                            .w(px(380.))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap(px(18.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(img("logo.svg").size(px(36.)))
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(p.text)
                                            .child("ZapFast"),
                                    ),
                            )
                            .child(
                                div()
                                    .id("login-title")
                                    .role(Role::Heading)
                                    .aria_level(1)
                                    .text_size(px(28.))
                                    .line_height(px(34.))
                                    .text_color(p.text)
                                    .child("Conecte o ZapFast ao seu WhatsApp"),
                            )
                            .child(step(1, "Abra o WhatsApp no seu celular.", p))
                            .child(step(
                                2,
                                "Toque em Configurações e escolha Dispositivos conectados.",
                                p,
                            ))
                            .child(step(3, "Toque em Conectar dispositivo.", p))
                            .child(step(
                                4,
                                match mode {
                                    PairingMethod::Qr => {
                                        "Aponte a câmera do celular para o código ao lado."
                                    }
                                    PairingMethod::PhoneNumber => {
                                        "Escolha conectar com número e digite o código ao lado."
                                    }
                                },
                                p,
                            ))
                            .child(
                                div()
                                    .id("toggle-mode")
                                    .role(Role::Button)
                                    .aria_label(toggle_label)
                                    .mt(px(6.))
                                    .text_size(px(15.))
                                    .text_color(p.link)
                                    .cursor(CursorStyle::PointingHand)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let next = match this.method(cx) {
                                            PairingMethod::Qr => PairingMethod::PhoneNumber,
                                            PairingMethod::PhoneNumber => PairingMethod::Qr,
                                        };
                                        this.set_method(next, cx)
                                    }))
                                    .child(toggle_label),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .text_size(px(13.))
                                    .text_color(p.secondary)
                                    .child(icon("icons/lock.svg", 14., p.secondary))
                                    .child("Suas mensagens ficam cifradas neste computador."),
                            ),
                    )
                    .child(right),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;
    use crate::views::testing::launch;
    use gpui::TestAppContext;

    #[test]
    fn the_pairing_code_is_grouped_like_the_phone_shows_it() {
        assert_eq!(group_code("ZAPF7K2Q"), "ZAPF-7K2Q");
        assert_eq!(group_code("ZAPF-7K2Q"), "ZAPF-7K2Q");
        assert_eq!(group_code("123"), "123");
        assert_eq!(digits("+55 (11) 91111-0001"), "5511911110001");
    }

    #[gpui::test]
    fn the_phone_number_asks_for_a_pairing_code_and_the_code_comes_back(cx: &mut TestAppContext) {
        let demo = launch(cx, false);
        let store = demo.store.clone();
        assert!(store.read_with(cx, |s, cx| s.session.read(cx).needs_login()));
        let (login, cx) = cx.add_window_view({
            let store = store.clone();
            move |window, cx| LoginView::new(store, window, cx)
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        // The QR is the default, and the demo's payload became modules.
        assert_eq!(login.read_with(cx, |l, cx| l.method(cx)), PairingMethod::Qr);
        assert!(store.read_with(cx, |s, cx| s.session.read(cx).qr.is_some()));

        login.update(cx, |login, cx| {
            login.set_method(PairingMethod::PhoneNumber, cx)
        });
        // Too short a number asks for nothing.
        let phone = login.read_with(cx, |l, _| l.phone_state().clone());
        phone.update_in(cx, |state, window, cx| {
            state.set_value("+55 11", window, cx)
        });
        login.update(cx, |login, cx| login.request_code(cx));
        cx.run_until_parked();
        assert!(store.read_with(cx, |s, cx| s.session.read(cx).pair_code.is_none()));
        phone.update_in(cx, |state, window, cx| {
            state.set_value("+55 (11) 91111-0001", window, cx)
        });
        login.update(cx, |login, cx| login.request_code(cx));
        cx.run_until_parked();
        // The backend answered with the code, and the screen shows it.
        assert_eq!(
            store.read_with(cx, |s, cx| s.session.read(cx).pair_code.clone()),
            Some(demo::PAIR_CODE.to_owned())
        );
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}
