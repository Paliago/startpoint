#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod appearance;
mod image_cache;
mod interface;

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
            ["icons/refresh.svg", "icons/pause.svg", "icons/play.svg"]
                .into_iter()
                .filter(|asset| asset.starts_with(path))
                .map(SharedString::from),
        );
        Ok(assets)
    }
}

fn main() {
    let example_library = std::env::args().any(|argument| argument == "--example-library");
    let application = if example_library {
        startpoint_core::create_example_application()
    } else {
        startpoint_core::create_application()
    };
    let api = match application {
        Ok(api) => api,
        Err(error) => {
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
    Application::new()
        .with_assets(LauncherAssets)
        .run(move |cx| {
            gpui_component::init(cx);
            appearance::apply(api.snapshot().settings.appearance.mode, None, cx);
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
                    items: vec![
                        MenuItem::action("Settings…", interface::OpenSettings),
                        MenuItem::separator(),
                        MenuItem::action("Quit Startpoint", interface::Quit),
                    ],
                },
                Menu {
                    name: "Edit".into(),
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
                    items: vec![
                        MenuItem::action("Find a game", interface::FocusSearch),
                        MenuItem::action("Actions…", interface::OpenCommands),
                        MenuItem::action("Toggle sidebar", interface::ToggleSidebar),
                        MenuItem::action("Refresh installed games", interface::RefreshLibrary),
                    ],
                },
            ]);
            let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
            if let Err(error) = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
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
                    window_background: appearance::background(),
                    app_id: Some("startpoint".into()),
                    ..Default::default()
                },
                move |window, cx| {
                    appearance::configure_window(window);
                    let view = cx.new(|cx| interface::LauncherView::new(api, window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            ) {
                eprintln!("Could not open the Startpoint window: {error}");
                cx.quit();
            }
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
