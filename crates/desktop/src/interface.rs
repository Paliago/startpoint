use crate::{
    appearance,
    components::{
        BUTTON_TEXT_REM, EditableFieldState, button, choice_button, choice_dropdown, choice_group,
        eyebrow, field, floating_panel, glass_panel, glass_panel_with_blur, hint, icon_button,
        image, input, modal_backdrop, navigation_row, navigation_tab, path_row, placeholder,
        sidebar_label, snug_sidebar_button, widget_surface,
    },
    image_cache::ArtworkImageCache,
    library_layout::{CONTENT_TOP, DETAIL_WIDTH, Geometry as LibraryGeometry, filter_reveal},
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
    button::{Button, ButtonVariants},
    input::{InputEvent, InputState, TextareaState},
    link::Link,
    menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
    spinner::Spinner,
    switch::Switch,
    tooltip::Tooltip,
};
use startpoint_sdk::{
    AppEvent, AppearanceMode, AppearanceSettings, Application as LauncherApplication, ArtworkAsset,
    ArtworkCandidate, ArtworkGame, ArtworkKind, Command, CredentialStatus, ExecutableLaunch, Game,
    GameId, JobId, JobState, LaunchTarget, LibraryDensity, LibraryLayout, Snapshot, StartScreen,
    UserMetadata,
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
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        #[cfg(target_os = "windows")]
        {
            div()
                .id("title-bar")
                .w_full()
                .h(px(HEADER_HEIGHT))
                .px(px(8.))
                .flex()
                .items_center()
                .bg(rgba(0x00000000))
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
            TitleBar::new()
                .bg(rgba(0x00000000))
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
    Manual,
    Hidden,
}

impl LibraryFilter {
    fn visible_in_navigation(self, count: usize) -> bool {
        match self {
            Self::Hidden => false,
            Self::Favourites | Self::Manual => count > 0,
            Self::Installed | Self::All => true,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Installed => "Installed",
            Self::All => "All games",
            Self::Favourites => "Favourites",
            Self::Manual => "Manually added",
            Self::Hidden => "Hidden",
        }
    }

    fn includes(self, game: &Game) -> bool {
        match self {
            Self::Hidden => game.hidden,
            Self::Installed => !game.hidden && game.available,
            Self::All => !game.hidden,
            Self::Favourites => !game.hidden && game.favourite,
            Self::Manual => !game.hidden && game.provider_id == "manual",
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

impl DetailTab {
    const EDITORS: [(Self, &'static str); 4] = [
        (Self::Metadata, "Metadata"),
        (Self::Artwork, "Images"),
        (Self::Launch, "Launch"),
        (Self::Journal, "Journal"),
    ];

    fn is_editor(self) -> bool {
        self != Self::Overview
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LibraryGameAction {
    Launch,
    Edit(DetailTab),
    Favourite,
    Hidden,
    Completed,
    Reidentify,
    Reveal,
    Remove,
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
    CustomizeLibrary,
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
    #[cfg(feature = "self-update")]
    updater: Entity<crate::updater::Updater>,
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
    translucency_slider: Entity<crate::translucency_slider::AmountSlider>,
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
    window_memory: crate::window_state::Memory,
    screen: StartScreen,
    settings_open: bool,
    jobs_open: bool,
    detail_tab: DetailTab,
    detail_closed: bool,
    editor_focus: FocusHandle,
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
    description: Entity<TextareaState>,
    developer: Entity<InputState>,
    genres: Entity<InputState>,
    metadata_baseline: UserMetadata,
    collections: Entity<TextareaState>,
    completed: bool,
    journal_text: Entity<TextareaState>,
    journal_entry: Option<String>,
    journal_delete: Option<String>,
    journal_job: Option<(JobId, GameId, String)>,
    activity_month: NaiveDate,
    activity_day: NaiveDate,
    executable: Entity<InputState>,
    arguments: Entity<TextareaState>,
    working_directory: Entity<InputState>,
    api_key: Entity<InputState>,
    artwork_search: Entity<InputState>,
    thumbnails: Entity<ArtworkImageCache>,
    full_resolution_images: Entity<ArtworkImageCache>,
    logos: Entity<crate::logo_cache::LogoCache>,
    focus: FocusHandle,
    library_list: ListState,
    library_list_key: Option<(LibraryGeometry, usize)>,
    library_reveal: Option<usize>,
    favourites_available: bool,
    favourites_transition: Option<(f32, Instant)>,
    artwork_texture: Arc<Image>,
    carousel_elapsed: Duration,
    carousel_tick: Option<Instant>,
    carousel_paused: bool,
    game_menu_open: bool,
    game_menu_subscription: Option<Subscription>,
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
        let mut window_memory = crate::window_state::Memory::new(&snapshot.paths.data_directory);
        window_memory.capture(window, cx);
        let bounds_subscription = cx.observe_window_bounds(window, |view, window, cx| {
            view.window_memory.capture(window, cx);
        });
        let quit_subscription = cx.on_app_quit(|view, _| {
            view.window_memory.flush();
            async {}
        });
        let appearance_settings = snapshot.settings.appearance.clone();
        let translucency_slider = cx.new(|cx| {
            crate::translucency_slider::AmountSlider::new(
                appearance_settings.translucency_amount,
                window,
                cx,
            )
        });
        let translucency_subscription = cx.subscribe_in(
            &translucency_slider,
            window,
            |view, _, event, window, cx| match event {
                crate::translucency_slider::Event::Changed(amount) => {
                    view.appearance.translucency_amount = *amount;
                    appearance::update_effects(&view.appearance, cx);
                    view.schedule_appearance_save(window, cx);
                }
                crate::translucency_slider::Event::Released => {
                    if view.appearance_save.is_some() {
                        view.set_appearance(view.appearance.clone(), window, cx);
                    }
                }
            },
        );
        let appearance_loaded = snapshot.ready;
        appearance::apply(&appearance_settings, Some(window), cx);
        let screen = appearance_settings.start_screen;
        let logos = cx.new(|_| crate::logo_cache::LogoCache::new(&snapshot.paths.cache_directory));
        let subscriptions = vec![
            subscription,
            appearance_subscription,
            key_subscription,
            bounds_subscription,
            quit_subscription,
            translucency_subscription,
        ];
        #[cfg(feature = "self-update")]
        let updater = cx.new(|cx| crate::updater::Updater::new(window, cx));
        #[cfg(feature = "self-update")]
        let subscriptions = {
            let mut subscriptions = subscriptions;
            subscriptions.push(cx.observe(&updater, |_, _, cx| cx.notify()));
            subscriptions.push(cx.subscribe_in(
                &updater,
                window,
                |view, updater, event, window, cx| match event {
                    crate::updater::Event::RestartRequested => {
                        if view.can_restart_for_update() {
                            updater.update(cx, |updater, cx| updater.restart(window, cx));
                        }
                    }
                    crate::updater::Event::RestartPrepared => {
                        view.window_memory.flush();
                        cx.quit();
                    }
                },
            ));
            subscriptions
        };
        let mut view = Self {
            api,
            #[cfg(feature = "self-update")]
            updater,
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
            translucency_slider,
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
            window_memory,
            screen,
            settings_open: false,
            jobs_open: false,
            detail_tab: DetailTab::Overview,
            detail_closed: false,
            editor_focus: cx.focus_handle(),
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
            description: textarea_input("Add a description", window, cx),
            developer: text_input("Developer", false, window, cx),
            genres: text_input("Action, Adventure…", false, window, cx),
            metadata_baseline: UserMetadata::default(),
            collections: textarea_input("One collection name per line", window, cx),
            completed: false,
            journal_text: textarea_input("What happened? What will you do next?", window, cx),
            journal_entry: None,
            journal_delete: None,
            journal_job: None,
            activity_month: Local::now()
                .date_naive()
                .with_day(1)
                .unwrap_or_else(|| Local::now().date_naive()),
            activity_day: Local::now().date_naive(),
            executable: text_input("Default launcher", false, window, cx),
            arguments: textarea_input("One argument per line", window, cx),
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
            logos,
            focus: cx.focus_handle(),
            library_list: ListState::new(1, ListAlignment::Top, px(400.)),
            library_list_key: None,
            library_reveal: None,
            favourites_available: false,
            favourites_transition: None,
            artwork_texture: Arc::new(Image::from_bytes(
                ImageFormat::Svg,
                include_bytes!("../assets/dither.svg").to_vec(),
            )),
            carousel_elapsed: Duration::ZERO,
            carousel_tick: None,
            carousel_paused: appearance::reduced_motion(),
            game_menu_open: false,
            game_menu_subscription: None,
            carousel_focus: cx.focus_handle(),
            carousel_transition: None,
            carousel_image_ready: false,
            _subscriptions: subscriptions,
        };
        view.filter(cx);
        view.ensure_selection(window, cx);
        view.focus.focus(window, cx);
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
                    let previous = self.metadata_baseline.description.as_deref().unwrap_or("");
                    let value = metadata.description.as_deref().unwrap_or("");
                    if previous != value && self.description.read(cx).value() == previous {
                        set_value(&self.description, value, window, cx);
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
                self.logos
                    .update(cx, |cache, cx| cache.invalidate_failed(cx));
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
        let previous = self
            .visible
            .iter()
            .map(|game| game.id.clone())
            .collect::<Vec<_>>();
        if self.library_filter == LibraryFilter::Favourites && self.favourite_count() == 0
            || self.library_filter == LibraryFilter::Manual && self.manual_count() == 0
        {
            self.library_filter = LibraryFilter::Installed;
        }
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
        if !previous.iter().eq(self.visible.iter().map(|game| &game.id)) {
            self.library_list_key = None;
        }
        self.library_list.splice(0..1, 1);
    }

    fn recent_games(&self) -> Vec<&Game> {
        installed_recents(&self.snapshot.games)
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
        self.detail_closed = false;
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
                    .child(match status {
                        CredentialStatus::Missing => "No key saved",
                        CredentialStatus::Stored => "Key saved",
                        CredentialStatus::Checking => "Checking…",
                        CredentialStatus::Verified => "Connected",
                        CredentialStatus::Invalid => "Key rejected",
                        CredentialStatus::Locked => "Unlock required",
                        CredentialStatus::Unavailable(_) => "Connection unavailable",
                    })
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
            .focus(window, cx);
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
                "Customize Library".into(),
                IconName::Palette.into(),
                LauncherCommand::CustomizeLibrary,
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
                    self.library_reveal = Some(index);
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
            LauncherCommand::CustomizeLibrary => {
                self.switch_screen(StartScreen::Library, window, cx);
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
            self.close_editor(window, cx);
            return;
        } else if self.screen == StartScreen::Library && self.shows_library_details() {
            self.close_detail_pane(window, cx);
            return;
        }
        self.ensure_selection(window, cx);
        self.focus.focus(window, cx);
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
        self.focus.focus(window, cx);
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
            self.library_reveal = Some(index);
        }
        self.focus.focus(window, cx);
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
        self.cancel_artwork();
        self.ensure_selection(window, cx);
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn shows_library_details(&self) -> bool {
        self.selected.is_some() && self.appearance.library_details && !self.detail_closed
    }

    #[cfg(feature = "self-update")]
    fn can_restart_for_update(&self) -> bool {
        let snapshot = self.api.snapshot();
        snapshot.ready
            && !snapshot.jobs.iter().any(|job| job.state.active())
            && !self.editor_is_modal()
            && self.theme_editor.is_none()
            && !self.credential_editing
            && self.appearance_save.is_none()
    }

    fn editor_is_modal(&self) -> bool {
        self.detail_tab.is_editor()
    }

    fn close_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.detail_tab = DetailTab::Overview;
        self.cancel_artwork();
        self.reidentifying = false;
        self.ensure_selection(window, cx);
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn close_detail_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_editor(window, cx);
        self.detail_closed = true;
        cx.notify();
    }

    fn library_geometry(&self, window: &Window) -> LibraryGeometry {
        let details = if self.screen == StartScreen::Library && self.shows_library_details() {
            DETAIL_WIDTH
        } else {
            0.
        };
        LibraryGeometry::new(
            f32::from(window.viewport_size().width)
                - details
                - self.appearance.sidebar_width * self.sidebar_progress(),
            &self.appearance,
            f32::from(window.rem_size()),
        )
    }

    fn prepare_library(&mut self, window: &Window) {
        let geometry = self.library_geometry(window);
        let key = (geometry, self.visible.len());
        if self.library_list_key != Some(key) {
            let previous = self
                .library_list_key
                .map(|_| self.library_list.logical_scroll_top());
            self.library_list
                .reset(1 + self.visible.len().div_ceil(geometry.columns).max(1));
            if let Some(previous) = previous {
                self.library_list.scroll_to(previous);
            }
            self.library_list_key = Some(key);
        }
        if let Some(index) = self.library_reveal.take() {
            self.library_list.scroll_to(ListOffset {
                item_ix: 1 + index / geometry.columns,
                offset_in_item: -px(CONTENT_TOP),
            });
        }
    }

    fn favourite_count(&self) -> usize {
        self.snapshot
            .games
            .iter()
            .filter(|game| !game.hidden && game.favourite)
            .count()
    }

    fn manual_count(&self) -> usize {
        self.snapshot
            .games
            .iter()
            .filter(|game| !game.hidden && game.provider_id == "manual")
            .count()
    }

    fn favourites_progress(&self) -> f32 {
        self.favourites_transition
            .map(|(from, started)| {
                let progress = (started.elapsed().as_secs_f32() / 0.22).clamp(0., 1.);
                filter_reveal(from, self.favourites_available, progress)
            })
            .unwrap_or(if self.favourites_available { 1. } else { 0. })
    }

    fn open_details(&mut self, tab: DetailTab, window: &mut Window, cx: &mut Context<Self>) {
        if !tab.is_editor() {
            self.close_editor(window, cx);
            return;
        }
        if self.selected.is_none() {
            return;
        }
        if self.detail_tab == DetailTab::Overview {
            self.populate_game_fields(window, cx);
        }
        self.settings_open = false;
        self.customization_open = false;
        self.detail_tab = tab;
        self.editor_focus.focus(window, cx);
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
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn resize_sidebar(&mut self, width: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.appearance.sidebar_width = width.clamp(176., 400.);
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
        let relayout = crate::library_layout::resets_scroll(&self.appearance, &settings);
        if self.appearance.library_layout != settings.library_layout
            || self.appearance.library_density != settings.library_density
            || self.appearance.library_details != settings.library_details
        {
            crate::diagnostics::record(&format!(
                "library layout={:?} size={:?} details={}",
                settings.library_layout, settings.library_density, settings.library_details
            ));
        }
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
            if relayout {
                self.library_list_key = None;
                self.library_reveal = None;
                self.library_list.scroll_to(ListOffset {
                    item_ix: 0,
                    offset_in_item: px(0.),
                });
            }
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
        self.switch_screen(
            if self.screen == StartScreen::Library {
                StartScreen::Library
            } else {
                StartScreen::Recents
            },
            window,
            cx,
        );
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
        let mut sources = BTreeMap::from([("manual".to_string(), self.manual_count())]);
        for provider in &self.snapshot.providers {
            if matches!(provider.id.as_str(), "steam" | "epic" | "gog" | "example") {
                sources.insert(provider.id.clone(), 0);
            }
        }
        for game in self
            .snapshot
            .games
            .iter()
            .filter(|game| !game.hidden && game.available && game.provider_id != "manual")
        {
            *sources.entry(game.provider_id.clone()).or_insert(0) += 1;
        }
        div()
            .id("sidebar-content")
            .w(px(self.appearance.sidebar_width))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .bg(crate::glass::sidebar(cx))
            .flex()
            .flex_col()
            .px_2()
            .pb_4()
            .pt(px(HEADER_HEIGHT + 16.))
            .gap(px(3.))
            .child(eyebrow("LIBRARY"))
            .child(
                button("all-games")
                    .map(|button| snug_sidebar_button(button, IconName::FolderClosed, cx))
                    .h(px(36.))
                    .ghost()
                    .child(sidebar_label(format!("Installed   {installed}")))
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
            .when(
                favourites > 0 || self.favourites_transition.is_some(),
                |sidebar| {
                    sidebar.child(
                        div()
                            .h(px(36. * self.favourites_progress()))
                            .flex_shrink_0()
                            .overflow_hidden()
                            .opacity(self.favourites_progress())
                            .child(
                                button("favourites")
                                    .map(|button| snug_sidebar_button(button, IconName::Star, cx))
                                    .h(px(36.))
                                    .ghost()
                                    .child(sidebar_label(format!("Favourites   {favourites}")))
                                    .disabled(favourites == 0)
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
                            ),
                    )
                },
            )
            .child(
                button("hidden-games")
                    .map(|button| snug_sidebar_button(button, IconName::EyeOff, cx))
                    .h(px(36.))
                    .ghost()
                    .border_0()
                    .w_full()
                    .child(sidebar_label(format!(
                        "Hidden   {}",
                        self.snapshot
                            .games
                            .iter()
                            .filter(|game| game.hidden)
                            .count()
                    )))
                    .selected(self.library_filter == LibraryFilter::Hidden)
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.library_filter = LibraryFilter::Hidden;
                        view.source_filter = None;
                        view.collection_filter = None;
                        view.switch_screen(StartScreen::Library, window, cx);
                    })),
            )
            .child(div().mt_4().child(eyebrow("SOURCES")))
            .children(
                sources
                    .into_iter()
                    .filter(|(source, count)| source != "manual" || *count > 0)
                    .map(|(source, count)| {
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
                        button(SharedString::from(format!("source-{source}")))
                            .map(|button| {
                                snug_sidebar_button(
                                    button,
                                    if source == "manual" {
                                        IconName::Plus
                                    } else {
                                        IconName::Folder
                                    },
                                    cx,
                                )
                            })
                            .h(px(36.))
                            .ghost()
                            .child(sidebar_label(format!("{name}   {count}")))
                            .selected(
                                self.source_filter.as_ref() == Some(&source)
                                    || source == "manual"
                                        && self.library_filter == LibraryFilter::Manual,
                            )
                            .border_0()
                            .w_full()
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.source_filter = if source == "manual" {
                                    None
                                } else {
                                    Some(source.clone())
                                };
                                view.collection_filter = None;
                                view.library_filter = if source == "manual" {
                                    LibraryFilter::Manual
                                } else {
                                    LibraryFilter::Installed
                                };
                                view.settings_open = false;
                                view.filter(cx);
                                view.ensure_selection(window, cx);
                                cx.notify();
                            }))
                    }),
            )
            .when(unavailable > 0, |sidebar| {
                sidebar.child(
                    button("unavailable")
                        .map(|button| snug_sidebar_button(button, IconName::FolderOpen, cx))
                        .h(px(36.))
                        .ghost()
                        .child(sidebar_label(format!(
                            "All saved   {}",
                            installed + unavailable
                        )))
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
                    .child(div().mt_4().child(eyebrow("COLLECTIONS")))
                    .children(self.collection_totals().into_iter().map(
                        |(name, (completed, total))| {
                            button(SharedString::from(format!("collection-{name}")))
                                .map(|button| {
                                    snug_sidebar_button(button, IconName::FolderClosed, cx)
                                })
                                .h(px(36.))
                                .ghost()
                                .w_full()
                                .border_0()
                                .child(sidebar_label(format!("{name}   {completed}/{total}")))
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
                button("settings")
                    .map(|button| snug_sidebar_button(button, IconName::Settings, cx))
                    .h(px(36.))
                    .ghost()
                    .child(sidebar_label("Settings"))
                    .selected(self.settings_open)
                    .border_0()
                    .w_full()
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.open_settings(&OpenSettings, window, cx)
                    })),
            )
            .into_any_element()
    }

    fn game_row(
        &self,
        game: &Game,
        geometry: LibraryGeometry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // ponytail: long labels clip to one line; restore ellipses after GPUI fixes width-sensitive text caching.
        let id = game.id.clone();
        let launch_id = id.clone();
        let group = SharedString::from(format!("library-hover-{}", id.0));
        let selected = self.selected.as_ref() == Some(&id);
        let cover = game
            .artwork
            .get(&ArtworkKind::Background)
            .or_else(|| game.artwork.get(&ArtworkKind::Cover))
            .or_else(|| game.artwork.get(&ArtworkKind::Icon));
        let row = div()
            .id(SharedString::from(id.0.clone()))
            .group(group.clone())
            .flex_1()
            .min_w_0()
            .h(px(geometry.row_height - geometry.gap))
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
                    .w(px(geometry.artwork_height * 1.8125))
                    .h(px(geometry.artwork_height))
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
                    .when(
                        self.appearance.library_last_played || self.appearance.library_sources,
                        |caption| caption.child(hint(&self.library_subtitle(game))),
                    ),
            )
            .when(self.appearance.library_favourite_badges, |row| {
                row.child(
                    div()
                        .when(!game.favourite, |star| {
                            star.opacity(0.)
                                .group_hover(group.clone(), |style| style.opacity(1.))
                        })
                        .child(self.library_favourite_button(game, cx)),
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
                view.focus.focus(window, cx);
            }))
            .on_double_click(cx.listener(move |view, _, window, cx| {
                view.run_library_game_action(&launch_id, LibraryGameAction::Launch, window, cx)
            }));
        self.with_library_context_menu(game, row, cx)
    }

    fn library(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let geometry = self.library_geometry(window);
        list(
            self.library_list.clone(),
            cx.processor(move |view, row: usize, window, cx| {
                if row == 0 {
                    return view.library_header(window, cx);
                }
                if !view.snapshot.ready || view.visible.is_empty() {
                    return div()
                        .w_full()
                        .h(px(320.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_4()
                        .px_5()
                        .when(
                            !view.snapshot.ready && view.snapshot.initialization_error.is_none(),
                            |content| content.child(Spinner::new()),
                        )
                        .child(div().text_lg().child(if !view.snapshot.ready {
                            view.snapshot
                                .initialization_error
                                .clone()
                                .unwrap_or_else(|| "Opening your cached library…".into())
                        } else if view.snapshot.games.is_empty() {
                            "No games yet".into()
                        } else {
                            "No games in this view".into()
                        }))
                        .when(view.snapshot.ready, |content| {
                            content.child(
                                button("empty-add").primary().label("Add a game").on_click(
                                    cx.listener(|view, _, window, cx| {
                                        view.pick(PickerPurpose::AddGame, window, cx)
                                    }),
                                ),
                            )
                        })
                        .into_any_element();
                }
                let mut line = div()
                    .w_full()
                    .h(px(geometry.row_height))
                    .px(px(20.))
                    .flex()
                    .gap(px(geometry.gap));
                for column in 0..geometry.columns {
                    if let Some(game) = view.visible.get((row - 1) * geometry.columns + column) {
                        line = line.child(view.library_card(game, geometry, window, cx));
                    } else {
                        line = line.child(div().flex_1().min_w_0());
                    }
                }
                line.into_any_element()
            }),
        )
        .flex_1()
        .min_w_0()
        .h_full()
        .into_any_element()
    }

    fn library_header(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let title = self
            .collection_filter
            .clone()
            .or_else(|| {
                self.source_filter
                    .as_deref()
                    .map(source_name)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| {
                if self.library_filter == LibraryFilter::Hidden {
                    "Hidden games"
                } else {
                    "Library"
                }
                .into()
            });
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .px(px(20.))
            .pt(px(CONTENT_TOP))
            .pb_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_2xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        button("sort-library")
                            .secondary()
                            .h(px(32.))
                            .px_2()
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
            .child(
                div().flex().flex_wrap().items_center().gap_0().children(
                    [
                        LibraryFilter::Installed,
                        LibraryFilter::All,
                        LibraryFilter::Favourites,
                        LibraryFilter::Manual,
                    ]
                    .into_iter()
                    .filter(|filter| {
                        (*filter == LibraryFilter::Favourites
                            && self.favourites_transition.is_some())
                            || filter.visible_in_navigation(
                                self.snapshot
                                    .games
                                    .iter()
                                    .filter(|game| filter.includes(game))
                                    .count(),
                            )
                    })
                    .map(|filter| {
                        let count = self
                            .snapshot
                            .games
                            .iter()
                            .filter(|game| filter.includes(game))
                            .count();
                        let label = format!("{}  {count}", filter.label());
                        let width = filter_button_width(&label, window);
                        let selected = self.library_filter == filter
                            && self.source_filter.is_none()
                            && self.collection_filter.is_none();
                        let button = choice_button(filter.label(), label, selected, cx)
                            .h(px(32.))
                            .px_2()
                            .disabled(filter == LibraryFilter::Favourites && count == 0)
                            .selected(selected)
                            .border_0()
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.library_filter = filter;
                                view.source_filter = None;
                                view.collection_filter = None;
                                view.filter(cx);
                                view.ensure_selection(window, cx);
                                cx.notify();
                            }));
                        let progress = if filter == LibraryFilter::Favourites {
                            self.favourites_progress()
                        } else {
                            1.
                        };
                        // Animate the trailing gap too; removing a zero-width wrapper
                        // must not make neighboring buttons jump at the last frame.
                        let extent = width + window.rem_size() * 0.25;
                        {
                            div()
                                .w(extent * progress)
                                .flex_shrink_0()
                                .overflow_hidden()
                                .opacity(progress)
                                .child(button.w(width))
                                .into_any_element()
                        }
                    }),
                ),
            )
            .into_any_element()
    }

    fn library_card(
        &self,
        game: &Game,
        geometry: LibraryGeometry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.appearance.library_layout == LibraryLayout::Pills {
            return self.game_row(game, geometry, cx);
        }
        let id = game.id.clone();
        let launch_id = id.clone();
        let group = SharedString::from(format!("library-hover-{}", id.0));
        let selected = self.selected.as_ref() == Some(&id);
        let compact = self.appearance.library_layout == LibraryLayout::Compact;
        let cover = if self.appearance.library_layout == LibraryLayout::Tiles {
            game.artwork
                .get(&ArtworkKind::Background)
                .or_else(|| game.artwork.get(&ArtworkKind::Cover))
        } else {
            game.artwork
                .get(&ArtworkKind::Cover)
                .or_else(|| game.artwork.get(&ArtworkKind::Background))
        }
        .or_else(|| game.artwork.get(&ArtworkKind::Icon));
        let artwork = div()
            .relative()
            .overflow_hidden()
            .rounded(px(8.))
            .bg(cx.theme().muted)
            .h(px(geometry.artwork_height))
            .flex_shrink_0()
            .when(compact, |art| {
                art.w(px(geometry.artwork_height)).flex_shrink_0()
            })
            .when(!compact, |art| art.w_full())
            .child(match cover {
                Some(asset) => {
                    image(asset.thumbnail_path.clone(), ObjectFit::Cover, cx).into_any_element()
                }
                None => placeholder("", cx.theme().colors).into_any_element(),
            })
            .when(
                !compact && self.appearance.library_favourite_badges,
                |art| {
                    art.child(
                        div()
                            .absolute()
                            .top(px(6.))
                            .right(px(6.))
                            .when(!game.favourite, |star| {
                                star.opacity(0.)
                                    .group_hover(group.clone(), |style| style.opacity(1.))
                            })
                            .child(self.library_favourite_button(game, cx)),
                    )
                },
            )
            .when(!compact, |art| {
                let play_id = game.id.clone();
                let menu_game = game.clone();
                let owner = cx.weak_entity();
                art.child(
                    div()
                        .absolute()
                        .bottom(px(6.))
                        .left(px(6.))
                        .right(px(6.))
                        .flex()
                        .justify_between()
                        .opacity(0.)
                        .group_hover(group.clone(), |style| style.opacity(1.))
                        .child(
                            button(SharedString::from(format!("card-play-{}", game.id.0)))
                                .secondary()
                                .w(px(28.))
                                .h(px(28.))
                                .p_0()
                                .icon(Icon::default().path("icons/play.svg"))
                                .bg(crate::glass::Palette::of(cx).button)
                                .tooltip("Play game · double-click the card to launch")
                                .disabled(!game.available && game.launch_override.is_none())
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    cx.stop_propagation();
                                    view.run_library_game_action(
                                        &play_id,
                                        LibraryGameAction::Launch,
                                        window,
                                        cx,
                                    );
                                })),
                        )
                        .child(
                            button(SharedString::from(format!("card-manage-{}", game.id.0)))
                                .secondary()
                                .w(px(28.))
                                .h(px(28.))
                                .p_0()
                                .icon(IconName::Ellipsis)
                                .bg(crate::glass::Palette::of(cx).button)
                                .tooltip("Manage game")
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .dropdown_menu(move |menu, _, cx| {
                                    library_game_menu(menu, &menu_game, owner.clone(), cx)
                                }),
                        ),
                )
            });
        let caption = div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(4.))
            .when(compact, |caption| caption.flex_1())
            .when(!compact, |caption| {
                caption
                    .w_full()
                    .px_1()
                    .h(px(geometry.caption_height))
                    .flex_shrink_0()
            })
            .child(recents_title(
                SharedString::from(format!("library-title-{}", id.0)),
                game.title(),
                px(geometry.title_font_size),
                FontWeight::SEMIBOLD,
                window,
                cx,
            ))
            .when(compact, |caption| {
                let launches = self
                    .snapshot
                    .activity
                    .iter()
                    .map(|day| day.games.get(&game.id).copied().unwrap_or(0))
                    .fold(0u32, u32::saturating_add);
                let summary = compact_summary(game, launches);
                caption.child(library_metadata(&summary, geometry))
            })
            .when(!compact && self.appearance.library_last_played, |caption| {
                caption.child(library_metadata(&last_played(game), geometry))
            })
            .when(!compact && self.appearance.library_sources, |caption| {
                caption.child(library_metadata(source_name(&game.provider_id), geometry))
            });
        let card = div()
            .id(SharedString::from(format!("library-card-{}", id.0)))
            .group(group.clone())
            .flex_1()
            .min_w_0()
            .h(px(geometry.row_height - geometry.gap))
            .flex()
            .gap(px(6.))
            .rounded(px(10.))
            .p(px(4.))
            .when(compact, |card| card.flex_row().items_center().px_2())
            .when(!compact, |card| card.flex_col())
            .when(selected, |card| card.bg(cx.theme().accent))
            .hover(|card| card.bg(cx.theme().muted))
            .cursor_pointer()
            .child(artwork)
            .child(caption)
            .when(compact, |card| {
                card.child(
                    div()
                        .w(window.rem_size() * 10.5)
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(library_metadata(
                            &library_source_status(game, self.appearance.library_sources),
                            geometry,
                        ))
                        .when(self.appearance.library_last_played, |column| {
                            column.child(library_metadata(&last_played(game), geometry))
                        }),
                )
            })
            .when(
                compact && self.appearance.library_favourite_badges,
                |card| {
                    card.child(
                        div()
                            .when(!game.favourite, |star| {
                                star.opacity(0.)
                                    .group_hover(group.clone(), |style| style.opacity(1.))
                            })
                            .child(self.library_favourite_button(game, cx)),
                    )
                },
            )
            .on_click(cx.listener(move |view, _, window, cx| {
                view.select_game(id.clone(), window, cx);
                view.focus.focus(window, cx);
            }))
            .on_double_click(cx.listener(move |view, _, window, cx| {
                view.run_library_game_action(&launch_id, LibraryGameAction::Launch, window, cx)
            }));
        self.with_library_context_menu(game, card, cx)
    }

    fn library_subtitle(&self, game: &Game) -> String {
        library_subtitle(
            game,
            self.appearance.library_last_played,
            self.appearance.library_sources,
        )
    }

    fn run_library_game_action(
        &mut self,
        id: &GameId,
        action: LibraryGameAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(game) = self
            .api
            .snapshot()
            .games
            .iter()
            .find(|game| &game.id == id)
            .cloned()
        else {
            return;
        };
        match action {
            LibraryGameAction::Launch => {
                if game.available || game.launch_override.is_some() {
                    self.submit(Command::LaunchGame { game_id: game.id }, cx);
                }
            }
            LibraryGameAction::Favourite => {
                self.submit(
                    Command::SetFavourite {
                        game_id: game.id,
                        favourite: !game.favourite,
                    },
                    cx,
                );
            }
            LibraryGameAction::Hidden => {
                self.submit(
                    Command::SetHidden {
                        game_id: game.id,
                        hidden: !game.hidden,
                    },
                    cx,
                );
            }
            LibraryGameAction::Completed => {
                self.submit(
                    Command::SetGameProgress {
                        game_id: game.id,
                        collections: game.collections,
                        completed: !game.completed,
                    },
                    cx,
                );
            }
            LibraryGameAction::Reveal => cx.reveal_path(&game.install_directory),
            LibraryGameAction::Edit(tab) => {
                if self.selected.as_ref() != Some(id) {
                    self.select_game(id.clone(), window, cx);
                }
                self.open_details(tab, window, cx);
            }
            LibraryGameAction::Reidentify => {
                if self.selected.as_ref() != Some(id) {
                    self.select_game(id.clone(), window, cx);
                }
                self.begin_reidentify(window, cx);
            }
            LibraryGameAction::Remove => {
                if game.provider_id == "manual" {
                    if self.selected.as_ref() != Some(id) {
                        self.select_game(id.clone(), window, cx);
                    }
                    self.remove_manual_game(window, cx);
                }
            }
        }
    }

    fn library_favourite_button(&self, game: &Game, cx: &mut Context<Self>) -> Button {
        let id = game.id.clone();
        button(SharedString::from(format!("star-{}", id.0)))
            .secondary()
            .w(px(26.))
            .h(px(26.))
            .p_0()
            .border_0()
            .icon(if game.favourite {
                Icon::default().path("icons/star-filled.svg")
            } else {
                Icon::new(IconName::Star)
            })
            .bg(crate::glass::Palette::of(cx).button)
            .text_color(cx.theme().foreground)
            .tooltip(if game.favourite {
                "Remove from favourites"
            } else {
                "Add to favourites"
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |view, _, window, cx| {
                cx.stop_propagation();
                view.run_library_game_action(&id, LibraryGameAction::Favourite, window, cx);
            }))
    }

    fn with_library_context_menu<E>(
        &self,
        game: &Game,
        card: E,
        cx: &mut Context<Self>,
    ) -> AnyElement
    where
        E: ParentElement + Styled + IntoElement + ContextMenuExt + 'static,
    {
        let owner = cx.weak_entity();
        let game = game.clone();
        // Give every context-menu wrapper its own scope; the extension's internal
        // element id is otherwise identical for each sibling card in a grid row.
        div()
            .id(SharedString::from(format!(
                "library-menu-scope-{}",
                game.id.0
            )))
            .flex_1()
            .min_w_0()
            .flex()
            .child(
                card.context_menu(move |menu, _, cx| {
                    library_game_menu(menu, &game, owner.clone(), cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|view, _, window, cx| {
                    if view.screen == StartScreen::Recents {
                        view.carousel_focus.focus(window, cx);
                        view.carousel_tick = None;
                    }
                }),
            )
            .into_any_element()
    }

    fn navigation_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        glass_panel(
            div()
                .id("navigation-controls")
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .flex_shrink_0()
                .h(px(36.))
                .p(px(2.))
                .rounded(px(12.))
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
                            navigation_tab(
                                SharedString::from(format!("view-{}", screen.label())),
                                screen.label(),
                                self.screen == screen,
                                cx,
                            )
                            .h(px(32.))
                            .px_3()
                            .label(screen.label())
                            .selected(self.screen == screen)
                            .border_0()
                            .on_click(cx.listener(
                                move |view, _, window, cx| view.switch_screen(screen, window, cx),
                            ))
                        }),
                    ),
                ),
            cx,
        )
        .into_any_element()
    }

    fn start_button(&self, game: &Game, cx: &mut Context<Self>) -> Button {
        button("start-game")
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
        let card = div()
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
                view.focus.focus(window, cx);
            }));
        self.with_library_context_menu(game, card, cx)
    }

    fn carousel_is_paused(&self, window: &Window, cx: &App) -> bool {
        self.carousel_paused
            || self.game_menu_open
            || self.carousel_focus.contains_focused(window, cx)
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

    fn recents_backdrop_image(
        &self,
        game: &Game,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        let artwork = game
            .artwork
            .get(&ArtworkKind::Background)
            .or_else(|| game.artwork.get(&ArtworkKind::Cover));
        let color = cx.theme().background.alpha(1.);
        // Use the settled sidebar width, not every intermediate animation frame.
        let sidebar = if self.appearance.sidebar_visible {
            self.appearance.sidebar_width
        } else {
            0.
        };
        let size = size(
            window.viewport_size().width - px(sidebar),
            window.viewport_size().height,
        );
        self.recents_fade.update(cx, |backdrop, cx| {
            backdrop.image(
                artwork.map(|asset| asset.image_path.clone()),
                color,
                size,
                self.appearance.dither_recents,
                self.appearance.fade_recents,
                window,
                cx,
            )
        })
    }

    fn recents_background(
        &self,
        game: &Game,
        opacity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let color = appearance::surface(cx);
        let image = self.recents_backdrop_image(game, window, cx);
        div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .opacity(opacity * color.a)
            .when(image.is_none(), |background| background.bg(color.alpha(1.)))
            .when_some(image, |background, image| {
                background.child(img(image).size_full().object_fit(ObjectFit::Cover))
            })
    }

    fn recents(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(game) = self.selected_game().filter(|_| self.snapshot.ready) else {
            return div()
                .size_full()
                .child(self.library(window, cx))
                .into_any_element();
        };
        let games = self.recent_games();
        let paused = self.carousel_is_paused(window, cx);
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
        let menu_game = game.clone();
        let owner = cx.weak_entity();
        div()
            .id("recents-carousel")
            .size_full()
            .relative()
            .overflow_hidden()
            .flex()
            .flex_col()
            .when_some(transition, |view, (previous, _)| {
                view.child(self.recents_background(previous, 1. - opacity, window, cx))
            })
            .child(self.recents_background(&game, opacity, window, cx))
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
                                                button("edit-recent-game")
                                                    .secondary()
                                                    .w(px(36.)).h(px(36.)).p_0()
                                                    .icon(IconName::Ellipsis)
                                                    .tooltip(
                                                        "Edit metadata, artwork or launch options",
                                                    )
                                                    .bg(crate::glass::Palette::of(cx).button)
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
                                                button("pause-recents")
                                                    .secondary()
                                                    .w(px(36.)).h(px(36.)).p_0()
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
                                                    .bg(crate::glass::Palette::of(cx).button)
                                                    .disabled(games.len() < 2)
                                                    .on_click(cx.listener(
                                                        |view, _, window, cx| {
                                                            view.carousel_paused = !view
                                                                .carousel_is_paused(window, cx);
                                                            view.carousel_tick = None;
                                                            if !view.carousel_paused {
                                                                view.focus.focus(window, cx);
                                                            }
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                            .child(
                                                button("previous-recent")
                                                    .secondary()
                                                    .w(px(36.)).h(px(36.)).p_0()
                                                    .icon(IconName::ChevronLeft)
                                                    .tooltip("Previous game")
                                                    .bg(crate::glass::Palette::of(cx).button)
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
                                                    button(("recent-position", index))
                                                        .secondary()
                                                        .border_0()
                                                        .p_0()
                                                        .tooltip(game.title().to_string())
                                                        .w(px(28.)).h(px(28.))
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
                                                                view.focus.focus(window, cx);
                                                            },
                                                        ))
                                                },
                                            ))
                                            .child(
                                                button("next-recent")
                                                    .secondary()
                                                    .w(px(36.)).h(px(36.)).p_0()
                                                    .icon(IconName::ChevronRight)
                                                    .tooltip("Next game")
                                                    .bg(crate::glass::Palette::of(cx).button)
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
                                        button("recents-view-library")
                                            .secondary()
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
                            .bg(appearance::surface(cx))
                            .when(self.appearance.recents_activity, |widgets| widgets.child(glass_panel(widget_surface(cx).child(self.activity_calendar(cx)), cx)))
                            .when(self.appearance.recents_progress, |widgets| widgets.child(glass_panel(widget_surface(cx).child(self.collection_progress(cx)), cx)))
                            .when(self.appearance.recents_journal, |widgets| widgets.child(glass_panel(widget_surface(cx).child(self.journal_widget(cx)), cx))))
                    }),
            )
            .on_mouse_down(MouseButton::Right, cx.listener(|view, _, window, cx| {
                view.carousel_focus.focus(window, cx);
                view.carousel_tick = None;
            }))
            .context_menu(move |menu, _, cx| library_game_menu(menu, &menu_game, owner.clone(), cx))
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
        self.toolbar_button(
            "background-jobs",
            IconName::Bell,
            "Activity and notifications",
        )
        .tooltip(self.notice.clone().unwrap_or(tooltip))
        .selected(self.jobs_open)
        .relative()
        .when(self.notice.is_some(), |button| {
            button.text_color(cx.theme().primary)
        })
        .when(busy || self.notice.is_some(), |button| {
            button.child(
                div()
                    .absolute()
                    .right(px(3.))
                    .top(px(3.))
                    .size(px(5.))
                    .rounded_full()
                    .bg(cx.theme().primary),
            )
        })
        .on_click(cx.listener(|view, _, window, cx| {
            view.jobs_open = !view.jobs_open;
            view.focus.focus(window, cx);
            cx.notify();
        }))
    }

    fn toolbar_button(
        &self,
        id: &'static str,
        icon: impl Into<Icon>,
        label: &'static str,
    ) -> Button {
        icon_button(id, icon, label)
            .ghost()
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
                            .child(glass_panel_with_blur(
                                div().absolute().inset_0().opacity(1. - sidebar),
                                crate::glass::BLUR_RADIUS * (1. - sidebar),
                                cx,
                            ))
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
                    .map(|bar| {
                        #[cfg(feature = "self-update")]
                        {
                            let restart_allowed = self.can_restart_for_update();
                            bar.child(
                                self.updater
                                    .update(cx, |updater, cx| updater.badge(restart_allowed, cx)),
                            )
                        }
                        #[cfg(not(feature = "self-update"))]
                        {
                            bar
                        }
                    })
                    .child(glass_panel(
                        div()
                            .id("action-toolbar-island")
                            .occlude()
                            .h(px(36.))
                            .px(px(6.))
                            .rounded(px(12.))
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .gap(px(2.))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .when(
                                matches!(self.screen, StartScreen::Recents | StartScreen::Library)
                                    && !self.settings_open,
                                |controls| {
                                    controls.child(
                                        self.toolbar_button(
                                            if self.screen == StartScreen::Library {
                                                "customize-library"
                                            } else {
                                                "customize-recents"
                                            },
                                            IconName::Palette,
                                            if self.screen == StartScreen::Library {
                                                "Customize Library"
                                            } else {
                                                "Customize Recents"
                                            },
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
                                                view.focus.focus(window, cx);
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
                        cx,
                    ))
                    .into_any_element(),
            })
    }

    fn hero(&self, game: &Game, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let artwork = crate::detail_artwork::background(game);
        let metadata = game.effective_metadata();
        div()
            .image_cache(self.full_resolution_images.clone())
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background.alpha(0.35))
            .text_color(cx.theme().foreground)
            .child(div().absolute().size_full().child(match artwork {
                Some(asset) => {
                    image(asset.image_path.clone(), ObjectFit::Cover, cx).into_any_element()
                }
                None => div().size_full().bg(cx.theme().muted).into_any_element(),
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
            .child(
                div()
                    .absolute()
                    .top(px(CONTENT_TOP))
                    .left_4()
                    .right(px(52.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_lg()
                            .bg(crate::glass::Palette::of(cx).button)
                            .text_xs()
                            .child(source_name(&game.provider_id).to_string()),
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
                    .child(crate::detail_artwork::heading(
                        game,
                        DETAIL_WIDTH - f32::from(window.rem_size()) * 2.5,
                        (f32::from(window.viewport_size().height) - CONTENT_TOP - 330.)
                            .clamp(160., 320.),
                        &self.logos,
                        cx,
                    ))
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
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(self.start_button(game, cx).w_auto().flex_1().min_w_0())
                                    .child(
                                        self.library_favourite_button(game, cx)
                                            .w(px(44.))
                                            .h(px(44.))
                                            .bg(cx.theme().secondary),
                                    )
                                    .child(
                                        icon_button(
                                            "edit-game",
                                            IconName::Ellipsis,
                                            "Edit metadata, artwork or launch options",
                                        )
                                        .w(px(44.))
                                        .h(px(44.))
                                        .bg(cx.theme().secondary)
                                        .on_click(
                                            cx.listener(|view, _, window, cx| {
                                                view.open_details(DetailTab::Metadata, window, cx)
                                            }),
                                        ),
                                    ),
                            )
                            .when(game.last_launched.is_some(), |footer| {
                                footer.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(last_played(game)),
                                )
                            }),
                    )
                    .when(artwork.is_none(), |hero| {
                        hero.child(
                            button("find-detail-background")
                                .secondary()
                                .h(px(36.))
                                .label("Choose background…")
                                .text_color(cx.theme().foreground)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.cancel_artwork();
                                    view.artwork_kind = ArtworkKind::Background;
                                    view.artwork_images.clear();
                                    view.open_details(DetailTab::Artwork, window, cx)
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    fn detail_overview(&self, game: &Game, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .relative()
            .w(px(DETAIL_WIDTH))
            .flex_shrink_0()
            .h_full()
            .child(
                div()
                    .id("overview-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .h(window.viewport_size().height.max(px(620.)))
                            .child(self.hero(game, window, cx)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(CONTENT_TOP))
                    .right(px(12.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        icon_button("close-detail-pane", IconName::Close, "Close detail pane")
                            .bg(crate::glass::Palette::of(cx).button)
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.close_detail_pane(window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    fn game_editor(&self, game: &Game, cx: &mut Context<Self>) -> AnyElement {
        let game_id = game.id.clone();
        let favourite_id = game_id.clone();
        let favourite = game.favourite;
        let content = match self.detail_tab {
            DetailTab::Overview => return div().into_any_element(),
            DetailTab::Metadata => div().w_full().flex().flex_col().gap_4()
                .child(button("reidentify-game").h(px(36.)).label("Re-identify game…").icon(IconName::Search)
                    .on_click(cx.listener(|view, _, window, cx| view.begin_reidentify(window, cx))))
                .child(field("Title", &self.title, cx)).child(field("Description", &self.description, cx)).child(field("Developer", &self.developer, cx)).child(field("Genres", &self.genres, cx))
                .child(button("save-metadata").h(px(36.)).primary().label("Save metadata").on_click(cx.listener(|view, _, _, cx| view.save_metadata(cx))))
                .child(div().mt_3().child(eyebrow("COLLECTIONS & PROGRESS")))
                .child(field("Collections — one per line", &self.collections, cx))
                .child(Switch::new("game-completed").checked(self.completed).label("Completed")
                    .on_click(cx.listener(|view, completed, _, cx| { view.completed = *completed; cx.notify(); })))
                .child(button("save-game-progress").h(px(36.)).label("Save collections and progress").on_click(cx.listener(|view, _, _, cx| view.save_progress(cx))))
                .child(button("set-game-hidden").h(px(36.)).secondary().label(if game.hidden { "Unhide game" } else { "Hide from library" })
                    .on_click(cx.listener(move |view, _, _, cx| {
                        if let Some(game) = view.selected_game() {
                            view.submit(Command::SetHidden { game_id: game.id, hidden: !game.hidden }, cx);
                        }
                    })))
                .child(hint("Hidden games remain saved. Use the Hidden view to restore them. No game files, notes or launch history are removed."))
                .when(game.provider_id == "manual", |content| content.child(button("remove-manual-game").h(px(36.)).secondary().label("Remove from library")
                    .on_click(cx.listener(|view, _, window, cx| view.remove_manual_game(window, cx)))))
                .into_any_element(),
            DetailTab::Launch => div().w_full().flex().flex_col().gap_4()
                .child(hint(&format!("{}\n{}", source_name(&game.provider_id), game.install_directory.display())))
                .child(field("Executable / application bundle", &self.executable, cx))
                .child(button("choose-executable").h(px(36.)).label("Choose executable…").tooltip("Overrides the store launcher and its update, DRM and launch configuration.").on_click(cx.listener(move |view, _, window, cx| view.pick(PickerPurpose::LaunchOverride(game_id.clone()), window, cx))))
                .child(field("Arguments — one per line", &self.arguments, cx))
                .child(field(if cfg!(target_os = "macos") { "Working directory (executable only)" } else { "Working directory" }, &self.working_directory, cx))
                .child(button("save-launch").h(px(36.)).primary().label("Save launch configuration").on_click(cx.listener(|view, _, _, cx| view.save_launch(cx))))
                .child(button("reset-launch").h(px(36.)).secondary().label("Restore launcher default").on_click(cx.listener(|view, _, window, cx| {
                    if let Some(game_id) = view.selected.clone() {
                        view.submit(Command::SetLaunchOverride { game_id, launch: None }, cx);
                        set_value(&view.executable, "", window, cx); set_value(&view.arguments, "", window, cx); set_value(&view.working_directory, "", window, cx);
                    }
                }))).into_any_element(),
            DetailTab::Artwork => self.artwork_picker(game, cx),
            DetailTab::Journal => self.game_journal(game, cx),
        };
        div()
            .id("game-editor-shell")
            .w_full()
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_5()
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_2xl()
                            .line_clamp(1)
                            .overflow_hidden()
                            .child(game.title().to_string()),
                    )
                    .child(
                        icon_button("close-game-editor", IconName::Close, "Close game editor")
                            .tooltip("Close game editor")
                            .on_click(
                                cx.listener(|view, _, window, cx| view.close_editor(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .id("game-editor-scroll")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .p_5()
                            .pt_0()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        button("details-play")
                                            .h(px(36.))
                                            .primary()
                                            .label("Play game")
                                            .disabled(
                                                !game.available && game.launch_override.is_none(),
                                            )
                                            .on_click(cx.listener(|view, _, _, cx| {
                                                if let Some(game_id) = view.selected.clone() {
                                                    view.submit(
                                                        Command::LaunchGame { game_id },
                                                        cx,
                                                    );
                                                }
                                            })),
                                    )
                                    .child(
                                        button("details-favourite")
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
                            .child(choice_group(cx).w_full().children(
                                DetailTab::EDITORS.into_iter().map(|(tab, label)| {
                                    choice_button(label, label, self.detail_tab == tab, cx)
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            view.detail_tab = tab;
                                            cx.notify();
                                        }))
                                }),
                            ))
                            .child(div().w_full().child(content)),
                    ),
            )
            .into_any_element()
    }

    fn game_editor_modal(
        &self,
        game: &Game,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        modal_backdrop("game-editor-backdrop")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| {
                    view.close_editor(window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(glass_panel(
                div()
                    .id("game-editor-modal")
                    .track_focus(&self.editor_focus)
                    .tab_group()
                    .w(px(680.).min(window.viewport_size().width - px(48.)))
                    .h((window.viewport_size().height - px(48.)).min(px(820.)))
                    .rounded(cx.theme().radius_lg)
                    .overflow_hidden()
                    .shadow_lg()
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.game_editor(game, cx)),
                cx,
            ))
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
                choice_group(cx).children(
                    [
                        ArtworkKind::Cover,
                        ArtworkKind::Background,
                        ArtworkKind::Logo,
                        ArtworkKind::Icon,
                    ]
                    .into_iter()
                    .map(|kind| {
                        choice_button(kind.label(), kind.label(), kind == self.artwork_kind, cx)
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
                button("import-artwork")
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
                button("reidentify-artwork-game")
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
                    button("configure-artwork")
                        .h(px(36.))
                        .secondary()
                        .label("Configure SteamGridDB")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.open_settings(&OpenSettings, window, cx)
                        })),
                )
            })
            .child(
                div().flex().child(
                    input(&self.artwork_search, cx)
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
                        button("search-artwork-games")
                            .h(px(36.))
                            .label("Find game")
                            .disabled(!key_configured || self.identity_job.is_some())
                            .on_click(cx.listener(|view, _, _, cx| view.search_artwork(cx))),
                    )
                    .child(
                        button("load-artwork")
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
                button(SharedString::from(format!("match-{}", game.id)))
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
                                    button(("select-remote", index))
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
                button("identify-configure").label("Configure SteamGridDB")
                    .on_click(cx.listener(|view, _, window, cx| view.open_settings(&OpenSettings, window, cx)))))
            .child(input(&self.artwork_search, cx).large().disabled(!configured || saving))
            .child(div().flex().gap_2()
                .child(button("identify-search").primary().label("Search games").disabled(!configured || saving)
                    .on_click(cx.listener(|view, _, _, cx| view.search_artwork(cx))))
                .child(button("identify-cancel").secondary().label("Cancel").disabled(saving)
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
                button(SharedString::from(format!("identify-{}", game.id)))
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
                button(SharedString::from(format!("saved-{}", asset.id)))
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
        let selected = names
            .iter()
            .find(|name| name.eq_ignore_ascii_case(&self.appearance.theme))
            .map(ToString::to_string)
            .unwrap_or_else(|| self.appearance.theme.clone());
        let view = cx.weak_entity();
        let selector = choice_dropdown(
            "select-theme",
            selected,
            names
                .into_iter()
                .map(|name| (name.to_string(), name))
                .collect(),
            self.snapshot.ready && !self.themes_loading,
            move |name, window, cx| {
                let _ = view.update(cx, |view, cx| {
                    view.set_appearance(
                        AppearanceSettings {
                            theme: name,
                            ..view.appearance.clone()
                        },
                        window,
                        cx,
                    );
                    view.focus.focus(window, cx);
                });
            },
        );
        div()
            .h(px(34.))
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

    fn glass_controls(&self, cx: &mut Context<Self>) -> Div {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                Switch::new("window-translucency")
                    .checked(self.appearance.window_translucency)
                    .label("Translucency")
                    .disabled(
                        !self.snapshot.ready
                            || !cfg!(any(target_os = "windows", target_os = "macos")),
                    )
                    .on_click(cx.listener(|view, enabled, window, cx| {
                        view.set_appearance(
                            AppearanceSettings {
                                window_translucency: *enabled,
                                ..view.appearance.clone()
                            },
                            window,
                            cx,
                        );
                    })),
            )
            .child(
                Switch::new("backdrop-blur")
                    .checked(self.appearance.backdrop_blur)
                    .label("Backdrop blur")
                    .disabled(!self.snapshot.ready)
                    .on_click(cx.listener(|view, enabled, window, cx| {
                        view.set_appearance(
                            AppearanceSettings {
                                backdrop_blur: *enabled,
                                ..view.appearance.clone()
                            },
                            window,
                            cx,
                        );
                    })),
            )
            .child(self.translucency_slider.clone())
    }

    fn appearance_controls(&self, cx: &mut Context<Self>) -> Div {
        let catalog = cx.global::<theme::Catalog>();
        let warnings = catalog.warnings.clone();
        let missing = !catalog.contains(&self.appearance.theme);
        let custom = catalog
            .files
            .contains_key(&self.appearance.theme.to_ascii_lowercase());
        let themes = self.snapshot.paths.data_directory.join("themes");
        let owner = cx.weak_entity();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(eyebrow("APPEARANCE"))
            .child(self.theme_selector(cx))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        button("edit-theme")
                            .icon(IconName::Palette)
                            .label("Edit theme…")
                            .disabled(!self.snapshot.ready || self.themes_loading)
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.open_theme_editor(window, cx)
                            })),
                    )
                    .when(custom, |row| {
                        row.child(
                            button("rename-theme")
                                .label("Rename…")
                                .disabled(!self.snapshot.ready || self.themes_loading)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.open_theme_editor(window, cx);
                                    if let Some(editor) = &view.theme_editor {
                                        editor
                                            .update(cx, |editor, cx| editor.focus_name(window, cx));
                                    }
                                })),
                        )
                    })
                    .child(
                        icon_button("open-themes", IconName::Folder, "Open themes directory")
                            .on_click(move |_, _, cx| cx.reveal_path(&themes)),
                    )
                    .child(
                        icon_button(
                            "reload-themes",
                            Icon::default().path("icons/refresh.svg"),
                            "Reload themes",
                        )
                        .disabled(self.themes_loading)
                        .on_click(
                            cx.listener(|view, _, window, cx| view.reload_themes(window, cx)),
                        ),
                    ),
            )
            .when(missing && !self.themes_loading, |panel| {
                panel.child(hint("Selected theme unavailable; using Startpoint."))
            })
            .children(warnings.iter().map(|warning| hint(warning)))
            .child(eyebrow("COLOUR MODE"))
            .child(choice_dropdown(
                "colour-mode",
                self.appearance.mode,
                [
                    AppearanceMode::System,
                    AppearanceMode::Light,
                    AppearanceMode::Dark,
                ]
                .into_iter()
                .map(|mode| (mode, mode.label().into()))
                .collect(),
                self.snapshot.ready,
                move |mode, window, cx| {
                    let _ = owner.update(cx, |view, cx| {
                        view.set_appearance(
                            AppearanceSettings {
                                mode,
                                ..view.appearance.clone()
                            },
                            window,
                            cx,
                        )
                    });
                },
            ))
            .child(self.glass_controls(cx))
    }

    fn customization(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if self.screen == StartScreen::Library {
            return self.library_customization(window, cx);
        }
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
                        icon_button(
                            "close-customization",
                            IconName::Close,
                            "Close customization",
                        )
                        .tooltip("Close customization")
                        .disabled(saving)
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.close_theme_editor(window, cx);
                            view.customization_open = false;
                            view.focus.focus(window, cx);
                            cx.notify();
                        })),
                    ),
            );
        if let Some(editor) = &self.theme_editor {
            panel = panel.child(editor.clone());
        } else {
            panel = panel
                .child(self.appearance_controls(cx))
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
        floating_panel(
            "recents-customization",
            HEADER_HEIGHT + 8.,
            320.,
            window,
            cx,
        )
        .child(panel)
        .map(|panel| glass_panel(panel, cx))
        .into_any_element()
    }

    fn library_customization(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let saving = self
            .theme_editor
            .as_ref()
            .is_some_and(|editor| editor.read(cx).is_saving());
        let panel = div()
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
                    .child(eyebrow("CUSTOMIZE LIBRARY"))
                    .child(
                        icon_button(
                            "close-library-customization",
                            IconName::Close,
                            "Close customization",
                        )
                        .disabled(saving)
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.close_theme_editor(window, cx);
                            view.customization_open = false;
                            view.focus.focus(window, cx);
                            cx.notify();
                        })),
                    ),
            )
            .when_some(self.theme_editor.clone(), |panel, editor| {
                panel.child(editor)
            })
            .when(self.theme_editor.is_none(), |panel| {
                panel
                    .child(self.appearance_controls(cx))
                    .child(eyebrow("LAYOUT"))
                    .child({
                        let view = cx.weak_entity();
                        choice_dropdown(
                            "library-layout",
                            self.appearance.library_layout,
                            [
                                LibraryLayout::Pills,
                                LibraryLayout::Covers,
                                LibraryLayout::Tiles,
                                LibraryLayout::Compact,
                            ]
                            .into_iter()
                            .map(|layout| (layout, layout.label().into()))
                            .collect(),
                            self.snapshot.ready,
                            move |layout, window, cx| {
                                let _ = view.update(cx, |view, cx| {
                                    view.set_appearance(
                                        AppearanceSettings {
                                            library_layout: layout,
                                            ..view.appearance.clone()
                                        },
                                        window,
                                        cx,
                                    )
                                });
                            },
                        )
                    })
                    .child(eyebrow("SIZE & SPACING"))
                    .child(
                        choice_group(cx).children(
                            [
                                LibraryDensity::Small,
                                LibraryDensity::Medium,
                                LibraryDensity::Large,
                            ]
                            .into_iter()
                            .map(|density| {
                                choice_button(
                                    density.label(),
                                    density.label(),
                                    self.appearance.library_density == density,
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |view, _, window, cx| {
                                        view.set_appearance(
                                            AppearanceSettings {
                                                library_density: density,
                                                ..view.appearance.clone()
                                            },
                                            window,
                                            cx,
                                        )
                                    },
                                ))
                            }),
                        ),
                    )
                    .child(eyebrow("DISPLAY"))
                    .children(
                        [
                            (
                                "library-details",
                                "Game detail panel",
                                self.appearance.library_details,
                            ),
                            (
                                "library-last-played",
                                "Last played",
                                self.appearance.library_last_played,
                            ),
                            (
                                "library-sources",
                                "Source labels",
                                self.appearance.library_sources,
                            ),
                            (
                                "library-favourites",
                                "Favourite stars",
                                self.appearance.library_favourite_badges,
                            ),
                            (
                                "library-dither",
                                "Dither Library artwork",
                                self.appearance.dither_artwork,
                            ),
                        ]
                        .into_iter()
                        .map(|(id, label, checked)| {
                            Switch::new(id)
                                .label(label)
                                .checked(checked)
                                .on_click(cx.listener(move |view, enabled, window, cx| {
                                    let mut settings = view.appearance.clone();
                                    match id {
                                        "library-details" => settings.library_details = *enabled,
                                        "library-last-played" => {
                                            settings.library_last_played = *enabled
                                        }
                                        "library-sources" => settings.library_sources = *enabled,
                                        "library-favourites" => {
                                            settings.library_favourite_badges = *enabled
                                        }
                                        _ => settings.dither_artwork = *enabled,
                                    }
                                    view.set_appearance(settings, window, cx);
                                }))
                        }),
                    )
            });
        floating_panel(
            "library-customization",
            HEADER_HEIGHT + 8.,
            320.,
            window,
            cx,
        )
        .child(panel)
        .map(|panel| glass_panel(panel, cx))
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
                                    button(("collection-progress", index))
                                        .secondary()
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
        self.select_game(game_id, window, cx);
        self.open_details(DetailTab::Journal, window, cx);
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
                            button(if forward {
                                "activity-next-month"
                            } else {
                                "activity-previous-month"
                            })
                            .secondary()
                            .w(px(32.))
                            .h(px(32.))
                            .p_0()
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
                            button(SharedString::from(format!("activity-day-{key}")))
                                .secondary()
                                .border_0()
                                .flex_1()
                                .min_w_0()
                                .h(px(32.))
                                .p_0()
                                .label(date.day().to_string())
                                .tooltip(format!("{key} · {}", launch_count(launches)))
                                .bg(if launches == 0 {
                                    cx.theme().muted.alpha(1.)
                                } else if launches < 3 {
                                    cx.theme()
                                        .muted
                                        .alpha(1.)
                                        .blend(cx.theme().primary.alpha(0.25))
                                        .alpha(1.)
                                } else {
                                    cx.theme().primary.alpha(1.)
                                })
                                .text_color(if launches >= 3 {
                                    cx.theme().primary_foreground
                                } else {
                                    cx.theme().foreground
                                })
                                .when(date == self.activity_day, |button| {
                                    button
                                        .bg(cx.theme().accent.alpha(1.))
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
                                button(SharedString::from(format!("activity-game-{}", game_id.0)))
                                    .secondary()
                                    .h(px(36.))
                                    .w_full()
                                    .label(format!("{} · {count}", game.title()))
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.open_game_journal(id.clone(), window, cx)
                                    })),
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
                        icon_button("write-journal-note", IconName::Plus, "Write a game note")
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
                            button(SharedString::from(format!("journal-widget-{}", entry.id)))
                                .secondary()
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
                        button("save-journal-note")
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
                            button("cancel-journal-edit")
                                .secondary()
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
                    .bg(crate::glass::Palette::of(cx).input.alpha(0.3))
                    .child(hint(&timestamp_label(entry.created_at)))
                    .child(div().text_sm().child(entry.text.clone()))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                button(SharedString::from(format!("edit-note-{}", entry.id)))
                                    .secondary()
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
                                button(SharedString::from(format!("delete-note-{}", entry.id)))
                                    .secondary()
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
                                    .on_click(cx.listener(move |view, _, _, cx| {
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
                                    })),
                            )
                            .when(self.journal_delete.as_ref() == Some(&entry.id), |row| {
                                row.child(
                                    button(SharedString::from(format!(
                                        "cancel-delete-note-{}",
                                        entry.id
                                    )))
                                    .secondary()
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
            .child(div().w_full().max_w(px(1100.)).mx_auto().pt(px(CONTENT_TOP)).flex().flex_col().gap_5()
                .child(div().text_3xl().child("Your activity"))
                .child(hint("Launches recorded through Startpoint, not playtime. Existing last-played dates are not retroactively counted."))
                .child(div().w_full().flex().flex_wrap().items_start().gap_4()
                    .child(widget_surface(cx).child(self.activity_calendar(cx)))
                    .child(widget_surface(cx).child(self.collection_progress(cx))))
                .child(widget_surface(cx).w_full().child(self.journal_widget(cx))))
            .into_any_element()
    }

    fn discovery_rows(
        &self,
        provider: &'static str,
        name: &'static str,
        configured: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let paths = self
            .snapshot
            .settings
            .discovery_directories
            .get(provider)
            .cloned()
            .unwrap_or_default();
        let paths = if paths.is_empty() {
            vec![None]
        } else {
            paths.into_iter().map(Some).collect()
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .children(paths.into_iter().enumerate().map(|(index, path)| {
                let label = format!(
                    "{name} · {}",
                    if configured { "Custom" } else { "Automatic" }
                );
                let displayed = path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "No directory detected".into());
                let actions = div()
                    .when_some(path, |actions, path| {
                        actions.child(
                            icon_button(
                                SharedString::from(format!("open-{provider}-{index}")),
                                IconName::Folder,
                                "Open directory",
                            )
                            .on_click(move |_, _, cx| cx.reveal_path(&path)),
                        )
                    })
                    .when(index == 0, |actions| {
                        actions
                            .child(
                                button(SharedString::from(format!("choose-{provider}")))
                                    .label("Change…")
                                    .disabled(!self.snapshot.ready)
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.pick(
                                            match provider {
                                                "steam" => PickerPurpose::SteamDirectory,
                                                "epic" => PickerPurpose::EpicDirectory,
                                                _ => PickerPurpose::GogDirectory,
                                            },
                                            window,
                                            cx,
                                        )
                                    })),
                            )
                            .when(configured, |actions| {
                                actions.child(
                                    button(SharedString::from(format!("reset-{provider}")))
                                        .label("Auto")
                                        .disabled(!self.snapshot.ready)
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            view.submit(
                                                match provider {
                                                    "steam" => Command::SetSteamDirectory {
                                                        directory: None,
                                                    },
                                                    "epic" => Command::SetEpicDirectory {
                                                        directory: None,
                                                    },
                                                    _ => {
                                                        Command::SetGogDirectory { directory: None }
                                                    }
                                                },
                                                cx,
                                            );
                                        })),
                                )
                            })
                    });
                path_row(label, displayed, actions, cx)
            }))
    }

    fn settings(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let key_configured = self
            .snapshot
            .settings
            .configured_credentials
            .contains("steamgriddb");
        let data_directory = self.snapshot.paths.data_directory.clone();
        let cache_directory = self.snapshot.paths.cache_directory.clone();
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
            .pb_8()
            .child(
                div()
                    .w(content_width.min(px(800.)))
                    .mx_auto()
                    .pt(px(CONTENT_TOP))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(div().text_3xl().child("Settings"))
                    .child(div().mt_3().child(eyebrow("STARTUP")))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .child(div().flex_1().text_sm().child("Preferred start screen"))
                            .child(div().w(px(200.)).child({
                                let owner = cx.weak_entity();
                                choice_dropdown(
                                    "preferred-start-screen",
                                    self.appearance.start_screen,
                                    [StartScreen::Library, StartScreen::Recents]
                                        .into_iter()
                                        .map(|screen| (screen, screen.label().into()))
                                        .collect(),
                                    self.snapshot.ready,
                                    move |start_screen, window, cx| {
                                        let _ = owner.update(cx, |view, cx| {
                                            view.set_appearance(
                                                AppearanceSettings {
                                                    start_screen,
                                                    ..view.appearance.clone()
                                                },
                                                window,
                                                cx,
                                            )
                                        });
                                    },
                                )
                            })),
                    )
                    .child(div().mt_4().child(eyebrow("STEAMGRIDDB API KEY")))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(self.credential_status(cx))
                            .child(
                                Link::new("get-api-key")
                                    .text_sm()
                                    .child("Get a key ↗")
                                    .href("https://www.steamgriddb.com/profile/preferences"),
                            ),
                    )
                    .when(!key_configured || self.credential_editing, |view| {
                        view.child(
                            div().flex().child(
                                input(&self.api_key, cx)
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
                                    button("save-api-key")
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
                            .when(key_configured && !self.credential_editing, |row| {
                                row.child(
                                    button("edit-api-key")
                                        .label("Replace key…")
                                        .disabled(
                                            !self.snapshot.ready || self.credential_job.is_some(),
                                        )
                                        .on_click(cx.listener(|view, _, _, cx| {
                                            view.credential_editing = !view.credential_editing;
                                            cx.notify();
                                        })),
                                )
                                .child({
                                    let verify_owner = cx.weak_entity();
                                    let remove_owner = verify_owner.clone();
                                    let locked =
                                        self.snapshot.settings.credential_status.get("steamgriddb")
                                            == Some(&CredentialStatus::Locked);
                                    button("api-key-options")
                                        .label("Options")
                                        .dropdown_caret(true)
                                        .disabled(
                                            !self.snapshot.ready
                                                || self.credential_job.is_some()
                                                || self
                                                    .snapshot
                                                    .settings
                                                    .credential_status
                                                    .get("steamgriddb")
                                                    == Some(&CredentialStatus::Checking),
                                        )
                                        .dropdown_menu(move |menu, _, _| {
                                            let verify_owner = verify_owner.clone();
                                            let remove_owner = remove_owner.clone();
                                            menu.item(
                                                PopupMenuItem::new(if locked {
                                                    "Unlock saved key"
                                                } else {
                                                    "Verify saved key"
                                                })
                                                .on_click(move |_, _, cx| {
                                                    let _ = verify_owner.update(cx, |view, cx| {
                                                        view.credential_job = view.submit(
                                                            Command::VerifyArtworkCredential {
                                                                provider_id: "steamgriddb".into(),
                                                            },
                                                            cx,
                                                        );
                                                    });
                                                }),
                                            )
                                            .item(
                                                PopupMenuItem::new("Remove key").on_click(
                                                    move |_, _, cx| {
                                                        let _ =
                                                            remove_owner.update(cx, |view, cx| {
                                                                view.credential_job = view.submit(
                                                                    Command::SetArtworkCredential {
                                                                        provider_id: "steamgriddb"
                                                                            .into(),
                                                                        credential: None,
                                                                    },
                                                                    cx,
                                                                );
                                                            });
                                                    },
                                                ),
                                            )
                                        })
                                })
                            })
                            .when(self.credential_editing, |row| {
                                row.child(
                                    button("cancel-api-key")
                                        .label("Cancel")
                                        .disabled(self.credential_job.is_some())
                                        .on_click(cx.listener(|view, _, window, cx| {
                                            view.credential_editing = false;
                                            set_value(&view.api_key, "", window, cx);
                                            cx.notify();
                                        })),
                                )
                            }),
                    )
                    .child(div().mt_5().child(eyebrow("GAME DISCOVERY")))
                    .child(self.discovery_rows(
                        "steam",
                        "Steam",
                        self.snapshot.settings.steam_directory.is_some(),
                        cx,
                    ))
                    .child(self.discovery_rows(
                        "epic",
                        "Epic manifests",
                        self.snapshot.settings.epic_directory.is_some(),
                        cx,
                    ))
                    .child(self.discovery_rows(
                        "gog",
                        "GOG games",
                        self.snapshot.settings.gog_directory.is_some(),
                        cx,
                    ))
                    .child(div().mt_5().child(eyebrow("LOCAL STORAGE")))
                    .child(path_row(
                        "Library & artwork",
                        data_directory.display().to_string(),
                        div().child(
                            button("open-data")
                                .icon(IconName::Folder)
                                .label("Open directory")
                                .on_click(move |_, _, cx| cx.reveal_path(&data_directory)),
                        ),
                        cx,
                    ))
                    .child(path_row(
                        "Thumbnail cache",
                        cache_directory.display().to_string(),
                        div().child(
                            button("open-cache")
                                .icon(IconName::Folder)
                                .label("Open directory")
                                .on_click(move |_, _, cx| cx.reveal_path(&cache_directory)),
                        ),
                        cx,
                    ))
                    .child(div().mt_5().child(eyebrow("UPDATES")))
                    .child(hint(&format!("Version {}", env!("CARGO_PKG_VERSION"))))
                    .map(|section| {
                        #[cfg(feature = "self-update")]
                        {
                            let restart_allowed = self.can_restart_for_update();
                            section.child(
                                self.updater.update(cx, |updater, cx| {
                                    updater.settings(restart_allowed, cx)
                                }),
                            )
                        }
                        #[cfg(not(feature = "self-update"))]
                        {
                            section.child(hint("Automatic updates unavailable in this build."))
                        }
                    }),
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
            .shadow_lg()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                input(&self.command_search, cx)
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
                                    navigation_row(
                                        ("command-result", index),
                                        view.command_selected == index,
                                        cx,
                                    )
                                    .w_full()
                                    .h(px(48.))
                                    .px_3()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .w(px(28.))
                                            .flex_shrink_0()
                                            .flex()
                                            .justify_center()
                                            .child(leading),
                                    )
                                    .child(
                                        div().flex_1().min_w_0().line_clamp(1).child(label.clone()),
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
                .track_scroll(&self.command_scroll),
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
            .child(glass_panel(panel, cx))
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
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
                        icon_button("close-activity", IconName::Close, "Close activity")
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
                            button("dismiss-message")
                                .secondary()
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
                                    button(("cancel-job", id))
                                        .secondary()
                                        .h(px(36.))
                                        .label("Cancel")
                                        .on_click(
                                            cx.listener(move |view, _, _, _| view.api.cancel(id)),
                                        ),
                                )
                            })
                    }),
            )
            .map(|panel| glass_panel(panel, cx))
            .into_any_element()
    }
}

impl Render for LauncherView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let favourites = self.favourite_count() > 0;
        if favourites != self.favourites_available && self.snapshot.ready {
            let from = self.favourites_progress();
            self.favourites_transition =
                (!appearance::reduced_motion()).then(|| (from, Instant::now()));
        }
        self.favourites_available = favourites;
        if let Some((_, started)) = self.favourites_transition {
            self.library_list.splice(0..1, 1);
            if started.elapsed() >= Duration::from_millis(220) || appearance::reduced_motion() {
                self.favourites_transition = None;
            } else {
                window.request_animation_frame();
            }
        }
        self.prepare_library(window);
        self.translucency_slider.update(cx, |slider, cx| {
            slider.sync(
                self.appearance.translucency_amount,
                self.snapshot.ready
                    && self.appearance.window_translucency
                    && cfg!(any(target_os = "windows", target_os = "macos")),
                window,
                cx,
            )
        });
        if self.screen == StartScreen::Recents && !self.settings_open {
            let games = self.recent_games();
            let next = games
                .iter()
                .position(|game| self.selected.as_ref() == Some(&game.id))
                .and_then(|index| games.get((index + 1) % games.len()));
            self.carousel_image_ready =
                next.is_none_or(|game| self.recents_backdrop_image(game, window, cx).is_some());
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
        let recents_backdrop = self.screen == StartScreen::Recents
            && !self.settings_open
            && self.snapshot.ready
            && self.selected_game().is_some();
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
                .child(self.library(window, cx));
            if self.shows_library_details()
                && let Some(game) = self.selected_game()
            {
                content = content.child(self.detail_overview(&game, window, cx));
            }
            content.into_any_element()
        };
        div()
            .size_full()
            .relative()
            .bg(if recents_backdrop {
                transparent_black()
            } else {
                appearance::surface(cx)
            })
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
                            .when(recents_backdrop, |container| {
                                container.bg(appearance::surface(cx))
                            })
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
                                                view.sidebar_resize_focus.focus(window, cx);
                                                cx.notify();
                                            }),
                                        )
                                        .on_key_down(cx.listener(
                                            |view, event: &KeyDownEvent, window, cx| {
                                                let width = match event.keystroke.key.as_str() {
                                                    "left" => view.appearance.sidebar_width - 8.,
                                                    "right" => view.appearance.sidebar_width + 8.,
                                                    "home" => 176.,
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
                    .child(div().flex_1().min_w_0().h_full().child(content)),
            )
            .child(self.toolbar(sidebar, window, cx))
            .when(self.customization_open, |view| {
                view.child(self.customization(window, cx))
            })
            .when(self.editor_is_modal() && !self.settings_open, |view| {
                view.when_some(self.selected_game(), |view, game| {
                    view.child(self.game_editor_modal(&game, window, cx))
                })
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

fn library_metadata(text: &str, geometry: LibraryGeometry) -> Div {
    div()
        .w_full()
        .min_w_0()
        .h(px(geometry.metadata_height))
        .flex_shrink_0()
        .text_size(px(geometry.metadata_font_size))
        .line_height(relative(1.2))
        .opacity(0.7)
        .overflow_hidden()
        .line_clamp(1)
        .child(text.to_string())
}

fn filter_button_width(label: &str, window: &Window) -> Pixels {
    let style = window.text_style();
    let mut font = style.font();
    font.weight = FontWeight::MEDIUM;
    let line = window.text_system().shape_line(
        label.to_string().into(),
        window.rem_size() * BUTTON_TEXT_REM,
        &[TextRun {
            len: label.len(),
            font,
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    // Match the shared button typography and the filters' px_2 side padding.
    line.width + window.rem_size()
}

fn library_source_status(game: &Game, source: bool) -> String {
    let status = if game.available {
        "Installed"
    } else {
        "Unavailable"
    };
    if source {
        format!("{} · {status}", source_name(&game.provider_id))
    } else {
        status.into()
    }
}

fn library_subtitle(game: &Game, last_launched: bool, source: bool) -> String {
    let mut parts = Vec::new();
    if last_launched {
        let date = last_played(game);
        if !date.is_empty() {
            parts.push(date);
        }
    }
    if source {
        parts.push(source_name(&game.provider_id).to_string());
    }
    parts.join(" · ")
}

fn compact_summary(game: &Game, launches: u32) -> String {
    let metadata = game.effective_metadata();
    let mut parts = [metadata.developer, metadata.genres]
        .into_iter()
        .flatten()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    parts.extend(game.collections.iter().cloned());
    if game.completed {
        parts.push("Completed".into());
    }
    parts.push(launch_count(launches));
    parts.join(" · ")
}

fn library_game_actions(game: &Game) -> Vec<(&'static str, IconName, LibraryGameAction, bool)> {
    let mut actions = vec![
        (
            "Play game",
            IconName::ExternalLink,
            LibraryGameAction::Launch,
            !game.available && game.launch_override.is_none(),
        ),
        (
            "Edit metadata & collections…",
            IconName::Settings2,
            LibraryGameAction::Edit(DetailTab::Metadata),
            false,
        ),
        (
            "Choose artwork…",
            IconName::GalleryVerticalEnd,
            LibraryGameAction::Edit(DetailTab::Artwork),
            false,
        ),
        (
            "Launch settings…",
            IconName::SquareTerminal,
            LibraryGameAction::Edit(DetailTab::Launch),
            false,
        ),
        (
            "Game journal…",
            IconName::BookOpen,
            LibraryGameAction::Edit(DetailTab::Journal),
            false,
        ),
        (
            "Re-identify game…",
            IconName::Search,
            LibraryGameAction::Reidentify,
            false,
        ),
        (
            if game.favourite {
                "Remove from favourites"
            } else {
                "Add to favourites"
            },
            IconName::Star,
            LibraryGameAction::Favourite,
            false,
        ),
        (
            if game.completed {
                "Mark not completed"
            } else {
                "Mark completed"
            },
            IconName::CircleCheck,
            LibraryGameAction::Completed,
            false,
        ),
        (
            if game.hidden {
                "Unhide game"
            } else {
                "Hide from library"
            },
            IconName::EyeOff,
            LibraryGameAction::Hidden,
            false,
        ),
        (
            "Show install folder",
            IconName::FolderOpen,
            LibraryGameAction::Reveal,
            !game.install_directory.is_dir(),
        ),
    ];
    if game.provider_id == "manual" {
        actions.push((
            "Remove from library…",
            IconName::Delete,
            LibraryGameAction::Remove,
            false,
        ));
    }
    actions
}

fn library_game_menu(
    mut menu: PopupMenu,
    game: &Game,
    owner: WeakEntity<LauncherView>,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let menu_entity = cx.entity();
    let _ = owner.update(cx, |view, cx| {
        view.game_menu_open = true;
        view.carousel_tick = None;
        view.game_menu_subscription =
            Some(cx.subscribe(&menu_entity, |view, _, _: &DismissEvent, cx| {
                view.game_menu_open = false;
                view.game_menu_subscription = None;
                cx.notify();
            }));
        cx.notify();
    });
    for (label, icon, action, disabled) in library_game_actions(game) {
        if matches!(
            action,
            LibraryGameAction::Edit(DetailTab::Metadata)
                | LibraryGameAction::Favourite
                | LibraryGameAction::Reveal
                | LibraryGameAction::Remove
        ) {
            menu = menu.separator();
        }
        let owner = owner.clone();
        let id = game.id.clone();
        menu = menu.item(
            PopupMenuItem::new(label)
                .icon(icon)
                .disabled(disabled)
                .on_click(move |_, window, cx| {
                    let _ = owner.update(cx, |view, cx| {
                        view.run_library_game_action(&id, action, window, cx)
                    });
                }),
        );
    }
    menu.min_w(px(220.)).max_h(px(520.)).scrollable(true)
}

/// Recents always shows the five most recently launched visible installed games,
/// regardless of the Library's saved filter, source, collection or sort order.
fn installed_recents(games: &[Game]) -> Vec<&Game> {
    let mut recent = games
        .iter()
        .filter(|game| game.available && !game.hidden)
        .collect::<Vec<_>>();
    recent.sort_by_key(|game| std::cmp::Reverse(game.last_launched.unwrap_or(0)));
    recent.truncate(5);
    recent
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
fn game_editor_tabs_exclude_the_detail_overview() {
    assert!(!DetailTab::Overview.is_editor());
    assert_eq!(DetailTab::EDITORS.len(), 4);
    for tab in [
        DetailTab::Metadata,
        DetailTab::Artwork,
        DetailTab::Launch,
        DetailTab::Journal,
    ] {
        assert!(tab.is_editor());
        assert_eq!(
            DetailTab::EDITORS
                .iter()
                .filter(|(editor, _)| *editor == tab)
                .count(),
            1
        );
    }
    assert!(DetailTab::EDITORS.iter().all(|(tab, _)| tab.is_editor()));
}

#[cfg(test)]
#[::core::prelude::v1::test]
fn optional_library_filters_only_appear_when_populated_and_hidden_stays_in_the_sidebar() {
    for filter in [LibraryFilter::Favourites, LibraryFilter::Manual] {
        assert!(!filter.visible_in_navigation(0));
        assert!(filter.visible_in_navigation(1));
    }
    assert!(!LibraryFilter::Hidden.visible_in_navigation(0));
    assert!(!LibraryFilter::Hidden.visible_in_navigation(10));
    assert!(LibraryFilter::All.visible_in_navigation(0));
    assert!(LibraryFilter::Installed.visible_in_navigation(0));
}

#[cfg(test)]
fn sample_library_game() -> Game {
    serde_json::from_value(serde_json::json!({
        "id": "steam:42", "provider_id": "steam", "source_game_id": "42",
        "discovered_title": "Menu target", "install_directory": "",
        "default_launch": { "Uri": "steam://rungameid/42" }, "launch_override": null,
        "metadata": {}, "favourite": false, "available": true, "last_launched": null,
        "artwork": {}, "artwork_history": [], "artwork_game_ids": {}
    }))
    .unwrap()
}

#[cfg(test)]
#[::core::prelude::v1::test]
fn recents_uses_installed_games_not_library_filters_and_orders_by_last_launch() {
    let mut games = (1..=8)
        .map(|index| {
            let mut game = sample_library_game();
            game.id = GameId(format!("steam:{index}"));
            game.last_launched = Some(index);
            game
        })
        .collect::<Vec<_>>();
    games[7].hidden = true;
    games[6].available = false;
    games[5].favourite = false;
    games[5].provider_id = "manual".into();
    assert_eq!(
        installed_recents(&games)
            .into_iter()
            .map(|game| &game.id.0)
            .collect::<Vec<_>>(),
        vec!["steam:6", "steam:5", "steam:4", "steam:3", "steam:2"]
    );
}

#[cfg(test)]
#[::core::prelude::v1::test]
fn game_menus_offer_launch_and_editing_without_an_overview_panel() {
    let mut game = sample_library_game();
    let menu = library_game_actions(&game);
    for action in [
        LibraryGameAction::Launch,
        LibraryGameAction::Edit(DetailTab::Metadata),
        LibraryGameAction::Edit(DetailTab::Artwork),
        LibraryGameAction::Edit(DetailTab::Launch),
        LibraryGameAction::Edit(DetailTab::Journal),
        LibraryGameAction::Favourite,
        LibraryGameAction::Hidden,
        LibraryGameAction::Completed,
        LibraryGameAction::Reidentify,
    ] {
        assert!(
            menu.iter()
                .any(|(_, _, item, disabled)| *item == action && !disabled)
        );
    }
    assert!(
        !menu
            .iter()
            .any(|(_, _, action, _)| *action == LibraryGameAction::Remove)
    );
    game.provider_id = "manual".into();
    game.available = false;
    game.favourite = true;
    game.hidden = true;
    let menu = library_game_actions(&game);
    assert!(
        menu.iter()
            .any(|(_, _, action, disabled)| *action == LibraryGameAction::Launch && *disabled)
    );
    assert!(
        menu.iter()
            .any(|(_, _, action, disabled)| *action == LibraryGameAction::Remove && !disabled)
    );
    assert!(menu.iter().any(
        |(label, _, action, _)| *action == LibraryGameAction::Favourite
            && *label == "Remove from favourites"
    ));
    assert!(menu.iter().any(
        |(label, _, action, _)| *action == LibraryGameAction::Hidden && *label == "Unhide game"
    ));
}

#[cfg(test)]
#[::core::prelude::v1::test]
fn dates_and_sources_are_independent_without_a_fallback_or_duplicate_separator() {
    let mut game = sample_library_game();
    assert!(last_played(&game).is_empty());
    assert!(library_subtitle(&game, true, false).is_empty());
    assert!(library_subtitle(&game, false, false).is_empty());
    assert_eq!(library_subtitle(&game, true, true), "Steam");
    assert_eq!(library_source_status(&game, false), "Installed");
    assert_eq!(library_source_status(&game, true), "Steam · Installed");
    game.last_launched = Some(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    );
    assert_eq!(library_subtitle(&game, false, true), "Steam");
    assert_eq!(
        library_subtitle(&game, true, false),
        "Last launched just now"
    );
    assert_eq!(
        library_subtitle(&game, true, true),
        "Last launched just now · Steam"
    );
}

#[cfg(test)]
#[::core::prelude::v1::test]
fn compact_summary_contains_metadata_collections_completion_and_activity() {
    let mut game = sample_library_game();
    assert_eq!(compact_summary(&game, 0), "0 launches");
    game.metadata.developer = Some("Studio".into());
    game.metadata.genres = Some("Adventure".into());
    game.collections.insert("Backlog".into());
    game.completed = true;
    assert_eq!(
        compact_summary(&game, 3),
        "Studio · Adventure · Backlog · Completed · 3 launches"
    );
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
    game.provider_id = "manual".into();
    game.available = false;
    assert!(
        LibraryFilter::Manual.includes(&game),
        "manual view includes saved unavailable games"
    );
    game.hidden = true;
    assert!(!LibraryFilter::Manual.includes(&game));
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
    assert!(!multiline, "multiline fields use TextareaState");
    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
}

fn textarea_input<T: 'static>(
    placeholder: &str,
    window: &mut Window,
    cx: &mut Context<T>,
) -> Entity<TextareaState> {
    let placeholder = placeholder.to_string();
    cx.new(|cx| {
        TextareaState::new(window, cx)
            .placeholder(placeholder)
            .auto_grow(4, 8)
    })
}

fn set_value<T: 'static, S: EditableFieldState>(
    field: &Entity<S>,
    value: &str,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    S::replace(field, value, window, cx);
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
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    });
                },
            )
            .size_full(),
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
        return String::new();
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
