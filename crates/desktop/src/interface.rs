use crate::{
    appearance,
    image_cache::ArtworkImageCache,
    recents_fade::RecentsFade,
    theme,
    theme_editor::{Editor, EditorEvent},
};
use chrono::{Datelike, Local, Months, NaiveDate, TimeZone};
use gpui::{prelude::*, *};
#[cfg(not(target_os = "windows"))]
use gpui_component::TitleBar;
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, InteractiveElementExt as _, Selectable, Sizable,
    ThemeColor,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    spinner::Spinner,
    switch::Switch,
    tooltip::Tooltip,
};
use startpoint_sdk::{
    AppEvent, AppearanceMode, AppearanceSettings, Application as LauncherApplication, ArtworkAsset,
    ArtworkCandidate, ArtworkGame, ArtworkKind, Command, CredentialStatus, ExecutableLaunch, Game,
    GameId, JobId, JobState, LaunchTarget, Snapshot, StartScreen, UserMetadata,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const HEADER_HEIGHT: f32 = 44.;
const RECENTS_INTERVAL: Duration = Duration::from_secs(8);
const RECENTS_TRANSITION: Duration = Duration::from_millis(240);

#[derive(IntoElement)]
struct LauncherTitleBar {
    content: AnyElement,
}

impl RenderOnce for LauncherTitleBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        #[cfg(target_os = "windows")]
        {
            div()
                .id("title-bar")
                .w_full()
                .h(px(HEADER_HEIGHT))
                .px(px(8.))
                .flex()
                .items_center()
                .bg(cx.theme().title_bar)
                .child(
                    div()
                        .id("title-bar-content")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.content),
                )
                .into_any_element()
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = cx;
            TitleBar::new()
                .h(px(HEADER_HEIGHT))
                .pl(px(8.))
                .pr(px(8.))
                .border_0()
                .child(self.content)
                .into_any_element()
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_window_controls(window: &Window, cx: &App) -> impl IntoElement {
    let maximized = window.is_maximized();
    div()
        .id("windows-window-controls")
        .h(px(32.))
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(2.))
        .children(
            [
                (
                    "window-minimize",
                    IconName::WindowMinimize,
                    WindowControlArea::Min,
                ),
                (
                    "window-maximize",
                    if maximized {
                        IconName::WindowRestore
                    } else {
                        IconName::WindowMaximize
                    },
                    WindowControlArea::Max,
                ),
                (
                    "window-close",
                    IconName::WindowClose,
                    WindowControlArea::Close,
                ),
            ]
            .into_iter()
            .map(|(id, icon, area)| {
                let close = matches!(area, WindowControlArea::Close);
                div()
                    .id(id)
                    .size(px(32.))
                    .rounded(px(6.))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().foreground)
                    .hover(|style| {
                        style
                            .bg(if close {
                                cx.theme().danger
                            } else {
                                cx.theme().secondary_hover
                            })
                            .text_color(if close {
                                cx.theme().danger_foreground
                            } else {
                                cx.theme().secondary_foreground
                            })
                    })
                    .active(|style| {
                        style.bg(if close {
                            cx.theme().danger_active
                        } else {
                            cx.theme().secondary_active
                        })
                    })
                    .window_control_area(area)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        match area {
                            WindowControlArea::Min => window.minimize_window(),
                            WindowControlArea::Max => appearance::toggle_window_maximize(window),
                            // Startpoint has one window. Quit before destroying it so GPUI's
                            // Windows foreground executor can still dispatch the quit request.
                            WindowControlArea::Close => cx.quit(),
                            WindowControlArea::Drag => {}
                        }
                    })
                    .child(Icon::new(icon).small())
            }),
        )
}

actions!(
    startpoint,
    [
        Quit,
        FocusSearch,
        OpenCommands,
        ToggleSidebar,
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
    Hidden,
}

impl LibraryFilter {
    fn label(self) -> &'static str {
        match self {
            Self::Installed => "Installed",
            Self::All => "All games",
            Self::Favourites => "Favourites",
            Self::Hidden => "Hidden",
        }
    }

    fn includes(self, game: &Game) -> bool {
        match self {
            Self::Hidden => game.hidden,
            Self::Installed => !game.hidden && game.available,
            Self::All => !game.hidden,
            Self::Favourites => !game.hidden && game.favourite,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailTab {
    Overview,
    Metadata,
    Launch,
    Artwork,
    Journal,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommandMode {
    Games,
    Actions,
}

#[derive(Clone)]
enum LauncherCommand {
    SelectGame(GameId),
    LaunchGame(GameId),
    Screen(StartScreen),
    Settings,
    Refresh,
    AddGame,
    Sidebar,
    Details(DetailTab),
    Favourite(GameId, bool),
    Hidden(GameId, bool),
    Reidentify,
    Appearance(AppearanceMode),
    DitherRecents,
    DitherArtwork,
    Activity,
    CustomizeRecents,
}

#[derive(Clone)]
struct SidebarResize;

impl Render for SidebarResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

enum PickerPurpose {
    AddGame,
    LaunchOverride(GameId),
    Artwork(GameId, ArtworkKind),
    SteamDirectory,
    EpicDirectory,
    GogDirectory,
}

pub struct LauncherView {
    api: LauncherApplication,
    snapshot: Snapshot,
    command_search: Entity<InputState>,
    command_mode: Option<CommandMode>,
    command_selected: usize,
    command_scroll: UniformListScrollHandle,
    command_previous_focus: Option<FocusHandle>,
    visible: Vec<Game>,
    selected: Option<GameId>,
    source_filter: Option<String>,
    collection_filter: Option<String>,
    library_filter: LibraryFilter,
    sort_recent: bool,
    appearance: AppearanceSettings,
    recents_fade: Entity<RecentsFade>,
    appearance_loaded: bool,
    appearance_job: Option<JobId>,
    themes_loading: bool,
    customization_open: bool,
    theme_editor: Option<Entity<Editor>>,
    theme_subscription: Option<Subscription>,
    sidebar_transition: Option<(f32, Instant)>,
    sidebar_resize_focus: FocusHandle,
    appearance_save: Option<Task<()>>,
    window_should_move: bool,
    screen: StartScreen,
    settings_open: bool,
    jobs_open: bool,
    detail_tab: DetailTab,
    detail_return_screen: Option<StartScreen>,
    artwork_kind: ArtworkKind,
    artwork_images: Vec<ArtworkCandidate>,
    artwork_games: Vec<ArtworkGame>,
    artwork_job: Option<JobId>,
    identity_job: Option<(JobId, GameId)>,
    manual_import_job: Option<JobId>,
    credential_job: Option<JobId>,
    credential_editing: bool,
    artwork_match: Option<ArtworkGame>,
    reidentifying: bool,
    artwork_message: Option<String>,
    notice: Option<String>,
    title: Entity<InputState>,
    description: Entity<InputState>,
    developer: Entity<InputState>,
    genres: Entity<InputState>,
    metadata_baseline: UserMetadata,
    collections: Entity<InputState>,
    completed: bool,
    journal_text: Entity<InputState>,
    journal_entry: Option<String>,
    journal_delete: Option<String>,
    journal_job: Option<(JobId, GameId, String)>,
    activity_month: NaiveDate,
    activity_day: NaiveDate,
    executable: Entity<InputState>,
    arguments: Entity<InputState>,
    working_directory: Entity<InputState>,
    api_key: Entity<InputState>,
    artwork_search: Entity<InputState>,
    thumbnails: Entity<ArtworkImageCache>,
    full_resolution_images: Entity<ArtworkImageCache>,
    focus: FocusHandle,
    list_scroll: UniformListScrollHandle,
    artwork_texture: Arc<Image>,
    carousel_elapsed: Duration,
    carousel_tick: Option<Instant>,
    carousel_paused: bool,
    carousel_focus: FocusHandle,
    carousel_transition: Option<(Game, Instant)>,
    carousel_image_ready: bool,
    _subscriptions: Vec<Subscription>,
}

impl LauncherView {
    pub fn new(api: LauncherApplication, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let events = api.subscribe();
        let snapshot = api.snapshot();
        let command_search = text_input("Find a game…", false, window, cx);
        let subscription = cx.subscribe_in(&command_search, window, |view, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                view.command_selected = 0;
                view.command_scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
        });
        let launcher = cx.entity().downgrade();
        let key_subscription = cx.intercept_keystrokes(move |event, window, cx| {
            let _ = launcher.update(cx, |view, cx| view.intercept_key(event, window, cx));
        });
        let appearance_subscription = cx.observe_window_appearance(window, |view, window, cx| {
            view.apply_appearance(window, cx);
        });
        let appearance_settings = snapshot.settings.appearance.clone();
        let appearance_loaded = snapshot.ready;
        appearance::apply(&appearance_settings, Some(window), cx);
        let screen = appearance_settings.start_screen;
        let mut view = Self {
            api,
            snapshot,
            command_search,
            command_mode: None,
            command_selected: 0,
            command_scroll: UniformListScrollHandle::new(),
            command_previous_focus: None,
            visible: Vec::new(),
            selected: None,
            source_filter: None,
            collection_filter: None,
            library_filter: LibraryFilter::Installed,
            sort_recent: true,
            appearance: appearance_settings,
            recents_fade: cx.new(|_| RecentsFade::default()),
            appearance_loaded,
            appearance_job: None,
            themes_loading: false,
            customization_open: false,
            theme_editor: None,
            theme_subscription: None,
            sidebar_transition: None,
            sidebar_resize_focus: cx.focus_handle(),
            appearance_save: None,
            window_should_move: false,
            screen,
            settings_open: false,
            jobs_open: false,
            detail_tab: DetailTab::Overview,
            detail_return_screen: None,
            artwork_kind: ArtworkKind::Cover,
            artwork_images: Vec::new(),
            artwork_games: Vec::new(),
            artwork_job: None,
            identity_job: None,
            manual_import_job: None,
            credential_job: None,
            credential_editing: false,
            artwork_match: None,
            reidentifying: false,
            artwork_message: None,
            notice: None,
            title: text_input("Game title", false, window, cx),
            description: text_input("Add a description", true, window, cx),
            developer: text_input("Developer", false, window, cx),
            genres: text_input("Action, Adventure…", false, window, cx),
            metadata_baseline: UserMetadata::default(),
            collections: text_input("One collection name per line", true, window, cx),
            completed: false,
            journal_text: text_input("What happened? What will you do next?", true, window, cx),
            journal_entry: None,
            journal_delete: None,
            journal_job: None,
            activity_month: Local::now()
                .date_naive()
                .with_day(1)
                .unwrap_or_else(|| Local::now().date_naive()),
            activity_day: Local::now().date_naive(),
            executable: text_input("Default launcher", false, window, cx),
            arguments: text_input("One argument per line", true, window, cx),
            working_directory: text_input("Executable directory", false, window, cx),
            api_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Your SteamGridDB API key")
                    .masked(true)
            }),
            artwork_search: text_input("Search SteamGridDB by game title", false, window, cx),
            thumbnails: cx
                .new(|cx| ArtworkImageCache::new(96, 64 * 1024 * 1024, 640 * 960 * 4, cx)),
            full_resolution_images: cx
                .new(|cx| ArtworkImageCache::new(3, 128 * 1024 * 1024, 3840 * 2160 * 4, cx)),
            focus: cx.focus_handle(),
            list_scroll: UniformListScrollHandle::new(),
            artwork_texture: Arc::new(Image::from_bytes(
                ImageFormat::Svg,
                include_bytes!("../assets/dither.svg").to_vec(),
            )),
            carousel_elapsed: Duration::ZERO,
            carousel_tick: None,
            carousel_paused: appearance::reduced_motion(),
            carousel_focus: cx.focus_handle(),
            carousel_transition: None,
            carousel_image_ready: false,
            _subscriptions: vec![subscription, appearance_subscription, key_subscription],
        };
        view.filter(cx);
        view.ensure_selection(window, cx);
        view.focus.focus(window);
        view.reload_themes(window, cx);
        view.submit(Command::RefreshLibrary, cx);
        cx.spawn_in(window, async move |view, cx| {
            let mut interval = Duration::from_millis(250);
            loop {
                cx.background_executor().timer(interval).await;
                match view.update_in(cx, |view, window, cx| view.advance_carousel(window, cx)) {
                    Ok(running) => interval = Duration::from_millis(if running { 50 } else { 250 }),
                    Err(_) => break,
                }
            }
        })
        .detach();
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
        crate::diagnostics::record(command.label());
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
            || (matches!(event, AppEvent::SettingsChanged)
                && self.appearance_job.is_none()
                && self.appearance_save.is_none())
        {
            if !self.appearance_loaded
                || self.appearance.start_screen != self.snapshot.settings.appearance.start_screen
            {
                self.screen = self.snapshot.settings.appearance.start_screen;
            }
            self.appearance_loaded = self.snapshot.ready;
            self.appearance = self.snapshot.settings.appearance.clone();
            self.apply_appearance(window, cx);
        }
        match event {
            AppEvent::LibraryChanged => {
                self.filter(cx);
                if !self.settings_open {
                    self.ensure_selection(window, cx);
                }
                if self.detail_tab == DetailTab::Overview {
                    self.populate_game_fields(window, cx);
                } else if let Some(game) = self.selected_game() {
                    let metadata = game.effective_metadata();
                    for (field, previous_value, value) in [
                        (
                            &self.title,
                            self.metadata_baseline.title.as_deref().unwrap_or(""),
                            game.title(),
                        ),
                        (
                            &self.description,
                            self.metadata_baseline.description.as_deref().unwrap_or(""),
                            metadata.description.as_deref().unwrap_or(""),
                        ),
                        (
                            &self.developer,
                            self.metadata_baseline.developer.as_deref().unwrap_or(""),
                            metadata.developer.as_deref().unwrap_or(""),
                        ),
                        (
                            &self.genres,
                            self.metadata_baseline.genres.as_deref().unwrap_or(""),
                            metadata.genres.as_deref().unwrap_or(""),
                        ),
                    ] {
                        if previous_value != value && field.read(cx).value() == previous_value {
                            set_value(field, value, window, cx);
                        }
                    }
                    self.metadata_baseline = UserMetadata {
                        title: Some(game.title().to_string()),
                        ..metadata
                    };
                }
                if let Some(game) = self.selected_game()
                    && !self
                        .identity_job
                        .as_ref()
                        .is_some_and(|(_, owner)| owner == &game.id)
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
            AppEvent::ArtworkCacheChanged => {
                for cache in [&self.thumbnails, &self.full_resolution_images] {
                    cache.update(cx, |cache, cx| cache.invalidate_failed(window, cx));
                }
            }
            AppEvent::SettingsChanged => {}
            AppEvent::JobChanged(job) => {
                crate::diagnostics::record(&format!("job {:?} {:?}", job.id, job.state));
                if self
                    .identity_job
                    .as_ref()
                    .is_some_and(|(id, _)| *id == job.id)
                    && !job.state.active()
                    && let Some((_, owner)) = self.identity_job.take()
                    && self.selected.as_ref() == Some(&owner)
                {
                    self.artwork_match =
                        self.selected_game().as_ref().and_then(saved_artwork_match);
                    self.reidentifying = self.artwork_match.is_none();
                    self.artwork_message = Some(if job.state == JobState::Completed {
                        "Identity saved for all image categories. Browse images to choose artwork."
                            .into()
                    } else {
                        "Check the saved identity below. The artwork refresh did not complete; see Jobs for details.".into()
                    });
                    if job.state == JobState::Completed {
                        self.load_artwork(None, cx);
                    }
                }
                if Some(job.id) == self.appearance_job && !job.state.active() {
                    self.appearance_job = None;
                    if job.state != JobState::Completed {
                        self.appearance = self.snapshot.settings.appearance.clone();
                        self.apply_appearance(window, cx);
                    }
                }
                if self
                    .journal_job
                    .as_ref()
                    .is_some_and(|(id, _, _)| *id == job.id)
                    && !job.state.active()
                    && let Some((_, game_id, text)) = self.journal_job.take()
                    && job.state == JobState::Completed
                    && self.selected.as_ref() == Some(&game_id)
                    && self.journal_text.read(cx).value() == text
                {
                    self.journal_entry = None;
                    set_value(&self.journal_text, "", window, cx);
                }
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
                        None
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
        if self.command_mode.is_some() {
            self.command_selected = self
                .command_selected
                .min(self.commands(cx).len().saturating_sub(1));
        }
        cx.notify();
    }

    fn filter(&mut self, _: &App) {
        self.visible = self
            .snapshot
            .games
            .iter()
            .filter(|game| {
                self.library_filter.includes(game)
                    && self
                        .source_filter
                        .as_ref()
                        .is_none_or(|source| source == &game.provider_id)
                    && self
                        .collection_filter
                        .as_ref()
                        .is_none_or(|collection| game.collections.contains(collection))
            })
            .cloned()
            .collect();
        if self.sort_recent {
            self.visible
                .sort_by_key(|game| std::cmp::Reverse(game.last_launched.unwrap_or(0)));
        }
    }

    fn recent_games(&self) -> Vec<&Game> {
        let mut games = self.visible.iter().collect::<Vec<_>>();
        games.sort_by_key(|game| std::cmp::Reverse(game.last_launched.unwrap_or(0)));
        games.truncate(5);
        games
    }

    fn displayed_games(&self) -> Vec<&Game> {
        match self.screen {
            StartScreen::Library | StartScreen::Activity => self.visible.iter().collect(),
            StartScreen::Recents => self.recent_games(),
        }
    }

    fn ensure_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preserve_selected = self.detail_tab != DetailTab::Overview
            && self
                .snapshot
                .games
                .iter()
                .any(|game| self.selected.as_ref() == Some(&game.id));
        let selected = reconciled_selection(
            self.selected.as_ref(),
            self.displayed_games().into_iter().map(|game| &game.id),
            preserve_selected,
        );
        if self.selected != selected {
            if let Some(id) = selected {
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
        if self.selected.as_ref() != Some(&id) {
            self.journal_entry = None;
            self.journal_delete = None;
            set_value(&self.journal_text, "", window, cx);
        }
        self.carousel_elapsed = Duration::ZERO;
        self.carousel_tick = None;
        self.carousel_transition = None;
        self.carousel_image_ready = false;
        self.cancel_artwork();
        self.selected = Some(id);
        self.settings_open = false;
        self.detail_tab = DetailTab::Overview;
        self.artwork_images.clear();
        self.artwork_games.clear();
        self.artwork_message = None;
        self.reidentifying = false;
        self.artwork_match = self.selected_game().as_ref().and_then(saved_artwork_match);
        self.populate_game_fields(window, cx);
        cx.notify();
    }

    fn populate_game_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(game) = self.selected_game() {
            set_value(
                &self.collections,
                &game
                    .collections
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
                window,
                cx,
            );
            self.completed = game.completed;
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
            self.metadata_baseline = UserMetadata {
                title: Some(game.title().to_string()),
                ..metadata
            };
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
        if self.identity_job.is_some() {
            self.artwork_message =
                Some("Saving the game identity; browse images when it finishes.".into());
            cx.notify();
            return;
        }
        if let Some(game_id) = self.selected.clone() {
            self.cancel_artwork();
            self.artwork_images.clear();
            self.artwork_message = None;
            self.artwork_job = self.submit(
                Command::ListArtwork {
                    provider_id: "steamgriddb".into(),
                    game_id,
                    remote_game_id: remote_game_id
                        .or_else(|| self.artwork_match.as_ref().map(|game| game.id.clone()))
                        .or_else(|| {
                            self.selected_game()
                                .as_ref()
                                .and_then(saved_artwork_match)
                                .map(|game| game.id)
                        }),
                    kind: self.artwork_kind,
                },
                cx,
            );
        }
    }

    fn choose_artwork_game(&mut self, matched: ArtworkGame, cx: &mut Context<Self>) {
        if self.identity_job.is_some() {
            return;
        }
        if let Some(game_id) = self.selected.clone() {
            if self.reidentifying {
                self.cancel_artwork();
                self.artwork_match = Some(matched.clone());
                self.artwork_games.clear();
                self.artwork_images.clear();
                self.identity_job = self
                    .submit(
                        Command::ReidentifyGame {
                            game_id: game_id.clone(),
                            matched,
                        },
                        cx,
                    )
                    .map(|id| (id, game_id));
                self.artwork_message =
                    Some("Saving identity and refreshing automatic artwork…".into());
                return;
            }
            self.artwork_match = Some(matched.clone());
            self.identity_job = self
                .submit(
                    Command::SetArtworkGame {
                        provider_id: "steamgriddb".into(),
                        game_id: game_id.clone(),
                        remote_game_id: matched.id.clone(),
                    },
                    cx,
                )
                .map(|id| (id, game_id));
            self.artwork_games.clear();
            self.artwork_message = Some("Saving this match for all image categories…".into());
        }
    }

    fn begin_reidentify(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.identity_job.is_some() {
            return;
        }
        let Some(game) = self.selected_game() else {
            return;
        };
        self.open_details(DetailTab::Artwork, window, cx);
        self.cancel_artwork();
        self.reidentifying = true;
        self.artwork_games.clear();
        self.artwork_images.clear();
        self.artwork_message = None;
        set_value(&self.artwork_search, &game.discovered_title, window, cx);
        self.artwork_search
            .update(cx, |input, cx| input.focus(window, cx));
        if self
            .snapshot
            .settings
            .configured_credentials
            .contains("steamgriddb")
        {
            self.search_artwork(cx);
        }
        cx.notify();
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
                    .child(div().size(px(8.)).rounded_full().bg(match status {
                        CredentialStatus::Verified => cx.theme().success,
                        CredentialStatus::Invalid
                        | CredentialStatus::Locked
                        | CredentialStatus::Unavailable(_) => cx.theme().warning,
                        _ => cx.theme().muted_foreground,
                    }))
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
        let directory = matches!(
            purpose,
            PickerPurpose::SteamDirectory
                | PickerPurpose::EpicDirectory
                | PickerPurpose::GogDirectory
        );
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: !directory,
            directories: directory,
            multiple: false,
            prompt: Some(
                match purpose {
                    PickerPurpose::SteamDirectory => "Choose Steam installation directory",
                    PickerPurpose::EpicDirectory => "Choose Epic manifest directory",
                    PickerPurpose::GogDirectory => "Choose GOG games directory",
                    _ => "Choose file",
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
                        PickerPurpose::EpicDirectory => {
                            view.submit(
                                Command::SetEpicDirectory {
                                    directory: Some(path),
                                },
                                cx,
                            );
                        }
                        PickerPurpose::GogDirectory => {
                            view.submit(
                                Command::SetGogDirectory {
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

    fn save_progress(&mut self, cx: &mut Context<Self>) {
        if let Some(game_id) = self.selected.clone() {
            let collections = self
                .collections
                .read(cx)
                .value()
                .lines()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(String::from)
                .collect();
            self.submit(
                Command::SetGameProgress {
                    game_id,
                    collections,
                    completed: self.completed,
                },
                cx,
            );
        }
    }

    fn remove_manual_game(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game) = self.selected_game() else {
            return;
        };
        let confirmation = window.prompt(PromptLevel::Warning, &format!("Remove {} from the library?", game.title()), Some("Its journal notes and Startpoint launch history will also be removed. The game files are not deleted."), &["Cancel", "Remove game"], cx);
        cx.spawn_in(window, async move |view, cx| {
            if confirmation.await == Ok(1) {
                let _ = view.update_in(cx, |view, _, cx| {
                    view.submit(Command::RemoveManualGame { game_id: game.id }, cx);
                });
            }
        })
        .detach();
    }

    fn save_journal(&mut self, cx: &mut Context<Self>) {
        if self.journal_job.is_some() {
            return;
        }
        if let Some(game_id) = self.selected.clone() {
            let text = self.journal_text.read(cx).value().to_string();
            if let Some(id) = self.submit(
                Command::SaveJournalEntry {
                    game_id: game_id.clone(),
                    entry_id: self.journal_entry.clone(),
                    text: text.clone(),
                },
                cx,
            ) {
                self.journal_job = Some((id, game_id, text));
            }
        }
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.open_command_bar(CommandMode::Games, window, cx);
    }

    fn intercept_key(
        &mut self,
        event: &KeystrokeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        if (modifiers.platform || modifiers.control)
            && modifiers.number_of_modifiers() == 1
            && matches!(event.keystroke.key.as_str(), "f" | "k")
        {
            self.open_command_bar(
                if event.keystroke.key == "f" {
                    CommandMode::Games
                } else {
                    CommandMode::Actions
                },
                window,
                cx,
            );
            cx.stop_propagation();
            return;
        }
        if self.command_mode.is_none() {
            return;
        }
        if self.command_search.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" if modifiers.number_of_modifiers() == 0 => self.close_command_bar(window, cx),
            "enter" if modifiers.number_of_modifiers() == 0 => {
                if let Some((_, _, command)) = self.commands(cx).get(self.command_selected) {
                    self.run_command(command.clone(), window, cx);
                }
            }
            "tab" => self
                .command_search
                .update(cx, |input, cx| input.focus(window, cx)),
            "up" | "down" if modifiers.number_of_modifiers() == 0 => {
                let count = self.commands(cx).len();
                if count > 0 {
                    self.command_selected = if event.keystroke.key == "down" {
                        (self.command_selected + 1) % count
                    } else {
                        (self.command_selected + count - 1) % count
                    };
                    self.command_scroll
                        .scroll_to_item(self.command_selected, ScrollStrategy::Center);
                }
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn open_commands(&mut self, _: &OpenCommands, window: &mut Window, cx: &mut Context<Self>) {
        self.open_command_bar(CommandMode::Actions, window, cx);
    }

    fn open_command_bar(&mut self, mode: CommandMode, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_mode.is_none() {
            self.command_previous_focus = window.focused(cx);
        }
        self.command_mode = Some(mode);
        self.command_selected = 0;
        self.command_scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.jobs_open = false;
        self.command_search.update(cx, |input, cx| {
            input.set_placeholder(
                match mode {
                    CommandMode::Games => "Find a game…",
                    CommandMode::Actions => "Search actions…",
                },
                window,
                cx,
            );
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn close_command_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_mode = None;
        self.command_previous_focus
            .take()
            .unwrap_or_else(|| self.focus.clone())
            .focus(window);
        cx.notify();
    }

    fn commands(&self, cx: &App) -> Vec<(String, Icon, LauncherCommand)> {
        let query = self.command_search.read(cx).value().to_lowercase();
        if self.command_mode == Some(CommandMode::Games) {
            return self
                .snapshot
                .games
                .iter()
                .filter(|game| game.hidden == (self.library_filter == LibraryFilter::Hidden))
                .filter(|game| {
                    let metadata = game.effective_metadata();
                    let text = format!(
                        "{} {}",
                        game.title(),
                        metadata.genres.as_deref().unwrap_or("")
                    )
                    .to_lowercase();
                    query.split_whitespace().all(|part| text.contains(part))
                })
                .map(|game| {
                    (
                        game.title().to_string(),
                        IconName::BookOpen.into(),
                        LauncherCommand::SelectGame(game.id.clone()),
                    )
                })
                .collect();
        }
        let mut commands = Vec::new();
        if let Some(game) = self.selected_game() {
            if game.available || game.launch_override.is_some() {
                commands.push((
                    format!("Start {}", game.title()),
                    IconName::ArrowRight.into(),
                    LauncherCommand::LaunchGame(game.id.clone()),
                ));
            }
            commands.extend([
                (
                    if game.hidden {
                        "Unhide game"
                    } else {
                        "Hide game"
                    }
                    .into(),
                    IconName::Eye.into(),
                    LauncherCommand::Hidden(game.id.clone(), !game.hidden),
                ),
                (
                    "Re-identify game".into(),
                    IconName::Search.into(),
                    LauncherCommand::Reidentify,
                ),
                (
                    "Edit game metadata".into(),
                    IconName::ALargeSmall.into(),
                    LauncherCommand::Details(DetailTab::Metadata),
                ),
                (
                    "Choose game artwork".into(),
                    IconName::GalleryVerticalEnd.into(),
                    LauncherCommand::Details(DetailTab::Artwork),
                ),
                (
                    "Edit launch configuration".into(),
                    IconName::SquareTerminal.into(),
                    LauncherCommand::Details(DetailTab::Launch),
                ),
                (
                    "Open game journal".into(),
                    IconName::BookOpen.into(),
                    LauncherCommand::Details(DetailTab::Journal),
                ),
                (
                    (if game.favourite {
                        "Remove from favourites"
                    } else {
                        "Add to favourites"
                    })
                    .into(),
                    IconName::Star.into(),
                    LauncherCommand::Favourite(game.id, !game.favourite),
                ),
            ]);
        }
        commands.extend([
            (
                "Show Library".into(),
                IconName::BookOpen.into(),
                LauncherCommand::Screen(StartScreen::Library),
            ),
            (
                "Show Recents".into(),
                IconName::GalleryVerticalEnd.into(),
                LauncherCommand::Screen(StartScreen::Recents),
            ),
            (
                "Show activity calendar and collection progress".into(),
                IconName::Calendar.into(),
                LauncherCommand::Screen(StartScreen::Activity),
            ),
            (
                "Customize Recents".into(),
                IconName::Palette.into(),
                LauncherCommand::CustomizeRecents,
            ),
            (
                "Add a game…".into(),
                IconName::Plus.into(),
                LauncherCommand::AddGame,
            ),
            (
                "Refresh installed games".into(),
                Icon::default().path("icons/refresh.svg"),
                LauncherCommand::Refresh,
            ),
            (
                "Settings".into(),
                IconName::Settings.into(),
                LauncherCommand::Settings,
            ),
            (
                (if self.appearance.sidebar_visible {
                    "Hide sidebar"
                } else {
                    "Show sidebar"
                })
                .into(),
                IconName::PanelLeft.into(),
                LauncherCommand::Sidebar,
            ),
            (
                "Activity and notifications".into(),
                IconName::Bell.into(),
                LauncherCommand::Activity,
            ),
            (
                (if self.appearance.dither_recents {
                    "Disable Recents dithering"
                } else {
                    "Enable Recents dithering"
                })
                .into(),
                IconName::Palette.into(),
                LauncherCommand::DitherRecents,
            ),
            (
                (if self.appearance.dither_artwork {
                    "Disable Library dithering"
                } else {
                    "Enable Library dithering"
                })
                .into(),
                IconName::Palette.into(),
                LauncherCommand::DitherArtwork,
            ),
        ]);
        commands.extend(
            [
                AppearanceMode::System,
                AppearanceMode::Light,
                AppearanceMode::Dark,
            ]
            .into_iter()
            .map(|mode| {
                (
                    format!("Appearance: {}", mode.label()),
                    IconName::Sun.into(),
                    LauncherCommand::Appearance(mode),
                )
            }),
        );
        commands.retain(|(label, _, _)| {
            query
                .split_whitespace()
                .all(|part| label.to_lowercase().contains(part))
        });
        commands
    }

    fn run_command(
        &mut self,
        command: LauncherCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_command_bar(window, cx);
        match command {
            LauncherCommand::SelectGame(id) => {
                self.library_filter = if self
                    .snapshot
                    .games
                    .iter()
                    .any(|game| game.id == id && game.hidden)
                {
                    LibraryFilter::Hidden
                } else {
                    LibraryFilter::All
                };
                self.source_filter = None;
                self.collection_filter = None;
                self.filter(cx);
                self.switch_screen(StartScreen::Library, window, cx);
                self.select_game(id, window, cx);
                if let Some(index) = self
                    .visible
                    .iter()
                    .position(|game| self.selected.as_ref() == Some(&game.id))
                {
                    self.list_scroll.scroll_to_item(
                        index / self.library_columns(window),
                        ScrollStrategy::Center,
                    );
                }
            }
            LauncherCommand::LaunchGame(game_id) => {
                self.submit(Command::LaunchGame { game_id }, cx);
            }
            LauncherCommand::Screen(screen) => self.switch_screen(screen, window, cx),
            LauncherCommand::Settings => {
                if !self.settings_open {
                    self.open_settings(&OpenSettings, window, cx);
                }
            }
            LauncherCommand::Refresh => self.refresh(&RefreshLibrary, window, cx),
            LauncherCommand::AddGame => self.pick(PickerPurpose::AddGame, window, cx),
            LauncherCommand::Sidebar => self.toggle_sidebar(&ToggleSidebar, window, cx),
            LauncherCommand::Details(tab) => self.open_details(tab, window, cx),
            LauncherCommand::Favourite(game_id, favourite) => {
                self.submit(Command::SetFavourite { game_id, favourite }, cx);
            }
            LauncherCommand::Hidden(game_id, hidden) => {
                self.submit(Command::SetHidden { game_id, hidden }, cx);
            }
            LauncherCommand::Reidentify => self.begin_reidentify(window, cx),
            LauncherCommand::Appearance(mode) => self.set_appearance(
                AppearanceSettings {
                    mode,
                    ..self.appearance.clone()
                },
                window,
                cx,
            ),
            LauncherCommand::DitherRecents => self.set_appearance(
                AppearanceSettings {
                    dither_recents: !self.appearance.dither_recents,
                    ..self.appearance.clone()
                },
                window,
                cx,
            ),
            LauncherCommand::DitherArtwork => self.set_appearance(
                AppearanceSettings {
                    dither_artwork: !self.appearance.dither_artwork,
                    ..self.appearance.clone()
                },
                window,
                cx,
            ),
            LauncherCommand::Activity => {
                self.jobs_open = true;
            }
            LauncherCommand::CustomizeRecents => {
                self.switch_screen(StartScreen::Recents, window, cx);
                self.customization_open = true;
            }
        }
        cx.notify();
    }

    fn refresh(&mut self, _: &RefreshLibrary, _: &mut Window, cx: &mut Context<Self>) {
        self.submit(Command::RefreshLibrary, cx);
    }
    fn dismiss(&mut self, _: &DismissPanel, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_mode.is_some() {
            self.close_command_bar(window, cx);
            return;
        }
        if self.jobs_open {
            self.jobs_open = false;
        } else if self.theme_editor.is_some() {
            self.close_theme_editor(window, cx);
            return;
        } else if self.customization_open {
            self.customization_open = false;
        } else if self.settings_open {
            self.settings_open = false;
        } else if self.detail_tab != DetailTab::Overview {
            self.detail_tab = DetailTab::Overview;
            self.cancel_artwork();
            if let Some(screen) = self.detail_return_screen.take() {
                self.screen = screen;
            }
        }
        self.ensure_selection(window, cx);
        self.focus.focus(window);
        cx.notify();
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_mode.is_some() {
            self.close_command_bar(window, cx);
        }
        self.settings_open = !self.settings_open;
        self.customization_open = false;
        self.close_theme_editor(window, cx);
        self.jobs_open = false;
        if !self.settings_open {
            self.ensure_selection(window, cx);
        }
        self.focus.focus(window);
        cx.notify();
    }

    fn launch_selected(&mut self, _: &LaunchSelected, _: &mut Window, cx: &mut Context<Self>) {
        if self.command_mode.is_none()
            && !self.settings_open
            && !self.jobs_open
            && !self.customization_open
            && self.theme_editor.is_none()
            && self.screen != StartScreen::Activity
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
        if self.command_mode.is_some()
            || self.settings_open
            || self.jobs_open
            || self.customization_open
            || self.theme_editor.is_some()
            || self.screen == StartScreen::Activity
            || self.detail_tab != DetailTab::Overview
        {
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
        self.carousel_elapsed = Duration::ZERO;
        self.carousel_tick = None;
        self.carousel_transition = None;
        self.screen = screen;
        self.settings_open = false;
        self.jobs_open = false;
        self.customization_open = false;
        self.close_theme_editor(window, cx);
        self.detail_tab = DetailTab::Overview;
        self.detail_return_screen = None;
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
            - px(self.appearance.sidebar_width * self.sidebar_progress());
        if width >= px(650.) { 2 } else { 1 }
    }

    fn open_details(&mut self, tab: DetailTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_none() {
            return;
        }
        let previous = self.screen;
        if self.screen != StartScreen::Library {
            self.switch_screen(StartScreen::Library, window, cx);
        }
        if self.detail_tab == DetailTab::Overview {
            self.populate_game_fields(window, cx);
            self.detail_return_screen = Some(previous);
        }
        self.settings_open = false;
        self.detail_tab = tab;
        cx.notify();
    }

    fn sidebar_progress(&self) -> f32 {
        let target = if self.appearance.sidebar_visible {
            1.
        } else {
            0.
        };
        let Some((from, started)) = self.sidebar_transition else {
            return target;
        };
        let elapsed = (started.elapsed().as_secs_f32() / 0.2).min(1.);
        let mut low = 0.;
        let mut high = 1.;
        for _ in 0..12 {
            let time = (low + high) / 2.;
            let position = time * time * (1.74 - 0.74 * time);
            if position < elapsed {
                low = time;
            } else {
                high = time;
            }
        }
        let time = (low + high) / 2.;
        let progress = time * time * (3. - 2. * time);
        from + (target - from) * progress
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        if !self.snapshot.ready || self.command_mode.is_some() {
            return;
        }
        let from = self.sidebar_progress();
        self.set_appearance(
            AppearanceSettings {
                sidebar_visible: !self.appearance.sidebar_visible,
                ..self.appearance.clone()
            },
            window,
            cx,
        );
        self.sidebar_transition = (!appearance::reduced_motion()).then(|| (from, Instant::now()));
        self.focus.focus(window);
        cx.notify();
    }

    fn resize_sidebar(&mut self, width: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.appearance.sidebar_width = width.clamp(192., 400.);
        self.schedule_appearance_save(window, cx);
    }

    fn schedule_appearance_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.appearance_save = Some(cx.spawn_in(window, async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.appearance_save = None;
                view.set_appearance(view.appearance.clone(), window, cx);
            });
        }));
        cx.notify();
    }

    fn set_appearance(
        &mut self,
        settings: AppearanceSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance_save = None;
        if let Some(id) = self.submit(
            Command::SetAppearance {
                settings: settings.clone(),
            },
            cx,
        ) {
            self.appearance_job = Some(id);
            if self.appearance.start_screen != settings.start_screen {
                self.screen = settings.start_screen;
                self.detail_tab = DetailTab::Overview;
                self.cancel_artwork();
            }
            self.appearance = settings;
            self.apply_appearance(window, cx);
            cx.notify();
        }
    }

    fn reload_themes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.themes_loading {
            return;
        }
        self.themes_loading = true;
        let directory = self.snapshot.paths.data_directory.join("themes");
        let task = cx
            .background_executor()
            .spawn(async move { theme::Catalog::load(&directory) });
        cx.spawn_in(window, async move |view, cx| {
            let themes = task.await;
            let _ = view.update_in(cx, |view, window, cx| {
                cx.set_global(themes);
                view.themes_loading = false;
                view.apply_appearance(window, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn apply_appearance(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = &self.theme_editor {
            let (draft, mode) = editor.read(cx).preview_theme();
            appearance::preview(&draft, mode, window, cx);
        } else {
            appearance::apply(&self.appearance, Some(window), cx);
        }
    }

    fn close_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .theme_editor
            .as_ref()
            .is_some_and(|editor| editor.read(cx).is_saving())
        {
            return;
        }
        if self.theme_editor.take().is_some() {
            self.theme_subscription = None;
            self.apply_appearance(window, cx);
        }
        cx.notify();
    }

    fn open_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_editor.is_some() || self.themes_loading {
            return;
        }
        self.switch_screen(StartScreen::Recents, window, cx);
        self.customization_open = true;
        let selected = self.appearance.theme.clone();
        let directory = self.snapshot.paths.data_directory.join("themes");
        let editor = cx.new(|cx| Editor::new(&selected, directory, window, cx));
        self.theme_subscription =
            Some(
                cx.subscribe_in(&editor, window, |view, _, event, window, cx| {
                    match event {
                        EditorEvent::Saved(name) => {
                            view.theme_editor = None;
                            view.theme_subscription = None;
                            view.set_appearance(
                                AppearanceSettings {
                                    theme: name.clone(),
                                    ..view.appearance.clone()
                                },
                                window,
                                cx,
                            );
                        }
                        EditorEvent::Closed => view.close_theme_editor(window, cx),
                    }
                    cx.notify();
                }),
            );
        self.theme_editor = Some(editor);
        cx.notify();
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let installed = self
            .snapshot
            .games
            .iter()
            .filter(|game| !game.hidden && game.available)
            .count();
        let unavailable = self
            .snapshot
            .games
            .iter()
            .filter(|game| !game.hidden && !game.available)
            .count();
        let favourites = self
            .snapshot
            .games
            .iter()
            .filter(|game| !game.hidden && game.favourite)
            .count();
        let mut sources = BTreeMap::from([("manual".to_string(), 0)]);
        for provider in &self.snapshot.providers {
            if matches!(provider.id.as_str(), "steam" | "epic" | "gog" | "example") {
                sources.insert(provider.id.clone(), 0);
            }
        }
        for game in self
            .snapshot
            .games
            .iter()
            .filter(|game| !game.hidden && game.available)
        {
            *sources.entry(game.provider_id.clone()).or_insert(0) += 1;
        }
        div()
            .id("sidebar-content")
            .w(px(self.appearance.sidebar_width))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .bg(cx.theme().sidebar)
            .flex()
            .flex_col()
            .px_3()
            .pb_4()
            .pt(px(HEADER_HEIGHT + 16.))
            .gap_2()
            .child(eyebrow("LIBRARY"))
            .child(
                Button::new("all-games")
                    .h(px(36.))
                    .ghost()
                    .label(format!("Installed   {installed}"))
                    .selected(
                        self.library_filter == LibraryFilter::Installed
                            && self.source_filter.is_none()
                            && self.collection_filter.is_none()
                            && !self.settings_open,
                    )
                    .border_0()
                    .w_full()
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.library_filter = LibraryFilter::Installed;
                        view.source_filter = None;
                        view.collection_filter = None;
                        view.settings_open = false;
                        view.filter(cx);
                        view.ensure_selection(window, cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("favourites")
                    .h(px(36.))
                    .ghost()
                    .label(format!("Favourites   {favourites}"))
                    .selected(self.library_filter == LibraryFilter::Favourites)
                    .border_0()
                    .w_full()
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.library_filter = LibraryFilter::Favourites;
                        view.source_filter = None;
                        view.collection_filter = None;
                        view.settings_open = false;
                        view.filter(cx);
                        view.ensure_selection(window, cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("hidden-games")
                    .h(px(36.))
                    .ghost()
                    .border_0()
                    .w_full()
                    .label(format!(
                        "Hidden   {}",
                        self.snapshot
                            .games
                            .iter()
                            .filter(|game| game.hidden)
                            .count()
                    ))
                    .selected(self.library_filter == LibraryFilter::Hidden)
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.library_filter = LibraryFilter::Hidden;
                        view.source_filter = None;
                        view.collection_filter = None;
                        view.switch_screen(StartScreen::Library, window, cx);
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
                    .h(px(36.))
                    .ghost()
                    .label(format!("{name}   {count}"))
                    .selected(self.source_filter.as_ref() == Some(&source))
                    .border_0()
                    .w_full()
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.source_filter = Some(source.clone());
                        view.collection_filter = None;
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
                        .h(px(36.))
                        .ghost()
                        .label(format!("All saved   {}", installed + unavailable))
                        .selected(self.library_filter == LibraryFilter::All)
                        .border_0()
                        .w_full()
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.library_filter = LibraryFilter::All;
                            view.source_filter = None;
                            view.collection_filter = None;
                            view.settings_open = false;
                            view.filter(cx);
                            view.ensure_selection(window, cx);
                            cx.notify();
                        })),
                )
            })
            .when(!self.collection_totals().is_empty(), |sidebar| {
                sidebar
                    .child(div().mt_6().child(eyebrow("COLLECTIONS")))
                    .children(self.collection_totals().into_iter().map(
                        |(name, (completed, total))| {
                            Button::new(SharedString::from(format!("collection-{name}")))
                                .h(px(36.))
                                .ghost()
                                .w_full()
                                .border_0()
                                .label(format!("{name}   {completed}/{total}"))
                                .tooltip(name.clone())
                                .selected(self.collection_filter.as_ref() == Some(&name))
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.collection_filter = Some(name.clone());
                                    view.source_filter = None;
                                    view.library_filter = LibraryFilter::All;
                                    view.filter(cx);
                                    view.switch_screen(StartScreen::Library, window, cx);
                                }))
                        },
                    ))
            })
            .child(div().flex_1())
            .child(
                Button::new("settings")
                    .h(px(36.))
                    .ghost()
                    .label("Settings")
                    .selected(self.settings_open)
                    .border_0()
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
                        Some(asset) => image(asset.thumbnail_path.clone(), ObjectFit::Cover, cx)
                            .rounded(px(6.))
                            .into_any_element(),
                        None => placeholder("", cx.theme().colors).into_any_element(),
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
                row.child(
                    Icon::new(IconName::Star)
                        .small()
                        .text_color(cx.theme().primary),
                )
            })
            .when(selected, |row| {
                row.child(
                    Icon::new(IconName::ChevronRight)
                        .small()
                        .text_color(cx.theme().primary),
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
                        .border_color(cx.theme().primary)
                        .bg(cx.theme().muted)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().w_8().h_1().bg(cx.theme().primary)),
                )
                .child(div().text_2xl().child(if self.snapshot.games.is_empty() {
                    "No games yet"
                } else {
                    "No games in this view"
                }))
                .child(
                    Button::new("empty-add")
                        .h(px(36.))
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
                    .h(px(44.))
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
                                            .filter(|game| !game.hidden && game.available)
                                            .count(),
                                    ),
                                    (
                                        LibraryFilter::All,
                                        "All games",
                                        self.snapshot
                                            .games
                                            .iter()
                                            .filter(|game| !game.hidden)
                                            .count(),
                                    ),
                                    (
                                        LibraryFilter::Favourites,
                                        "Favourites",
                                        self.snapshot
                                            .games
                                            .iter()
                                            .filter(|game| !game.hidden && game.favourite)
                                            .count(),
                                    ),
                                    (
                                        LibraryFilter::Hidden,
                                        "Hidden",
                                        self.snapshot
                                            .games
                                            .iter()
                                            .filter(|game| game.hidden)
                                            .count(),
                                    ),
                                ]
                                .into_iter()
                                .map(|(filter, label, count)| {
                                    Button::new(label)
                                        .ghost()
                                        .h(px(36.))
                                        .label(format!("{label}  {count}"))
                                        .selected(self.library_filter == filter)
                                        .border_0()
                                        .when(self.library_filter == filter, |button| {
                                            button.bg(cx.theme().secondary)
                                        })
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.library_filter = filter;
                                            view.source_filter = None;
                                            view.collection_filter = None;
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
                            .h(px(36.))
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
            .when_some(self.collection_filter.clone(), |library, collection| {
                library.child(hint(&format!("Collection: {collection}")))
            })
            .child(content)
            .into_any_element()
    }

    fn navigation_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.weak_entity();
        let selected_filter = self.library_filter;
        div()
            .id("navigation-controls")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .flex_shrink_0()
            .h(px(36.))
            .px(px(4.))
            .rounded(px(12.))
            .bg(cx.theme().secondary)
            .flex()
            .items_center()
            .gap_2()
            .child(
                div().flex().gap_1().children(
                    [
                        StartScreen::Recents,
                        StartScreen::Library,
                        StartScreen::Activity,
                    ]
                    .into_iter()
                    .map(|screen| {
                        Button::new(SharedString::from(format!("view-{}", screen.label())))
                            .ghost()
                            .h(px(32.))
                            .px_3()
                            .label(screen.label())
                            .selected(self.screen == screen)
                            .border_0()
                            .when(self.screen == screen, |button| button.bg(cx.theme().muted))
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
                        .h(px(32.))
                        .px_3()
                        .border_0()
                        .label(selected_filter.label())
                        .dropdown_caret(true)
                        .dropdown_menu(move |mut menu, _, _| {
                            for filter in [
                                LibraryFilter::Installed,
                                LibraryFilter::All,
                                LibraryFilter::Favourites,
                                LibraryFilter::Hidden,
                            ] {
                                let view = view.clone();
                                menu = menu.item(
                                    PopupMenuItem::element(move |_, _| {
                                        div()
                                            .h(px(36.))
                                            .flex()
                                            .items_center()
                                            .text_base()
                                            .child(filter.label())
                                    })
                                    .checked(filter == selected_filter)
                                    .on_click(
                                        move |_, window, cx| {
                                            let _ = view.update(cx, |view, cx| {
                                                view.library_filter = filter;
                                                view.source_filter = None;
                                                view.collection_filter = None;
                                                view.filter(cx);
                                                view.ensure_selection(window, cx);
                                                view.focus.focus(window);
                                                cx.notify();
                                            });
                                        },
                                    ),
                                );
                            }
                            menu.min_w(px(180.))
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
            .icon(Icon::default().path("icons/play.svg"))
            .label("Start game")
            .disabled(!game.available && game.launch_override.is_none())
            .on_click(cx.listener(|view, _, _, cx| {
                if let Some(game_id) = view.selected.clone() {
                    view.submit(Command::LaunchGame { game_id }, cx);
                }
            }))
    }

    fn recent_card(&self, game: &Game, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
            .p_1()
            .rounded(px(10.))
            .when(!self.appearance.fade_recents, |card| {
                card.bg(cx.theme().background.alpha(0.94))
            })
            .when(selected, |card| card.bg(cx.theme().accent))
            .hover(|card| card.bg(cx.theme().muted))
            .cursor_pointer()
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(120.))
                    .rounded(px(8.))
                    .overflow_hidden()
                    .child(match artwork {
                        Some(asset) => image(asset.thumbnail_path.clone(), ObjectFit::Cover, cx)
                            .absolute()
                            .inset_0()
                            .rounded(px(6.))
                            .into_any_element(),
                        None => placeholder("", cx.theme().colors).into_any_element(),
                    }),
            )
            .child(recents_title(
                SharedString::from(format!("recent-card-title-{}", id.0)),
                game.title(),
                window.rem_size() * 0.875,
                FontWeight::SEMIBOLD,
                window,
                cx,
            ))
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

    fn carousel_is_paused(&self, window: &Window, cx: &App) -> bool {
        self.carousel_paused || self.carousel_focus.contains_focused(window, cx)
    }

    fn advance_carousel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.screen != StartScreen::Recents
            || !self.snapshot.ready
            || self.settings_open
            || self.jobs_open
            || self.command_mode.is_some()
            || self.customization_open
            || self.theme_editor.is_some()
            || self.detail_tab != DetailTab::Overview
            || self.carousel_is_paused(window, cx)
            || !window.is_window_active()
            || self.recent_games().len() < 2
        {
            self.carousel_tick = None;
            return false;
        }
        if let Some(previous) = self.carousel_tick.replace(Instant::now()) {
            self.carousel_elapsed += previous.elapsed();
        }
        if self.carousel_elapsed >= RECENTS_INTERVAL && self.carousel_image_ready {
            let games = self.recent_games();
            let index = games
                .iter()
                .position(|game| self.selected.as_ref() == Some(&game.id))
                .unwrap_or(0);
            let next = games[(index + 1) % games.len()].id.clone();
            let previous = self.selected_game();
            self.select_game(next, window, cx);
            if !appearance::reduced_motion() {
                self.carousel_transition = previous.map(|game| (game, Instant::now()));
            }
        }
        cx.notify();
        true
    }

    fn artwork_texture(&self, window: &Window) -> Div {
        let mut texture = div().absolute().inset_0().overflow_hidden().opacity(0.12);
        let viewport = window.viewport_size();
        let mut y = px(0.);
        while y < viewport.height {
            let mut x = px(0.);
            while x < viewport.width {
                texture = texture.child(
                    img(self.artwork_texture.clone())
                        .absolute()
                        .left(x)
                        .top(y)
                        .size(px(256.)),
                );
                x += px(256.);
            }
            y += px(256.);
        }
        texture
    }

    fn recents_background(
        &self,
        game: &Game,
        opacity: f32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let artwork = game
            .artwork
            .get(&ArtworkKind::Background)
            .or_else(|| game.artwork.get(&ArtworkKind::Cover));
        div()
            .image_cache(self.full_resolution_images.clone())
            .absolute()
            .inset_0()
            .overflow_hidden()
            .opacity(opacity)
            .bg(cx.theme().background)
            .when_some(artwork, |background, asset| {
                background.child(image(asset.image_path.clone(), ObjectFit::Cover, cx))
            })
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
        let paused = self.carousel_is_paused(window, cx);
        let artwork = game
            .artwork
            .get(&ArtworkKind::Background)
            .or_else(|| game.artwork.get(&ArtworkKind::Cover));
        let background = cx.theme().background.alpha(1.);
        let has_history = games.iter().any(|game| game.last_launched.is_some());
        let compact = window.viewport_size().width
            - px(self.appearance.sidebar_width * self.sidebar_progress())
            < px(800.);
        let transition = self.carousel_transition.as_ref();
        let opacity = transition
            .map(|(_, started)| {
                let progress =
                    (started.elapsed().as_secs_f32() / RECENTS_TRANSITION.as_secs_f32()).min(1.);
                1. - (1. - progress).powi(3)
            })
            .unwrap_or(1.);
        div()
            .id("recents-carousel")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(background)
            .flex()
            .flex_col()
            .when_some(transition, |view, (previous, _)| {
                view.child(self.recents_background(previous, 1., window, cx))
            })
            .child(self.recents_background(&game, opacity, window, cx))
            .when(
                self.appearance.dither_recents && artwork.is_some(),
                |view| view.child(self.artwork_texture(window)),
            )
            .when(self.appearance.fade_recents, |view| {
                let image = self.recents_fade.update(cx, |fade, cx| {
                    fade.image(background, window, cx)
                });
                view.child(div().absolute().inset_0().child(img(image).size_full().object_fit(ObjectFit::Fill)))
            })
            .child(div().id("recents-content").size_full().overflow_y_scroll().flex().flex_col()
            .child(div().id("recents-hero").w_full().min_h(window.viewport_size().height).flex_shrink_0().flex().flex_col()
            .child(div().flex_1().min_h(px(HEADER_HEIGHT + 12.)))
            .child(
                div()
                    .id("recents-information")
                    .relative()
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .track_focus(&self.carousel_focus)
                    .child(div().relative().w_full().flex().flex_col()
                    .child(
                        div()
                            .relative()
                            .mx(px(32.))
                            .mb_4()
                            .flex()
                            .items_end()
                            .justify_between()
                            .gap_4()
                            .when(compact, |row| row.flex_col())
                            .child(
                                div()
                                    .min_w_0()
                                    .when(!compact, |panel| panel.w(px(520.)).max_w(relative(0.6)))
                                    .when(compact, |panel| panel.w_full())
                                    .p_4()
                                    .rounded(cx.theme().radius_lg)
                                    .when(!self.appearance.fade_recents, |panel| {
                                        panel.bg(background.alpha(0.96))
                                    })
                                    .flex()
                                    .flex_col()
                                    .items_start()
                                    .gap_2()
                                    .child(recents_title(
                                        "selected-recent-title".into(),
                                        game.title(),
                                        px(40.),
                                        FontWeight::BOLD,
                                        window,
                                        cx,
                                    ))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .text_sm()
                                            .child(div().size(px(8.)).rounded_full().bg(
                                                if game.available {
                                                    cx.theme().success
                                                } else {
                                                    cx.theme().warning
                                                },
                                            ))
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
                                    .flex_col()
                                    .items_end()
                                    .gap_3()
                                    .p_3()
                                    .rounded(cx.theme().radius_lg)
                                    .when(!self.appearance.fade_recents, |controls| {
                                        controls.bg(background.alpha(0.94))
                                    })
                                    .child(
                                        div()
                                            .w(px(156.))
                                            .h(px(3.))
                                            .rounded_full()
                                            .overflow_hidden()
                                            .bg(cx.theme().muted_foreground.opacity(0.2))
                                            .child(
                                                div()
                                                    .h_full()
                                                    .w(relative(
                                                        (1. - self.carousel_elapsed.as_secs_f32()
                                                            / RECENTS_INTERVAL.as_secs_f32())
                                                        .clamp(0., 1.),
                                                    ))
                                                    .bg(cx.theme().primary),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                Button::new("edit-recent-game")
                                                    .ghost()
                                                    .size(px(36.))
                                                    .icon(IconName::Ellipsis)
                                                    .tooltip(
                                                        "Edit metadata, artwork or launch options",
                                                    )
                                                    .bg(background.opacity(0.85))
                                                    .on_click(cx.listener(
                                                        |view, _, window, cx| {
                                                            view.open_details(
                                                                DetailTab::Metadata,
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("pause-recents")
                                                    .ghost()
                                                    .size(px(36.))
                                                    .rounded(px(18.))
                                                    .icon(Icon::default().path(if paused {
                                                        "icons/play.svg"
                                                    } else {
                                                        "icons/pause.svg"
                                                    }))
                                                    .tooltip(if paused {
                                                        "Resume carousel"
                                                    } else {
                                                        "Pause carousel"
                                                    })
                                                    .bg(background.opacity(0.85))
                                                    .disabled(games.len() < 2)
                                                    .on_click(cx.listener(
                                                        |view, _, window, cx| {
                                                            view.carousel_paused = !view
                                                                .carousel_is_paused(window, cx);
                                                            view.carousel_tick = None;
                                                            if !view.carousel_paused {
                                                                view.focus.focus(window);
                                                            }
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                            .child(
                                                Button::new("previous-recent")
                                                    .ghost()
                                                    .size(px(36.))
                                                    .rounded(px(18.))
                                                    .icon(IconName::ChevronLeft)
                                                    .tooltip("Previous game")
                                                    .bg(background.opacity(0.85))
                                                    .disabled(games.len() < 2)
                                                    .on_click(cx.listener(
                                                        |view, _, window, cx| {
                                                            view.move_selection(false, window, cx)
                                                        },
                                                    )),
                                            )
                                            .children(games.iter().enumerate().map(
                                                |(index, game)| {
                                                    let id = game.id.clone();
                                                    Button::new(("recent-position", index))
                                                        .ghost()
                                                        .border_0()
                                                        .p_0()
                                                        .tooltip(game.title().to_string())
                                                        .size(px(28.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .cursor_pointer()
                                                        .child(
                                                            div().size(px(6.)).rounded_full().bg(
                                                                if self.selected.as_ref()
                                                                    == Some(&id)
                                                                {
                                                                    cx.theme().primary
                                                                } else {
                                                                    cx.theme()
                                                                        .muted_foreground
                                                                        .opacity(0.35)
                                                                },
                                                            ),
                                                        )
                                                        .on_click(cx.listener(
                                                            move |view, _, window, cx| {
                                                                view.select_game(
                                                                    id.clone(),
                                                                    window,
                                                                    cx,
                                                                );
                                                                view.focus.focus(window);
                                                            },
                                                        ))
                                                },
                                            ))
                                            .child(
                                                Button::new("next-recent")
                                                    .ghost()
                                                    .size(px(36.))
                                                    .rounded(px(18.))
                                                    .icon(IconName::ChevronRight)
                                                    .tooltip("Next game")
                                                    .bg(background.opacity(0.85))
                                                    .disabled(games.len() < 2)
                                                    .on_click(cx.listener(
                                                        |view, _, window, cx| {
                                                            view.move_selection(true, window, cx)
                                                        },
                                                    )),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .flex_shrink_0()
                            .px(px(32.))
                            .pb_5()
                            .pt_2()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .px_3()
                                    .py_1()
                                    .rounded(cx.theme().radius)
                                    .when(!self.appearance.fade_recents, |heading| {
                                        heading.bg(background.alpha(0.94))
                                    })
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
                                            .h(px(32.))
                                            .label("View library")
                                            .icon(IconName::ArrowRight)
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.switch_screen(StartScreen::Library, window, cx)
                                            })),
                                    ),
                            )
                            .child(
                                div().w_full().flex().gap_3().children(
                                    games
                                        .into_iter()
                                        .map(|game| self.recent_card(game, window, cx)),
                                ),
                            ),
                    )))
            )
                    .when(self.appearance.recents_activity || self.appearance.recents_progress || self.appearance.recents_journal, |content| {
                        content.child(div().id("recents-widgets").relative().w_full().flex_shrink_0().px(px(32.)).pt_4().pb_5().flex().flex_wrap().items_start().gap_4()
                            .when(self.appearance.fade_recents, |widgets| widgets.bg(background))
                            .when(self.appearance.recents_activity, |widgets| widgets.child(widget_surface(cx).child(self.activity_calendar(cx))))
                            .when(self.appearance.recents_progress, |widgets| widgets.child(widget_surface(cx).child(self.collection_progress(cx))))
                            .when(self.appearance.recents_journal, |widgets| widgets.child(widget_surface(cx).child(self.journal_widget(cx)))))
                    }),
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
            .border_0()
            .h(px(32.))
            .px_2()
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

    fn toolbar_button(
        &self,
        id: &'static str,
        icon: impl Into<Icon>,
        label: &'static str,
    ) -> Button {
        Button::new(id)
            .ghost()
            .border_0()
            .size(px(32.))
            .p_0()
            .rounded(px(6.))
            .icon(icon)
            .tooltip(label)
            .disabled(self.command_mode.is_some())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    fn toolbar(&self, sidebar: f32, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let inset = if cfg!(target_os = "macos") && !window.is_fullscreen() {
            82.
        } else {
            0.
        };
        let clearance = 12. + (self.appearance.sidebar_width - (142. + inset)).max(0.) * sidebar;
        div()
            .id("window-toolbar")
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(HEADER_HEIGHT))
            .when(
                cfg!(any(target_os = "macos", target_os = "windows")),
                |toolbar| {
                    toolbar
                        .on_mouse_down_out(
                            cx.listener(|view, _, _, _| view.window_should_move = false),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, event: &MouseDownEvent, _, _| {
                                view.window_should_move = event.click_count == 1
                            }),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|view, _, _, _| view.window_should_move = false),
                        )
                        .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, window, _| {
                            if view.window_should_move
                                && event.pressed_button == Some(MouseButton::Left)
                            {
                                view.window_should_move = false;
                                appearance::start_window_move(window);
                            }
                        }))
                },
            )
            .when(cfg!(target_os = "windows"), |toolbar| {
                toolbar.on_double_click(|_, window, cx| {
                    appearance::toggle_window_maximize(window);
                    cx.stop_propagation();
                })
            })
            .child(LauncherTitleBar {
                content: div()
                    .w_full()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id("window-control-island")
                            .occlude()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .relative()
                            .h(px(36.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .px(px(6.))
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .rounded(px(12.))
                                    .bg(cx.theme().secondary)
                                    .shadow_sm()
                                    .opacity(1. - sidebar),
                            )
                            .child(div().w(px(inset)).flex_shrink_0())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(2.))
                                    .child(
                                        self.toolbar_button(
                                            "toggle-sources",
                                            IconName::PanelLeft,
                                            "Toggle sidebar",
                                        )
                                        .disabled(
                                            !self.snapshot.ready || self.command_mode.is_some(),
                                        )
                                        .on_click(
                                            cx.listener(|view, _, window, cx| {
                                                view.toggle_sidebar(&ToggleSidebar, window, cx)
                                            }),
                                        ),
                                    )
                                    .child(
                                        self.toolbar_button(
                                            "navigation-back",
                                            IconName::ArrowLeft,
                                            "Back",
                                        )
                                        .disabled(
                                            self.command_mode.is_some()
                                                || (!self.settings_open
                                                    && !self.customization_open
                                                    && self.theme_editor.is_none()
                                                    && self.detail_tab == DetailTab::Overview),
                                        )
                                        .on_click(
                                            cx.listener(|view, _, window, cx| {
                                                view.dismiss(&DismissPanel, window, cx)
                                            }),
                                        ),
                                    )
                                    .child(
                                        self.toolbar_button(
                                            "open-commands",
                                            IconName::Search,
                                            "Search games or actions",
                                        )
                                        .on_click(
                                            cx.listener(|view, _, window, cx| {
                                                view.open_commands(&OpenCommands, window, cx)
                                            }),
                                        ),
                                    )
                                    .child(
                                        self.toolbar_button(
                                            "open-settings",
                                            IconName::Settings,
                                            "Settings",
                                        )
                                        .on_click(
                                            cx.listener(|view, _, window, cx| {
                                                view.open_settings(&OpenSettings, window, cx)
                                            }),
                                        ),
                                    ),
                            ),
                    )
                    .child(div().w(px(clearance)).flex_shrink_0())
                    .when(!self.settings_open, |bar| {
                        bar.child(self.navigation_bar(cx))
                    })
                    .when(self.settings_open, |bar| {
                        bar.child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Settings"),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("library-controls")
                            .occlude()
                            .h(px(36.))
                            .px(px(6.))
                            .rounded(px(12.))
                            .bg(cx.theme().secondary)
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap(px(2.))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .when(
                                self.screen == StartScreen::Recents && !self.settings_open,
                                |controls| {
                                    controls.child(
                                        self.toolbar_button(
                                            "customize-recents",
                                            IconName::Palette,
                                            "Customize Recents",
                                        )
                                        .selected(self.customization_open)
                                        .disabled(
                                            !self.snapshot.ready || self.command_mode.is_some(),
                                        )
                                        .on_click(
                                            cx.listener(|view, _, window, cx| {
                                                if view.theme_editor.is_some() {
                                                    view.close_theme_editor(window, cx);
                                                } else {
                                                    view.customization_open =
                                                        !view.customization_open;
                                                }
                                                view.jobs_open = false;
                                                view.focus.focus(window);
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                },
                            )
                            .child(
                                self.toolbar_button(
                                    "refresh-library",
                                    Icon::default().path("icons/refresh.svg"),
                                    "Refresh installed games",
                                )
                                .disabled(!self.snapshot.ready || self.command_mode.is_some())
                                .on_click(cx.listener(
                                    |view, _, _, cx| {
                                        view.submit(Command::RefreshLibrary, cx);
                                    },
                                )),
                            )
                            .child(
                                self.toolbar_button("add-game", IconName::Plus, "Add a game")
                                    .disabled(!self.snapshot.ready || self.command_mode.is_some())
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.pick(PickerPurpose::AddGame, window, cx)
                                    })),
                            )
                            .child(self.activity_button(cx))
                            .when(cfg!(target_os = "windows"), |controls| {
                                #[cfg(target_os = "windows")]
                                {
                                    controls.child(windows_window_controls(window, cx))
                                }
                                #[cfg(not(target_os = "windows"))]
                                {
                                    controls
                                }
                            }),
                    )
                    .into_any_element(),
            })
    }

    fn hero(&self, game: &Game, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let artwork = game
            .artwork
            .get(&ArtworkKind::Cover)
            .or_else(|| game.artwork.get(&ArtworkKind::Background));
        let metadata = game.effective_metadata();
        let favourite_id = game.id.clone();
        let favourite = game.favourite;
        div()
            .image_cache(self.full_resolution_images.clone())
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(div().absolute().size_full().child(match artwork {
                Some(asset) => {
                    image(asset.image_path.clone(), ObjectFit::Cover, cx).into_any_element()
                }
                None => placeholder(game.title(), cx.theme().colors).into_any_element(),
            }))
            .when(
                self.appearance.dither_artwork && artwork.is_some(),
                |hero| hero.child(self.artwork_texture(window)),
            )
            .child(div().absolute().size_full().bg(linear_gradient(
                180.,
                linear_color_stop(cx.theme().background.alpha(0.), 0.25),
                linear_color_stop(cx.theme().background, 0.88),
            )))
            .when_some(game.artwork.get(&ArtworkKind::Logo), |hero, logo| {
                hero.child(
                    div()
                        .absolute()
                        .top(px(56.))
                        .left_5()
                        .right_5()
                        .h(px(64.))
                        .child(image(logo.image_path.clone(), ObjectFit::Contain, cx)),
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
                            .bg(cx.theme().popover.alpha(0.94))
                            .text_xs()
                            .child(source_name(&game.provider_id).to_string()),
                    )
                    .child(
                        Button::new("hero-favourite")
                            .ghost()
                            .size(px(36.))
                            .icon(if favourite {
                                IconName::Star
                            } else {
                                IconName::StarOff
                            })
                            .text_color(if favourite {
                                cx.theme().accent_foreground
                            } else {
                                cx.theme().foreground
                            })
                            .bg(cx.theme().popover.alpha(0.94))
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
                                    .size(px(36.))
                                    .icon(IconName::Ellipsis)
                                    .text_color(cx.theme().foreground)
                                    .tooltip("Edit metadata, artwork or launch options")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.open_details(DetailTab::Metadata, window, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().size(px(9.)).rounded_full().bg(if game.available {
                                cx.theme().success
                            } else {
                                cx.theme().warning
                            }))
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
                            .text_color(cx.theme().muted_foreground)
                            .child(last_played(game)),
                    )
                    .when_some(metadata.description, |hero, description| {
                        hero.child(
                            div()
                                .w_full()
                                .max_h(px(56.))
                                .overflow_hidden()
                                .text_sm()
                                .text_color(cx.theme().foreground)
                                .child(description),
                        )
                    })
                    .when_some(metadata.developer, |hero, developer| {
                        hero.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(developer),
                        )
                    })
                    .child(self.start_button(game, cx))
                    .when(artwork.is_none(), |hero| {
                        hero.child(
                            Button::new("find-cover")
                                .ghost()
                                .h(px(36.))
                                .label("Choose artwork…")
                                .text_color(cx.theme().foreground)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.open_details(DetailTab::Artwork, window, cx)
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    fn details(&self, game: &Game, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if self.detail_tab == DetailTab::Overview {
            return div()
                .w(px(380.))
                .flex_shrink_0()
                .h_full()
                .child(self.hero(game, window, cx))
                .into_any_element();
        }
        let game_id = game.id.clone();
        let favourite_id = game_id.clone();
        let favourite = game.favourite;
        let content = match self.detail_tab {
            DetailTab::Overview => self.hero(game, window, cx),
            DetailTab::Metadata => div().w_full().flex().flex_col().gap_4()
                .child(Button::new("reidentify-game").h(px(36.)).label("Re-identify game…").icon(IconName::Search)
                    .on_click(cx.listener(|view, _, window, cx| view.begin_reidentify(window, cx))))
                .child(field("Title", &self.title, cx)).child(field("Description", &self.description, cx)).child(field("Developer", &self.developer, cx)).child(field("Genres", &self.genres, cx))
                .child(Button::new("save-metadata").h(px(36.)).primary().label("Save metadata").on_click(cx.listener(|view, _, _, cx| view.save_metadata(cx))))
                .child(div().mt_3().child(eyebrow("COLLECTIONS & PROGRESS")))
                .child(field("Collections — one per line", &self.collections, cx))
                .child(Switch::new("game-completed").checked(self.completed).label("Completed")
                    .on_click(cx.listener(|view, completed, _, cx| { view.completed = *completed; cx.notify(); })))
                .child(Button::new("save-game-progress").h(px(36.)).label("Save collections and progress").on_click(cx.listener(|view, _, _, cx| view.save_progress(cx))))
                .child(Button::new("set-game-hidden").h(px(36.)).ghost().label(if game.hidden { "Unhide game" } else { "Hide from library" })
                    .on_click(cx.listener(move |view, _, _, cx| {
                        if let Some(game) = view.selected_game() {
                            view.submit(Command::SetHidden { game_id: game.id, hidden: !game.hidden }, cx);
                        }
                    })))
                .child(hint("Hidden games remain saved. Use the Hidden view to restore them. No game files, notes or launch history are removed."))
                .when(game.provider_id == "manual", |content| content.child(Button::new("remove-manual-game").h(px(36.)).ghost().label("Remove from library")
                    .on_click(cx.listener(|view, _, window, cx| view.remove_manual_game(window, cx)))))
                .into_any_element(),
            DetailTab::Launch => div().w_full().flex().flex_col().gap_4()
                .child(hint(&format!("{}\n{}", source_name(&game.provider_id), game.install_directory.display())))
                .child(field("Executable / application bundle", &self.executable, cx))
                .child(Button::new("choose-executable").h(px(36.)).label("Choose executable…").tooltip("Overrides the store launcher and its update, DRM and launch configuration.").on_click(cx.listener(move |view, _, window, cx| view.pick(PickerPurpose::LaunchOverride(game_id.clone()), window, cx))))
                .child(field("Arguments — one per line", &self.arguments, cx))
                .child(field(if cfg!(target_os = "macos") { "Working directory (executable only)" } else { "Working directory" }, &self.working_directory, cx))
                .child(Button::new("save-launch").h(px(36.)).primary().label("Save launch configuration").on_click(cx.listener(|view, _, _, cx| view.save_launch(cx))))
                .child(Button::new("reset-launch").h(px(36.)).ghost().label("Restore launcher default").on_click(cx.listener(|view, _, window, cx| {
                    if let Some(game_id) = view.selected.clone() {
                        view.submit(Command::SetLaunchOverride { game_id, launch: None }, cx);
                        set_value(&view.executable, "", window, cx); set_value(&view.arguments, "", window, cx); set_value(&view.working_directory, "", window, cx);
                    }
                }))).into_any_element(),
            DetailTab::Artwork => self.artwork_picker(game, cx),
            DetailTab::Journal => self.game_journal(game, cx),
        };
        div()
            .w(px(380.))
            .flex_shrink_0()
            .h_full()
            .bg(cx.theme().secondary.opacity(0.7))
            .flex()
            .flex_col()
            .p_5()
            .gap_4()
            .child(div().text_2xl().child(game.title().to_string()))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("details-play")
                            .h(px(36.))
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
                            .h(px(36.))
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
                div().w_full().flex().flex_wrap().gap_1().children(
                    [
                        (DetailTab::Overview, "Overview"),
                        (DetailTab::Metadata, "Metadata"),
                        (DetailTab::Artwork, "Images"),
                        (DetailTab::Launch, "Launch"),
                        (DetailTab::Journal, "Journal"),
                    ]
                    .into_iter()
                    .map(|(tab, label)| {
                        Button::new(label)
                            .ghost()
                            .h(px(36.))
                            .px_3()
                            .label(label)
                            .selected(self.detail_tab == tab)
                            .border_0()
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
        if self.reidentifying {
            return self.reidentify_picker(cx);
        }
        let key_configured = self
            .snapshot
            .settings
            .configured_credentials
            .contains("steamgriddb");
        let game_id = game.id.clone();
        let kind = self.artwork_kind;
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div().flex().flex_wrap().gap_1().children(
                    [
                        ArtworkKind::Cover,
                        ArtworkKind::Background,
                        ArtworkKind::Logo,
                        ArtworkKind::Icon,
                    ]
                    .into_iter()
                    .map(|kind| {
                        Button::new(kind.label())
                            .ghost()
                            .h(px(36.))
                            .px_3()
                            .border_0()
                            .label(kind.label())
                            .selected(kind == self.artwork_kind)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.cancel_artwork();
                                view.artwork_kind = kind;
                                view.artwork_images.clear();
                                view.artwork_message = None;
                                if view
                                    .snapshot
                                    .settings
                                    .configured_credentials
                                    .contains("steamgriddb")
                                    && (view.artwork_match.is_some()
                                        || view
                                            .selected_game()
                                            .as_ref()
                                            .and_then(saved_artwork_match)
                                            .is_some()
                                        || view.selected_game().is_some_and(|game| {
                                            matches!(game.provider_id.as_str(), "steam" | "example")
                                        }))
                                {
                                    view.load_artwork(None, cx);
                                }
                                cx.notify();
                            }))
                    }),
                ),
            )
            .child(
                Button::new("import-artwork")
                    .h(px(36.))
                    .label("Upload image…")
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.pick(PickerPurpose::Artwork(game_id.clone(), kind), window, cx)
                    })),
            )
            .child(eyebrow("SAVED IMAGES"))
            .child(
                div().flex().flex_wrap().gap_3().children(
                    game.artwork_history
                        .iter()
                        .filter(|asset| asset.kind == kind)
                        .map(|asset| self.saved_image(game, asset, cx)),
                ),
            )
            .child(div().mt_3().child(eyebrow("STEAMGRIDDB")))
            .child(self.credential_status(cx))
            .child(
                Button::new("reidentify-artwork-game")
                    .h(px(36.))
                    .label("Re-identify game…")
                    .on_click(cx.listener(|view, _, window, cx| view.begin_reidentify(window, cx))),
            )
            .when_some(self.artwork_match.clone(), |view, matched| {
                let label = if self.identity_job.is_some() {
                    "Saving match"
                } else {
                    "Saved match for all image types"
                };
                view.child(hint(&format!(
                    "{label}: {} · #{}",
                    matched.name, matched.id
                )))
            })
            .when(!key_configured, |view| {
                view.child(
                    Button::new("configure-artwork")
                        .h(px(36.))
                        .ghost()
                        .label("Configure SteamGridDB")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.open_settings(&OpenSettings, window, cx)
                        })),
                )
            })
            .child(
                div().flex().child(
                    Input::new(&self.artwork_search)
                        .large()
                        .w_auto()
                        .flex_1()
                        .min_w_0()
                        .disabled(!key_configured),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("search-artwork-games")
                            .h(px(36.))
                            .label("Find game")
                            .disabled(!key_configured || self.identity_job.is_some())
                            .on_click(cx.listener(|view, _, _, cx| view.search_artwork(cx))),
                    )
                    .child(
                        Button::new("load-artwork")
                            .h(px(36.))
                            .label("Browse images")
                            .disabled(!key_configured || self.identity_job.is_some())
                            .on_click(cx.listener(|view, _, _, cx| view.load_artwork(None, cx))),
                    ),
            )
            .when(
                self.artwork_job.is_some() || self.identity_job.is_some(),
                |view| {
                    view.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(Spinner::new().small())
                            .child(hint("Loading artwork…")),
                    )
                },
            )
            .when_some(self.artwork_message.clone(), |view, message| {
                view.child(hint(&message))
            })
            .children(self.artwork_games.iter().map(|game| {
                let matched = game.clone();
                Button::new(SharedString::from(format!("match-{}", game.id)))
                    .h(px(36.))
                    .label(game.name.clone())
                    .w_full()
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.choose_artwork_game(matched.clone(), cx)
                    }))
            }))
            .child(
                div().flex().flex_wrap().gap_3().children(
                    self.artwork_images
                        .iter()
                        .enumerate()
                        .map(|(index, candidate)| {
                            let candidate = candidate.clone();
                            let selected = candidate.clone();
                            div()
                                .w(px(140.))
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    div()
                                        .h(px(160.))
                                        .rounded_md()
                                        .overflow_hidden()
                                        .bg(cx.theme().muted)
                                        .child(match candidate.preview_path {
                                            Some(path) => image(path, ObjectFit::Contain, cx)
                                                .into_any_element(),
                                            None => placeholder(
                                                if self.artwork_job.is_some() {
                                                    "Loading preview…"
                                                } else {
                                                    "Preview unavailable"
                                                },
                                                cx.theme().colors,
                                            )
                                            .into_any_element(),
                                        }),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .line_clamp(1)
                                        .child(format!("By {}", candidate.author)),
                                )
                                .child(hint(&format!("{} × {}", candidate.width, candidate.height)))
                                .child(
                                    Button::new(("select-remote", index))
                                        .h(px(36.))
                                        .label("Use image")
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            if let Some(game_id) = view.selected.clone() {
                                                view.cancel_artwork();
                                                view.submit(
                                                    Command::SelectArtwork {
                                                        game_id,
                                                        candidate: selected.clone(),
                                                    },
                                                    cx,
                                                );
                                            }
                                        })),
                                )
                        }),
                ),
            )
            .into_any_element()
    }

    fn reidentify_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let configured = self
            .snapshot
            .settings
            .configured_credentials
            .contains("steamgriddb");
        let saving = self.identity_job.is_some();
        div().w_full().flex().flex_col().gap_4()
            .child(eyebrow("RE-IDENTIFY GAME"))
            .child(hint("Search the correct title, then choose a result. This saves one identity for covers, backgrounds, logos and icons. Your launcher, notes and uploaded images are kept."))
            .when(!configured, |view| view.child(self.credential_status(cx)).child(
                Button::new("identify-configure").label("Configure SteamGridDB")
                    .on_click(cx.listener(|view, _, window, cx| view.open_settings(&OpenSettings, window, cx)))))
            .child(Input::new(&self.artwork_search).large().disabled(!configured || saving))
            .child(div().flex().gap_2()
                .child(Button::new("identify-search").primary().label("Search games").disabled(!configured || saving)
                    .on_click(cx.listener(|view, _, _, cx| view.search_artwork(cx))))
                .child(Button::new("identify-cancel").ghost().label("Cancel").disabled(saving)
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.cancel_artwork();
                        view.reidentifying = false;
                        view.artwork_games.clear();
                        view.artwork_message = None;
                        cx.notify();
                    }))))
            .when(self.artwork_job.is_some() || saving, |view| view.child(Spinner::new().small()))
            .when_some(self.artwork_message.clone(), |view, message| view.child(hint(&message)))
            .children(self.artwork_games.iter().map(|game| {
                let matched = game.clone();
                Button::new(SharedString::from(format!("identify-{}", game.id)))
                    .w_full().label(format!("Use {}", game.name)).disabled(saving)
                    .on_click(cx.listener(move |view, _, _, cx| view.choose_artwork_game(matched.clone(), cx)))
            }))
            .into_any_element()
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
                    .bg(cx.theme().muted)
                    .child(image(asset.thumbnail_path.clone(), ObjectFit::Contain, cx)),
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
                    .h(px(36.))
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

    fn theme_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let names = cx
            .global::<theme::Catalog>()
            .themes
            .iter()
            .map(|theme| theme.name.clone())
            .collect::<Vec<_>>();
        let selected = self.appearance.theme.clone();
        let view = cx.weak_entity();
        let selector = Button::new("select-theme")
            .w_full()
            .min_w_0()
            .h(px(32.))
            .px_3()
            .child(div().flex_1().min_w_0().truncate().child(selected.clone()))
            .tooltip(selected.clone())
            .dropdown_caret(true)
            .disabled(!self.snapshot.ready || self.themes_loading)
            .dropdown_menu(move |mut menu, _, _| {
                for name in &names {
                    let view = view.clone();
                    let name = name.to_string();
                    let label = name.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| {
                            div()
                                .h(px(36.))
                                .flex()
                                .items_center()
                                .text_base()
                                .child(label.clone())
                        })
                        .checked(name.eq_ignore_ascii_case(&selected))
                        .on_click(move |_, window, cx| {
                            let _ = view.update(cx, |view, cx| {
                                view.set_appearance(
                                    AppearanceSettings {
                                        theme: name.clone(),
                                        ..view.appearance.clone()
                                    },
                                    window,
                                    cx,
                                );
                                view.focus.focus(window);
                            });
                        }),
                    );
                }
                menu.min_w(px(200.)).scrollable(true)
            });
        div()
            .h(px(32.))
            .min_h_0()
            .w(px(160.))
            .min_w_0()
            .flex_1()
            .map(|mut selector| {
                selector.style().align_self = Some(AlignSelf::FlexStart);
                selector
            })
            .child(selector)
    }

    fn customization(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let saving = self
            .theme_editor
            .as_ref()
            .is_some_and(|editor| editor.read(cx).is_saving());
        // Keep the content at its natural height inside a separate scroll viewport.
        // Constraining the flex column itself can shrink controls into one another.
        let mut panel = div()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(eyebrow("CUSTOMIZE RECENTS"))
                    .child(
                        Button::new("close-customization")
                            .size(px(32.))
                            .icon(IconName::Close)
                            .tooltip("Close customization")
                            .disabled(saving)
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.close_theme_editor(window, cx);
                                view.customization_open = false;
                                view.focus.focus(window);
                                cx.notify();
                            })),
                    ),
            );
        if let Some(editor) = &self.theme_editor {
            panel = panel.child(editor.clone());
        } else {
            panel = panel
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(self.theme_selector(cx))
                        .child(
                            Button::new("edit-theme")
                                .h(px(32.))
                                .px_2()
                                .icon(IconName::Palette)
                                .label("Edit theme…")
                                .disabled(!self.snapshot.ready || self.themes_loading)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.open_theme_editor(window, cx)
                                })),
                        ),
                )
                .child(
                    div().flex().gap_1().children(
                        [
                            AppearanceMode::System,
                            AppearanceMode::Light,
                            AppearanceMode::Dark,
                        ]
                        .into_iter()
                        .map(|mode| {
                            Button::new(mode.label())
                                .h(px(32.))
                                .px_3()
                                .label(mode.label())
                                .selected(self.appearance.mode == mode)
                                .border_0()
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.set_appearance(
                                        AppearanceSettings {
                                            mode,
                                            ..view.appearance.clone()
                                        },
                                        window,
                                        cx,
                                    );
                                }))
                        }),
                    ),
                )
                .child(eyebrow("ARTWORK"))
                .child(
                    Switch::new("recents-dithering")
                        .checked(self.appearance.dither_recents)
                        .label("Dithering")
                        .on_click(cx.listener(|view, enabled, window, cx| {
                            view.set_appearance(
                                AppearanceSettings {
                                    dither_recents: *enabled,
                                    ..view.appearance.clone()
                                },
                                window,
                                cx,
                            );
                        })),
                )
                .child(
                    Switch::new("recents-background-fade")
                        .checked(self.appearance.fade_recents)
                        .label("Background fade")
                        .on_click(cx.listener(|view, enabled, window, cx| {
                            view.set_appearance(
                                AppearanceSettings {
                                    fade_recents: *enabled,
                                    ..view.appearance.clone()
                                },
                                window,
                                cx,
                            );
                        })),
                )
                .child(div().mt_2().child(eyebrow("OPTIONAL WIDGETS")))
                .child(
                    Switch::new("recents-calendar-widget")
                        .checked(self.appearance.recents_activity)
                        .label("Activity calendar")
                        .on_click(cx.listener(|view, enabled, window, cx| {
                            view.set_appearance(
                                AppearanceSettings {
                                    recents_activity: *enabled,
                                    ..view.appearance.clone()
                                },
                                window,
                                cx,
                            );
                        })),
                )
                .child(
                    Switch::new("recents-progress-widget")
                        .checked(self.appearance.recents_progress)
                        .label("Collection progress")
                        .on_click(cx.listener(|view, enabled, window, cx| {
                            view.set_appearance(
                                AppearanceSettings {
                                    recents_progress: *enabled,
                                    ..view.appearance.clone()
                                },
                                window,
                                cx,
                            );
                        })),
                )
                .child(
                    Switch::new("recents-journal-widget")
                        .checked(self.appearance.recents_journal)
                        .label("Game journal")
                        .on_click(cx.listener(|view, enabled, window, cx| {
                            view.set_appearance(
                                AppearanceSettings {
                                    recents_journal: *enabled,
                                    ..view.appearance.clone()
                                },
                                window,
                                cx,
                            );
                        })),
                );
        }
        div()
            .id("recents-customization")
            .absolute()
            .top(px(HEADER_HEIGHT + 8.))
            .right(px(12.))
            .w(px(320.))
            .max_h(window.viewport_size().height - px(HEADER_HEIGHT + 24.))
            .overflow_y_scroll()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .rounded(cx.theme().radius_lg)
            .bg(cx.theme().background)
            .shadow_lg()
            .p_4()
            .child(panel)
            .into_any_element()
    }

    fn collection_totals(&self) -> BTreeMap<String, (u32, u32)> {
        let mut totals = BTreeMap::new();
        for game in self.snapshot.games.iter().filter(|game| !game.hidden) {
            for name in &game.collections {
                let (completed, total) = totals.entry(name.clone()).or_insert((0, 0));
                *total += 1;
                *completed += u32::from(game.completed);
            }
        }
        totals
    }

    fn collection_progress(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut totals = self
            .collection_totals()
            .into_iter()
            .map(|(name, counts)| (Some(name), counts))
            .collect::<Vec<_>>();
        totals.insert(
            0,
            (
                None,
                (
                    u32::try_from(
                        self.snapshot
                            .games
                            .iter()
                            .filter(|game| game.completed)
                            .count(),
                    )
                    .unwrap_or(u32::MAX),
                    u32::try_from(self.snapshot.games.len()).unwrap_or(u32::MAX),
                ),
            ),
        );
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Collection progress"),
            )
            .child(hint("Completion is marked by you, never inferred."))
            .child(
                div()
                    .id("collection-progress-list")
                    .max_h(px(220.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(totals.into_iter().enumerate().map(
                        |(index, (name, (completed, total)))| {
                            let label = name.clone().unwrap_or_else(|| "All saved games".into());
                            let progress = if total == 0 {
                                0
                            } else {
                                u16::try_from(u64::from(completed) * 1000 / u64::from(total))
                                    .unwrap_or(1000)
                            };
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    Button::new(("collection-progress", index))
                                        .ghost()
                                        .h(px(36.))
                                        .w_full()
                                        .border_0()
                                        .label(format!("{label} · {completed} / {total}"))
                                        .tooltip(format!("Open {label}"))
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.collection_filter = name.clone();
                                            view.source_filter = None;
                                            view.library_filter = LibraryFilter::All;
                                            view.filter(cx);
                                            view.switch_screen(StartScreen::Library, window, cx);
                                        })),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .h(px(4.))
                                        .rounded_full()
                                        .overflow_hidden()
                                        .bg(cx.theme().muted)
                                        .child(
                                            div()
                                                .h_full()
                                                .w(relative(f32::from(progress) / 1000.))
                                                .bg(cx.theme().primary),
                                        ),
                                )
                        },
                    )),
            )
            .when(self.collection_totals().is_empty(), |panel| {
                panel.child(hint(
                    "Add collections in a game's Metadata tab to track groups here.",
                ))
            })
            .into_any_element()
    }

    fn open_game_journal(&mut self, game_id: GameId, window: &mut Window, cx: &mut Context<Self>) {
        let previous = self.screen;
        self.collection_filter = None;
        self.source_filter = None;
        self.library_filter = LibraryFilter::All;
        self.filter(cx);
        self.switch_screen(StartScreen::Library, window, cx);
        self.select_game(game_id, window, cx);
        self.open_details(DetailTab::Journal, window, cx);
        self.detail_return_screen = Some(previous);
    }

    fn activity_calendar(&self, cx: &mut Context<Self>) -> AnyElement {
        let leading = self.activity_month.weekday().num_days_from_monday();
        let start = self
            .activity_month
            .checked_sub_days(chrono::Days::new(u64::from(leading)))
            .unwrap_or(self.activity_month);
        let days = self
            .snapshot
            .activity
            .iter()
            .map(|day| (day.date.clone(), day))
            .collect::<BTreeMap<_, _>>();
        let month_prefix = self.activity_month.format("%Y-%m-").to_string();
        let month_launches = self
            .snapshot
            .activity
            .iter()
            .filter(|day| day.date.starts_with(&month_prefix))
            .map(|day| day.launches)
            .sum::<u32>();
        let selected = self.activity_day.format("%Y-%m-%d").to_string();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Activity calendar"),
                    )
                    .child(div().flex().gap_1().children([false, true].into_iter().map(
                        |forward| {
                            Button::new(if forward {
                                "activity-next-month"
                            } else {
                                "activity-previous-month"
                            })
                            .ghost()
                            .size(px(32.))
                            .icon(if forward {
                                IconName::ChevronRight
                            } else {
                                IconName::ChevronLeft
                            })
                            .tooltip(if forward {
                                "Next month"
                            } else {
                                "Previous month"
                            })
                            .on_click(cx.listener(
                                move |view, _, _, cx| {
                                    let month = if forward {
                                        view.activity_month.checked_add_months(Months::new(1))
                                    } else {
                                        view.activity_month.checked_sub_months(Months::new(1))
                                    };
                                    if let Some(month) = month {
                                        view.activity_month = month;
                                        view.activity_day = month;
                                    }
                                    cx.notify();
                                },
                            ))
                        },
                    ))),
            )
            .child(div().text_sm().child(format!(
                "{} · {}",
                self.activity_month.format("%B %Y"),
                launch_count(month_launches)
            )))
            .child(div().flex().gap_1().children(
                ["M", "T", "W", "T", "F", "S", "S"].into_iter().map(|day| {
                    div()
                        .flex_1()
                        .text_xs()
                        .text_center()
                        .text_color(cx.theme().muted_foreground)
                        .child(day)
                }),
            ))
            .children((0u32..6).map(|week| {
                div()
                    .flex()
                    .gap_1()
                    .children((0u32..7).filter_map(|weekday| {
                        let date = start
                            .checked_add_days(chrono::Days::new(u64::from(week * 7 + weekday)))?;
                        let key = date.format("%Y-%m-%d").to_string();
                        let launches = days.get(&key).map(|day| day.launches).unwrap_or(0);
                        let enabled = date.month() == self.activity_month.month();
                        Some(
                            Button::new(SharedString::from(format!("activity-day-{key}")))
                                .ghost()
                                .border_0()
                                .flex_1()
                                .min_w_0()
                                .h(px(32.))
                                .p_0()
                                .label(date.day().to_string())
                                .tooltip(format!("{key} · {}", launch_count(launches)))
                                .bg(if launches == 0 {
                                    cx.theme().muted
                                } else if launches < 3 {
                                    cx.theme().primary.opacity(0.25)
                                } else {
                                    cx.theme().primary
                                })
                                .text_color(if launches >= 3 {
                                    cx.theme().primary_foreground
                                } else {
                                    cx.theme().foreground
                                })
                                .when(date == self.activity_day, |button| {
                                    button
                                        .bg(cx.theme().accent)
                                        .text_color(cx.theme().accent_foreground)
                                })
                                .disabled(!enabled)
                                .when(!enabled, |button| button.opacity(0.35))
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.activity_day = date;
                                    cx.notify();
                                })),
                        )
                    }))
            }))
            .child(hint(&format!(
                "{} · {}",
                self.activity_day.format("%e %b"),
                launch_count(days.get(&selected).map(|day| day.launches).unwrap_or(0))
            )))
            .when_some(days.get(&selected), |panel, day| {
                panel.child(
                    div()
                        .id("activity-day-games")
                        .max_h(px(128.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(day.games.iter().filter_map(|(game_id, count)| {
                            let game = self
                                .snapshot
                                .games
                                .iter()
                                .find(|game| &game.id == game_id)?;
                            let id = game_id.clone();
                            Some(
                                Button::new(SharedString::from(format!(
                                    "activity-game-{}",
                                    game_id.0
                                )))
                                .ghost()
                                .h(px(36.))
                                .w_full()
                                .label(format!("{} · {count}", game.title()))
                                .on_click(cx.listener(
                                    move |view, _, window, cx| {
                                        view.open_game_journal(id.clone(), window, cx)
                                    },
                                )),
                            )
                        })),
                )
            })
            .when(month_launches == 0, |panel| {
                panel.child(hint(
                    "Launch a game through Startpoint to begin your history.",
                ))
            })
            .into_any_element()
    }

    fn journal_widget(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Game journal"),
                    )
                    .child(
                        Button::new("write-journal-note")
                            .ghost()
                            .size(px(32.))
                            .icon(IconName::Plus)
                            .tooltip("Write a game note")
                            .disabled(self.selected.is_none())
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.open_details(DetailTab::Journal, window, cx)
                            })),
                    ),
            )
            .when(self.snapshot.journal.is_empty(), |panel| {
                panel.child(hint(
                    "Keep a note of milestones, new characters, or what to do next.",
                ))
            })
            .children(self.snapshot.journal.iter().take(3).filter_map(|entry| {
                let game = self
                    .snapshot
                    .games
                    .iter()
                    .find(|game| game.id == entry.game_id)?;
                let id = game.id.clone();
                Some(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            Button::new(SharedString::from(format!("journal-widget-{}", entry.id)))
                                .ghost()
                                .h(px(36.))
                                .w_full()
                                .border_0()
                                .label(game.title().to_string())
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.open_game_journal(id.clone(), window, cx)
                                })),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(timestamp_label(entry.created_at)),
                        )
                        .child(div().text_sm().line_clamp(2).child(entry.text.clone())),
                )
            }))
            .into_any_element()
    }

    fn game_journal(&self, game: &Game, cx: &mut Context<Self>) -> AnyElement {
        let entries = self
            .snapshot
            .journal
            .iter()
            .filter(|entry| entry.game_id == game.id)
            .collect::<Vec<_>>();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(field(
                if self.journal_entry.is_some() {
                    "Edit note"
                } else {
                    "New note"
                },
                &self.journal_text,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("save-journal-note")
                            .primary()
                            .h(px(36.))
                            .label(if self.journal_entry.is_some() {
                                "Save changes"
                            } else {
                                "Add note"
                            })
                            .disabled(self.journal_job.is_some())
                            .on_click(cx.listener(|view, _, _, cx| view.save_journal(cx))),
                    )
                    .when(self.journal_entry.is_some(), |row| {
                        row.child(
                            Button::new("cancel-journal-edit")
                                .ghost()
                                .h(px(36.))
                                .label("Cancel edit")
                                .disabled(self.journal_job.is_some())
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.journal_entry = None;
                                    set_value(&view.journal_text, "", window, cx);
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .when(entries.is_empty(), |panel| {
                panel.child(hint("No notes yet. Your journal stays on this computer."))
            })
            .children(entries.into_iter().map(|entry| {
                let saved = entry.clone();
                let delete_id = entry.id.clone();
                let game_id = game.id.clone();
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().background)
                    .child(hint(&timestamp_label(entry.created_at)))
                    .child(div().text_sm().child(entry.text.clone()))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new(SharedString::from(format!("edit-note-{}", entry.id)))
                                    .ghost()
                                    .h(px(36.))
                                    .label("Edit")
                                    .disabled(self.journal_job.is_some())
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.journal_entry = Some(saved.id.clone());
                                        set_value(&view.journal_text, &saved.text, window, cx);
                                        view.journal_text
                                            .update(cx, |field, cx| field.focus(window, cx));
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "delete-note-{}",
                                    entry.id
                                )))
                                .ghost()
                                .h(px(36.))
                                .label(if self.journal_delete.as_ref() == Some(&entry.id) {
                                    "Confirm delete"
                                } else {
                                    "Delete"
                                })
                                .disabled(
                                    self.journal_entry.as_ref() == Some(&entry.id)
                                        || self.journal_job.is_some(),
                                )
                                .on_click(cx.listener(
                                    move |view, _, _, cx| {
                                        if view.journal_delete.as_ref() == Some(&delete_id) {
                                            view.submit(
                                                Command::DeleteJournalEntry {
                                                    game_id: game_id.clone(),
                                                    entry_id: delete_id.clone(),
                                                },
                                                cx,
                                            );
                                            view.journal_delete = None;
                                        } else {
                                            view.journal_delete = Some(delete_id.clone());
                                        }
                                        cx.notify();
                                    },
                                )),
                            )
                            .when(self.journal_delete.as_ref() == Some(&entry.id), |row| {
                                row.child(
                                    Button::new(SharedString::from(format!(
                                        "cancel-delete-note-{}",
                                        entry.id
                                    )))
                                    .ghost()
                                    .h(px(36.))
                                    .label("Keep note")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.journal_delete = None;
                                        cx.notify();
                                    })),
                                )
                            }),
                    )
            }))
            .into_any_element()
    }

    fn activity_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        div().id("library-activity").size_full().overflow_y_scroll().px_8().pb_8()
            .child(div().w_full().max_w(px(1100.)).mx_auto().flex().flex_col().gap_5()
                .child(div().text_3xl().child("Your activity"))
                .child(hint("Launches recorded through Startpoint, not playtime. Existing last-played dates are not retroactively counted."))
                .child(div().w_full().flex().flex_wrap().items_start().gap_4()
                    .child(widget_surface(cx).child(self.activity_calendar(cx)))
                    .child(widget_surface(cx).child(self.collection_progress(cx))))
                .child(widget_surface(cx).w_full().child(self.journal_widget(cx))))
            .into_any_element()
    }

    fn settings(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let key_configured = self
            .snapshot
            .settings
            .configured_credentials
            .contains("steamgriddb");
        let data_directory = self.snapshot.paths.data_directory.clone();
        let cache_directory = self.snapshot.paths.cache_directory.clone();
        let theme_directory = data_directory.join("themes");
        let catalog = cx.global::<theme::Catalog>();
        let theme_warnings = catalog.warnings.clone();
        let theme_missing = !catalog.contains(&self.appearance.theme);
        let content_width = window.viewport_size().width
            - px(self.appearance.sidebar_width * self.sidebar_progress())
            - window.rem_size() * 4.;
        div()
            .id("settings-content")
            .w_full()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .px_8()
            .py_4()
            .child(
                div()
                    .w(content_width.min(px(800.)))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(div().text_3xl().child("Settings"))
                    .child(div().mt_3().child(eyebrow("START SCREEN")))
                    .child(
                        div().flex().gap_2().children(
                            [StartScreen::Library, StartScreen::Recents]
                                .into_iter()
                                .map(|start_screen| {
                                    Button::new(SharedString::from(format!(
                                        "start-screen-{}",
                                        start_screen.label()
                                    )))
                                    .h(px(36.))
                                    .label(start_screen.label())
                                    .selected(self.appearance.start_screen == start_screen)
                                    .border_0()
                                    .disabled(!self.snapshot.ready)
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.set_appearance(
                                            AppearanceSettings {
                                                start_screen,
                                                ..view.appearance.clone()
                                            },
                                            window,
                                            cx,
                                        );
                                    }))
                                }),
                        ),
                    )
                    .child(
                        Button::new("customize-recents-from-settings")
                            .h(px(32.))
                            .px_3()
                            .map(|mut button| {
                                button.style().align_self = Some(AlignSelf::FlexStart);
                                button
                            })
                            .label("Customize Recents…")
                            .icon(IconName::Palette)
                            .disabled(!self.snapshot.ready)
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.switch_screen(StartScreen::Recents, window, cx);
                                view.customization_open = true;
                                cx.notify();
                            })),
                    )
                    .child(div().mt_3().child(eyebrow("APPEARANCE")))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(self.theme_selector(cx))
                            .child(
                                Button::new("edit-theme-from-settings")
                                    .h(px(32.))
                                    .px_3()
                                    .label("Edit theme…")
                                    .icon(IconName::Palette)
                                    .disabled(!self.snapshot.ready || self.themes_loading)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.open_theme_editor(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("open-themes")
                                    .h(px(32.))
                                    .px_3()
                                    .label("Open themes directory")
                                    .disabled(self.themes_loading)
                                    .on_click(move |_, _, cx| cx.reveal_path(&theme_directory)),
                            )
                            .child(
                                Button::new("reload-themes")
                                    .h(px(32.))
                                    .px_3()
                                    .label("Reload")
                                    .icon(Icon::default().path("icons/refresh.svg"))
                                    .disabled(self.themes_loading)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.reload_themes(window, cx)
                                    })),
                            ),
                    )
                    .when(theme_missing && !self.themes_loading, |settings| {
                        settings.child(hint("Selected theme unavailable; using Startpoint."))
                    })
                    .children(theme_warnings.iter().map(|warning| hint(warning)))
                    .child(
                        div().flex().gap_2().children(
                            [
                                AppearanceMode::System,
                                AppearanceMode::Light,
                                AppearanceMode::Dark,
                            ]
                            .into_iter()
                            .map(|mode| {
                                Button::new(mode.label())
                                    .h(px(32.))
                                    .px_3()
                                    .label(mode.label())
                                    .selected(self.appearance.mode == mode)
                                    .border_0()
                                    .icon(match mode {
                                        AppearanceMode::System => IconName::Frame,
                                        AppearanceMode::Light => IconName::Sun,
                                        AppearanceMode::Dark => IconName::Moon,
                                    })
                                    .disabled(!self.snapshot.ready)
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.set_appearance(
                                            AppearanceSettings {
                                                mode,
                                                ..view.appearance.clone()
                                            },
                                            window,
                                            cx,
                                        );
                                    }))
                            }),
                        ),
                    )
                    .child(
                        Switch::new("artwork-dithering")
                            .checked(self.appearance.dither_artwork)
                            .label("Dither Library artwork")
                            .disabled(!self.snapshot.ready)
                            .on_click(cx.listener(|view, enabled, window, cx| {
                                view.set_appearance(
                                    AppearanceSettings {
                                        dither_artwork: *enabled,
                                        ..view.appearance.clone()
                                    },
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(div().mt_4().child(eyebrow("STEAMGRIDDB API KEY")))
                    .child(self.credential_status(cx))
                    .when(!key_configured || self.credential_editing, |view| {
                        view.child(
                            div().flex().child(
                                Input::new(&self.api_key)
                                    .large()
                                    .w_auto()
                                    .flex_1()
                                    .min_w_0()
                                    .mask_toggle(),
                            ),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .when(!key_configured || self.credential_editing, |row| {
                                row.child(
                                    Button::new("save-api-key")
                                        .h(px(36.))
                                        .primary()
                                        .label(if key_configured {
                                            "Replace saved key"
                                        } else {
                                            "Save key"
                                        })
                                        .disabled(
                                            !self.snapshot.ready || self.credential_job.is_some(),
                                        )
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            let credential =
                                                view.api_key.read(cx).value().to_string();
                                            if credential.trim().is_empty() {
                                                view.notice = Some(
                                                    "Enter an API key, or choose Remove key."
                                                        .into(),
                                                );
                                                view.jobs_open = true;
                                                cx.notify();
                                                return;
                                            }
                                            view.credential_job = view.submit(
                                                Command::SetArtworkCredential {
                                                    provider_id: "steamgriddb".into(),
                                                    credential: Some(credential),
                                                },
                                                cx,
                                            );
                                        })),
                                )
                            })
                            .when(key_configured, |row| {
                                row.child(
                                    Button::new("verify-api-key")
                                        .h(px(36.))
                                        .label(
                                            if self
                                                .snapshot
                                                .settings
                                                .credential_status
                                                .get("steamgriddb")
                                                == Some(&CredentialStatus::Locked)
                                            {
                                                "Unlock saved key"
                                            } else {
                                                "Verify saved key"
                                            },
                                        )
                                        .disabled(
                                            self.credential_job.is_some()
                                                || self
                                                    .snapshot
                                                    .settings
                                                    .credential_status
                                                    .get("steamgriddb")
                                                    == Some(&CredentialStatus::Checking),
                                        )
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            view.credential_job = view.submit(
                                                Command::VerifyArtworkCredential {
                                                    provider_id: "steamgriddb".into(),
                                                },
                                                cx,
                                            );
                                        })),
                                )
                                .child(
                                    Button::new("edit-api-key")
                                        .h(px(36.))
                                        .label(if self.credential_editing {
                                            "Cancel replacement"
                                        } else {
                                            "Replace key…"
                                        })
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            view.credential_editing = !view.credential_editing;
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(
                                Button::new("remove-api-key")
                                    .h(px(36.))
                                    .label("Remove key")
                                    .disabled(!key_configured || self.credential_job.is_some())
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.credential_job = view.submit(
                                            Command::SetArtworkCredential {
                                                provider_id: "steamgriddb".into(),
                                                credential: None,
                                            },
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new("get-api-key")
                                    .h(px(36.))
                                    .label("Get a key ↗")
                                    .on_click(|_, _, cx| {
                                        cx.open_url(
                                            "https://www.steamgriddb.com/profile/preferences",
                                        )
                                    }),
                            ),
                    )
                    .child(div().mt_5().child(eyebrow("STEAM DISCOVERY")))
                    .child(hint(
                        &self
                            .snapshot
                            .settings
                            .steam_directory
                            .as_ref()
                            .map(|path| path.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "Automatic detection".into()),
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("choose-steam")
                                    .h(px(36.))
                                    .label("Choose Steam directory…")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.pick(PickerPurpose::SteamDirectory, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("reset-steam")
                                    .h(px(36.))
                                    .label("Use automatic detection")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.submit(
                                            Command::SetSteamDirectory { directory: None },
                                            cx,
                                        );
                                    })),
                            ),
                    )
                    .child(div().mt_5().child(eyebrow("EPIC DISCOVERY")))
                    .child(hint(&self.snapshot.settings.epic_directory.as_ref()
                        .map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|| "Automatic detection · Epic launcher manifests".into())))
                    .child(div().flex().flex_wrap().gap_2()
                        .child(Button::new("choose-epic").h(px(36.)).label("Choose manifest directory…")
                            .on_click(cx.listener(|view, _, window, cx| view.pick(PickerPurpose::EpicDirectory, window, cx))))
                        .child(Button::new("reset-epic").h(px(36.)).label("Use automatic detection")
                            .on_click(cx.listener(|view, _, _, cx| { view.submit(Command::SetEpicDirectory { directory: None }, cx); }))))
                    .child(hint("Choose the folder containing Epic's .item files, not the games folder."))
                    .child(div().mt_5().child(eyebrow("GOG DISCOVERY")))
                    .child(hint(&self.snapshot.settings.gog_directory.as_ref()
                        .map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|| "Automatic detection · Windows installer records or macOS Applications".into())))
                    .child(div().flex().flex_wrap().gap_2()
                        .child(Button::new("choose-gog").h(px(36.)).label("Choose games directory…")
                            .on_click(cx.listener(|view, _, window, cx| view.pick(PickerPurpose::GogDirectory, window, cx))))
                        .child(Button::new("reset-gog").h(px(36.)).label("Use automatic detection")
                            .on_click(cx.listener(|view, _, _, cx| { view.submit(Command::SetGogDirectory { directory: None }, cx); }))))
                    .child(hint("Reads goggame-*.info files in this folder, immediate game folders and native macOS bundles. No account login is needed. Refresh installed games after changing discovery folders."))
                    .child(div().mt_5().child(eyebrow("LOCAL STORAGE")))
                    .child(hint(&format!(
                        "Library and selected images\n{}\n\nDisposable thumbnails\n{}",
                        data_directory.display(),
                        cache_directory.display()
                    )))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("open-data")
                                    .h(px(36.))
                                    .label("Open data directory")
                                    .on_click(move |_, _, cx| cx.reveal_path(&data_directory)),
                            )
                            .child(
                                Button::new("open-cache")
                                    .h(px(36.))
                                    .label("Open cache directory")
                                    .on_click(move |_, _, cx| cx.reveal_path(&cache_directory)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn command_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = self.commands(cx).len();
        let height = f32::from(u16::try_from(count.min(8)).unwrap_or(8)) * 48.;
        let panel = div()
            .w(px(560.))
            .max_w(relative(0.9))
            .rounded(px(12.))
            .bg(cx.theme().popover)
            .shadow_lg()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Input::new(&self.command_search)
                    .large()
                    .w_full()
                    .prefix(Icon::new(IconName::Search))
                    .cleanable(true),
            )
            .child(
                uniform_list(
                    "command-results",
                    count,
                    cx.processor(|view, range: std::ops::Range<usize>, _, cx| {
                        let commands = view.commands(cx);
                        range
                            .filter_map(|index| {
                                commands.get(index).map(|(label, icon, command)| {
                                    let command = command.clone();
                                    let leading = match &command {
                                        LauncherCommand::SelectGame(id) => view
                                            .snapshot
                                            .games
                                            .iter()
                                            .find(|game| &game.id == id)
                                            .and_then(|game| {
                                                game.artwork.get(&ArtworkKind::Icon).or_else(|| {
                                                    game.artwork.get(&ArtworkKind::Cover)
                                                })
                                            })
                                            .map(|asset| {
                                                img(asset.thumbnail_path.clone())
                                                    .size(px(28.))
                                                    .object_fit(ObjectFit::Cover)
                                                    .rounded(px(5.))
                                                    .into_any_element()
                                            }),
                                        _ => None,
                                    }
                                    .unwrap_or_else(|| icon.clone().small().into_any_element());
                                    div()
                                        .id(("command-result", index))
                                        .w_full()
                                        .h(px(48.))
                                        .px_3()
                                        .flex()
                                        .items_center()
                                        .gap_3()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .when(view.command_selected == index, |row| {
                                            row.bg(cx.theme().accent)
                                        })
                                        .hover(|row| row.bg(cx.theme().muted))
                                        .child(
                                            div()
                                                .w(px(28.))
                                                .flex_shrink_0()
                                                .flex()
                                                .justify_center()
                                                .child(leading),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .line_clamp(1)
                                                .child(label.clone()),
                                        )
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.run_command(command.clone(), window, cx)
                                        }))
                                        .into_any_element()
                                })
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .w_full()
                .h(px(height))
                .track_scroll(self.command_scroll.clone()),
            )
            .when(count == 0, |panel| {
                panel.child(div().p_4().text_color(cx.theme().muted_foreground).child(
                    if self.command_mode == Some(CommandMode::Games) {
                        "No matching games"
                    } else {
                        "No matching actions"
                    },
                ))
            });
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(cx.theme().overlay)
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| view.close_command_bar(window, cx)),
            )
            .child(panel)
            .into_any_element()
    }

    fn activity(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .absolute()
            .top(px(HEADER_HEIGHT + 6.))
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
                            .size(px(32.))
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
                                .h(px(36.))
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
                                        .h(px(36.))
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
        if self.screen == StartScreen::Recents && !self.settings_open {
            let games = self.recent_games();
            let next = games
                .iter()
                .position(|game| self.selected.as_ref() == Some(&game.id))
                .and_then(|index| games.get((index + 1) % games.len()));
            let resource = next.and_then(|game| {
                game.artwork
                    .get(&ArtworkKind::Background)
                    .or_else(|| game.artwork.get(&ArtworkKind::Cover))
                    .map(|asset| Resource::from(asset.image_path.clone()))
            });
            self.carousel_image_ready = resource.is_none_or(|resource| {
                self.full_resolution_images
                    .update(cx, |cache, cx| cache.load(&resource, window, cx).is_some())
            });
        }
        if let Some((_, started)) = self.sidebar_transition {
            if started.elapsed().as_secs_f32() >= 0.2 || appearance::reduced_motion() {
                self.sidebar_transition = None;
            } else {
                window.request_animation_frame();
            }
        }
        if let Some((_, started)) = &self.carousel_transition {
            if started.elapsed() >= RECENTS_TRANSITION
                || appearance::reduced_motion()
                || self.screen != StartScreen::Recents
                || self.settings_open
            {
                self.carousel_transition = None;
            } else {
                window.request_animation_frame();
            }
        }
        let sidebar = self.sidebar_progress();
        let content = if self.settings_open {
            self.settings(window, cx)
        } else if self.screen == StartScreen::Recents {
            self.recents(window, cx)
        } else if self.screen == StartScreen::Activity {
            self.activity_overview(cx)
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
                .child(self.library(window, cx));
            if let Some(game) = self.selected_game() {
                content = content.child(self.details(&game, window, cx));
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
            .on_action(cx.listener(Self::open_commands))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::launch_selected))
            .on_action(cx.listener(Self::open_settings))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(
                        div()
                            .relative()
                            .w(px(self.appearance.sidebar_width * sidebar))
                            .h_full()
                            .flex_shrink_0()
                            .overflow_hidden()
                            .when(sidebar > 0., |container| container.child(self.sidebar(cx)))
                            .when(sidebar == 1. && self.command_mode.is_none(), |container| {
                                container.child(
                                    div()
                                        .id("sidebar-resize")
                                        .absolute()
                                        .right_0()
                                        .top(px(HEADER_HEIGHT))
                                        .bottom_0()
                                        .w(px(6.))
                                        .cursor_col_resize()
                                        .occlude()
                                        .key_context("SidebarResize")
                                        .track_focus(&self.sidebar_resize_focus)
                                        .tab_stop(true)
                                        .focus(|style| style.bg(cx.theme().primary.opacity(0.35)))
                                        .hover(|style| style.bg(cx.theme().primary.opacity(0.35)))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|view, _, window, cx| {
                                                view.sidebar_resize_focus.focus(window);
                                                cx.notify();
                                            }),
                                        )
                                        .on_key_down(cx.listener(
                                            |view, event: &KeyDownEvent, window, cx| {
                                                let width = match event.keystroke.key.as_str() {
                                                    "left" => view.appearance.sidebar_width - 8.,
                                                    "right" => view.appearance.sidebar_width + 8.,
                                                    "home" => 192.,
                                                    "end" => 400.,
                                                    _ => return,
                                                };
                                                view.resize_sidebar(width, window, cx);
                                                cx.stop_propagation();
                                            },
                                        ))
                                        .on_drag(SidebarResize, |value, _, _, cx| {
                                            cx.new(|_| value.clone())
                                        })
                                        .on_drag_move(cx.listener(
                                            |view,
                                             event: &DragMoveEvent<SidebarResize>,
                                             window,
                                             cx| {
                                                view.resize_sidebar(
                                                    f32::from(event.event.position.x),
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .when(
                                self.settings_open || self.screen != StartScreen::Recents,
                                |main| main.pt(px(HEADER_HEIGHT + 12.)),
                            )
                            .child(content),
                    ),
            )
            .child(self.toolbar(sidebar, window, cx))
            .when(self.customization_open, |view| {
                view.child(self.customization(window, cx))
            })
            .when(self.jobs_open, |view| view.child(self.activity(cx)))
            .when(self.command_mode.is_some(), |view| {
                view.child(self.command_bar(cx))
            })
    }
}

fn saved_artwork_match(game: &Game) -> Option<ArtworkGame> {
    game.artwork_game_ids
        .get("steamgriddb")
        .map(|id| ArtworkGame {
            id: id.clone(),
            name: game.title().to_string(),
        })
}

fn reconciled_selection<'a>(
    selected: Option<&GameId>,
    displayed: impl Iterator<Item = &'a GameId>,
    preserve_selected: bool,
) -> Option<GameId> {
    if preserve_selected {
        return selected.cloned();
    }
    let mut first = None;
    for id in displayed {
        if Some(id) == selected {
            return Some(id.clone());
        }
        first.get_or_insert(id);
    }
    first.cloned()
}

#[cfg(test)]
#[::core::prelude::v1::test]
fn hidden_games_are_excluded_from_normal_views_and_restorable_in_hidden_view() {
    let mut game: Game = serde_json::from_value(serde_json::json!({
        "id": "epic:metro", "provider_id": "epic", "source_game_id": "metro",
        "discovered_title": "Metro Exodus Enhanced Edition", "install_directory": "C:/Games/Metro",
        "default_launch": { "Uri": "com.epicgames.launcher://apps/metro" }, "launch_override": null,
        "metadata": {}, "favourite": true, "available": true, "last_launched": null,
        "artwork": {}, "artwork_history": [], "artwork_game_ids": {}
    }))
    .expect("legacy discovered game");
    assert!(!game.hidden);
    assert!(saved_artwork_match(&game).is_none());
    game.artwork_game_ids
        .insert("steamgriddb".into(), "456".into());
    let reopened: Game = serde_json::from_value(serde_json::to_value(&game).unwrap()).unwrap();
    assert_eq!(saved_artwork_match(&reopened).unwrap().id, "456");
    for filter in [
        LibraryFilter::Installed,
        LibraryFilter::All,
        LibraryFilter::Favourites,
    ] {
        assert!(filter.includes(&game));
    }
    assert!(!LibraryFilter::Hidden.includes(&game));
    game.hidden = true;
    for filter in [
        LibraryFilter::Installed,
        LibraryFilter::All,
        LibraryFilter::Favourites,
    ] {
        assert!(!filter.includes(&game));
    }
    assert!(LibraryFilter::Hidden.includes(&game));
    game.hidden = false;
    assert!(LibraryFilter::Installed.includes(&game));
}

#[cfg(test)]
#[::core::prelude::v1::test]
fn filtered_selection_preserves_the_editor_owner_until_it_is_removed() {
    let edited = GameId("manual:edited".into());
    let other = GameId("manual:other".into());
    for displayed in [vec![&other], Vec::new(), vec![&other, &edited]] {
        assert_eq!(
            reconciled_selection(Some(&edited), displayed.into_iter(), true),
            Some(edited.clone())
        );
    }
    assert_eq!(
        reconciled_selection(Some(&edited), [&other].into_iter(), false),
        Some(other.clone())
    );
    assert_eq!(
        reconciled_selection(Some(&edited), [&other, &edited].into_iter(), false),
        Some(edited.clone())
    );
    assert_eq!(
        reconciled_selection(Some(&edited), std::iter::empty(), false),
        None
    );
    assert_eq!(
        reconciled_selection(None, [&other].into_iter(), false),
        Some(other)
    );
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
        .child(
            div()
                .text_base()
                .text_color(cx.theme().foreground)
                .font_weight(FontWeight::MEDIUM)
                .child(label.to_string()),
        )
        .child(
            div()
                .flex()
                .child(Input::new(state).large().w_auto().flex_1().min_w_0()),
        )
}

fn widget_surface(cx: &App) -> Div {
    div()
        .flex_1()
        .min_w(px(280.))
        .p_4()
        .rounded(cx.theme().radius_lg)
        .bg(cx.theme().background.alpha(0.96))
}

fn timestamp_label(timestamp: u64) -> String {
    i64::try_from(timestamp)
        .ok()
        .and_then(|timestamp| Local.timestamp_opt(timestamp, 0).single())
        .map(|date| date.format("%e %b %Y · %H:%M").to_string())
        .unwrap_or_else(|| "Date unavailable".into())
}

fn launch_count(count: u32) -> String {
    format!("{count} launch{}", if count == 1 { "" } else { "es" })
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

fn recents_title(
    id: SharedString,
    title: &str,
    font_size: Pixels,
    weight: FontWeight,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let title = SharedString::from(title.to_string());
    let started = *window
        .use_keyed_state(
            (SharedString::from(format!("{id}-{title}")), 0usize),
            cx,
            |_, _| Instant::now(),
        )
        .read(cx);
    let tooltip = title.clone();
    let reduced_motion = appearance::reduced_motion();
    div()
        .id(id)
        .w_full()
        .h(font_size * 1.2)
        .text_size(font_size)
        .font_weight(weight)
        .overflow_hidden()
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(
            canvas(
                move |_, window, _| {
                    let style = window.text_style();
                    window.text_system().shape_line(
                        title.clone(),
                        font_size,
                        &[TextRun {
                            len: title.len(),
                            font: style.font(),
                            color: style.color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        None,
                    )
                },
                move |bounds, line, window, cx| {
                    let overflow = f32::from((line.width - bounds.size.width).max(px(0.)));
                    let mut offset = 0.;
                    if overflow > 0. && !reduced_motion && window.is_window_active() {
                        let travel = overflow / 28.;
                        let elapsed = started.elapsed().as_secs_f32() % (2. * travel + 2.4);
                        offset = if elapsed < 1.2 {
                            0.
                        } else if elapsed < travel + 1.2 {
                            (elapsed - 1.2) * 28.
                        } else if elapsed < travel + 2.4 {
                            overflow
                        } else {
                            overflow - (elapsed - travel - 2.4) * 28.
                        };
                        window.request_animation_frame();
                    }
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        let _ = line.paint(
                            bounds.origin - point(px(offset), px(0.)),
                            bounds.size.height,
                            window,
                            cx,
                        );
                    });
                },
            )
            .size_full(),
        )
}

fn image(path: PathBuf, fit: ObjectFit, cx: &App) -> Img {
    let colors = cx.theme().colors;
    img(path)
        .size_full()
        .object_fit(fit)
        .with_fallback(move || placeholder("Image unavailable", colors).into_any_element())
}

fn placeholder(title: &str, colors: ThemeColor) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_5()
        .px_4()
        .bg(colors.muted)
        .child(div().w_6().h_1().bg(colors.primary))
        .child(
            div()
                .text_sm()
                .text_color(colors.muted_foreground)
                .text_center()
                .child(title.to_string()),
        )
}

fn source_name(provider_id: &str) -> &str {
    match provider_id {
        "steam" => "Steam",
        "epic" => "Epic Games",
        "gog" => "GOG",
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
