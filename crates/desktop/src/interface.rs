use crate::{appearance, image_cache::ThumbnailCache};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, TitleBar,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    spinner::Spinner,
    switch::Switch,
};
use startpoint_sdk::{
    AppEvent, AppearanceMode, AppearanceSettings, Application as LauncherApplication, ArtworkAsset,
    ArtworkCandidate, ArtworkGame, ArtworkKind, Command, CredentialStatus, ExecutableLaunch, Game,
    GameId, JobId, JobState, LaunchTarget, Snapshot, StartScreen, UserMetadata,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const ACCENT: u32 = 0xff650d;

actions!(
    startpoint,
    [
        Quit,
        FocusSearch,
        RefreshLibrary,
        DismissPanel,
        SelectNext,
        SelectPrevious,
        LaunchSelected,
        OpenSettings
    ]
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum LibraryFilter {
    Installed,
    All,
    Favourites,
}

impl LibraryFilter {
    fn label(self) -> &'static str {
        match self {
            Self::Installed => "Installed",
            Self::All => "All games",
            Self::Favourites => "Favourites",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailTab {
    Overview,
    Metadata,
    Launch,
    Artwork,
}

enum PickerPurpose {
    AddGame,
    LaunchOverride(GameId),
    Artwork(GameId, ArtworkKind),
    SteamDirectory,
}

pub struct LauncherView {
    api: LauncherApplication,
    snapshot: Snapshot,
    search: Entity<InputState>,
    visible: Vec<Game>,
    selected: Option<GameId>,
    source_filter: Option<String>,
    library_filter: LibraryFilter,
    sort_recent: bool,
    sources_open: bool,
    appearance: AppearanceSettings,
    appearance_loaded: bool,
    screen: StartScreen,
    settings_open: bool,
    jobs_open: bool,
    detail_tab: DetailTab,
    artwork_kind: ArtworkKind,
    artwork_images: Vec<ArtworkCandidate>,
    artwork_games: Vec<ArtworkGame>,
    artwork_job: Option<JobId>,
    manual_import_job: Option<JobId>,
    credential_job: Option<JobId>,
    credential_editing: bool,
    artwork_match: Option<ArtworkGame>,
    artwork_message: Option<String>,
    notice: Option<String>,
    title: Entity<InputState>,
    description: Entity<InputState>,
    developer: Entity<InputState>,
    genres: Entity<InputState>,
    executable: Entity<InputState>,
    arguments: Entity<InputState>,
    working_directory: Entity<InputState>,
    api_key: Entity<InputState>,
    artwork_search: Entity<InputState>,
    thumbnails: Entity<ThumbnailCache>,
    focus: FocusHandle,
    list_scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl LauncherView {
    pub fn new(api: LauncherApplication, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let events = api.subscribe();
        let snapshot = api.snapshot();
        let search = text_input("Find a game…", false, window, cx);
        let subscription =
            cx.subscribe_in(&search, window, |view, _, event, window, cx| match event {
                InputEvent::Change => {
                    view.filter(cx);
                    view.ensure_selection(window, cx);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => view.launch_selected(&LaunchSelected, window, cx),
                _ => {}
            });
        let appearance_subscription = cx.observe_window_appearance(window, |view, window, cx| {
            appearance::apply(view.appearance.mode, Some(window), cx);
        });
        let appearance_settings = snapshot.settings.appearance;
        let appearance_loaded = snapshot.ready;
        appearance::apply(appearance_settings.mode, Some(window), cx);
        let mut view = Self {
            api,
            snapshot,
            search,
            visible: Vec::new(),
            selected: None,
            source_filter: None,
            library_filter: LibraryFilter::Installed,
            sort_recent: true,
            sources_open: false,
            appearance: appearance_settings,
            appearance_loaded,
            screen: appearance_settings.start_screen,
            settings_open: false,
            jobs_open: false,
            detail_tab: DetailTab::Overview,
            artwork_kind: ArtworkKind::Cover,
            artwork_images: Vec::new(),
            artwork_games: Vec::new(),
            artwork_job: None,
            manual_import_job: None,
            credential_job: None,
            credential_editing: false,
            artwork_match: None,
            artwork_message: None,
            notice: None,
            title: text_input("Game title", false, window, cx),
            description: text_input("Add a description", true, window, cx),
            developer: text_input("Developer", false, window, cx),
            genres: text_input("Action, Adventure…", false, window, cx),
            executable: text_input("Leave empty to use the default launcher", false, window, cx),
            arguments: text_input(
                "One argument per line. Do not add quotes.",
                true,
                window,
                cx,
            ),
            working_directory: text_input("Default: executable directory", false, window, cx),
            api_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Your SteamGridDB API key")
                    .masked(true)
            }),
            artwork_search: text_input("Search SteamGridDB by game title", false, window, cx),
            thumbnails: cx.new(ThumbnailCache::new),
            focus: cx.focus_handle(),
            list_scroll: UniformListScrollHandle::new(),
            _subscriptions: vec![subscription, appearance_subscription],
        };
        view.filter(cx);
        view.ensure_selection(window, cx);
        view.focus.focus(window);
        view.submit(Command::RefreshLibrary, cx);
        cx.spawn_in(window, async move |view, cx| {
            while let Ok(event) = events.recv().await {
                if view
                    .update_in(cx, |view, window, cx| view.receive(event, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        view
    }

    fn submit(&mut self, command: Command, cx: &mut Context<Self>) -> Option<JobId> {
        match self.api.submit(command) {
            Ok(id) => {
                self.snapshot = self.api.snapshot();
                cx.notify();
                Some(id)
            }
            Err(error) => {
                self.notice = Some(error.to_string());
                self.jobs_open = true;
                cx.notify();
                None
            }
        }
    }

    fn receive(&mut self, event: AppEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.snapshot = self.api.snapshot();
        if (!self.appearance_loaded && self.snapshot.ready)
            || matches!(event, AppEvent::SettingsChanged)
        {
            if !self.appearance_loaded
                || self.appearance.start_screen != self.snapshot.settings.appearance.start_screen
            {
                self.screen = self.snapshot.settings.appearance.start_screen;
            }
            self.appearance_loaded = self.snapshot.ready;
            self.appearance = self.snapshot.settings.appearance;
            appearance::apply(self.appearance.mode, Some(window), cx);
        }
        match event {
            AppEvent::LibraryChanged => {
                self.filter(cx);
                if !self.settings_open {
                    self.ensure_selection(window, cx);
                }
                if self.detail_tab == DetailTab::Overview {
                    self.populate_game_fields(window, cx);
                }
                if let Some(game) = self.selected_game()
                    && let Some(id) = game.artwork_game_ids.get("steamgriddb")
                    && self
                        .artwork_match
                        .as_ref()
                        .is_none_or(|matched| &matched.id != id)
                {
                    self.artwork_match = Some(ArtworkGame {
                        id: id.clone(),
                        name: game.title().to_string(),
                    });
                }
            }
            AppEvent::ArtworkCacheChanged => self
                .thumbnails
                .update(cx, |cache, cx| cache.invalidate_failed(window, cx)),
            AppEvent::SettingsChanged => {}
            AppEvent::JobChanged(job) => {
                if let Some(error) = job.error {
                    self.notice = Some(error);
                    self.jobs_open = true;
                }
                if Some(job.id) == self.credential_job && !job.state.active() {
                    self.credential_job = None;
                    if job.state == JobState::Completed {
                        self.api_key
                            .update(cx, |field, cx| field.set_value("", window, cx));
                        self.credential_editing = false;
                    }
                    if job.state == JobState::Completed
                        && self
                            .snapshot
                            .settings
                            .configured_credentials
                            .contains("steamgriddb")
                    {
                        self.submit(Command::RefreshLibrary, cx);
                    }
                }
                if Some(job.id) == self.artwork_job && !job.state.active() {
                    self.artwork_job = None;
                    if job.state == JobState::Cancelled {
                        self.artwork_message = Some("Artwork request cancelled".into());
                    }
                }
            }
            AppEvent::GameAdded { job_id, game_id } => {
                if Some(job_id) == self.manual_import_job {
                    self.switch_screen(StartScreen::Library, window, cx);
                    self.select_game(game_id, window, cx);
                    self.manual_import_job = None;
                }
            }
            AppEvent::ArtworkGamesFound {
                job_id,
                game_id,
                games,
            } => {
                if self.artwork_job == Some(job_id) && self.selected.as_ref() == Some(&game_id) {
                    self.artwork_message = if games.is_empty() {
                        Some("No matching games were found".into())
                    } else {
                        Some("Choose the correct game; we never guess a title match.".into())
                    };
                    self.artwork_games = games;
                }
            }
            AppEvent::ArtworkFound {
                job_id,
                game_id,
                kind,
                images,
            } => {
                if self.artwork_job == Some(job_id)
                    && self.selected.as_ref() == Some(&game_id)
                    && self.artwork_kind == kind
                {
                    self.artwork_message = if images.is_empty() {
                        Some("No artwork was found for this image type".into())
                    } else {
                        None
                    };
                    self.artwork_images = images;
                    self.artwork_games.clear();
                }
            }
        }
        cx.notify();
    }

    fn filter(&mut self, cx: &App) {
        let query = self.search.read(cx).value().to_lowercase();
        self.visible = self
            .snapshot
            .games
            .iter()
            .filter(|game| {
                (match self.library_filter {
                    LibraryFilter::Installed => game.available,
                    LibraryFilter::All => true,
                    LibraryFilter::Favourites => game.favourite,
                }) && self
                    .source_filter
                    .as_ref()
                    .is_none_or(|source| source == &game.provider_id)
                    && (query.is_empty()
                        || game.title().to_lowercase().contains(&query)
                        || game
                            .effective_metadata()
                            .genres
                            .as_ref()
                            .is_some_and(|genres| genres.to_lowercase().contains(&query)))
            })
            .cloned()
            .collect();
        if self.sort_recent {
            self.visible
                .sort_by_key(|game| std::cmp::Reverse(game.last_launched.unwrap_or(0)));
        }
    }

    fn recent_games(&self) -> Vec<&Game> {
        let mut games = self
            .visible
            .iter()
            .filter(|game| game.last_launched.is_some())
            .collect::<Vec<_>>();
        if games.is_empty() {
            games = self.visible.iter().collect();
        }
        games.sort_by_key(|game| std::cmp::Reverse(game.last_launched.unwrap_or(0)));
        games.truncate(5);
        games
    }

    fn displayed_games(&self) -> Vec<&Game> {
        match self.screen {
            StartScreen::Library => self.visible.iter().collect(),
            StartScreen::Recents => self.recent_games(),
        }
    }

    fn ensure_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let games = self.displayed_games();
        if !games
            .iter()
            .any(|game| self.selected.as_ref() == Some(&game.id))
        {
            if let Some(id) = games.first().map(|game| game.id.clone()) {
                self.select_game(id, window, cx);
            } else {
                self.selected = None;
                self.cancel_artwork();
            }
        }
    }

    fn selected_game(&self) -> Option<Game> {
        self.snapshot
            .games
            .iter()
            .find(|game| self.selected.as_ref() == Some(&game.id))
            .cloned()
    }

    fn select_game(&mut self, id: GameId, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_artwork();
        self.selected = Some(id);
        self.settings_open = false;
        self.detail_tab = DetailTab::Overview;
        self.artwork_images.clear();
        self.artwork_games.clear();
        self.artwork_message = None;
        self.artwork_match = self.selected_game().and_then(|game| {
            game.artwork_game_ids
                .get("steamgriddb")
                .map(|id| ArtworkGame {
                    id: id.clone(),
                    name: game.title().to_string(),
                })
        });
        self.populate_game_fields(window, cx);
        cx.notify();
    }

    fn populate_game_fields(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(game) = self.selected_game() {
            let metadata = game.effective_metadata();
            set_value(&self.title, game.title(), window, cx);
            set_value(
                &self.description,
                metadata.description.as_deref().unwrap_or(""),
                window,
                cx,
            );
            set_value(
                &self.developer,
                metadata.developer.as_deref().unwrap_or(""),
                window,
                cx,
            );
            set_value(
                &self.genres,
                metadata.genres.as_deref().unwrap_or(""),
                window,
                cx,
            );
            set_value(&self.artwork_search, game.title(), window, cx);
            let launch =
                game.launch_override
                    .as_ref()
                    .or(match &game.default_launch {
                        LaunchTarget::Executable(launch)
                        | LaunchTarget::ApplicationBundle(launch) => Some(launch),
                        LaunchTarget::Uri(_) => None,
                    });
            set_value(
                &self.executable,
                &launch
                    .map(|launch| launch.executable.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                window,
                cx,
            );
            set_value(
                &self.arguments,
                &launch
                    .map(|launch| launch.arguments.join("\n"))
                    .unwrap_or_default(),
                window,
                cx,
            );
            set_value(
                &self.working_directory,
                &launch
                    .and_then(|launch| launch.working_directory.as_ref())
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                window,
                cx,
            );
        }
    }

    fn cancel_artwork(&mut self) {
        if let Some(id) = self.artwork_job.take() {
            self.api.cancel(id);
        }
    }

    fn search_artwork(&mut self, cx: &mut Context<Self>) {
        if let Some(game_id) = self.selected.clone() {
            self.cancel_artwork();
            self.artwork_images.clear();
            self.artwork_games.clear();
            self.artwork_message = None;
            let query = self.artwork_search.read(cx).value().to_string();
            self.artwork_job = self.submit(
                Command::SearchArtworkGames {
                    provider_id: "steamgriddb".into(),
                    game_id,
                    query,
                },
                cx,
            );
        }
    }

    fn load_artwork(&mut self, remote_game_id: Option<String>, cx: &mut Context<Self>) {
        if let Some(game_id) = self.selected.clone() {
            self.cancel_artwork();
            self.artwork_images.clear();
            self.artwork_message = None;
            self.artwork_job = self.submit(
                Command::ListArtwork {
                    provider_id: "steamgriddb".into(),
                    game_id,
                    remote_game_id: remote_game_id
                        .or_else(|| self.artwork_match.as_ref().map(|game| game.id.clone())),
                    kind: self.artwork_kind,
                },
                cx,
            );
        }
    }

    fn choose_artwork_game(&mut self, matched: ArtworkGame, cx: &mut Context<Self>) {
        if let Some(game_id) = self.selected.clone() {
            self.artwork_match = Some(matched.clone());
            self.submit(
                Command::SetArtworkGame {
                    provider_id: "steamgriddb".into(),
                    game_id,
                    remote_game_id: matched.id.clone(),
                },
                cx,
            );
            self.load_artwork(Some(matched.id), cx);
        }
    }

    fn credential_status(&self, cx: &App) -> AnyElement {
        let status = self
            .snapshot
            .settings
            .credential_status
            .get("steamgriddb")
            .unwrap_or(&CredentialStatus::Missing);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .child(div().size(px(8.)).rounded_full().bg(rgb(match status {
                        CredentialStatus::Verified => 0x43ad6d,
                        CredentialStatus::Invalid | CredentialStatus::Unavailable(_) => 0xd89938,
                        _ => 0x92969a,
                    })))
                    .child(status.label())
                    .when(*status == CredentialStatus::Checking, |row| {
                        row.child(Spinner::new().small().color(cx.theme().primary))
                    }),
            )
            .when_some(
                match status {
                    CredentialStatus::Unavailable(message) => Some(message.clone()),
                    _ => None,
                },
                |view, message| view.child(hint(&message)),
            )
            .into_any_element()
    }

    fn pick(&mut self, purpose: PickerPurpose, window: &mut Window, cx: &mut Context<Self>) {
        let directory = matches!(purpose, PickerPurpose::SteamDirectory);
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: !directory,
            directories: directory,
            multiple: false,
            prompt: Some(
                if directory {
                    "Choose Steam installation directory"
                } else {
                    "Choose file"
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = prompt.await;
            let _ = view.update_in(cx, move |view, window, cx| match result {
                Ok(Ok(Some(paths))) => {
                    let Some(path) = paths.into_iter().next() else {
                        return;
                    };
                    match purpose {
                        PickerPurpose::AddGame => {
                            let title = path
                                .file_stem()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "New game".into());
                            view.manual_import_job = view.submit(
                                Command::AddManualGame {
                                    title,
                                    launch: ExecutableLaunch {
                                        executable: path,
                                        arguments: Vec::new(),
                                        working_directory: None,
                                    },
                                },
                                cx,
                            );
                        }
                        PickerPurpose::LaunchOverride(id)
                            if view.selected.as_ref() == Some(&id) =>
                        {
                            set_value(&view.executable, &path.to_string_lossy(), window, cx)
                        }
                        PickerPurpose::LaunchOverride(_) => {}
                        PickerPurpose::Artwork(game_id, kind) => {
                            view.submit(
                                Command::ImportArtwork {
                                    game_id,
                                    kind,
                                    path,
                                },
                                cx,
                            );
                        }
                        PickerPurpose::SteamDirectory => {
                            view.submit(
                                Command::SetSteamDirectory {
                                    directory: Some(path),
                                },
                                cx,
                            );
                        }
                    }
                    cx.notify();
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    view.notice = Some(error.to_string());
                    view.jobs_open = true;
                    cx.notify();
                }
                Err(_) => {
                    view.notice = Some("The file picker could not be opened".into());
                    view.jobs_open = true;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn save_launch(&mut self, cx: &mut Context<Self>) {
        let Some(game_id) = self.selected.clone() else {
            return;
        };
        let executable = self.executable.read(cx).value().trim().to_string();
        let arguments = self
            .arguments
            .read(cx)
            .value()
            .lines()
            .map(String::from)
            .collect::<Vec<_>>();
        let directory = self.working_directory.read(cx).value().trim().to_string();
        if executable.is_empty() && (!arguments.is_empty() || !directory.is_empty()) {
            self.notice = Some("Choose an executable to use custom arguments or a working directory. Steam's default launch uses the options configured in Steam.".into());
            self.jobs_open = true;
            cx.notify();
            return;
        }
        let launch = if executable.is_empty() {
            None
        } else {
            Some(ExecutableLaunch {
                executable: PathBuf::from(executable),
                arguments,
                working_directory: if directory.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(directory))
                },
            })
        };
        self.submit(Command::SetLaunchOverride { game_id, launch }, cx);
    }

    fn save_metadata(&mut self, cx: &mut Context<Self>) {
        if let Some(game_id) = self.selected.clone() {
            let metadata = UserMetadata {
                title: Some(self.title.read(cx).value().to_string()),
                description: Some(self.description.read(cx).value().to_string()),
                developer: Some(self.developer.read(cx).value().to_string()),
                genres: Some(self.genres.read(cx).value().to_string()),
            };
            self.submit(Command::UpdateMetadata { game_id, metadata }, cx);
        }
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = false;
        self.jobs_open = false;
        self.ensure_selection(window, cx);
        self.search.update(cx, |field, cx| field.focus(window, cx));
        cx.notify();
    }
    fn refresh(&mut self, _: &RefreshLibrary, _: &mut Window, cx: &mut Context<Self>) {
        self.submit(Command::RefreshLibrary, cx);
    }
    fn dismiss(&mut self, _: &DismissPanel, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = false;
        self.jobs_open = false;
        self.detail_tab = DetailTab::Overview;
        self.cancel_artwork();
        self.ensure_selection(window, cx);
        self.focus.focus(window);
        cx.notify();
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = !self.settings_open;
        self.jobs_open = false;
        if !self.settings_open {
            self.ensure_selection(window, cx);
        }
        self.focus.focus(window);
        cx.notify();
    }

    fn launch_selected(&mut self, _: &LaunchSelected, _: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_open
            && !self.jobs_open
            && self.detail_tab == DetailTab::Overview
            && let Some(game_id) = self.selected.clone()
        {
            self.submit(Command::LaunchGame { game_id }, cx);
        }
    }

    fn select_next(&mut self, _: &SelectNext, window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(true, window, cx);
    }

    fn select_previous(&mut self, _: &SelectPrevious, window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(false, window, cx);
    }

    fn move_selection(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open || self.jobs_open || self.detail_tab != DetailTab::Overview {
            return;
        }
        let games = self.displayed_games();
        if games.is_empty() {
            return;
        }
        let current = games
            .iter()
            .position(|game| self.selected.as_ref() == Some(&game.id))
            .unwrap_or(0);
        let index = if self.screen == StartScreen::Recents {
            if forward {
                (current + 1) % games.len()
            } else {
                (current + games.len() - 1) % games.len()
            }
        } else if forward {
            (current + 1).min(games.len() - 1)
        } else {
            current.saturating_sub(1)
        };
        let id = games[index].id.clone();
        self.select_game(id, window, cx);
        if self.screen == StartScreen::Library {
            self.list_scroll
                .scroll_to_item(index / self.library_columns(window), ScrollStrategy::Center);
        }
        self.focus.focus(window);
    }

    fn switch_screen(&mut self, screen: StartScreen, window: &mut Window, cx: &mut Context<Self>) {
        self.screen = screen;
        self.settings_open = false;
        self.jobs_open = false;
        self.detail_tab = DetailTab::Overview;
        self.cancel_artwork();
        if screen == StartScreen::Recents && self.source_filter.take().is_some() {
            self.filter(cx);
        }
        self.ensure_selection(window, cx);
        self.focus.focus(window);
        cx.notify();
    }

    fn library_columns(&self, window: &Window) -> usize {
        let width = window.viewport_size().width
            - px(460.)
            - if self.sources_open { px(172.) } else { px(0.) };
        if width >= px(650.) { 2 } else { 1 }
    }

    fn set_appearance(
        &mut self,
        settings: AppearanceSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .submit(Command::SetAppearance { settings }, cx)
            .is_some()
        {
            if self.appearance.start_screen != settings.start_screen {
                self.screen = settings.start_screen;
                self.detail_tab = DetailTab::Overview;
                self.cancel_artwork();
            }
            self.appearance = settings;
            appearance::apply(settings.mode, Some(window), cx);
            cx.notify();
        }
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let installed = self
            .snapshot
            .games
            .iter()
            .filter(|game| game.available)
            .count();
        let unavailable = self.snapshot.games.len() - installed;
        let favourites = self
            .snapshot
            .games
            .iter()
            .filter(|game| game.favourite)
            .count();
        let mut sources = BTreeMap::from([("steam".to_string(), 0), ("manual".to_string(), 0)]);
        for game in self.snapshot.games.iter().filter(|game| game.available) {
            *sources.entry(game.provider_id.clone()).or_insert(0) += 1;
        }
        div()
            .w(px(168.))
            .flex_shrink_0()
            .h_full()
            .bg(cx.theme().muted.opacity(0.4))
            .border_r_1()
            .border_color(cx.theme().border)
            .flex()
            .flex_col()
            .p_4()
            .gap_2()
            .child(eyebrow("LIBRARY"))
            .child(
                Button::new("all-games")
                    .ghost()
                    .label(format!("Installed   {installed}"))
                    .selected(
                        self.library_filter == LibraryFilter::Installed
                            && self.source_filter.is_none()
                            && !self.settings_open,
                    )
                    .w_full()
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.library_filter = LibraryFilter::Installed;
                        view.source_filter = None;
                        view.settings_open = false;
                        view.filter(cx);
                        view.ensure_selection(window, cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("favourites")
                    .ghost()
                    .label(format!("Favourites   {favourites}"))
                    .selected(self.library_filter == LibraryFilter::Favourites)
                    .w_full()
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.library_filter = LibraryFilter::Favourites;
                        view.source_filter = None;
                        view.settings_open = false;
                        view.filter(cx);
                        view.ensure_selection(window, cx);
                        cx.notify();
                    })),
            )
            .child(div().mt_6().child(eyebrow("SOURCES")))
            .children(sources.into_iter().map(|(source, count)| {
                let name = self
                    .snapshot
                    .providers
                    .iter()
                    .find(|provider| provider.id == source)
                    .map(|provider| provider.name.clone())
                    .unwrap_or_else(|| {
                        if source == "manual" {
                            "Manually added".into()
                        } else {
                            source.clone()
                        }
                    });
                Button::new(SharedString::from(format!("source-{source}")))
                    .ghost()
                    .label(format!("{name}   {count}"))
                    .selected(self.source_filter.as_ref() == Some(&source))
                    .w_full()
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.source_filter = Some(source.clone());
                        view.library_filter = LibraryFilter::Installed;
                        view.settings_open = false;
                        view.filter(cx);
                        view.ensure_selection(window, cx);
                        cx.notify();
                    }))
            }))
            .when(unavailable > 0, |sidebar| {
                sidebar.child(
                    Button::new("unavailable")
                        .ghost()
                        .label(format!("All saved   {}", installed + unavailable))
                        .selected(self.library_filter == LibraryFilter::All)
                        .w_full()
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.library_filter = LibraryFilter::All;
                            view.source_filter = None;
                            view.settings_open = false;
                            view.filter(cx);
                            view.ensure_selection(window, cx);
                            cx.notify();
                        })),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .mb_4()
                    .child("Your games.\nOne place to start."),
            )
            .child(
                Button::new("settings")
                    .ghost()
                    .label("Settings")
                    .selected(self.settings_open)
                    .w_full()
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.open_settings(&OpenSettings, window, cx)
                    })),
            )
            .into_any_element()
    }

    fn game_row(&self, game: &Game, cx: &mut Context<Self>) -> AnyElement {
        // ponytail: long labels clip to one line; restore ellipses after GPUI fixes width-sensitive text caching.
        let id = game.id.clone();
        let selected = self.selected.as_ref() == Some(&id);
        let cover = game
            .artwork
            .get(&ArtworkKind::Background)
            .or_else(|| game.artwork.get(&ArtworkKind::Cover))
            .or_else(|| game.artwork.get(&ArtworkKind::Icon));
        div()
            .id(SharedString::from(id.0.clone()))
            .flex_1()
            .min_w_0()
            .h(px(84.))
            .flex()
            .items_center()
            .gap_3()
            .px_2()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.6))
            .border_l_2()
            .border_color(if selected {
                cx.theme().primary
            } else {
                cx.theme().border.opacity(0.4)
            })
            .rounded_md()
            .cursor_pointer()
            .when(selected, |row| row.bg(cx.theme().accent))
            .hover(|style| style.bg(cx.theme().muted))
            .child(
                div()
                    .w(px(116.))
                    .h(px(64.))
                    .flex_shrink_0()
                    .rounded(px(6.))
                    .overflow_hidden()
                    .bg(cx.theme().muted)
                    .child(match cover {
                        Some(asset) => image(asset.thumbnail_path.clone(), ObjectFit::Cover)
                            .rounded(px(6.))
                            .into_any_element(),
                        None => placeholder("").into_any_element(),
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_clamp(1)
                            .overflow_hidden()
                            .child(game.title().to_string()),
                    )
                    .child(hint(&last_played(game))),
            )
            .when(game.favourite, |row| {
                row.child(Icon::new(IconName::Star).small().text_color(rgb(ACCENT)))
            })
            .when(selected, |row| {
                row.child(
                    Icon::new(IconName::ChevronRight)
                        .small()
                        .text_color(rgb(ACCENT)),
                )
            })
            .on_click(cx.listener(move |view, _, window, cx| {
                view.select_game(id.clone(), window, cx);
                view.focus.focus(window);
            }))
            .into_any_element()
    }

    fn library(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let columns = self.library_columns(window);
        let count = self.visible.len();
        let rows = count.div_ceil(columns);
        let content = if !self.snapshot.ready {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .when(self.snapshot.initialization_error.is_none(), |view| {
                    view.child(Spinner::new())
                })
                .child(
                    self.snapshot
                        .initialization_error
                        .clone()
                        .unwrap_or_else(|| "Opening your cached library…".into()),
                )
                .into_any_element()
        } else if count == 0 {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .px_8()
                .child(
                    div()
                        .w(px(80.))
                        .h(px(100.))
                        .rounded(px(7.))
                        .border_1()
                        .border_color(rgb(ACCENT))
                        .bg(cx.theme().muted)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().w_8().h_1().bg(rgb(ACCENT))),
                )
                .child(div().text_2xl().child(if self.snapshot.games.is_empty() {
                    "Your next game starts here."
                } else {
                    "No games in this view"
                }))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(if self.snapshot.games.is_empty() {
                            "Steam games appear automatically. Add any other game yourself."
                        } else {
                            "Try another source or clear your search."
                        }),
                )
                .child(
                    Button::new("empty-add")
                        .primary()
                        .label("Add a game")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.pick(PickerPurpose::AddGame, window, cx)
                        })),
                )
                .into_any_element()
        } else {
            uniform_list(
                "game-list",
                rows,
                cx.processor(move |view, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|row| {
                            let mut line = div().flex().gap_3().h(px(88.)).w_full();
                            for column in 0..columns {
                                if let Some(game) = view.visible.get(row * columns + column) {
                                    line = line.child(view.game_row(game, cx));
                                } else {
                                    line = line.child(div().flex_1().min_w_0());
                                }
                            }
                            line
                        })
                        .collect()
                }),
            )
            .track_scroll(self.list_scroll.clone())
            .flex_1()
            .min_h_0()
            .w_full()
            .into_any_element()
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(40.))
                    .flex_shrink_0()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .p_1()
                            .rounded_lg()
                            .bg(cx.theme().muted.opacity(0.7))
                            .children(
                                [
                                    (
                                        LibraryFilter::Installed,
                                        "Installed",
                                        self.snapshot
                                            .games
                                            .iter()
                                            .filter(|game| game.available)
                                            .count(),
                                    ),
                                    (LibraryFilter::All, "All games", self.snapshot.games.len()),
                                    (
                                        LibraryFilter::Favourites,
                                        "Favourites",
                                        self.snapshot
                                            .games
                                            .iter()
                                            .filter(|game| game.favourite)
                                            .count(),
                                    ),
                                ]
                                .into_iter()
                                .map(|(filter, label, count)| {
                                    Button::new(label)
                                        .ghost()
                                        .small()
                                        .label(format!("{label}  {count}"))
                                        .selected(self.library_filter == filter)
                                        .when(self.library_filter == filter, |button| {
                                            button
                                                .bg(cx.theme().secondary)
                                                .border_b_2()
                                                .border_color(rgb(ACCENT))
                                        })
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.library_filter = filter;
                                            view.source_filter = None;
                                            view.filter(cx);
                                            view.ensure_selection(window, cx);
                                            cx.notify();
                                        }))
                                }),
                            ),
                    )
                    .child(
                        Button::new("sort-library")
                            .ghost()
                            .small()
                            .label(if self.sort_recent { "Recent" } else { "A–Z" })
                            .icon(IconName::ChevronsUpDown)
                            .tooltip("Switch between recent launches and title order")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.sort_recent = !view.sort_recent;
                                view.filter(cx);
                                cx.notify();
                            })),
                    ),
            )
            .when_some(self.source_filter.clone(), |library, source| {
                library.child(hint(&format!("Source: {source}")))
            })
            .child(content)
            .into_any_element()
    }

    fn navigation_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.weak_entity();
        let selected_filter = self.library_filter;
        div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_3()
            .px_5()
            .pb_4()
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.search)
                        .w_full()
                        .flex_shrink_0()
                        .bg(cx.theme().secondary.opacity(0.8))
                        .large()
                        .prefix(Icon::new(IconName::Search).text_color(cx.theme().muted_foreground))
                        .suffix(keycap(if cfg!(target_os = "macos") {
                            "⌘ F"
                        } else {
                            "Ctrl F"
                        }))
                        .cleanable(true),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .p_1()
                    .rounded_lg()
                    .bg(cx.theme().muted.opacity(0.6))
                    .children(
                        [StartScreen::Recents, StartScreen::Library]
                            .into_iter()
                            .map(|screen| {
                                Button::new(SharedString::from(format!("view-{}", screen.label())))
                                    .ghost()
                                    .label(screen.label())
                                    .selected(self.screen == screen)
                                    .border_0()
                                    .when(self.screen == screen, |button| {
                                        button
                                            .bg(cx.theme().secondary)
                                            .border_b_2()
                                            .border_color(rgb(ACCENT))
                                    })
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.switch_screen(screen, window, cx)
                                    }))
                            }),
                    ),
            )
            .when(self.screen == StartScreen::Recents, |bar| {
                bar.child(
                    Button::new("recents-filter")
                        .ghost()
                        .label(selected_filter.label())
                        .dropdown_caret(true)
                        .dropdown_menu(move |mut menu, _, _| {
                            for filter in [
                                LibraryFilter::Installed,
                                LibraryFilter::All,
                                LibraryFilter::Favourites,
                            ] {
                                let view = view.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(filter.label())
                                        .checked(filter == selected_filter)
                                        .on_click(move |_, window, cx| {
                                            let _ = view.update(cx, |view, cx| {
                                                view.library_filter = filter;
                                                view.source_filter = None;
                                                view.filter(cx);
                                                view.ensure_selection(window, cx);
                                                view.focus.focus(window);
                                                cx.notify();
                                            });
                                        }),
                                );
                            }
                            menu
                        }),
                )
            })
            .into_any_element()
    }

    fn start_button(&self, game: &Game, cx: &mut Context<Self>) -> Button {
        Button::new("start-game")
            .primary()
            .w(px(172.))
            .h(px(44.))
            .text_color(cx.theme().primary_foreground)
            .text_size(px(19.))
            .font_weight(FontWeight::BOLD)
            .label("▶  Start game")
            .disabled(!game.available && game.launch_override.is_none())
            .on_click(cx.listener(|view, _, _, cx| {
                if let Some(game_id) = view.selected.clone() {
                    view.submit(Command::LaunchGame { game_id }, cx);
                }
            }))
    }

    fn recent_card(&self, game: &Game, cx: &mut Context<Self>) -> AnyElement {
        let id = game.id.clone();
        let artwork = game
            .artwork
            .get(&ArtworkKind::Background)
            .or_else(|| game.artwork.get(&ArtworkKind::Cover))
            .or_else(|| game.artwork.get(&ArtworkKind::Icon));
        let selected = self.selected.as_ref() == Some(&id);
        div()
            .id(SharedString::from(format!("recent-{}", id.0)))
            .flex_1()
            .min_w_0()
            .max_w(px(280.))
            .flex()
            .flex_col()
            .gap_2()
            .cursor_pointer()
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(120.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .border_2()
                    .border_color(if selected {
                        cx.theme().primary
                    } else {
                        cx.theme().border.opacity(0.4)
                    })
                    .child(match artwork {
                        Some(asset) => image(asset.thumbnail_path.clone(), ObjectFit::Cover)
                            .rounded(px(6.))
                            .into_any_element(),
                        None => placeholder("").into_any_element(),
                    }),
            )
            .child(
                div()
                    .w_full()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_clamp(1)
                    .child(game.title().to_string()),
            )
            .child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .line_clamp(1)
                    .child(last_played(game)),
            )
            .on_click(cx.listener(move |view, _, window, cx| {
                view.select_game(id.clone(), window, cx);
                view.focus.focus(window);
            }))
            .into_any_element()
    }

    fn recents(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(game) = self.selected_game().filter(|_| self.snapshot.ready) else {
            return div()
                .size_full()
                .p_5()
                .child(self.library(window, cx))
                .into_any_element();
        };
        let games = self.recent_games();
        let artwork = game
            .artwork
            .get(&ArtworkKind::Background)
            .or_else(|| game.artwork.get(&ArtworkKind::Cover));
        let background = cx.theme().background.alpha(1.);
        let has_history = games.iter().any(|game| game.last_launched.is_some());
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .relative()
                    .w_full()
                    .flex_1()
                    .min_h(px(220.))
                    .overflow_hidden()
                    .bg(background)
                    .when_some(artwork, |hero, asset| {
                        let original = asset.image_path.clone();
                        let path = if self.appearance.dither_recents {
                            asset.recents_dithered_path()
                        } else {
                            asset.recents_path()
                        };
                        hero.child(
                            div().absolute().top_0().left_0().size_full().child(
                                img(path)
                                    .size_full()
                                    .object_fit(ObjectFit::Cover)
                                    .with_fallback(move || {
                                        image(original.clone(), ObjectFit::Cover)
                                            .opacity(0.2)
                                            .into_any_element()
                                    }),
                            ),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .bg(linear_gradient(
                                180.,
                                linear_color_stop(background.alpha(0.), 0.86),
                                linear_color_stop(background, 1.),
                            )),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(32.))
                            .right(px(32.))
                            .bottom_6()
                            .flex()
                            .items_end()
                            .justify_between()
                            .gap_4()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .max_w(relative(0.6))
                                    .flex()
                                    .flex_col()
                                    .items_start()
                                    .gap_3()
                                    .child(eyebrow(if game.last_launched.is_some() {
                                        "JUMP BACK IN"
                                    } else {
                                        "YOUR NEXT GAME"
                                    }))
                                    .child(
                                        div()
                                            .w_full()
                                            .text_size(px(46.))
                                            .line_height(relative(1.08))
                                            .font_weight(FontWeight::BOLD)
                                            .line_clamp(2)
                                            .child(game.title().to_string()),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .text_sm()
                                            .child(div().size(px(8.)).rounded_full().bg(rgb(
                                                if game.available { 0x53bf78 } else { 0xd6a15b },
                                            )))
                                            .child(if game.available {
                                                "Ready to play"
                                            } else {
                                                "Installation unavailable"
                                            })
                                            .when(game.last_launched.is_some(), |row| {
                                                row.child(
                                                    div()
                                                        .w(px(1.))
                                                        .h_3()
                                                        .mx_2()
                                                        .bg(cx.theme().border),
                                                )
                                                .child(
                                                    div()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(last_played(&game)),
                                                )
                                            }),
                                    )
                                    .child(self.start_button(&game, cx)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Button::new("edit-recent-game")
                                            .ghost()
                                            .small()
                                            .icon(IconName::Ellipsis)
                                            .tooltip("Edit metadata, artwork or launch options")
                                            .bg(background.opacity(0.85))
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.switch_screen(
                                                    StartScreen::Library,
                                                    window,
                                                    cx,
                                                );
                                                view.detail_tab = DetailTab::Metadata;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("previous-recent")
                                            .ghost()
                                            .size(px(30.))
                                            .rounded(px(15.))
                                            .icon(IconName::ChevronLeft)
                                            .tooltip("Previous game")
                                            .bg(background.opacity(0.85))
                                            .disabled(games.len() < 2)
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.move_selection(false, window, cx)
                                            })),
                                    )
                                    .children(games.iter().enumerate().map(|(index, game)| {
                                        let id = game.id.clone();
                                        div()
                                            .id(("recent-position", index))
                                            .size(px(24.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .child(div().size(px(6.)).rounded_full().bg(
                                                if self.selected.as_ref() == Some(&id) {
                                                    cx.theme().primary
                                                } else {
                                                    cx.theme().muted_foreground.opacity(0.35)
                                                },
                                            ))
                                            .on_click(cx.listener(move |view, _, window, cx| {
                                                view.select_game(id.clone(), window, cx);
                                                view.focus.focus(window);
                                            }))
                                    }))
                                    .child(
                                        Button::new("next-recent")
                                            .ghost()
                                            .size(px(30.))
                                            .rounded(px(15.))
                                            .icon(IconName::ChevronRight)
                                            .tooltip("Next game")
                                            .bg(background.opacity(0.85))
                                            .disabled(games.len() < 2)
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.move_selection(true, window, cx)
                                            })),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .px(px(32.))
                    .pb_5()
                    .pt_2()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .bg(background)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(
                                if has_history {
                                    "Recently played"
                                } else {
                                    "Ready to play"
                                },
                            ))
                            .child(
                                Button::new("recents-view-library")
                                    .ghost()
                                    .small()
                                    .label("View library")
                                    .icon(IconName::ArrowRight)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.switch_screen(StartScreen::Library, window, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .gap_3()
                            .children(games.into_iter().map(|game| self.recent_card(game, cx))),
                    ),
            )
            .into_any_element()
    }

    fn activity_button(&self, cx: &mut Context<Self>) -> Button {
        let active = self
            .snapshot
            .jobs
            .iter()
            .filter(|job| job.state.active())
            .collect::<Vec<_>>();
        let busy = !active.is_empty()
            || (!self.snapshot.ready && self.snapshot.initialization_error.is_none());
        let tooltip = active
            .iter()
            .find(|job| job.state == JobState::Running)
            .or_else(|| active.first())
            .map(|job| job.label.clone())
            .unwrap_or_else(|| "Activity and notifications".into());
        Button::new("background-jobs")
            .ghost()
            .small()
            .icon(IconName::Bell)
            .tooltip(self.notice.clone().unwrap_or(tooltip))
            .selected(self.jobs_open)
            .when(self.notice.is_some(), |button| {
                button.text_color(cx.theme().primary)
            })
            .when(busy, |button| {
                button.child(Spinner::new().small().color(cx.theme().primary))
            })
            .when(self.notice.is_some() || !active.is_empty(), |button| {
                button.child(
                    div()
                        .text_xs()
                        .child((active.len() + usize::from(self.notice.is_some())).to_string()),
                )
            })
            .on_click(cx.listener(|view, _, window, cx| {
                view.jobs_open = !view.jobs_open;
                view.focus.focus(window);
                cx.notify();
            }))
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new()
            .h(px(52.))
            .pl(px(if cfg!(target_os = "macos") { 100. } else { 18. }))
            .pr_4()
            .child(
                div()
                    .w_full()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        Button::new("toggle-sources")
                            .ghost()
                            .small()
                            .icon(IconName::PanelLeft)
                            .tooltip("Show or hide library sources")
                            .selected(self.sources_open)
                            .on_click(cx.listener(|view, _, window, cx| {
                                if view.screen == StartScreen::Recents {
                                    view.switch_screen(StartScreen::Library, window, cx);
                                }
                                view.sources_open = !view.sources_open;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_xl().text_color(rgb(ACCENT)).child("▶"))
                            .child(
                                div().text_lg().font_weight(FontWeight::SEMIBOLD).child(
                                    if self
                                        .snapshot
                                        .providers
                                        .iter()
                                        .any(|provider| provider.id == "example")
                                    {
                                        "startpoint · Example library"
                                    } else {
                                        "startpoint"
                                    },
                                ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new("refresh-library")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Redo2)
                                    .tooltip("Refresh installed games")
                                    .disabled(!self.snapshot.ready)
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.submit(Command::RefreshLibrary, cx);
                                    })),
                            )
                            .child(
                                Button::new("add-game")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Plus)
                                    .tooltip("Add a game")
                                    .disabled(!self.snapshot.ready)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.pick(PickerPurpose::AddGame, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("open-settings")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Settings)
                                    .tooltip("Settings")
                                    .selected(self.settings_open)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.open_settings(&OpenSettings, window, cx)
                                    })),
                            )
                            .child(self.activity_button(cx)),
                    ),
            )
    }

    fn hero(&self, game: &Game, cx: &mut Context<Self>) -> AnyElement {
        let artwork = game
            .artwork
            .get(&ArtworkKind::Cover)
            .or_else(|| game.artwork.get(&ArtworkKind::Background));
        let metadata = game.effective_metadata();
        let favourite_id = game.id.clone();
        let favourite = game.favourite;
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded(px(12.))
            .bg(rgb(0x11191b))
            .text_color(rgb(0xffffff))
            .child(
                div().absolute().size_full().child(match artwork {
                    Some(asset) => image(asset.image_path.clone(), ObjectFit::Cover)
                        .rounded(px(12.))
                        .into_any_element(),
                    None => placeholder(game.title()).into_any_element(),
                }),
            )
            .when_some(
                artwork.filter(|_| self.appearance.dither_artwork),
                |hero, asset| {
                    hero.child(
                        div().absolute().size_full().opacity(0.16).child(
                            img(asset.dithered_path())
                                .size_full()
                                .object_fit(ObjectFit::Cover)
                                .rounded(px(12.))
                                .with_fallback(|| div().size_full().into_any_element()),
                        ),
                    )
                },
            )
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(12.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(rgba(0x10181a00), 0.25),
                        linear_color_stop(rgba(0x10181aff), 0.88),
                    )),
            )
            .when_some(game.artwork.get(&ArtworkKind::Logo), |hero, logo| {
                hero.child(
                    div()
                        .absolute()
                        .top(px(56.))
                        .left_5()
                        .right_5()
                        .h(px(64.))
                        .child(image(logo.image_path.clone(), ObjectFit::Contain)),
                )
            })
            .child(
                div()
                    .absolute()
                    .top_4()
                    .left_4()
                    .right_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_lg()
                            .bg(rgba(0x10181a99))
                            .text_xs()
                            .child(source_name(&game.provider_id).to_string()),
                    )
                    .child(
                        Button::new("hero-favourite")
                            .ghost()
                            .small()
                            .icon(if favourite {
                                IconName::Star
                            } else {
                                IconName::StarOff
                            })
                            .text_color(rgb(if favourite { 0xffa269 } else { 0xffffff }))
                            .bg(rgba(0x10181a99))
                            .tooltip(if favourite {
                                "Remove from favourites"
                            } else {
                                "Add to favourites"
                            })
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.submit(
                                    Command::SetFavourite {
                                        game_id: favourite_id.clone(),
                                        favourite: !favourite,
                                    },
                                    cx,
                                );
                            })),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .p_5()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_3xl()
                                    .font_weight(FontWeight::BOLD)
                                    .child(game.title().to_string()),
                            )
                            .child(
                                Button::new("edit-game")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Ellipsis)
                                    .text_color(rgb(0xffffff))
                                    .tooltip("Edit metadata, artwork or launch options")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.detail_tab = DetailTab::Metadata;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().size(px(9.)).rounded_full().bg(rgb(if game.available {
                                0x61d18a
                            } else {
                                0xeaa65a
                            })))
                            .child(div().text_xs().font_weight(FontWeight::MEDIUM).child(
                                if game.available {
                                    "READY TO PLAY"
                                } else {
                                    "INSTALLATION UNAVAILABLE"
                                },
                            )),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xb1b8b9))
                            .child(last_played(game)),
                    )
                    .when_some(metadata.description, |hero, description| {
                        hero.child(
                            div()
                                .w_full()
                                .max_h(px(56.))
                                .overflow_hidden()
                                .text_sm()
                                .text_color(rgb(0xd0d5d5))
                                .child(description),
                        )
                    })
                    .when_some(metadata.developer, |hero, developer| {
                        hero.child(div().text_xs().text_color(rgb(0xb1b8b9)).child(developer))
                    })
                    .child(self.start_button(game, cx))
                    .when(artwork.is_none(), |hero| {
                        hero.child(
                            Button::new("find-cover")
                                .ghost()
                                .small()
                                .label("Choose artwork…")
                                .text_color(rgb(0xffffff))
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.detail_tab = DetailTab::Artwork;
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    fn details(&self, game: &Game, cx: &mut Context<Self>) -> AnyElement {
        if self.detail_tab == DetailTab::Overview {
            return div()
                .w(px(380.))
                .flex_shrink_0()
                .h_full()
                .child(self.hero(game, cx))
                .into_any_element();
        }
        let game_id = game.id.clone();
        let favourite_id = game_id.clone();
        let favourite = game.favourite;
        let content = match self.detail_tab {
            DetailTab::Overview => self.hero(game, cx),
            DetailTab::Metadata => div().w_full().flex().flex_col().gap_4()
                .child(field("Title", &self.title, cx)).child(field("Description", &self.description, cx)).child(field("Developer", &self.developer, cx)).child(field("Genres", &self.genres, cx))
                .child(hint("Your edits are preserved when the library is refreshed. Empty fields restore discovered values."))
                .child(Button::new("save-metadata").primary().label("Save metadata").on_click(cx.listener(|view, _, _, cx| view.save_metadata(cx))))
                .when(game.provider_id == "manual", |content| content.child(Button::new("remove-manual-game").ghost().label("Remove from library")
                    .on_click(cx.listener(|view, _, _, cx| {
                        if let Some(game_id) = view.selected.take() { view.submit(Command::RemoveManualGame { game_id }, cx); }
                    }))))
                .into_any_element(),
            DetailTab::Launch => div().w_full().flex().flex_col().gap_4()
                .child(eyebrow("OPTIONAL EXECUTABLE OVERRIDE"))
                .child(hint(&format!("{}\n{}", source_name(&game.provider_id), game.install_directory.display())))
                .child(hint("The store launcher is used by default. An executable override can bypass its updates, DRM or launch configuration."))
                .child(field("Executable / application bundle", &self.executable, cx))
                .child(Button::new("choose-executable").label("Choose executable…").on_click(cx.listener(move |view, _, window, cx| view.pick(PickerPurpose::LaunchOverride(game_id.clone()), window, cx))))
                .child(field("Arguments — one per line", &self.arguments, cx))
                .child(hint("Each line is passed as one exact argument. Spaces and Windows paths are preserved. No quoting or shell expansion."))
                .child(field("Working directory", &self.working_directory, cx))
                .when(cfg!(target_os = "macos"), |view| view.child(hint("For a custom working directory, select the executable inside Contents/MacOS instead of the .app bundle.")))
                .child(Button::new("save-launch").primary().label("Save launch configuration").on_click(cx.listener(|view, _, _, cx| view.save_launch(cx))))
                .child(Button::new("reset-launch").ghost().label("Restore launcher default").on_click(cx.listener(|view, _, window, cx| {
                    if let Some(game_id) = view.selected.clone() {
                        view.submit(Command::SetLaunchOverride { game_id, launch: None }, cx);
                        set_value(&view.executable, "", window, cx); set_value(&view.arguments, "", window, cx); set_value(&view.working_directory, "", window, cx);
                    }
                }))).into_any_element(),
            DetailTab::Artwork => self.artwork_picker(game, cx),
        };
        div()
            .w(px(380.))
            .flex_shrink_0()
            .h_full()
            .bg(cx.theme().secondary.opacity(0.7))
            .rounded(px(12.))
            .border_1()
            .border_color(cx.theme().border)
            .flex()
            .flex_col()
            .p_5()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(eyebrow("GAME DETAILS"))
                    .child(
                        Button::new("close-details")
                            .ghost()
                            .small()
                            .label("Done")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.detail_tab = DetailTab::Overview;
                                view.cancel_artwork();
                                cx.notify();
                            })),
                    ),
            )
            .child(div().text_2xl().child(game.title().to_string()))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("details-play")
                            .primary()
                            .label("Play game")
                            .disabled(!game.available && game.launch_override.is_none())
                            .on_click(cx.listener(|view, _, _, cx| {
                                if let Some(game_id) = view.selected.clone() {
                                    view.submit(Command::LaunchGame { game_id }, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("details-favourite")
                            .label(if favourite {
                                "Unfavourite"
                            } else {
                                "Favourite"
                            })
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.submit(
                                    Command::SetFavourite {
                                        game_id: favourite_id.clone(),
                                        favourite: !favourite,
                                    },
                                    cx,
                                );
                            })),
                    ),
            )
            .child(
                div().w_full().flex().gap_1().children(
                    [
                        (DetailTab::Overview, "Overview"),
                        (DetailTab::Metadata, "Metadata"),
                        (DetailTab::Artwork, "Images"),
                        (DetailTab::Launch, "Launch"),
                    ]
                    .into_iter()
                    .map(|(tab, label)| {
                        Button::new(label)
                            .ghost()
                            .small()
                            .label(label)
                            .selected(self.detail_tab == tab)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.detail_tab = tab;
                                cx.notify();
                            }))
                    }),
                ),
            )
            .child(
                div()
                    .id("details-scroll")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(content),
            )
            .into_any_element()
    }

    fn artwork_picker(&self, game: &Game, cx: &mut Context<Self>) -> AnyElement {
        let key_configured = self
            .snapshot
            .settings
            .configured_credentials
            .contains("steamgriddb");
        let game_id = game.id.clone();
        let kind = self.artwork_kind;
        div().w_full().flex().flex_col().gap_4()
            .child(div().flex().flex_wrap().gap_1().children([ArtworkKind::Cover, ArtworkKind::Background, ArtworkKind::Logo, ArtworkKind::Icon].into_iter().map(|kind|
                Button::new(kind.label()).ghost().small().label(kind.label()).selected(kind == self.artwork_kind).on_click(cx.listener(move |view, _, _, cx| {
                    view.cancel_artwork(); view.artwork_kind = kind; view.artwork_images.clear(); view.artwork_message = None;
                    if view.snapshot.settings.configured_credentials.contains("steamgriddb")
                        && (view.artwork_match.is_some() || view.selected_game().is_some_and(|game| matches!(game.provider_id.as_str(), "steam" | "example"))) { view.load_artwork(None, cx); }
                    cx.notify();
                })))))
            .child(Button::new("import-artwork").label("Upload image…").on_click(cx.listener(move |view, _, window, cx| view.pick(PickerPurpose::Artwork(game_id.clone(), kind), window, cx))))
            .child(hint("PNG, JPEG or WebP · Up to 20 MB. Your original file is never modified."))
            .child(eyebrow("SAVED IMAGES"))
            .child(div().flex().flex_wrap().gap_3().children(game.artwork_history.iter().filter(|asset| asset.kind == kind).map(|asset| self.saved_image(game, asset, cx))))
            .child(div().mt_3().child(eyebrow("STEAMGRIDDB")))
            .child(self.credential_status(cx))
            .when_some(self.artwork_match.clone(), |view, matched| view.child(hint(&format!("Selected match: {} · #{} · shared by all image categories", matched.name, matched.id))))
            .when(!key_configured, |view| view.child(hint("Add your own API key in Settings to browse community artwork. Uploaded and saved images work offline.")))
            .child(Input::new(&self.artwork_search).w_full().flex_shrink_0().bg(cx.theme().secondary).disabled(!key_configured))
            .child(div().flex().gap_2()
                .child(Button::new("search-artwork-games").label("Find game").disabled(!key_configured).on_click(cx.listener(|view, _, _, cx| view.search_artwork(cx))))
                .child(Button::new("load-artwork").label("Browse images").disabled(!key_configured).on_click(cx.listener(|view, _, _, cx| view.load_artwork(None, cx)))))
            .when(self.artwork_job.is_some(), |view| view.child(div().flex().items_center().gap_2().child(Spinner::new().small()).child(hint("Fetching in the background…"))))
            .when_some(self.artwork_message.clone(), |view, message| view.child(hint(&message)))
            .children(self.artwork_games.iter().map(|game| {
                let matched = game.clone();
                Button::new(SharedString::from(format!("match-{}", game.id))).label(game.name.clone()).w_full().on_click(cx.listener(move |view, _, _, cx| view.choose_artwork_game(matched.clone(), cx)))
            }))
            .child(div().flex().flex_wrap().gap_3().children(self.artwork_images.iter().enumerate().map(|(index, candidate)| {
                let candidate = candidate.clone(); let selected = candidate.clone();
                div().w(px(140.)).flex().flex_col().gap_2()
                    .child(div().h(px(160.)).rounded_md().overflow_hidden().bg(cx.theme().muted).child(match candidate.preview_path {
                        Some(path) => image(path, ObjectFit::Contain).into_any_element(), None => placeholder(if self.artwork_job.is_some() { "Loading preview…" } else { "Preview unavailable" }).into_any_element(),
                    }))
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).line_clamp(1).child(format!("By {}", candidate.author)))
                    .child(hint(&format!("{} × {} · score {}", candidate.width, candidate.height, candidate.score)))
                    .child(Button::new(("select-remote", index)).small().label("Use image").on_click(cx.listener(move |view, _, _, cx| {
                         if let Some(game_id) = view.selected.clone() { view.cancel_artwork(); view.submit(Command::SelectArtwork { game_id, candidate: selected.clone() }, cx); }
                    })))
            }))).into_any_element()
    }

    fn saved_image(&self, game: &Game, asset: &ArtworkAsset, cx: &mut Context<Self>) -> AnyElement {
        let selected = game
            .artwork
            .get(&asset.kind)
            .is_some_and(|chosen| chosen.id == asset.id);
        let asset_id = asset.id.clone();
        div()
            .w(px(140.))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .h(px(140.))
                    .rounded_md()
                    .overflow_hidden()
                    .border_1()
                    .border_color(if selected {
                        cx.theme().primary
                    } else {
                        cx.theme().border
                    })
                    .child(image(asset.thumbnail_path.clone(), ObjectFit::Contain)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .line_clamp(1)
                    .overflow_hidden()
                    .child(asset.author.clone()),
            )
            .child(
                Button::new(SharedString::from(format!("saved-{}", asset.id)))
                    .small()
                    .label(if selected { "Selected" } else { "Use image" })
                    .disabled(selected)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        if let Some(game_id) = view.selected.clone() {
                            view.submit(
                                Command::SelectLocalArtwork {
                                    game_id,
                                    asset_id: asset_id.clone(),
                                },
                                cx,
                            );
                        }
                    })),
            )
            .into_any_element()
    }

    fn settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let key_configured = self
            .snapshot
            .settings
            .configured_credentials
            .contains("steamgriddb");
        let data_directory = self.snapshot.paths.data_directory.clone();
        let cache_directory = self.snapshot.paths.cache_directory.clone();
        div().id("settings-content").w_full().min_w_0().h_full().overflow_y_scroll().px_8().py_4()
            .child(div().w(px(640.)).flex().flex_col().gap_5()
                .child(div().w_full().flex().items_center().justify_between()
                    .child(div().text_3xl().child("Settings"))
                    .child(Button::new("close-settings").ghost().small().label(format!("Back to {}", self.screen.label())).icon(IconName::ArrowLeft)
                        .on_click(cx.listener(|view, _, window, cx| view.open_settings(&OpenSettings, window, cx)))))
                .child(hint("Local first. A saved SteamGridDB key enables automatic metadata and artwork filling."))
                .child(div().mt_3().child(eyebrow("START SCREEN")))
                .child(div().flex().gap_2().children([StartScreen::Library, StartScreen::Recents].into_iter().map(|start_screen| {
                    Button::new(SharedString::from(format!("start-screen-{}", start_screen.label())))
                        .label(start_screen.label()).selected(self.appearance.start_screen == start_screen)
                        .disabled(!self.snapshot.ready)
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.set_appearance(AppearanceSettings { start_screen, ..view.appearance }, window, cx);
                        }))
                })))
                .child(hint("Choose what opens at startup. Recents shows your five latest Startpoint launches, or ready-to-play games before your first launch. You can switch views using the search-bar tabs."))
                .child(Switch::new("recents-dithering").checked(self.appearance.dither_recents).label("Dither Recents artwork")
                    .disabled(!self.snapshot.ready).on_click(cx.listener(|view, enabled, window, cx| {
                        view.set_appearance(AppearanceSettings { dither_recents: *enabled, ..view.appearance }, window, cx);
                    })))
                .child(hint("Optional fine stippling as the Recents banner fades around the title and into the page. Off by default; original images and cards are unchanged."))
                .child(div().mt_3().child(eyebrow("APPEARANCE")))
                .child(div().flex().gap_2().children([AppearanceMode::System, AppearanceMode::Light, AppearanceMode::Dark].into_iter().map(|mode| {
                    Button::new(mode.label()).label(mode.label()).selected(self.appearance.mode == mode)
                        .icon(match mode { AppearanceMode::System => IconName::Frame, AppearanceMode::Light => IconName::Sun, AppearanceMode::Dark => IconName::Moon })
                        .disabled(!self.snapshot.ready)
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.set_appearance(AppearanceSettings { mode, ..view.appearance }, window, cx);
                        }))
                })))
                .child(hint("System follows your operating system, including automatic appearance changes. Your choice is saved locally."))
                .child(Switch::new("artwork-dithering").checked(self.appearance.dither_artwork).label("Dither Library artwork")
                    .disabled(!self.snapshot.ready).on_click(cx.listener(|view, enabled, window, cx| {
                        view.set_appearance(AppearanceSettings { dither_artwork: *enabled, ..view.appearance }, window, cx);
                    })))
                .child(hint("A subtle ordered-dither texture on the selected artwork in Library. Originals and Library thumbnails are unchanged."))
                .child(div().mt_4().child(eyebrow("STEAMGRIDDB API KEY")))
                .child(self.credential_status(cx))
                .child(hint(if key_configured { "Your saved key is already in use. Choose Replace key… only to change it." } else { "Optional. Saved in Windows Credential Manager or macOS Keychain—not in the database." }))
                .when(!key_configured || self.credential_editing, |view| view.child(Input::new(&self.api_key).w_full().flex_shrink_0().bg(cx.theme().secondary).mask_toggle()))
                .child(div().flex().gap_2()
                    .when(!key_configured || self.credential_editing, |row| row.child(Button::new("save-api-key").primary().label(if key_configured { "Replace saved key" } else { "Save key" }).disabled(!self.snapshot.ready || self.credential_job.is_some()).on_click(cx.listener(|view, _, _, cx| {
                        let credential = view.api_key.read(cx).value().to_string();
                        if credential.trim().is_empty() { view.notice = Some("Enter an API key, or choose Remove key.".into()); view.jobs_open = true; cx.notify(); return; }
                        view.credential_job = view.submit(Command::SetArtworkCredential { provider_id: "steamgriddb".into(), credential: Some(credential) }, cx);
                    }))))
                    .when(key_configured, |row| row
                        .child(Button::new("verify-api-key").label("Verify saved key").disabled(self.credential_job.is_some() || self.snapshot.settings.credential_status.get("steamgriddb") == Some(&CredentialStatus::Checking))
                            .on_click(cx.listener(|view, _, _, cx| { view.submit(Command::VerifyArtworkCredential { provider_id: "steamgriddb".into() }, cx); })))
                        .child(Button::new("edit-api-key").ghost().label(if self.credential_editing { "Cancel replacement" } else { "Replace key…" })
                            .on_click(cx.listener(|view, _, _, cx| { view.credential_editing = !view.credential_editing; cx.notify(); }))))
                    .child(Button::new("remove-api-key").label("Remove key").disabled(!key_configured || self.credential_job.is_some()).on_click(cx.listener(|view, _, _, cx| {
                        view.credential_job = view.submit(Command::SetArtworkCredential { provider_id: "steamgriddb".into(), credential: None }, cx);
                    })))
                    .child(Button::new("get-api-key").ghost().label("Get a key ↗").on_click(|_, _, cx| cx.open_url("https://www.steamgriddb.com/profile/preferences"))))
                .child(hint("Missing covers, high-resolution backgrounds, logos and icons are filled with popular SteamGridDB choices. Steam-linked descriptions, developers and genres come from Steam's catalogue. Your edits and selected images are preserved."))
                .child(div().mt_5().child(eyebrow("STEAM DISCOVERY")))
                .child(hint(&self.snapshot.settings.steam_directory.as_ref().map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|| "Automatic detection".into())))
                .child(div().flex().gap_2()
                    .child(Button::new("choose-steam").label("Choose Steam directory…").on_click(cx.listener(|view, _, window, cx| view.pick(PickerPurpose::SteamDirectory, window, cx))))
                    .child(Button::new("reset-steam").ghost().label("Use automatic detection").on_click(cx.listener(|view, _, _, cx| { view.submit(Command::SetSteamDirectory { directory: None }, cx); }))))
                .child(div().mt_5().child(eyebrow("LOCAL STORAGE")))
                .child(hint(&format!("Library and selected images\n{}\n\nDisposable thumbnails\n{}", data_directory.display(), cache_directory.display())))
                .child(div().flex().gap_2()
                    .child(Button::new("open-data").label("Open data directory").on_click(move |_, _, cx| cx.reveal_path(&data_directory)))
                    .child(Button::new("open-cache").label("Open cache directory").on_click(move |_, _, cx| cx.reveal_path(&cache_directory))))
                .child(div().mt_5().child(eyebrow("APPLICATION SDK")))
                .child(hint("The desktop uses the same versioned application API available to compiled extensions. Discovery and artwork providers can be registered without depending on GPUI. Dynamic plugin loading is not enabled in this version.")))
            .into_any_element()
    }

    fn activity(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .absolute()
            .top(px(52.))
            .right(px(12.))
            .w(px(460.))
            .max_h(px(420.))
            .id("activity-panel")
            .overflow_y_scroll()
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .rounded_md()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(eyebrow("ACTIVITY & NOTIFICATIONS"))
                    .child(
                        Button::new("close-activity")
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .tooltip("Close activity")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.jobs_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .when_some(self.notice.clone(), |panel, notice| {
                panel.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_3()
                        .rounded_md()
                        .bg(cx.theme().accent)
                        .child(div().text_sm().child(notice))
                        .child(
                            Button::new("dismiss-message")
                                .ghost()
                                .small()
                                .label("Dismiss notification")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.notice = None;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when(!self.snapshot.ready, |panel| {
                panel.child(hint(
                    self.snapshot
                        .initialization_error
                        .as_deref()
                        .unwrap_or("Opening your cached library…"),
                ))
            })
            .when(
                self.snapshot.ready && self.snapshot.jobs.is_empty() && self.notice.is_none(),
                |panel| panel.child(hint("No background activity yet.")),
            )
            .children(
                self.snapshot
                    .jobs
                    .iter()
                    .rev()
                    .filter(|job| job.state.active())
                    .chain(
                        self.snapshot
                            .jobs
                            .iter()
                            .rev()
                            .filter(|job| !job.state.active())
                            .take(8),
                    )
                    .map(|job| {
                        let id = job.id;
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_sm().child(job.label.clone()))
                                    .child(hint(
                                        &job.error
                                            .clone()
                                            .unwrap_or_else(|| format!("{:?}", job.state)),
                                    )),
                            )
                            .when(job.state.active(), |row| {
                                row.child(
                                    Button::new(("cancel-job", id))
                                        .ghost()
                                        .small()
                                        .label("Cancel")
                                        .on_click(
                                            cx.listener(move |view, _, _, _| view.api.cancel(id)),
                                        ),
                                )
                            })
                    }),
            )
            .into_any_element()
    }
}

impl Render for LauncherView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if self.settings_open {
            self.settings(cx)
        } else if self.screen == StartScreen::Recents {
            self.recents(window, cx)
        } else {
            let mut content = div()
                .flex()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .h_full()
                .px_5()
                .pb_4()
                .gap_5()
                .when(self.sources_open, |content| content.child(self.sidebar(cx)))
                .child(self.library(window, cx));
            if let Some(game) = self.selected_game() {
                content = content.child(self.details(&game, cx));
            }
            content.into_any_element()
        };
        div()
            .size_full()
            .relative()
            .bg(appearance::surface(cx))
            .text_color(cx.theme().foreground)
            .flex()
            .flex_col()
            .image_cache(self.thumbnails.clone())
            .key_context("Startpoint")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::launch_selected))
            .on_action(cx.listener(Self::open_settings))
            .child(self.toolbar(cx))
            .when(!self.settings_open, |view| {
                view.child(self.navigation_bar(cx))
            })
            .child(div().flex().flex_1().min_h_0().min_w_0().child(content))
            .when(self.jobs_open, |view| view.child(self.activity(cx)))
    }
}

fn text_input<T: 'static>(
    placeholder: &str,
    multiline: bool,
    window: &mut Window,
    cx: &mut Context<T>,
) -> Entity<InputState> {
    let placeholder = placeholder.to_string();
    cx.new(|cx| {
        let state = InputState::new(window, cx).placeholder(placeholder);
        if multiline {
            state.auto_grow(4, 8)
        } else {
            state
        }
    })
}

fn set_value<T: 'static>(
    field: &Entity<InputState>,
    value: &str,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    field.update(cx, |field, cx| {
        field.set_value(value.to_string(), window, cx)
    });
}

fn field(label: &str, state: &Entity<InputState>, cx: &App) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_sm().child(label.to_string()))
        .child(
            Input::new(state)
                .w_full()
                .flex_shrink_0()
                .bg(cx.theme().secondary),
        )
}

fn eyebrow(label: &str) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .opacity(0.65)
        .child(label.to_string())
}
fn hint(text: &str) -> impl IntoElement {
    div().text_xs().opacity(0.7).child(text.to_string())
}

fn image(path: PathBuf, fit: ObjectFit) -> Img {
    img(path)
        .size_full()
        .object_fit(fit)
        .with_fallback(|| placeholder("Image unavailable").into_any_element())
}

fn placeholder(title: &str) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_5()
        .px_4()
        .bg(rgb(0x24282d))
        .child(div().w_6().h_1().bg(rgb(ACCENT)))
        .child(
            div()
                .text_sm()
                .text_color(rgb(0xb7bec4))
                .text_center()
                .child(title.to_string()),
        )
}

fn keycap(label: &str) -> impl IntoElement {
    div()
        .px_2()
        .py_1()
        .rounded(px(4.))
        .border_1()
        .border_color(rgba(0x8b8c8d30))
        .text_xs()
        .opacity(0.65)
        .child(label.to_string())
}

fn source_name(provider_id: &str) -> &str {
    match provider_id {
        "steam" => "Steam",
        "manual" => "Manually added",
        "example" => "Example library",
        other => other,
    }
}

fn last_played(game: &Game) -> String {
    let Some(timestamp) = game.last_launched else {
        return format!(
            "{} · {}",
            source_name(&game.provider_id),
            if game.available {
                "Installed"
            } else {
                "Unavailable"
            }
        );
    };
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(timestamp);
    match elapsed / 86_400 {
        0 if elapsed < 60 => "Last launched just now".into(),
        0 => "Last launched today".into(),
        1 => "Last launched yesterday".into(),
        days if days < 7 => format!("Last launched {days} days ago"),
        days => format!("Last launched {} weeks ago", days / 7),
    }
}
