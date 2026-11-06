use crate::{
    artwork::ArtworkBrowser,
    components::*,
    state::{Overlay, State},
};
use rune::{El, FileDialogOptions, TextEdit, Tx, col, row};
use rune_kit::{ButtonVariant, Kit, SegmentedControl, Switch};
use startpoint_sdk::*;
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum EditorSection {
    #[default]
    Details,
    Artwork,
    Launch,
    Journal,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum LaunchMode {
    #[default]
    Default,
    Executable,
    SteamUrl,
    SteamShortcut,
}

#[derive(Default)]
pub struct EditorDraft {
    pub game_id: Option<GameId>,
    pub section: EditorSection,
    pub title: TextEdit,
    pub description: TextEdit,
    pub developer: TextEdit,
    pub genres: TextEdit,
    pub collections: TextEdit,
    pub completed: bool,
    pub hidden: bool,
    pub organization_open: bool,
    pub launch_mode: LaunchMode,
    pub executable: TextEdit,
    pub arguments: TextEdit,
    pub directory: TextEdit,
    pub steam_url: TextEdit,
    pub shortcut: Option<SteamShortcut>,
    pub journal: TextEdit,
    pub journal_entry: Option<String>,
    pub artwork: ArtworkBrowser,
    pub import_job: Option<JobId>,
    pub reload_job: Option<JobId>,
    pub journal_job: Option<JobId>,
}

#[derive(Default)]
pub struct ManualDraft {
    pub title: TextEdit,
    pub executable: TextEdit,
    pub arguments: TextEdit,
    pub directory: TextEdit,
    pub pending_job: Option<JobId>,
}

impl State {
    pub fn open_editor(&mut self, id: GameId, section: EditorSection) {
        let Some(game) = self.game(&id) else {
            self.error = Some("This game is no longer in the library.".into());
            return;
        };
        let metadata = game.effective_metadata();
        let override_launch = game
            .launch_override
            .as_ref()
            .or(match &game.default_launch {
                LaunchTarget::Executable(launch) | LaunchTarget::ApplicationBundle(launch) => {
                    Some(launch)
                }
                _ => None,
            });
        let launch_mode = match &game.steam_launch {
            Some(SteamLaunch::Shortcut { .. }) => LaunchMode::SteamShortcut,
            Some(SteamLaunch::Url(_)) => LaunchMode::SteamUrl,
            None if game.launch_override.is_some() => LaunchMode::Executable,
            None => LaunchMode::Default,
        };
        let shortcut = match &game.steam_launch {
            Some(SteamLaunch::Shortcut {
                account_id,
                app_id,
                name,
            }) => Some(SteamShortcut {
                account_id: *account_id,
                app_id: *app_id,
                name: name.clone(),
                executable: String::new(),
                start_directory: String::new(),
            }),
            _ => None,
        };
        self.editor = EditorDraft {
            game_id: Some(id),
            section,
            title: TextEdit::new(
                metadata
                    .title
                    .clone()
                    .unwrap_or_else(|| game.title().into()),
            ),
            description: TextEdit::new(metadata.description.clone().unwrap_or_default()),
            developer: TextEdit::new(metadata.developer.clone().unwrap_or_default()),
            genres: TextEdit::new(metadata.genres.clone().unwrap_or_default()),
            collections: TextEdit::new(
                game.collections
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            completed: game.completed,
            hidden: game.hidden,
            organization_open: false,
            launch_mode,
            executable: TextEdit::new(
                override_launch
                    .map(|launch| launch.executable.display().to_string())
                    .unwrap_or_default(),
            ),
            arguments: TextEdit::new(
                override_launch
                    .map(|launch| launch.arguments.join("\n"))
                    .unwrap_or_default(),
            ),
            directory: TextEdit::new(
                override_launch
                    .and_then(|launch| launch.working_directory.as_ref())
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            ),
            steam_url: TextEdit::new(
                game.steam_launch
                    .as_ref()
                    .map(SteamLaunch::uri)
                    .unwrap_or_default(),
            ),
            shortcut,
            ..EditorDraft::default()
        };
        self.show_overlay(Overlay::Editor);
        if section == EditorSection::Launch {
            self.submit(Command::RefreshSteamShortcuts);
        }
    }

    pub fn save_metadata(&mut self) {
        let Some(id) = self.editor.game_id.clone() else {
            return;
        };
        let Some(game) = self.game(&id) else {
            return;
        };
        let mut current = game.effective_metadata();
        current.title = Some(game.title().into());
        let mut metadata = game.metadata.clone();
        for (draft, original, target) in [
            (&self.editor.title.text, &current.title, &mut metadata.title),
            (
                &self.editor.description.text,
                &current.description,
                &mut metadata.description,
            ),
            (
                &self.editor.developer.text,
                &current.developer,
                &mut metadata.developer,
            ),
            (
                &self.editor.genres.text,
                &current.genres,
                &mut metadata.genres,
            ),
        ] {
            if draft.trim() != original.as_deref().unwrap_or("").trim() {
                *target = Some(draft.trim().into());
            }
        }
        self.submit(Command::UpdateMetadata {
            game_id: id,
            metadata,
        });
    }

    pub fn save_organization(&mut self) {
        let Some(id) = self.editor.game_id.clone() else {
            return;
        };
        let collections: BTreeSet<_> = self
            .editor
            .collections
            .text
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
        self.submit(Command::SetGameProgress {
            game_id: id,
            collections,
            completed: self.editor.completed,
        });
    }

    pub fn save_launch(&mut self) {
        let Some(id) = self.editor.game_id.clone() else {
            return;
        };
        let command = match self.editor.launch_mode {
            LaunchMode::Default => Command::SetLaunchOverride {
                game_id: id,
                launch: None,
            },
            LaunchMode::Executable => {
                match executable_launch(
                    &self.editor.executable,
                    &self.editor.arguments,
                    &self.editor.directory,
                ) {
                    Ok(launch) => Command::SetLaunchOverride {
                        game_id: id,
                        launch: Some(launch),
                    },
                    Err(error) => {
                        self.error = Some(error);
                        return;
                    }
                }
            }
            LaunchMode::SteamUrl => Command::SetSteamLaunch {
                game_id: id,
                launch: SteamLaunch::Url(self.editor.steam_url.text.trim().into()),
            },
            LaunchMode::SteamShortcut => {
                let Some(shortcut) = &self.editor.shortcut else {
                    self.error = Some("Choose a Steam shortcut first.".into());
                    return;
                };
                Command::SetSteamLaunch {
                    game_id: id,
                    launch: shortcut.launch(),
                }
            }
        };
        self.submit(command);
    }

    pub fn save_journal(&mut self) {
        let Some(game_id) = self.editor.game_id.clone() else {
            return;
        };
        if self.editor.journal.text.trim().is_empty() {
            self.error = Some("Write a journal entry before saving.".into());
            return;
        }
        self.editor.journal_job = self.submit(Command::SaveJournalEntry {
            game_id,
            entry_id: self.editor.journal_entry.clone(),
            text: self.editor.journal.text.trim().into(),
        });
    }

    pub fn add_manual_game(&mut self) {
        if self.manual.title.text.trim().is_empty() {
            self.error = Some("Enter a game title.".into());
            return;
        }
        match executable_launch(
            &self.manual.executable,
            &self.manual.arguments,
            &self.manual.directory,
        ) {
            Ok(launch) => {
                self.manual.pending_job = self.submit(Command::AddManualGame {
                    title: self.manual.title.text.trim().into(),
                    launch,
                })
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn choose_executable(&mut self, tx: &mut Tx, manual: bool, bundle: bool) {
        let game = self.editor.game_id.clone();
        let mut options = FileDialogOptions {
            title: if bundle {
                "Choose application bundle"
            } else {
                "Choose game executable"
            }
            .into(),
            ..Default::default()
        };
        let answer =
            move |state: &mut State, result: rune::FileDialogResult, _: &mut Tx| match result {
                Ok(Some(path)) => {
                    if manual && state.overlay == Overlay::AddGame {
                        state.manual.executable = TextEdit::new(path.display().to_string());
                    } else if !manual && state.editor.game_id == game {
                        state.editor.executable = TextEdit::new(path.display().to_string());
                    }
                }
                Err(error) => state.error = Some(error.to_string()),
                _ => {}
            };
        if bundle {
            options = options.filter("Application bundle", &["app"]);
        }
        tx.open_file(options, answer);
    }
}

fn executable_launch(
    executable: &TextEdit,
    arguments: &TextEdit,
    directory: &TextEdit,
) -> Result<ExecutableLaunch, String> {
    if executable.text.trim().is_empty() {
        return Err("Choose an executable or application bundle.".into());
    }
    Ok(ExecutableLaunch {
        executable: PathBuf::from(executable.text.trim()),
        arguments: arguments.text.lines().map(str::to_owned).collect(),
        working_directory: (!directory.text.trim().is_empty())
            .then(|| PathBuf::from(directory.text.trim())),
    })
}

pub fn view(state: &State, kit: &Kit, layers: &mut Vec<El>) -> El {
    let section = state.editor.section;
    let index = match section {
        EditorSection::Details => 0,
        EditorSection::Artwork => 1,
        EditorSection::Launch => 2,
        EditorSection::Journal => 3,
    };
    let tabs = SegmentedControl::new(&["Details", "Artwork", "Launch", "Journal"], Some(index))
        .width(None)
        .on_pick(|state: &mut State, index, _| {
            state.editor.section = match index {
                1 => EditorSection::Artwork,
                2 => EditorSection::Launch,
                3 => EditorSection::Journal,
                _ => EditorSection::Details,
            };
            state.open_select = None;
            if index == 2 {
                state.submit(Command::RefreshSteamShortcuts);
            }
        })
        .build(kit)
        .key("editor-sections");
    let content = match section {
        EditorSection::Details => details(state, kit),
        EditorSection::Artwork => crate::artwork::view(state, kit, layers),
        EditorSection::Launch => launch(state, kit, layers),
        EditorSection::Journal => journal(state, kit),
    };
    col().w_full().gap(18.0).child(tabs).child(content)
}

fn details(state: &State, kit: &Kit) -> El {
    let mut body = col()
        .w_full()
        .gap(14.0)
        .child(field(
            "Title",
            "editor-title",
            &state.editor.title,
            |s| &mut s.editor.title,
            kit,
        ))
        .child(area(
            "Description",
            "editor-description",
            &state.editor.description,
            100.0,
            |s| &mut s.editor.description,
            kit,
        ))
        .child(field(
            "Developer",
            "editor-developer",
            &state.editor.developer,
            |s| &mut s.editor.developer,
            kit,
        ))
        .child(field(
            "Genres",
            "editor-genres",
            &state.editor.genres,
            |s| &mut s.editor.genres,
            kit,
        ))
        .child(
            row()
                .gap(10.0)
                .child(
                    primary("Save details")
                        .on_press(|s: &mut State| s.save_metadata())
                        .build(kit)
                        .key("save-details"),
                )
                .child(
                    button("Re-identify game…")
                        .on_press(|s: &mut State| {
                            s.editor.section = EditorSection::Artwork;
                            s.editor.artwork.identifying = true;
                        })
                        .build(kit),
                ),
        )
        .child(
            button("Library organization")
                .on_press(|s: &mut State| s.editor.organization_open = !s.editor.organization_open)
                .build(kit),
        );
    if state.editor.organization_open {
        body = body.child(
            col()
                .w_full()
                .gap(14.0)
                .child(field(
                    "Collections (comma-separated)",
                    "editor-collections",
                    &state.editor.collections,
                    |s| &mut s.editor.collections,
                    kit,
                ))
                .child(
                    Switch::new(state.editor.completed)
                        .label("Completed")
                        .on_toggle(|s: &mut State| s.editor.completed = !s.editor.completed)
                        .build(kit),
                )
                .child(
                    primary("Save organization")
                        .on_press(|s: &mut State| s.save_organization())
                        .build(kit)
                        .key("save-organization"),
                )
                .child(
                    button(if state.editor.hidden {
                        "Unhide game"
                    } else {
                        "Hide game"
                    })
                    .on_press(|s: &mut State| {
                        if let Some(id) = s.editor.game_id.clone() {
                            let hidden = !s.editor.hidden;
                            s.submit(Command::SetHidden {
                                game_id: id,
                                hidden,
                            });
                            s.editor.hidden = hidden;
                        }
                    })
                    .build(kit),
                ),
        );
        if state
            .editor
            .game_id
            .as_ref()
            .and_then(|id| state.game(id))
            .is_some_and(|game| game.provider_id == "manual")
        {
            body = body.child(button("Remove manual game…").variant(ButtonVariant::Danger).on_press(|s: &mut State| {
                if let Some(game_id) = s.editor.game_id.clone() {
                    s.confirm("Remove game?", "Remove this manual game from Startpoint. Installed files will not be deleted.", Command::RemoveManualGame { game_id });
                }
            }).build(kit));
        }
    }
    body
}

fn launch(state: &State, kit: &Kit, layers: &mut Vec<El>) -> El {
    let current = match state.editor.launch_mode {
        LaunchMode::Default => "Launcher default",
        LaunchMode::Executable => "Executable / application bundle",
        LaunchMode::SteamUrl => "Steam launch URL",
        LaunchMode::SteamShortcut => "Steam non-Steam shortcut",
    };
    let mut body = col().w_full().gap(16.0).child(choice(
        state,
        ("launch-mode", "Launch via"),
        &[
            "Launcher default",
            "Executable / application bundle",
            "Steam launch URL",
            "Steam non-Steam shortcut",
        ],
        current,
        |s, value, _| {
            s.editor.launch_mode = match value {
                "Executable / application bundle" => LaunchMode::Executable,
                "Steam launch URL" => LaunchMode::SteamUrl,
                "Steam non-Steam shortcut" => LaunchMode::SteamShortcut,
                _ => LaunchMode::Default,
            };
        },
        kit,
        layers,
    ));
    if let Some(game) = state.editor.game_id.as_ref().and_then(|id| state.game(id)) {
        body = body.child(hint(
            format!(
                "Original source: {} · {}",
                game.provider_id,
                game.install_directory.display()
            ),
            kit,
        ));
    }
    match state.editor.launch_mode {
        LaunchMode::Default => {
            body = body.child(hint(
                "Uses the game's original launcher and clears saved launch overrides when saved.",
                kit,
            ));
        }
        LaunchMode::Executable => {
            body = body
                .child(field(
                    "Executable / application bundle",
                    "launch-executable",
                    &state.editor.executable,
                    |s| &mut s.editor.executable,
                    kit,
                ))
                .child(
                    row()
                        .gap(8.0)
                        .child(
                            button("Choose file…")
                                .on_press_tx(|s: &mut State, tx| {
                                    s.choose_executable(tx, false, false)
                                })
                                .build(kit),
                        )
                        .child(
                            button("Choose bundle…")
                                .on_press_tx(|s: &mut State, tx| {
                                    s.choose_executable(tx, false, true)
                                })
                                .build(kit),
                        ),
                )
                .child(area(
                    "Arguments (one argument per line)",
                    "launch-arguments",
                    &state.editor.arguments,
                    80.0,
                    |s| &mut s.editor.arguments,
                    kit,
                ))
                .child(field(
                    "Working directory (optional)",
                    "launch-directory",
                    &state.editor.directory,
                    |s| &mut s.editor.directory,
                    kit,
                ));
        }
        LaunchMode::SteamUrl => {
            body = body
                .child(field(
                    "Steam game launch URL",
                    "launch-steam-url",
                    &state.editor.steam_url,
                    |s| &mut s.editor.steam_url,
                    kit,
                ))
                .child(
                    button("Import Steam desktop shortcut…")
                        .on_press_tx(|s: &mut State, tx| {
                            let id = s.editor.game_id.clone();
                            tx.open_file(
                                FileDialogOptions::default()
                                    .filter("Steam desktop shortcut", &["url"]),
                                move |s: &mut State, result, _| match result {
                                    Ok(Some(path)) if s.editor.game_id == id => {
                                        if let Some(game_id) = id {
                                            s.editor.import_job =
                                                s.submit(Command::ReadSteamShortcutUrl {
                                                    game_id,
                                                    path,
                                                });
                                        }
                                    }
                                    Err(error) => s.error = Some(error.to_string()),
                                    _ => {}
                                },
                            );
                        })
                        .build(kit),
                );
        }
        LaunchMode::SteamShortcut => {
            let options: Vec<_> = state
                .snapshot
                .settings
                .steam_shortcuts
                .iter()
                .map(|shortcut| {
                    format!(
                        "{} · account {} · {}",
                        shortcut.name, shortcut.account_id, shortcut.app_id
                    )
                })
                .collect();
            let refs: Vec<_> = options.iter().map(String::as_str).collect();
            let current = state
                .editor
                .shortcut
                .as_ref()
                .map(|shortcut| {
                    format!(
                        "{} · account {} · {}",
                        shortcut.name, shortcut.account_id, shortcut.app_id
                    )
                })
                .unwrap_or_default();
            body = body
                .child(choice(
                    state,
                    ("steam-shortcut", "Saved shortcut"),
                    &refs,
                    &current,
                    |s, value, _| {
                        s.editor.shortcut = s
                            .snapshot
                            .settings
                            .steam_shortcuts
                            .iter()
                            .find(|shortcut| {
                                format!(
                                    "{} · account {} · {}",
                                    shortcut.name, shortcut.account_id, shortcut.app_id
                                ) == value
                            })
                            .cloned();
                    },
                    kit,
                    layers,
                ))
                .child(
                    button("Refresh Steam shortcuts")
                        .on_press(|s: &mut State| {
                            s.submit(Command::RefreshSteamShortcuts);
                        })
                        .build(kit),
                );
            if options.is_empty() {
                body = body.child(hint("No saved non-Steam shortcuts found. Add the game in Steam, restart Steam, then refresh.", kit));
            }
            for warning in &state.snapshot.settings.steam_shortcut_warnings {
                body = body.child(hint(warning, kit));
            }
        }
    }
    body.child(
        primary("Save launch configuration")
            .on_press(|s: &mut State| s.save_launch())
            .build(kit)
            .key("save-launch"),
    )
}

fn journal(state: &State, kit: &Kit) -> El {
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(area(
            "Journal entry",
            "journal-text",
            &state.editor.journal,
            130.0,
            |s| &mut s.editor.journal,
            kit,
        ))
        .child(
            row()
                .gap(8.0)
                .child(
                    primary("Save entry")
                        .disabled(
                            state
                                .editor
                                .journal_job
                                .is_some_and(|id| state.job_active(id)),
                        )
                        .on_press(|s: &mut State| s.save_journal())
                        .build(kit)
                        .key("save-journal"),
                )
                .child(
                    button("New entry")
                        .on_press(|s: &mut State| {
                            s.editor.journal_entry = None;
                            s.editor.journal = TextEdit::new("");
                        })
                        .build(kit),
                ),
        );
    for entry in state
        .snapshot
        .journal
        .iter()
        .filter(|entry| Some(&entry.game_id) == state.editor.game_id.as_ref())
    {
        let entry_id = entry.id.clone();
        let text = entry.text.clone();
        let delete_id = entry.id.clone();
        body = body.child(
            col().w_full().gap(8.0).child(hint(&entry.text, kit)).child(
                row()
                    .gap(8.0)
                    .child(
                        button("Edit")
                            .on_press(move |s: &mut State| {
                                s.editor.journal_entry = Some(entry_id.clone());
                                s.editor.journal = TextEdit::new(&text);
                            })
                            .build(kit),
                    )
                    .child(
                        button("Delete…")
                            .variant(ButtonVariant::Danger)
                            .on_press(move |s: &mut State| {
                                if let Some(game_id) = s.editor.game_id.clone() {
                                    s.confirm(
                                        "Delete journal entry?",
                                        "This entry will be permanently removed.",
                                        Command::DeleteJournalEntry {
                                            game_id,
                                            entry_id: delete_id.clone(),
                                        },
                                    );
                                }
                            })
                            .build(kit),
                    ),
            ),
        );
    }
    body
}

pub fn manual_view(state: &State, kit: &Kit) -> El {
    col()
        .w_full()
        .gap(14.0)
        .child(field(
            "Title",
            "manual-title",
            &state.manual.title,
            |s| &mut s.manual.title,
            kit,
        ))
        .child(field(
            "Executable / application bundle",
            "manual-executable",
            &state.manual.executable,
            |s| &mut s.manual.executable,
            kit,
        ))
        .child(
            row()
                .gap(8.0)
                .child(
                    button("Choose file…")
                        .on_press_tx(|s: &mut State, tx| s.choose_executable(tx, true, false))
                        .build(kit),
                )
                .child(
                    button("Choose bundle…")
                        .on_press_tx(|s: &mut State, tx| s.choose_executable(tx, true, true))
                        .build(kit),
                ),
        )
        .child(area(
            "Arguments (one argument per line)",
            "manual-arguments",
            &state.manual.arguments,
            70.0,
            |s| &mut s.manual.arguments,
            kit,
        ))
        .child(field(
            "Working directory (optional)",
            "manual-directory",
            &state.manual.directory,
            |s| &mut s.manual.directory,
            kit,
        ))
        .child(
            primary("Add game")
                .disabled(
                    state
                        .manual
                        .pending_job
                        .is_some_and(|id| state.job_active(id)),
                )
                .on_press(|s: &mut State| s.add_manual_game())
                .build(kit)
                .key("add-manual-game"),
        )
}
