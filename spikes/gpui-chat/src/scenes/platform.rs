//! The Platform scene: the menu-bar item and the Dock badge, native
//! notifications, the application menu bar and the accessibility probe.
//!
//! The tray lives in this view, which exists from startup (the shell keeps all
//! three scenes alive), so clicks on the menu-bar icon work whichever scene is
//! shown. Nothing here polls: the tray's wake-up and the notification worker's
//! events each arrive on a futures channel a foreground task awaits.

use crate::platform::notify::{self, Fastframe};
use crate::platform::tray::{self, Hub, TrayAction};
use crate::platform::{APP_NAME, BUNDLE_ID};
use crate::ui::palette;
use futures::StreamExt as _;
use gpui::{
    ClipboardItem, Context, CursorStyle, InteractiveElement, IntoElement, ParentElement, Render,
    Role, SharedString, StatefulInteractiveElement, Styled, Task, Window, div, px,
};
use std::collections::VecDeque;

/// Lines kept in the activity log.
const LOG_LINES: usize = 10;

pub struct PlatformScene {
    hub: Hub,
    fastframe: Fastframe,
    log: VecDeque<String>,
    tray_events: u32,
    gpui_serial: u32,
    _pumps: Vec<Task<()>>,
}

impl PlatformScene {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (hub, tray_wake) = Hub::start();
        let (notice_tx, mut notice_rx) = futures::channel::mpsc::unbounded::<String>();
        let mut tray_wake = tray_wake;

        // Each pump sleeps until its channel has something: no timer, so an
        // idle app with the tray active stays idle.
        let tray_pump = cx.spawn_in(window, async move |this, cx| {
            while tray_wake.next().await.is_some() {
                // One click can wake several times; read them all at once.
                if this
                    .update_in(cx, |this, window, cx| this.drain_tray(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let notice_pump = cx.spawn(async move |this, cx| {
            while let Some(line) = notice_rx.next().await {
                if this
                    .update(cx, |this, cx| {
                        this.say(line);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        cx.set_app_identity(BUNDLE_ID, APP_NAME);
        let mut scene = Self {
            hub,
            fastframe: Fastframe::new(notice_tx),
            log: VecDeque::new(),
            tray_events: 0,
            gpui_serial: 0,
            _pumps: vec![tray_pump, notice_pump],
        };
        scene.say(if scene.hub.has_tray() {
            "Ícone na barra de menus criado".to_owned()
        } else {
            "Sem ícone na barra de menus: o sistema não deixou criar".to_owned()
        });
        scene
    }

    fn say(&mut self, line: String) {
        self.log.push_front(line);
        self.log.truncate(LOG_LINES);
    }

    /// The activity log, newest first.
    pub fn log_lines(&self) -> Vec<String> {
        self.log.iter().cloned().collect()
    }

    pub fn badge(&self) -> u32 {
        self.hub.count()
    }

    pub fn set_badge(&mut self, count: u32, cx: &mut Context<Self>) {
        self.hub.set_count(count);
        self.say(match count {
            0 => "Contador do Dock zerado".to_owned(),
            n => format!("Contador do Dock: {n}"),
        });
        cx.notify();
    }

    fn drain_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for event in self.hub.events() {
            self.tray_events += 1;
            self.say(format!("barra de menus: {event:?}"));
            match tray::action_for(event) {
                Some(TrayAction::Open) => {
                    cx.activate(true);
                    window.activate_window();
                }
                Some(TrayAction::Quit) => cx.quit(),
                None => {}
            }
        }
        cx.notify();
    }

    pub fn notify_gpui(&mut self, cx: &mut Context<Self>) {
        self.gpui_serial += 1;
        let line = notify::post_gpui(cx, self.gpui_serial);
        self.say(line);
        cx.notify();
    }

    pub fn notify_fastframe(&mut self, cx: &mut Context<Self>) {
        self.fastframe.post();
        self.say("fastframe: pedido enviado ao worker".to_owned());
        cx.notify();
    }

    fn copy_a11y_tree(&mut self, window: &Window, cx: &mut Context<Self>) {
        match window.debug_a11y_tree_json() {
            Some(json) => {
                let bytes = json.len();
                cx.write_to_clipboard(ClipboardItem::new_string(json));
                self.say(format!("árvore de acessibilidade copiada ({bytes} bytes)"));
            }
            None => self.say(
                "árvore indisponível: nenhum leitor de tela a pediu ainda (ligue o VoiceOver)"
                    .to_owned(),
            ),
        }
        cx.notify();
    }
}

impl Render for PlatformScene {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let bundled = crate::platform::is_bundled();
        let a11y_active = window.is_a11y_active();
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .px(px(14.))
                .py(px(7.))
                .rounded(px(8.))
                .bg(p.accent)
                .text_color(p.on_accent)
                .text_size(px(13.5))
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.opacity(0.85))
                .child(SharedString::from(label))
        };
        let card = |title: &'static str| {
            div()
                .id(title)
                .role(Role::Group)
                .aria_label(title)
                .flex()
                .flex_col()
                .gap(px(8.))
                .p(px(16.))
                .rounded(px(12.))
                .bg(p.panel)
                .border_1()
                .border_color(p.outline)
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(p.text)
                        .child(SharedString::from(title)),
                )
        };
        // Plain text children are not accessibility nodes (only `text!` ones
        // are), so each note is a labelled node of its own.
        let notes = std::cell::Cell::new(0usize);
        let note = |text: String| {
            let n = notes.get();
            notes.set(n + 1);
            div()
                .id(("note", n))
                .role(Role::Label)
                .aria_label(text.clone())
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(p.secondary)
                .child(SharedString::from(text))
        };
        let count = self.hub.count();

        div()
            .id("platform-scene")
            .size_full()
            .overflow_y_scroll()
            .bg(p.chat)
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                card("Ícone na barra de menus e contador do Dock")
                    .child(note(format!(
                        "fastframe-tray e fastframe-dock, os mesmos do ZapFast. O ícone tem o menu Abrir ZapFast e Sair; o contador aparece no ícone do Dock. Ícone na barra: {}. Contador: {count}. Cliques recebidos: {}.",
                        if self.hub.has_tray() { "sim" } else { "não" },
                        self.tray_events
                    )))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(button("badge-up", "+1 no contador").on_click(
                                cx.listener(|this, _, _, cx| {
                                    let next = this.badge() + 1;
                                    this.set_badge(next, cx)
                                }),
                            ))
                            .child(button("badge-clear", "Zerar contador").on_click(
                                cx.listener(|this, _, _, cx| this.set_badge(0, cx)),
                            )),
                    ),
            )
            .child(
                card("Notificação nativa")
                    .child(note(format!(
                        "Executando de um .app: {}. O macOS só entrega notificações a um .app assinado com identificador ({BUNDLE_ID}); para testar, rode scripts/bundle-macos.sh e abra o app gerado.",
                        if bundled { "sim" } else { "não" }
                    )))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(button("notify-gpui", "Notificar (GPUI)").on_click(
                                cx.listener(|this, _, _, cx| this.notify_gpui(cx)),
                            ))
                            .child(
                                button("notify-fastframe", "Notificar (fastframe)").on_click(
                                    cx.listener(|this, _, _, cx| this.notify_fastframe(cx)),
                                ),
                            ),
                    ),
            )
            .child(
                card("Barra de menus do aplicativo").child(note(
                    "O menu Editar tem Desfazer, Refazer, Recortar, Copiar, Colar e Selecionar tudo; cada item só fica ativo quando o elemento com foco trata a ação. Na cena Conversa, Cmd+C, Cmd+V e Cmd+A valem para o campo de mensagem e, com a lista em foco, Cmd+C e Cmd+A valem para a seleção de mensagens."
                        .to_owned(),
                )),
            )
            .child(
                card("Acessibilidade")
                    .child(note(format!(
                        "A árvore de acessibilidade do GPUI (AccessKit) só é montada quando um leitor de tela a pede. Agora: {}. Linhas de mensagem, itens da lista de conversas, o campo de mensagem e as abas das cenas têm função e rótulo.",
                        if a11y_active { "ativa" } else { "inativa" }
                    )))
                    .child(button("a11y-copy", "Copiar árvore (JSON)").on_click(
                        cx.listener(|this, _, window, cx| this.copy_a11y_tree(window, cx)),
                    )),
            )
            .child(
                card("Atividade").children(if self.log.is_empty() {
                    vec![note("Nada ainda.".to_owned())]
                } else {
                    self.log.iter().cloned().map(note).collect()
                }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::Palette;
    use crate::ui::ActivePalette;
    use gpui::TestAppContext;

    #[gpui::test]
    fn the_badge_counts_up_and_clears_and_is_logged(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(ActivePalette(Palette::dark()));
            gpui_component::init(cx);
        });
        let (scene, cx) = cx.add_window_view(PlatformScene::new);
        scene.update(cx, |scene, cx| {
            let next = scene.badge() + 1;
            scene.set_badge(next, cx);
            let next = scene.badge() + 1;
            scene.set_badge(next, cx);
        });
        assert_eq!(scene.read_with(cx, |s, _| s.badge()), 2);
        assert!(scene.read_with(cx, |s, _| s.log[0].contains("2")));
        scene.update(cx, |scene, cx| scene.set_badge(0, cx));
        assert_eq!(scene.read_with(cx, |s, _| s.badge()), 0);
        assert!(scene.read_with(cx, |s, _| s.log[0].contains("zerado")));
        // The log keeps its newest lines only.
        scene.update(cx, |scene, cx| {
            for n in 0..30 {
                scene.set_badge(n, cx);
            }
        });
        assert_eq!(scene.read_with(cx, |s, _| s.log.len()), LOG_LINES);
    }

    #[gpui::test]
    fn the_scene_renders_its_sections(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(ActivePalette(Palette::light()));
            gpui_component::init(cx);
        });
        let (_scene, cx) = cx.add_window_view(PlatformScene::new);
        cx.run_until_parked();
    }
}
