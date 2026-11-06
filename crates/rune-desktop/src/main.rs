mod activity;
mod artwork;
mod commands;
mod components;
mod editor;
mod effects;
mod icons;
mod journal;
mod library;
mod motion;
mod settings;
mod state;
mod tasks;
mod theme;
mod toolbar;
mod view;
mod wall;
mod widgets;

use rune::{AppSpec, Key, KeyEvent, Tx};
use state::{Overlay, Screen, State};

fn main() {
    rune::prewarm_gpu();
    rune::tooling::set_app_id("com.startpoint.rune-preview");
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
            "Startpoint — Rune Preview",
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
