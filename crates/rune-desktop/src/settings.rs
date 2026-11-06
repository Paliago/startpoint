use crate::{
    components::*,
    state::{HostEvent, Overlay, State},
    theme::{ThemeDocument, ThemeDraft},
};
use rune::{El, FileDialogOptions, Phase, col, el, row};
use rune_kit::{ButtonVariant, Kit, Menu, MenuItem, Switch, TextInput};
use startpoint_sdk::*;

pub fn view(state: &State, kit: &Kit, layers: &mut Vec<El>) -> El {
    let startup = choice(
        state,
        ("start-screen", "Preferred start screen"),
        &["Recents", "Library", "Activity"],
        state.appearance.start_screen.label(),
        |s, value, _| {
            s.appearance.start_screen = match value {
                "Library" => StartScreen::Library,
                "Activity" => StartScreen::Activity,
                _ => StartScreen::Recents,
            };
            s.save_appearance();
        },
        kit,
        layers,
    );
    let status = state
        .snapshot
        .settings
        .credential_status
        .get("steamgriddb")
        .map(CredentialStatus::label)
        .unwrap_or("No API key supplied");
    let credential_options = Menu::new("credential-options")
        .trigger(
            button("Options")
                .on_press(|s: &mut State| {
                    s.open_select = if s.open_select.as_deref() == Some("credential-options") {
                        None
                    } else {
                        Some("credential-options".into())
                    };
                })
                .build(kit)
                .key("credential-options-button"),
        )
        .open(state.open_select.as_deref() == Some("credential-options"))
        .on_close(|s: &mut State| s.open_select = None)
        .item(
            MenuItem::new("Verify / unlock API key")
                .disabled(state.headless || state.busy())
                .on_select(|s: &mut State| {
                    s.open_select = None;
                    s.verify_artwork();
                }),
        )
        .separator()
        .item(
            MenuItem::new("Remove API key…")
                .danger(true)
                .disabled(
                    state.headless
                        || state.busy()
                        || !state
                            .snapshot
                            .settings
                            .configured_credentials
                            .contains("steamgriddb"),
                )
                .on_select(|s: &mut State| {
                    s.open_select = None;
                    s.confirm(
                        "Remove API key?",
                        "Remove the SteamGridDB key from the OS credential store.",
                        Command::SetArtworkCredential {
                            provider_id: "steamgriddb".into(),
                            credential: None,
                        },
                    );
                }),
        )
        .build(kit, layers);
    let credential = section("SteamGridDB API key", kit)
        .child(hint(status, kit))
        .child(
            TextInput::new(&state.credential)
                .secure()
                .width(None)
                .height(36.0)
                .label("SteamGridDB API key")
                .placeholder("Enter API key")
                .on_edit(|s: &mut State| &mut s.credential)
                .disabled(state.headless)
                .build(kit)
                .key("artwork-credential"),
        )
        .child(
            row()
                .gap(8.0)
                .child(
                    primary("Save API key")
                        .disabled(
                            state.headless
                                || state.busy()
                                || state.credential.text.trim().is_empty(),
                        )
                        .on_press(|s: &mut State| {
                            s.connect_artwork();
                        })
                        .build(kit),
                )
                .child(credential_options),
        )
        .child(
            row().child(
                button("Get an API key ↗")
                    .variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut State| s.open_key_page())
                    .build(kit),
            ),
        )
        .child(
            row().child(
                button("Download missing artwork")
                    .disabled(state.headless || state.busy() || !state.artwork_connected())
                    .on_press(|s: &mut State| s.fetch_artwork())
                    .build(kit)
                    .key("download-artwork"),
            ),
        );
    let mut discovery = section("Game discovery", kit);
    for (provider, title, directory) in [
        (
            "steam",
            "Steam directory",
            &state.snapshot.settings.steam_directory,
        ),
        (
            "epic",
            "Epic manifests directory",
            &state.snapshot.settings.epic_directory,
        ),
        (
            "gog",
            "GOG directory",
            &state.snapshot.settings.gog_directory,
        ),
    ] {
        let locations = state
            .snapshot
            .settings
            .discovery_directories
            .get(provider)
            .map(|paths| {
                paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|| "Automatic discovery".into());
        let provider = provider.to_owned();
        let reset_provider = provider.clone();
        discovery = discovery.child(setting_row(
            title,
            directory
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or(locations),
            row()
                .gap(8.0)
                .child(
                    button("Choose…")
                        .disabled(state.example)
                        .on_press_tx(move |_: &mut State, tx| {
                            let provider = provider.clone();
                            tx.open_folder(
                                FileDialogOptions {
                                    title: "Choose discovery directory".into(),
                                    ..Default::default()
                                },
                                move |s: &mut State, result, _| match result {
                                    Ok(Some(directory)) => {
                                        s.submit(directory_command(&provider, Some(directory)));
                                    }
                                    Err(error) => s.error = Some(error.to_string()),
                                    _ => {}
                                },
                            );
                        })
                        .build(kit),
                )
                .child(
                    button("Automatic")
                        .disabled(state.example || directory.is_none())
                        .on_press(move |s: &mut State| {
                            s.submit(directory_command(&reset_provider, None));
                        })
                        .build(kit),
                ),
            kit,
        ));
    }
    discovery = discovery.child(
        row()
            .gap(8.0)
            .child(
                primary("Refresh installed games")
                    .disabled(state.busy())
                    .on_press(|s: &mut State| {
                        s.submit(Command::RefreshLibrary);
                    })
                    .build(kit),
            )
            .child(
                button("Refresh metadata and missing artwork")
                    .disabled(state.headless || state.busy())
                    .on_press(|s: &mut State| {
                        s.submit(Command::RefreshMetadata);
                    })
                    .build(kit),
            ),
    );
    let mut storage = section("Local storage", kit);
    for (title, path) in [
        ("Library data", &state.snapshot.paths.data_directory),
        ("Artwork cache", &state.snapshot.paths.cache_directory),
    ] {
        let path = path.clone();
        storage = storage.child(setting_row(
            title,
            path.display().to_string(),
            button("Open directory")
                .disabled(state.headless)
                .on_press(move |s: &mut State| s.open_path(path.clone()))
                .build(kit),
            kit,
        ));
    }
    let mut page = col()
        .key("settings-page")
        .w_full()
        .max_w(920.0)
        .gap(24.0)
        .child(label("Settings", 32.0, 600.0, kit));
    if let Some(error) = &state.error {
        page = page.child(error_message(error, kit));
    }
    page = page
        .child(section("Startup", kit).child(startup))
        .child(credential).child(discovery).child(storage)
        .child(section("Updates", kit).child(hint(format!("Startpoint Rune {} · Development frontend. Automatic installation and restart are disabled until Rune has its own packaged release channel.", env!("CARGO_PKG_VERSION")), kit)));
    if state.developer_tools {
        page = page.child(
            section("Developer tools", kit).child(setting_row(
                "Component effects",
                "GPU materials and interactive Rune components. Changes remain in this session.",
                button("Open effects page")
                    .on_press(|s: &mut State| s.navigate(crate::state::Screen::ComponentEffects))
                    .build(kit)
                    .key("open-component-effects"),
                kit,
            )),
        );
    }
    page
}

fn directory_command(provider: &str, directory: Option<std::path::PathBuf>) -> Command {
    match provider {
        "epic" => Command::SetEpicDirectory { directory },
        "gog" => Command::SetGogDirectory { directory },
        _ => Command::SetSteamDirectory { directory },
    }
}

pub fn customize(state: &State, kit: &Kit, layers: &mut Vec<El>) -> El {
    let a = &state.appearance;
    let names: Vec<_> = state
        .themes
        .iter()
        .map(|theme| theme.name.as_str())
        .collect();
    let mut content = col()
        .w_full()
        .gap(14.0)
        .child(eyebrow("APPEARANCE", kit))
        .child(choice(
            state,
            ("interface-style", "Interface style"),
            &["Classic", "Infinite Wall"],
            style_name(a.recents_layout),
            |s, value, _| {
                s.appearance.recents_layout = match value {
                    "Infinite Wall" => RecentsLayout::InfiniteWall,
                    _ => RecentsLayout::Carousel,
                };
                s.save_appearance();
            },
            kit,
            layers,
        ))
        .child(choice(
            state,
            ("colour-mode", "Colour mode"),
            &["System", "Light", "Dark"],
            a.mode.label(),
            |s, value, _| {
                s.appearance.mode = match value {
                    "Light" => AppearanceMode::Light,
                    "Dark" => AppearanceMode::Dark,
                    _ => AppearanceMode::System,
                };
                s.save_appearance();
            },
            kit,
            layers,
        ))
        .child(choice(
            state,
            ("colour-theme", "Theme"),
            &names,
            &a.theme,
            |s, value, _| {
                s.appearance.theme = value.into();
                s.save_appearance();
            },
            kit,
            layers,
        ))
        .child(
            row()
                .gap(8.0)
                .child(
                    button("Edit / copy theme…")
                        .on_press(|s: &mut State| {
                            let document = s
                                .themes
                                .iter()
                                .find(|theme| theme.name == s.appearance.theme)
                                .cloned()
                                .unwrap_or_default();
                            let dark = match s.appearance.mode {
                                AppearanceMode::Dark => true,
                                AppearanceMode::Light => false,
                                AppearanceMode::System => s.system_dark,
                            };
                            s.theme_draft = ThemeDraft::new(document, dark);
                            s.show_overlay(Overlay::Theme);
                        })
                        .build(kit),
                )
                .child(
                    button("Import theme…")
                        .on_press_tx(|_: &mut State, tx| {
                            tx.open_file(
                                FileDialogOptions::default().filter("Startpoint theme", &["json"]),
                                |s: &mut State, result, _| match result {
                                    Ok(Some(path)) => {
                                        let sender = s.host_sender.clone();
                                        s.spawn("startpoint-rune-import-theme", move || {
                                            let result = (|| {
                                                let metadata = std::fs::metadata(&path)
                                                    .map_err(|error| error.to_string())?;
                                                if metadata.len() > 262_144 {
                                                    return Err(
                                                        "Theme file exceeds 256 KiB.".into()
                                                    );
                                                }
                                                let source = std::fs::read_to_string(path)
                                                    .map_err(|error| error.to_string())?;
                                                let document: ThemeDocument =
                                                    serde_json::from_str(&source)
                                                        .map_err(|error| error.to_string())?;
                                                document.validate()?;
                                                Ok(document)
                                            })(
                                            );
                                            let _ =
                                                sender.try_send(HostEvent::ThemeImported(result));
                                        });
                                    }
                                    Err(error) => s.error = Some(error.to_string()),
                                    _ => {}
                                },
                            );
                        })
                        .build(kit),
                ),
        )
        .child(eyebrow("LIBRARY PRESENTATION", kit).pt(12.0))
        .child(choice(
            state,
            ("library-layout", "Library layout"),
            &["Pills", "Cover grid", "Landscape tiles", "Compact list"],
            a.library_layout.label(),
            |s, value, _| {
                s.appearance.library_layout = match value {
                    "Pills" => LibraryLayout::Pills,
                    "Landscape tiles" => LibraryLayout::Tiles,
                    "Compact list" => LibraryLayout::Compact,
                    _ => LibraryLayout::Covers,
                };
                s.page = 0;
                s.save_appearance();
            },
            kit,
            layers,
        ))
        .child(choice(
            state,
            ("library-density", "Artwork size"),
            &["Small", "Medium", "Large"],
            a.library_density.label(),
            |s, value, _| {
                s.appearance.library_density = match value {
                    "Small" => LibraryDensity::Small,
                    "Large" => LibraryDensity::Large,
                    _ => LibraryDensity::Medium,
                };
                s.save_appearance();
            },
            kit,
            layers,
        ));
    let groups: [(&str, &[Toggle]); 3] = [
        (
            "LIBRARY DETAILS",
            &[
                Toggle::Sidebar,
                Toggle::Details,
                Toggle::Sources,
                Toggle::Dates,
                Toggle::FavouriteBadges,
            ],
        ),
        (
            "RECENTS WIDGETS",
            &[Toggle::Activity, Toggle::Progress, Toggle::Journal],
        ),
        (
            "MATERIALS AND EFFECTS",
            &[
                Toggle::Translucency,
                Toggle::BackdropBlur,
                Toggle::RecentDither,
                Toggle::SelectedDither,
                Toggle::PauseInBackground,
            ],
        ),
    ];
    for (title, settings) in groups {
        let mut group = col()
            .key(format!(
                "customization-{}",
                title.to_lowercase().replace(' ', "-")
            ))
            .w_full()
            .gap(4.0)
            .child(el().w_full().h(1.0).fill(kit.c.text_base.alpha(0.07)))
            .child(eyebrow(title, kit).py(10.0));
        for &setting in settings {
            group = group.child(
                Switch::new(setting.get(a))
                    .label(setting.label())
                    .on_toggle(move |s: &mut State| {
                        let value = !setting.get(&s.appearance);
                        setting.set(&mut s.appearance, value);
                        s.save_appearance();
                    })
                    .build(kit)
                    .key(format!(
                        "customization-{}",
                        setting.label().to_lowercase().replace(' ', "-")
                    ))
                    .w_full()
                    .justify_between()
                    .h(32.0),
            );
        }
        content = content.child(group);
    }
    content = content.child(eyebrow("FINE ADJUSTMENTS", kit).pt(12.0));
    for setting in [
        Numeric::ControlRadius,
        Numeric::Translucency,
        Numeric::RecentStrength,
        Numeric::RecentPixel,
        Numeric::SelectedStrength,
        Numeric::SelectedPixel,
    ] {
        content = content.child(slider(state, kit, setting));
    }
    content.child(
        button("Reset control styling")
            .on_press(|s: &mut State| {
                s.appearance.controls = ControlAppearance::default();
                s.save_appearance();
            })
            .build(kit),
    )
}

pub fn style_name(style: RecentsLayout) -> &'static str {
    match style {
        RecentsLayout::Carousel => "Classic",
        RecentsLayout::Gallery | RecentsLayout::Cinematic => "Classic",
        RecentsLayout::InfiniteWall => "Infinite Wall",
        RecentsLayout::FilmReel | RecentsLayout::Mosaic => "Classic",
    }
}

#[derive(Clone, Copy)]
enum Toggle {
    Translucency,
    BackdropBlur,
    RecentDither,
    SelectedDither,
    PauseInBackground,
    Sidebar,
    Details,
    Sources,
    Dates,
    FavouriteBadges,
    Activity,
    Progress,
    Journal,
}
impl Toggle {
    fn label(self) -> &'static str {
        match self {
            Self::Translucency => "Translucent glass surfaces",
            Self::BackdropBlur => "Desktop backdrop blur",
            Self::RecentDither => "Recents artwork dithering",
            Self::SelectedDither => "Selected artwork dithering",
            Self::PauseInBackground => "Pause effects in background",
            Self::Sidebar => "Show sidebar",
            Self::Details => "Game detail pane",
            Self::Sources => "Source labels",
            Self::Dates => "Last played",
            Self::FavouriteBadges => "Favourite badges",
            Self::Activity => "Weekly activity widget",
            Self::Progress => "Collection progress widget",
            Self::Journal => "Journal preview widget",
        }
    }
    fn get(self, a: &AppearanceSettings) -> bool {
        match self {
            Self::Translucency => a.window_translucency,
            Self::BackdropBlur => a.backdrop_blur,
            Self::RecentDither => a.dither_recents,
            Self::SelectedDither => a.dither_artwork,
            Self::PauseInBackground => a.pause_in_background,
            Self::Sidebar => a.sidebar_visible,
            Self::Details => a.library_details,
            Self::Sources => a.library_sources,
            Self::Dates => a.library_last_played,
            Self::FavouriteBadges => a.library_favourite_badges,
            Self::Activity => a.recents_activity,
            Self::Progress => a.recents_progress,
            Self::Journal => a.recents_journal,
        }
    }
    fn set(self, a: &mut AppearanceSettings, value: bool) {
        match self {
            Self::Translucency => a.window_translucency = value,
            Self::BackdropBlur => a.backdrop_blur = value,
            Self::RecentDither => a.dither_recents = value,
            Self::SelectedDither => a.dither_artwork = value,
            Self::PauseInBackground => a.pause_in_background = value,
            Self::Sidebar => a.sidebar_visible = value,
            Self::Details => a.library_details = value,
            Self::Sources => a.library_sources = value,
            Self::Dates => a.library_last_played = value,
            Self::FavouriteBadges => a.library_favourite_badges = value,
            Self::Activity => a.recents_activity = value,
            Self::Progress => a.recents_progress = value,
            Self::Journal => a.recents_journal = value,
        }
    }
}

#[derive(Clone, Copy)]
enum Numeric {
    ControlRadius,
    Translucency,
    RecentStrength,
    RecentPixel,
    SelectedStrength,
    SelectedPixel,
}
impl Numeric {
    fn key(self) -> &'static str {
        match self {
            Self::ControlRadius => "control-radius",
            Self::Translucency => "translucency-amount",
            Self::RecentStrength => "recents-dither-strength",
            Self::RecentPixel => "recents-dither-pixel",
            Self::SelectedStrength => "selected-dither-strength",
            Self::SelectedPixel => "selected-dither-pixel",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::ControlRadius => "Control rounding",
            Self::Translucency => "Window translucency",
            Self::RecentStrength => "Recents Bayer treatment",
            Self::RecentPixel => "Recents dither pixel size",
            Self::SelectedStrength => "Selected artwork Bayer treatment",
            Self::SelectedPixel => "Selected artwork dither pixel size",
        }
    }
    fn range(self) -> (f32, f32, &'static str) {
        match self {
            Self::ControlRadius => (0.0, 12.0, "px"),
            Self::Translucency | Self::RecentStrength | Self::SelectedStrength => (0.0, 100.0, "%"),
            Self::RecentPixel | Self::SelectedPixel => (1.0, 12.0, "px"),
        }
    }
    fn get(self, a: &AppearanceSettings) -> f32 {
        match self {
            Self::ControlRadius => a.controls.radius,
            Self::Translucency => a.translucency_amount * 100.0,
            Self::RecentStrength => a.dither_recents_strength * 100.0,
            Self::RecentPixel => a.dither_recents_size,
            Self::SelectedStrength => a.dither_artwork_strength * 100.0,
            Self::SelectedPixel => a.dither_artwork_size,
        }
    }
    fn set(self, a: &mut AppearanceSettings, value: f32) {
        match self {
            Self::ControlRadius => a.controls.radius = value.clamp(0.0, 12.0),
            Self::Translucency => a.translucency_amount = value.clamp(0.0, 100.0) / 100.0,
            Self::RecentStrength => a.dither_recents_strength = value.clamp(0.0, 100.0) / 100.0,
            Self::RecentPixel => a.dither_recents_size = value.clamp(1.0, 12.0),
            Self::SelectedStrength => a.dither_artwork_strength = value.clamp(0.0, 100.0) / 100.0,
            Self::SelectedPixel => a.dither_artwork_size = value.clamp(1.0, 12.0),
        }
    }
}

fn slider(state: &State, kit: &Kit, setting: Numeric) -> El {
    let value = setting.get(&state.appearance);
    let (minimum, maximum, unit) = setting.range();
    let fraction = (value - minimum) / (maximum - minimum);
    col()
        .w_full()
        .gap(8.0)
        .child(label(
            format!("{} · {value:.0}{unit}", setting.label()),
            13.0,
            500.0,
            kit,
        ))
        .child(
            el().key(setting.key())
                .role("slider")
                .label(setting.label())
                .focusable(true)
                .range_value(f64::from(value), f64::from(minimum), f64::from(maximum))
                .range_step(1.0)
                .w_full()
                .h(24.0)
                .cursor(rune::Cursor::Pointer)
                .child(
                    col().w_full().h_full().center_y().child(
                        el().w_full().h(4.0).radius(2.0).fill(kit.c.layer_03).child(
                            el().w_frac(fraction)
                                .h_full()
                                .radius(2.0)
                                .fill(kit.c.text_accent)
                                .child(
                                    el().pin(None, Some(-4.0), Some(-4.0), None)
                                        .size(12.0, 12.0)
                                        .radius(6.0)
                                        .fill(kit.c.text_base),
                                ),
                        ),
                    ),
                )
                .on_drag(move |s: &mut State, drag, _| {
                    if drag.phase == Phase::Down {
                        drag.data = setting.get(&s.appearance);
                    }
                    setting.set(
                        &mut s.appearance,
                        (minimum + drag.x / drag.w.max(1.0) * (maximum - minimum)).round(),
                    );
                    s.appearance_dirty = true;
                    if drag.phase == Phase::Up {
                        if drag.cancelled() {
                            setting.set(&mut s.appearance, drag.data);
                        }
                        s.save_appearance();
                    }
                })
                .on_step(move |s: &mut State, direction, _| {
                    let value = setting.get(&s.appearance)
                        + i16::try_from(direction).map(f32::from).unwrap_or(0.0);
                    setting.set(&mut s.appearance, value);
                    s.save_appearance();
                })
                .on_step_limit(move |s: &mut State, end, _| {
                    setting.set(&mut s.appearance, if end { maximum } else { minimum });
                    s.save_appearance();
                }),
        )
}

pub fn theme_editor(state: &State, kit: &Kit) -> El {
    let draft = &state.theme_draft;
    col()
        .w_full()
        .gap(14.0)
        .child(field(
            "Theme name",
            "theme-name",
            &draft.name,
            |s| &mut s.theme_draft.name,
            kit,
        ))
        .child(field(
            "Background",
            "theme-background",
            &draft.background,
            |s| &mut s.theme_draft.background,
            kit,
        ))
        .child(field(
            "Text",
            "theme-foreground",
            &draft.foreground,
            |s| &mut s.theme_draft.foreground,
            kit,
        ))
        .child(field(
            "Accent",
            "theme-accent",
            &draft.accent,
            |s| &mut s.theme_draft.accent,
            kit,
        ))
        .child(field(
            "Border",
            "theme-border",
            &draft.border,
            |s| &mut s.theme_draft.border,
            kit,
        ))
        .child(field(
            "Surface corner radius",
            "theme-radius",
            &draft.radius,
            |s| &mut s.theme_draft.radius,
            kit,
        ))
        .child(
            form_actions().child(
                primary("Save theme")
                    .on_press(|s: &mut State| match s.theme_draft.resolve() {
                        Ok(document) => {
                            s.store_theme(document, Some(s.theme_draft.document.name.clone()));
                        }
                        Err(error) => s.error = Some(error),
                    })
                    .build(kit)
                    .key("save-theme"),
            ),
        )
}
