#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod activity;
mod artwork;
mod branding;
mod commands;
mod component_effects;
mod components;
mod diagnostics;
mod editor;
mod effects;
#[cfg(test)]
mod icon_tests;
mod icons;
mod journal;
mod library;
mod motion;
mod settings;
mod state;
mod tasks;
mod theme;
mod toolbar;
#[cfg(feature = "self-update")]
mod updater;
mod view;
mod wall;
mod widgets;

use rune::{AppSpec, Key, KeyEvent, Tx};
use state::{Overlay, Screen, State};

fn main() {
    let headless = rune::tooling::headless_run().is_some();
    let example = std::env::args().any(|argument| argument == "--example-library");
    #[cfg(feature = "self-update")]
    velopack::VelopackApp::build()
        .set_auto_apply_on_startup(
            updater::supported_platform()
                && !branding::IS_DEV
                && !headless
                && !example
                && !std::env::args().any(|argument| {
                    matches!(argument.as_str(), "--developer-tools" | "--diagnostics")
                }),
        )
        .run();
    let _diagnostics = if headless {
        None
    } else {
        diagnostics::initialize()
    };
    if std::env::args().any(|argument| argument == "--diagnostics") {
        diagnostics::run();
        return;
    }
    if !headless {
        let icon = if branding::IS_DEV {
            include_bytes!("../assets/icons/startpoint-dev.png").to_vec()
        } else {
            include_bytes!("../assets/icons/startpoint.png").to_vec()
        };
        let _ = rune::set_app_icon(icon);
    }
    rune::prewarm_gpu();
    rune::tooling::set_app_id(branding::APP_ID);
    rune::App::new(AppSpec {
        setup: Some(State::setup),
        poll: Some(State::poll),
        on_key: Some(on_key),
        min_size: Some((900.0, 640.0)),
        families: (
            if cfg!(target_os = "macos") {
                "Avenir Next, Inter"
            } else if cfg!(target_os = "windows") {
                "Bahnschrift, Segoe UI, Inter"
            } else {
                "Inter"
            },
            "ui-monospace",
        ),
        scenes: &[
            ("library", |s| s.navigate(Screen::Library)),
            ("activity", |s| s.navigate(Screen::Activity)),
            ("settings", |s| s.navigate(Screen::Settings)),
            ("component-effects", |s| {
                s.navigate(Screen::ComponentEffects)
            }),
            ("editor", |s| {
                if let Some(game) = s.selected_game() {
                    s.open_editor(game.id.clone(), editor::EditorSection::Details);
                }
            }),
            ("artwork", |s| {
                if let Some(game) = s.selected_game() {
                    s.open_editor(game.id.clone(), editor::EditorSection::Artwork);
                }
            }),
            ("launch", |s| {
                if let Some(game) = s.selected_game() {
                    s.open_editor(game.id.clone(), editor::EditorSection::Launch);
                }
            }),
            ("journal", |s| {
                if let Some(game) = s.selected_game() {
                    s.open_editor(game.id.clone(), editor::EditorSection::Journal);
                }
            }),
            ("customize", |s| s.show_overlay(Overlay::Customize)),
            ("add-game", |s| s.show_overlay(Overlay::AddGame)),
            ("commands", |s| s.open_commands(String::new(), false)),
            ("notifications", |s| s.show_overlay(Overlay::Notifications)),
            ("infinite-wall", |s| {
                s.preview_style(startpoint_sdk::RecentsLayout::InfiniteWall)
            }),
            ("compact", |s| {
                s.navigate(Screen::Library);
                s.appearance.library_layout = startpoint_sdk::LibraryLayout::Compact;
                s.appearance_dirty = true;
                s.refresh_library_order();
            }),
        ],
        ..AppSpec::new(
            branding::window_title(example),
            (1280.0, 840.0),
            State::new,
            view::view,
        )
    })
    .window_options(rune::WindowOptions {
        transparent: true,
        ..Default::default()
    })
    .chrome(rune::Chrome {
        full_size_content: true,
        traffic_lights: Some((18.0, 21.0)),
        drag_regions: true,
    })
    .compositor(effects::EffectsCompositor::default())
    .on_focus(|state: &mut State, focused, _| state.window_focused = focused)
    .run();
}

fn on_key(state: &mut State, event: &KeyEvent, tx: &mut Tx) -> bool {
    tx.instant();
    if state.overlay != Overlay::None {
        return false;
    }
    if event.is_command('[')
        || (!cfg!(target_os = "macos") && event.mods.alt && event.key == Key::Left)
    {
        state.go_back(tx);
        return true;
    }
    if event.is_command(']')
        || (!cfg!(target_os = "macos") && event.mods.alt && event.key == Key::Right)
    {
        state.go_forward(tx);
        return true;
    }
    if event.is_command('f') {
        state.open_commands(String::new(), false);
        return true;
    }
    if event.is_command('p') || event.is_command('k') {
        state.open_commands(String::new(), false);
        return true;
    }
    if event.is_command('b') {
        state.appearance.sidebar_visible = !state.appearance.sidebar_visible;
        state.save_appearance();
        return true;
    }
    if state.search.focused {
        if event.key == Key::Escape {
            state.search = rune::TextEdit::new("");
            tx.blur();
            return true;
        }
        return false;
    }
    match event.key {
        Key::Left => state.move_selection(false, tx),
        Key::Right => state.move_selection(true, tx),
        Key::Enter if matches!(state.screen, Screen::Recents | Screen::Library) => {
            state.launch_selected();
            true
        }
        _ => false,
    }
}
