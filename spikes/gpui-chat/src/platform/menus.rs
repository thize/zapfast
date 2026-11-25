//! The native menu bar. Without one, macOS shows an app menu only and no Edit
//! menu, so the standard editing commands and the Emoji & Symbols item are
//! missing; with `OsAction`s on the Edit items GPUI wires them to NSMenu's
//! standard selectors, and enables each one only when the focused element
//! handles its action (`is_action_available`).

use crate::shell::{ShowChat, ShowMedia, ShowPlatform};
use gpui::{App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType, actions};
use gpui_component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};

actions!(spike_app, [Quit]);

/// The menus, in menu-bar order. The first is the application menu on macOS
/// (its title is replaced by the bundle's name).
pub fn app_menus() -> Vec<Menu> {
    vec![
        Menu::new(super::APP_NAME).items([
            MenuItem::os_submenu("Serviços", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(format!("Sair de {}", super::APP_NAME), Quit),
        ]),
        Menu::new("Editar").items([
            MenuItem::os_action("Desfazer", Undo, OsAction::Undo),
            MenuItem::os_action("Refazer", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Recortar", Cut, OsAction::Cut),
            MenuItem::os_action("Copiar", Copy, OsAction::Copy),
            MenuItem::os_action("Colar", Paste, OsAction::Paste),
            MenuItem::separator(),
            MenuItem::os_action("Selecionar tudo", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("Visualizar").items([
            MenuItem::action("Conversa", ShowChat),
            MenuItem::action("Mídia", ShowMedia),
            MenuItem::action("Plataforma", ShowPlatform),
        ]),
    ]
}

/// Installs the menu bar, Cmd+Q, and the Quit action.
pub fn install(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([KeyBinding::new(
        if cfg!(target_os = "macos") {
            "cmd-q"
        } else {
            "ctrl-q"
        },
        Quit,
        None,
    )]);
    cx.set_menus(app_menus());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(menu: &Menu) -> Vec<String> {
        menu.items
            .iter()
            .map(|item| match item {
                MenuItem::Separator => "-".to_owned(),
                MenuItem::Submenu(menu) => menu.name.to_string(),
                MenuItem::SystemMenu(menu) => menu.name.to_string(),
                MenuItem::Action { name, .. } => name.to_string(),
            })
            .collect()
    }

    #[test]
    fn the_edit_menu_carries_the_standard_commands_as_os_actions() {
        let menus = app_menus();
        assert_eq!(
            menus.iter().map(|m| m.name.to_string()).collect::<Vec<_>>(),
            [super::super::APP_NAME, "Editar", "Visualizar"]
        );
        let edit = &menus[1];
        assert_eq!(
            names(edit),
            [
                "Desfazer",
                "Refazer",
                "-",
                "Recortar",
                "Copiar",
                "Colar",
                "-",
                "Selecionar tudo"
            ]
        );
        let os_actions: Vec<_> = edit
            .items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action {
                    os_action: Some(os),
                    ..
                } => Some(*os),
                _ => None,
            })
            .collect();
        assert!(
            os_actions
                == [
                    OsAction::Undo,
                    OsAction::Redo,
                    OsAction::Cut,
                    OsAction::Copy,
                    OsAction::Paste,
                    OsAction::SelectAll
                ]
        );
        assert_eq!(names(&menus[0])[0], "Serviços");
    }
}
