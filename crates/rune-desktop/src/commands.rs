use crate::{
    components::*,
    editor::EditorSection,
    icons,
    state::{LibraryFilter, Overlay, Screen, State},
};
use rune::{El, Key, ScrollSpec, Tx, col, el, row};
use rune_kit::{Kit, Menu, MenuItem, TextInput};
use startpoint_sdk::*;

#[derive(Clone, Copy)]
enum Action {
    Recents,
    Library,
    Activity,
    Settings,
    Customize,
    Add,
    Refresh,
    Metadata,
    Hidden,
    Play,
    Details,
    Artwork,
    Launch,
    Journal,
    Favourite,
}

fn actions(state: &State) -> Vec<(&'static str, Action)> {
    let mut actions = vec![
        ("Go to Recents", Action::Recents),
        ("Go to Library", Action::Library),
        ("Go to Activity", Action::Activity),
        ("Settings", Action::Settings),
        ("Customize appearance", Action::Customize),
        ("Add a game", Action::Add),
        ("Refresh installed games", Action::Refresh),
        (
            "Refresh metadata and missing artwork for all games",
            Action::Metadata,
        ),
        ("Show hidden games", Action::Hidden),
    ];
    if state.selected_game().is_some() {
        actions.extend([
            ("Play selected game", Action::Play),
            ("Edit game details", Action::Details),
            ("Manage artwork", Action::Artwork),
            ("Launch settings", Action::Launch),
            ("Open journal", Action::Journal),
            ("Toggle favourite", Action::Favourite),
        ]);
    }
    let query = state.command_query.text.trim().to_lowercase();
    actions.retain(|(label, _)| query.is_empty() || label.to_lowercase().contains(&query));
    actions
}

fn perform(state: &mut State, action: Action) {
    state.close_overlay();
    match action {
        Action::Recents => state.navigate(Screen::Recents),
        Action::Library => state.navigate(Screen::Library),
        Action::Activity => state.navigate(Screen::Activity),
        Action::Settings => state.navigate(Screen::Settings),
        Action::Customize => state.show_overlay(Overlay::Customize),
        Action::Add => {
            state.manual = crate::editor::ManualDraft::default();
            state.show_overlay(Overlay::AddGame);
        }
        Action::Refresh => {
            state.submit(Command::RefreshLibrary);
        }
        Action::Metadata => {
            if !state.headless {
                state.submit(Command::RefreshMetadata);
            }
        }
        Action::Hidden => state.set_filter(LibraryFilter::Hidden),
        Action::Play => state.launch_selected(),
        Action::Favourite => state.toggle_favourite(),
        Action::Details | Action::Artwork | Action::Launch | Action::Journal => {
            if let Some(game) = state.selected_game() {
                let section = match action {
                    Action::Artwork => EditorSection::Artwork,
                    Action::Launch => EditorSection::Launch,
                    Action::Journal => EditorSection::Journal,
                    _ => EditorSection::Details,
                };
                state.open_editor(game.id.clone(), section);
            }
        }
    }
}

#[derive(Clone)]
enum CommandEntry {
    Game(GameId),
    Action(&'static str, Action),
}

fn entries(state: &State) -> Vec<CommandEntry> {
    let query = state.command_query.text.trim().to_lowercase();
    let mut results: Vec<_> = state
        .snapshot
        .games
        .iter()
        .filter(|game| {
            game.available
                && !game.hidden
                && (query.is_empty() || game.title().to_lowercase().contains(&query))
        })
        .take(if query.is_empty() { 4 } else { 20 })
        .map(|game| CommandEntry::Game(game.id.clone()))
        .collect();
    results.extend(
        actions(state)
            .into_iter()
            .map(|(title, action)| CommandEntry::Action(title, action)),
    );
    results
}

fn choose(state: &mut State, entry: CommandEntry, tx: &mut Tx) {
    tx.instant();
    match entry {
        CommandEntry::Action(_, action) => perform(state, action),
        CommandEntry::Game(id) => {
            state.close_overlay();
            state.set_filter(LibraryFilter::All);
            state.select(id.clone());
            state.refresh_library_order();
            state.reveal_game(&id, tx);
        }
    }
}

pub fn palette(state: &State, kit: &Kit) -> El {
    let items = entries(state);
    let selected = state.command_index.min(items.len().saturating_sub(1));
    let mut results = col()
        .w_full()
        .gap(2.0)
        .padding(8.0)
        .role("listbox")
        .label("Games and commands");
    let mut last_group = "";
    for (index, entry) in items.iter().enumerate() {
        let (title, detail, icon, group) = match entry {
            CommandEntry::Game(id) => {
                let Some(game) = state.game(id) else {
                    continue;
                };
                (game.title(), "Open in Library", &icons::GRID, "GAMES")
            }
            CommandEntry::Action(title, action) => (
                *title,
                "Application command",
                match action {
                    Action::Play => &icons::PLAY,
                    Action::Add => &icons::ADD,
                    Action::Refresh | Action::Metadata => &icons::REFRESH,
                    Action::Settings | Action::Details | Action::Launch => &icons::SETTINGS,
                    Action::Customize => &icons::CUSTOMIZE,
                    _ => &icons::LIST,
                },
                "COMMANDS",
            ),
        };
        if group != last_group {
            results = results.child(eyebrow(group, kit).px(12.0).pt(10.0).pb(6.0));
            last_group = group;
        }
        let entry = entry.clone();
        let active = index == selected;
        let visual = if let CommandEntry::Game(id) = &entry
            && let Some(game) = state.game(id)
            && game.artwork.contains_key(&ArtworkKind::Cover)
        {
            crate::library::artwork(state, game, ArtworkKind::Cover, false, kit)
                .size(30.0, 40.0)
                .clip_rounded(4.0)
        } else {
            el().size(30.0, 30.0)
                .radius(7.0)
                .fill(kit.c.text_base.alpha(0.05))
                .center()
                .child(rune::icon(icon, 16.0).color(kit.c.icon_muted))
        };
        results = results.child(
            row()
                .key(format!("command-result-{index}"))
                .w_full()
                .h(52.0)
                .px(12.0)
                .gap(12.0)
                .center_y()
                .radius(8.0)
                .role("option")
                .label(title)
                .focusable(true)
                .focus_selected(active)
                .fill(if active {
                    kit.c.text_accent.alpha(0.12)
                } else {
                    rune::Color::TRANSPARENT
                })
                .hover(kit.c.hover)
                .cursor(rune::Cursor::Pointer)
                .child(visual)
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(3.0)
                        .child(label(title, 13.0, 500.0, kit).truncate())
                        .child(label(detail, 10.0, 400.0, kit).color(kit.c.text_muted)),
                )
                .child(
                    label(if active { "↵" } else { "" }, 15.0, 400.0, kit).color(kit.c.text_muted),
                )
                .on_hover(move |s: &mut State| s.command_index = index)
                .on_press_tx(move |s: &mut State, tx| choose(s, entry.clone(), tx)),
        );
    }
    if items.is_empty() {
        results = results.child(
            col()
                .padding(24.0)
                .gap(8.0)
                .child(label("No results", 16.0, 550.0, kit))
                .child(hint(
                    "Try a game title, a destination, or an action such as Add or Refresh.",
                    kit,
                )),
        );
    }
    col()
        .w_full()
        .gap(0.0)
        .child(
            row()
                .w_full()
                .px(16.0)
                .gap(10.0)
                .center_y()
                .child(
                    TextInput::new(&state.command_query)
                        .plain()
                        .width(None)
                        .height(58.0)
                        .leading_icon(kit.glyph.search)
                        .label("Search games and commands")
                        .placeholder("Search games or run a command…")
                        .on_edit(|s: &mut State| &mut s.command_query)
                        .on_change(|s: &mut State, _| {
                            s.command_index = 0;
                            s.update_images();
                        })
                        .on_key(|s: &mut State, event, tx| {
                            if event.key == Key::Escape {
                                tx.instant();
                                if s.command_query.text.is_empty() {
                                    s.close_overlay();
                                    tx.blur();
                                } else {
                                    s.command_query = rune::TextEdit::new("");
                                    s.command_query.focused = true;
                                    s.command_index = 0;
                                }
                                return true;
                            }
                            if matches!(event.key, Key::Down | Key::Up) {
                                tx.instant();
                                let count = entries(s).len();
                                s.command_index = if event.key == Key::Down {
                                    (s.command_index + 1).min(count.saturating_sub(1))
                                } else {
                                    s.command_index.saturating_sub(1)
                                };
                                tx.scroll_into_view(format!("command-result-{}", s.command_index));
                                return true;
                            }
                            false
                        })
                        .on_submit(|s: &mut State, tx| {
                            if let Some(entry) = entries(s).get(s.command_index).cloned() {
                                choose(s, entry, tx);
                            }
                        })
                        .build(kit)
                        .key("command-query")
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(
                    toolbar_icon(kit.glyph.close, "Close command panel", kit)
                        .on_press(|s: &mut State| s.close_overlay())
                        .key("command-close"),
                ),
        )
        .child(el().w_full().h(1.0).fill(kit.c.border_base.alpha(0.6)))
        .child(
            col()
                .key("command-results")
                .w_full()
                .max_h(360.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(results),
        )
        .child(
            row()
                .w_full()
                .px(20.0)
                .h(38.0)
                .center_y()
                .gap(16.0)
                .child(label("↑ ↓  Navigate", 10.0, 450.0, kit).color(kit.c.text_muted))
                .child(label("↵  Open", 10.0, 450.0, kit).color(kit.c.text_muted))
                .child(el().grow(1.0))
                .child(label("Esc  Close", 10.0, 450.0, kit).color(kit.c.text_muted)),
        )
}

pub fn overlay(state: &State, kit: &Kit, size: [f32; 2], layers: &mut Vec<El>) {
    let mut panel = col()
        .key("command-panel")
        .role("dialog")
        .label("Search and commands")
        .w_full()
        .max_w(640.0)
        .radius(14.0)
        .clip_rounded(14.0)
        .fill(kit.c.layer_01)
        .border(0.5, kit.c.border_base)
        .shadow(20.0, 60.0, rune::Color::BLACK.alpha(0.3))
        .child(palette(state, kit))
        .modal(true, |s: &mut State| s.close_overlay())
        .modal_click_outside(true)
        .focus_return("sidebar-toggle")
        .motion(rune::Spring::new(0.24, 0.0));
    if state.command_pointer {
        panel = panel.matched("command-surface");
    } else {
        panel = panel.transition(rune::Transition::none());
    }
    layers.push(
        el().key("command-scrim")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(kit.c.scrim)
            .a11y_hidden(),
    );
    layers.push(
        el().key("command-layer")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .padding(24.0)
            .center()
            .child(panel.max_h(size[1] - 80.0)),
    );
}

pub fn notifications(state: &State, kit: &Kit) -> El {
    let visible: Vec<_> = state
        .snapshot
        .jobs
        .iter()
        .rev()
        .filter(|job| !state.dismissed_notifications.contains(&job.id))
        .collect();
    let mut content = col().w_full().gap(16.0).child(
        row()
            .w_full()
            .justify_between()
            .center_y()
            .child(eyebrow("RECENT OPERATIONS", kit))
            .child(
                button("Clear completed")
                    .disabled(!visible.iter().any(|job| !job.state.active()))
                    .on_press(|s: &mut State| {
                        s.dismissed_notifications.extend(
                            s.snapshot
                                .jobs
                                .iter()
                                .filter(|job| !job.state.active())
                                .map(|job| job.id),
                        );
                        s.dismissed_notifications.sort_unstable();
                        s.dismissed_notifications.dedup();
                        s.error = None;
                    })
                    .build(kit)
                    .key("notifications-clear"),
            ),
    );
    for job in &visible {
        let id = job.id;
        let mut entry = col()
            .w_full()
            .gap(6.0)
            .child(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(
                        label(&job.label, 13.0, 500.0, kit)
                            .grow(1.0)
                            .min_w(0.0)
                            .wrap(),
                    )
                    .child(
                        toolbar_icon(kit.glyph.close, "Dismiss notification", kit)
                            .disabled(job.state.active())
                            .on_press(move |s: &mut State| {
                                s.dismissed_notifications.push(id);
                                if s.last_job == Some(id) {
                                    s.error = None;
                                }
                            })
                            .key(format!("notification-dismiss-{id}")),
                    ),
            )
            .child(hint(format!("{:?}", job.state), kit));
        if let Some(error) = &job.error {
            entry = entry.child(hint(error, kit));
        }
        if job.state.active() {
            entry = entry.child(
                button("Cancel operation")
                    .on_press(move |s: &mut State| s.api.cancel(id))
                    .build(kit),
            );
        }
        content = content.child(entry);
    }
    if visible.is_empty() {
        content = content.child(hint("You’re all caught up.", kit));
    }
    content
}

pub fn context_menu(state: &State, kit: &Kit, layers: &mut Vec<El>) {
    let Some((id, point)) = &state.context else {
        return;
    };
    let Some(game) = state.game(id) else {
        return;
    };
    let play = id.clone();
    let favourite = id.clone();
    let completed = id.clone();
    let hidden = id.clone();
    let directory = game.install_directory.clone();
    let mut menu = Menu::new("game-context")
        .min_width(240.0)
        .trigger(el())
        .open(true)
        .at(point.x, point.y)
        .on_close(|s: &mut State| s.context = None)
        .item(
            MenuItem::new("Play")
                .icon(&icons::PLAY)
                .disabled(!game.available || game.hidden)
                .on_select(move |s: &mut State| {
                    s.context = None;
                    s.launch(play.clone());
                }),
        )
        .item(
            MenuItem::new(if game.favourite {
                "Remove favourite"
            } else {
                "Add favourite"
            })
            .icon(&icons::FAVOURITE)
            .on_select(move |s: &mut State| {
                s.context = None;
                if let Some(game) = s.game(&favourite) {
                    s.submit(Command::SetFavourite {
                        game_id: favourite.clone(),
                        favourite: !game.favourite,
                    });
                }
            }),
        )
        .separator();
    for (title, section) in [
        ("Details…", EditorSection::Details),
        ("Artwork…", EditorSection::Artwork),
        ("Launch settings…", EditorSection::Launch),
        ("Journal…", EditorSection::Journal),
    ] {
        let id = id.clone();
        let icon = match section {
            EditorSection::Details => &icons::SETTINGS,
            EditorSection::Artwork => &icons::GRID,
            EditorSection::Launch => &icons::PLAY,
            EditorSection::Journal => &icons::PEN,
        };
        menu = menu.item(
            MenuItem::new(title)
                .icon(icon)
                .on_select(move |s: &mut State| {
                    s.context = None;
                    s.open_editor(id.clone(), section);
                }),
        );
    }
    menu = menu
        .separator()
        .item(
            MenuItem::new(if game.completed {
                "Mark unfinished"
            } else {
                "Mark completed"
            })
            .icon(kit.glyph.check)
            .on_select(move |s: &mut State| {
                s.context = None;
                if let Some(game) = s.game(&completed) {
                    s.submit(Command::SetGameProgress {
                        game_id: completed.clone(),
                        collections: game.collections.clone(),
                        completed: !game.completed,
                    });
                }
            }),
        )
        .item(
            MenuItem::new(if game.hidden {
                "Unhide game"
            } else {
                "Hide game"
            })
            .icon(if game.hidden {
                kit.glyph.eye
            } else {
                kit.glyph.eye_off
            })
            .on_select(move |s: &mut State| {
                s.context = None;
                if let Some(game) = s.game(&hidden) {
                    s.submit(Command::SetHidden {
                        game_id: hidden.clone(),
                        hidden: !game.hidden,
                    });
                }
            }),
        )
        .item(
            MenuItem::new("Open install directory")
                .icon(&icons::FOLDER)
                .disabled(state.headless || game.install_directory.as_os_str().is_empty())
                .on_select(move |s: &mut State| {
                    s.context = None;
                    s.open_path(directory.clone());
                }),
        );
    if game.provider_id == "manual" {
        let id = id.clone();
        menu = menu.separator().item(
            MenuItem::new("Remove manual game…")
                .icon(&icons::TRASH)
                .danger(true)
                .on_select(move |s: &mut State| {
                    s.context = None;
                    s.confirm(
                        "Remove game?",
                        "Remove this game from Startpoint. Installed files will not be deleted.",
                        Command::RemoveManualGame {
                            game_id: id.clone(),
                        },
                    );
                }),
        );
    }
    let _ = menu.build(kit, layers);
}
