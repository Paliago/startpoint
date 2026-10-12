#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod appearance;
mod components;
mod detail_artwork;
mod diagnostics;
mod glass;
#[cfg(test)]
mod icon_tests;
mod image_cache;
mod interface;
mod library_layout;
mod logo_cache;
mod recents_fade;
mod theme;
mod theme_editor;
mod window_state;

use gpui::{prelude::*, *};
use gpui_component::Root;
use std::borrow::Cow;

struct LauncherAssets;

impl AssetSource for LauncherAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: Option<&'static [u8]> = match path {
            "icons/refresh.svg" => Some(include_bytes!("../assets/refresh.svg")),
            "icons/pause.svg" => Some(include_bytes!("../assets/pause.svg")),
            "icons/play.svg" => Some(include_bytes!("../assets/play.svg")),
            "icons/star-filled.svg" => Some(include_bytes!("../assets/star-filled.svg")),
            _ => None,
        };
        match bytes {
            Some(bytes) => Ok(Some(Cow::Borrowed(bytes))),
            None => gpui_component_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = gpui_component_assets::Assets.list(path)?;
        assets.extend(
            [
                "icons/refresh.svg",
                "icons/pause.svg",
                "icons/play.svg",
                "icons/star-filled.svg",
            ]
            .into_iter()
            .filter(|asset| asset.starts_with(path))
            .map(SharedString::from),
        );
        Ok(assets)
    }
}

fn main() {
    let _diagnostics = diagnostics::initialize();
    let example_library = std::env::args().any(|argument| argument == "--example-library");
    let application = if example_library {
        startpoint_core::create_example_application()
    } else {
        startpoint_core::create_application()
    };
    let api = match application {
        Ok(api) => api,
        Err(error) => {
            diagnostics::record("application initialization failed");
            eprintln!("Startpoint could not start: {error}");
            return;
        }
    };
    if std::env::args().any(|argument| argument == "--diagnostics") {
        let events = api.subscribe();
        match api.submit(startpoint_sdk::Command::RefreshLibrary) {
            Ok(id) => {
                while let Ok(event) = events.recv_blocking() {
                    if let startpoint_sdk::AppEvent::JobChanged(job) = event
                        && job.id == id
                        && !job.state.active()
                    {
                        let snapshot = api.snapshot();
                        println!(
                            "Data: {}\nCache: {}\nInstalled games: {}\nRefresh: {:?}\nSteamGridDB: {}",
                            snapshot.paths.data_directory.display(),
                            snapshot.paths.cache_directory.display(),
                            snapshot.games.iter().filter(|game| game.available).count(),
                            job.state,
                            snapshot
                                .settings
                                .credential_status
                                .get("steamgriddb")
                                .map(|status| status.label())
                                .unwrap_or("Not configured")
                        );
                        if let Some(error) = job.error {
                            eprintln!("{error}");
                        }
                        return;
                    }
                    if let Some(error) = api.snapshot().initialization_error {
                        eprintln!("{error}");
                        return;
                    }
                }
            }
            Err(error) => eprintln!("{error}"),
        }
        return;
    }
    gpui_platform::application()
        .with_assets(LauncherAssets)
        .run(move |cx| {
            gpui_component::init(cx);
            cx.set_global(theme::Catalog::default());
            appearance::apply(&api.snapshot().settings.appearance, None, cx);
            cx.bind_keys([
                KeyBinding::new("cmd-q", interface::Quit, None),
                KeyBinding::new("ctrl-q", interface::Quit, None),
                KeyBinding::new("cmd-f", interface::FocusSearch, Some("Startpoint")),
                KeyBinding::new("ctrl-f", interface::FocusSearch, Some("Startpoint")),
                KeyBinding::new("cmd-k", interface::OpenCommands, Some("Startpoint")),
                KeyBinding::new("ctrl-k", interface::OpenCommands, Some("Startpoint")),
                KeyBinding::new("cmd-b", interface::ToggleSidebar, Some("Startpoint")),
                KeyBinding::new("ctrl-b", interface::ToggleSidebar, Some("Startpoint")),
                KeyBinding::new("cmd-r", interface::RefreshLibrary, Some("Startpoint")),
                KeyBinding::new("ctrl-r", interface::RefreshLibrary, Some("Startpoint")),
                KeyBinding::new("escape", interface::DismissPanel, Some("Startpoint")),
                KeyBinding::new(
                    "/",
                    interface::FocusSearch,
                    Some("Startpoint && !Input && !SidebarResize"),
                ),
                KeyBinding::new(
                    "down",
                    interface::SelectNext,
                    Some("Startpoint && !Input && !SidebarResize"),
                ),
                KeyBinding::new(
                    "right",
                    interface::SelectNext,
                    Some("Startpoint && !Input && !SidebarResize"),
                ),
                KeyBinding::new(
                    "left",
                    interface::SelectPrevious,
                    Some("Startpoint && !Input && !SidebarResize"),
                ),
                KeyBinding::new(
                    "up",
                    interface::SelectPrevious,
                    Some("Startpoint && !Input && !SidebarResize"),
                ),
                KeyBinding::new(
                    "enter",
                    interface::LaunchSelected,
                    Some("Startpoint && !Input && !SidebarResize"),
                ),
                KeyBinding::new("cmd-,", interface::OpenSettings, Some("Startpoint")),
                KeyBinding::new("ctrl-,", interface::OpenSettings, Some("Startpoint")),
            ]);
            cx.on_action(|_: &interface::Quit, cx| cx.quit());
            cx.set_menus(vec![
                Menu {
                    name: "Startpoint".into(),
                    disabled: false,
                    items: vec![
                        MenuItem::action("Settings…", interface::OpenSettings),
                        MenuItem::separator(),
                        MenuItem::action("Quit Startpoint", interface::Quit),
                    ],
                },
                Menu {
                    name: "Edit".into(),
                    disabled: false,
                    items: vec![
                        MenuItem::os_action("Undo", gpui_component::input::Undo, OsAction::Undo),
                        MenuItem::os_action("Redo", gpui_component::input::Redo, OsAction::Redo),
                        MenuItem::separator(),
                        MenuItem::os_action("Cut", gpui_component::input::Cut, OsAction::Cut),
                        MenuItem::os_action("Copy", gpui_component::input::Copy, OsAction::Copy),
                        MenuItem::os_action("Paste", gpui_component::input::Paste, OsAction::Paste),
                        MenuItem::os_action(
                            "Select All",
                            gpui_component::input::SelectAll,
                            OsAction::SelectAll,
                        ),
                    ],
                },
                Menu {
                    name: "Library".into(),
                    disabled: false,
                    items: vec![
                        MenuItem::action("Find a game", interface::FocusSearch),
                        MenuItem::action("Actions…", interface::OpenCommands),
                        MenuItem::action("Toggle sidebar", interface::ToggleSidebar),
                        MenuItem::action("Refresh installed games", interface::RefreshLibrary),
                    ],
                },
            ]);
            let bounds = window_state::restore(&api.snapshot().paths.data_directory, cx);
            if let Err(error) = cx.open_window(
                WindowOptions {
                    window_bounds: Some(bounds),
                    window_min_size: Some(size(px(960.), px(620.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some(
                            if example_library {
                                "Startpoint — Example library"
                            } else {
                                "Startpoint"
                            }
                            .into(),
                        ),
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(18.), px(14.))),
                    }),
                    window_background: appearance::background(cx),
                    app_id: Some("startpoint".into()),
                    ..Default::default()
                },
                move |window, cx| {
                    appearance::configure_window(window);
                    let view = cx.new(|cx| interface::LauncherView::new(api, window, cx));
                    // LauncherView owns the window tint; an opaque Root would hide
                    // Acrylic/vibrancy and stacking two tints would flatten the glass.
                    cx.new(|cx| Root::new(view, window, cx).bg(gpui::transparent_black()))
                },
            ) {
                diagnostics::record("window creation failed");
                eprintln!("Could not open the Startpoint window: {error}");
                cx.quit();
            }
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
