//! Cross-platform native application menu bar and keyboard shortcuts.

use gpui_kit::base::GlobalState;
use gpui_kit::component::input::{Copy, Cut, Paste, SelectAll};
use gpui_kit::{App, KeyBinding, Menu, MenuItem, OsAction, actions};

actions!(
    release_desk,
    [
        QuitApp,
        CloseWindow,
        OpenSettings,
        NewProject,
        CheckForUpdates,
        ImportRepositories,
        RefreshIcons,
        ToggleUpdateQueue,
        ToggleAppearance,
        NavProjectHub,
        NavAppStore,
        NavSettings,
        GoBack,
        GoForward,
        DismissDialog,
    ]
);

/// Builds the cross-platform menu bar items and registers global + workspace keybindings.
pub fn install(cx: &mut App) {
    #[cfg(target_os = "macos")]
    let (
        quit_key,
        close_key,
        settings_key,
        new_key,
        check_key,
        import_key,
        icons_key,
        queue_key,
        theme_key,
        hub_key,
        store_key,
        prefs_key,
        back_key,
        fwd_key,
    ) = (
        "cmd-q",
        "cmd-w",
        "cmd-,",
        "cmd-n",
        "cmd-r",
        "cmd-i",
        "cmd-shift-i",
        "cmd-j",
        "cmd-shift-t",
        "cmd-1",
        "cmd-2",
        "cmd-3",
        "cmd-[",
        "cmd-]",
    );

    #[cfg(not(target_os = "macos"))]
    let (
        quit_key,
        close_key,
        settings_key,
        new_key,
        check_key,
        import_key,
        icons_key,
        queue_key,
        theme_key,
        hub_key,
        store_key,
        prefs_key,
        back_key,
        fwd_key,
    ) = (
        "ctrl-q",
        "ctrl-w",
        "ctrl-,",
        "ctrl-n",
        "ctrl-r",
        "ctrl-i",
        "ctrl-shift-i",
        "ctrl-j",
        "ctrl-shift-t",
        "ctrl-1",
        "ctrl-2",
        "ctrl-3",
        "alt-left",
        "alt-right",
    );

    cx.bind_keys([
        KeyBinding::new(quit_key, QuitApp, None),
        KeyBinding::new(close_key, CloseWindow, Some("ReleaseDesk")),
        KeyBinding::new(settings_key, OpenSettings, Some("ReleaseDesk")),
        KeyBinding::new(new_key, NewProject, Some("ReleaseDesk")),
        KeyBinding::new(check_key, CheckForUpdates, Some("ReleaseDesk")),
        KeyBinding::new("f5", CheckForUpdates, Some("ReleaseDesk")),
        KeyBinding::new(import_key, ImportRepositories, Some("ReleaseDesk")),
        KeyBinding::new(icons_key, RefreshIcons, Some("ReleaseDesk")),
        KeyBinding::new(queue_key, ToggleUpdateQueue, Some("ReleaseDesk")),
        KeyBinding::new(theme_key, ToggleAppearance, Some("ReleaseDesk")),
        KeyBinding::new(hub_key, NavProjectHub, Some("ReleaseDesk")),
        KeyBinding::new(store_key, NavAppStore, Some("ReleaseDesk")),
        KeyBinding::new(prefs_key, NavSettings, Some("ReleaseDesk")),
        KeyBinding::new(back_key, GoBack, Some("ReleaseDesk")),
        KeyBinding::new(fwd_key, GoForward, Some("ReleaseDesk")),
        KeyBinding::new("escape", DismissDialog, Some("ReleaseDesk")),
    ]);

    cx.on_action(|_: &QuitApp, cx| cx.quit());

    let app_or_file_title = if cfg!(target_os = "macos") {
        "Release Desk"
    } else {
        "File"
    };

    let menus = vec![
        Menu::new(app_or_file_title).items([
            MenuItem::action("New Project…", NewProject),
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
            MenuItem::separator(),
            MenuItem::action("Quit Release Desk", QuitApp),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::separator(),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items([
            MenuItem::action("Projects", NavProjectHub),
            MenuItem::action("App Store", NavAppStore),
            MenuItem::action("Settings", NavSettings),
            MenuItem::separator(),
            MenuItem::action("Back", GoBack),
            MenuItem::action("Forward", GoForward),
            MenuItem::separator(),
            MenuItem::action("Toggle Light / Dark Theme", ToggleAppearance),
        ]),
        Menu::new("Tools").items([
            MenuItem::action("Check for Updates", CheckForUpdates),
            MenuItem::action("Scan & Import Local Repositories…", ImportRepositories),
            MenuItem::action("Recreate Repository Icons", RefreshIcons),
            MenuItem::separator(),
            MenuItem::action("Repository Update Queue", ToggleUpdateQueue),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Close Window", CloseWindow),
            MenuItem::separator(),
            MenuItem::action("Quit Release Desk", QuitApp),
        ]),
    ];

    let owned: Vec<_> = menus
        .iter()
        .map(|m| {
            Menu {
                name: m.name.clone(),
                items: m.items.iter().map(clone_menu_item).collect(),
                disabled: m.disabled,
            }
            .owned()
        })
        .collect();
    cx.set_menus(menus);
    GlobalState::init(cx);
    GlobalState::global_mut(cx).set_app_menus(owned);
}

fn clone_menu_item(item: &MenuItem) -> MenuItem {
    match item {
        MenuItem::Separator => MenuItem::Separator,
        MenuItem::Submenu(sub) => MenuItem::Submenu(Menu {
            name: sub.name.clone(),
            items: sub.items.iter().map(clone_menu_item).collect(),
            disabled: sub.disabled,
        }),
        MenuItem::SystemMenu(sys) => MenuItem::os_submenu(sys.name.clone(), sys.menu_type),
        MenuItem::Action {
            name,
            action,
            os_action,
            checked,
            disabled,
        } => MenuItem::Action {
            name: name.clone(),
            action: action.boxed_clone(),
            os_action: *os_action,
            checked: *checked,
            disabled: *disabled,
        },
    }
}
