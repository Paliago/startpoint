use crate::{
    adjustment_slider::{Adjustment, AdjustmentSlider, Event as AdjustmentEvent},
    appearance,
    components::{
        BUTTON_TEXT_REM, EditableFieldState, artwork_card_caption, artwork_card_frame,
        artwork_card_states, button, choice_button, choice_dropdown, choice_group, eyebrow, field,
        floating_panel, glass_panel, hint, icon_button, image, input, modal_backdrop,
        navigation_row, navigation_tab, path_row, rounded_image, rounded_placeholder,
        sidebar_label, snug_sidebar_button, toolbar_icon_button, toolbar_island, toolbar_panel,
        toolbar_panel_with_blur, toolbar_rounding, widget_surface, with_accessible_label,
    },
    image_cache::ArtworkImageCache,
    library_layout::{CONTENT_TOP, DETAIL_WIDTH, Geometry as LibraryGeometry, filter_reveal},
    recents_fade::RecentsFade,
    rounding::Rounding,
    theme,
    theme_editor::{Editor, EditorEvent},
};
#[cfg(all(feature = "automation", target_os = "macos"))]
mod automation;
mod background;
mod cinematic;
mod collection;
mod gallery;
mod scenes;
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
    GameId, JobId, JobState, LaunchTarget, LibraryDensity, LibraryLayout, RecentsLayout, Snapshot,
    StartScreen, SteamLaunch, SteamShortcut, UserMetadata,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::window_chrome::HEADER_HEIGHT;
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
                .h(px(crate::design::current(_cx).header_height))
                .px(px(crate::window_chrome::TITLEBAR_PADDING))
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
                .h(px(crate::design::current(_cx).header_height))
                .pl(px(crate::window_chrome::TITLEBAR_PADDING))
                .pr(px(crate::window_chrome::TITLEBAR_PADDING))
                .border_0()
                .child(self.content)
                .into_any_element()
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_window_controls(window: &Window, cx: &App) -> impl IntoElement {
    let maximized = window.is_maximized();
    let foreground = cx.theme().foreground;
    let hover = cx.theme().tokens.button_hover.color;
    let active = cx.theme().tokens.button_active.color;
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
                    .rounded(toolbar_rounding(cx))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .text_color(foreground)
                    .hover(|style| {
                        style
                            .bg(if close { cx.theme().danger } else { hover })
                            .text_color(if close {
                                cx.theme().danger_foreground
                            } else {
                                foreground
                            })
                    })
                    .active(|style| {
                        style.bg(if close {
                            cx.theme().danger_active
                        } else {
                            active
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LaunchMode {
    Default,
    SteamShortcut,
    SteamUrl,
    Executable,
}

impl LaunchMode {
    fn for_game(game: &Game) -> Self {
        match &game.steam_launch {
            Some(SteamLaunch::Shortcut { .. }) => Self::SteamShortcut,
            Some(SteamLaunch::Url(_)) => Self::SteamUrl,
            None if game.launch_override.is_some() => Self::Executable,
            None => Self::Default,
        }
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
    HiddenLibrary,
    Settings,
    Refresh,
    RefreshMetadata,
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
    SteamUrl(GameId),
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
    // Remember the game for returning to Library/Recents, without keeping an
    // active game selection on Settings or the Activity overview.
    remembered_selection: Option<GameId>,
    source_filter: Option<String>,
    collection_filter: Option<String>,
    library_filter: LibraryFilter,
    sort_recent: bool,
    appearance: AppearanceSettings,
    appearance_sliders: [Entity<AdjustmentSlider>; 8],
    #[cfg(debug_assertions)]
    stress_case: &'static str,
    recents_fade: Entity<RecentsFade>,
    library_artwork: Entity<RecentsFade>,
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
    editor_organization: bool,
    launch_advanced: bool,
    artwork_online: bool,
    detail_closed: bool,
    editor_focus: FocusHandle,
    artwork_kind: ArtworkKind,
    artwork_images: Vec<ArtworkCandidate>,
    artwork_games: Vec<ArtworkGame>,
    artwork_games_visible: usize,
    artwork_page: u32,
    artwork_has_more: bool,
    artwork_page_loaded: bool,
    artwork_page_pending: bool,
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
    launch_mode: LaunchMode,
    steam_shortcut: Option<SteamLaunch>,
    steam_url: Entity<InputState>,
    steam_shortcuts_job: Option<JobId>,
    steam_url_import_job: Option<JobId>,
    launch_save_job: Option<(JobId, GameId)>,
    launch_message: Option<String>,
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
    carousel_elapsed: Duration,
    carousel_tick: Option<Instant>,
    carousel_paused: bool,
    game_menu_open: bool,
    game_menu_subscription: Option<Subscription>,
    carousel_focus: FocusHandle,
    carousel_transition: Option<(Game, Instant)>,
    carousel_image_ready: bool,
    gallery_motion: crate::gallery::Motion,
    render_power: crate::render_power::State,
    gallery_reflections: Entity<crate::gallery::Reflections>,
    gallery_gpu: Entity<crate::gallery_gpu::GalleryGpu>,
    background_clock: crate::artwork_motion::FlowClock,
    mosaic_wheel: crate::gallery::PageWheel,
    gallery_wheel: crate::gallery::Wheel,
    cinematic_scene: crate::cinematic::Scene,
    cinematic_artwork: Entity<crate::cinematic::ArtworkCache>,
    cinematic_motion: crate::gallery::Motion,
    cinematic_launch_hovered: bool,
    art_scenes: crate::art_scenes::State,
    scene_artwork: Entity<crate::cinematic::ArtworkCache>,
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
        let activation_subscription = cx.observe_window_activation(window, |view, window, cx| {
            view.refresh_render_power(window, true, cx);
            cx.notify();
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
        let adjustments = [
            (
                Adjustment::Translucency,
                appearance_settings.translucency_amount,
            ),
            (Adjustment::Radius, appearance_settings.controls.radius),
            (
                Adjustment::Padding,
                appearance_settings.controls.horizontal_padding,
            ),
            (
                Adjustment::OutlineStrength,
                appearance_settings.controls.outline_strength,
            ),
            (
                Adjustment::RecentsDithering,
                appearance_settings.dither_recents_strength,
            ),
            (
                Adjustment::ArtworkDithering,
                appearance_settings.dither_artwork_strength,
            ),
            (
                Adjustment::RecentsDitherSize,
                appearance_settings.dither_recents_size,
            ),
            (
                Adjustment::ArtworkDitherSize,
                appearance_settings.dither_artwork_size,
            ),
        ];
        let appearance_sliders = adjustments.map(|(adjustment, value)| {
            cx.new(|cx| AdjustmentSlider::new(adjustment, value, window, cx))
        });
        let adjustment_subscriptions = appearance_sliders
            .iter()
            .zip(adjustments)
            .map(|(slider, (adjustment, _))| {
                cx.subscribe_in(
                    slider,
                    window,
                    move |view, _, event, window, cx| match event {
                        AdjustmentEvent::Changed(value) => {
                            match adjustment {
                                Adjustment::Translucency => {
                                    view.appearance.translucency_amount = *value
                                }
                                Adjustment::Radius => view.appearance.controls.radius = *value,
                                Adjustment::Padding => {
                                    view.appearance.controls.horizontal_padding = *value
                                }
                                Adjustment::OutlineStrength => {
                                    view.appearance.controls.outline_strength = *value
                                }
                                Adjustment::RecentsDithering => {
                                    view.appearance.dither_recents_strength = *value
                                }
                                Adjustment::ArtworkDithering => {
                                    view.appearance.dither_artwork_strength = *value
                                }
                                Adjustment::RecentsDitherSize => {
                                    view.appearance.dither_recents_size = *value
                                }
                                Adjustment::ArtworkDitherSize => {
                                    view.appearance.dither_artwork_size = *value
                                }
                            }
                            appearance::update_effects(&view.appearance, cx);
                            view.schedule_appearance_save(window, cx);
                        }
                        AdjustmentEvent::Released => {
                            if view.appearance_save.is_some() {
                                view.set_appearance(view.appearance.clone(), window, cx);
                            }
                        }
                    },
                )
            })
            .collect::<Vec<_>>();
        let appearance_loaded = snapshot.ready;
        appearance::apply(&appearance_settings, Some(window), cx);
        let screen = appearance_settings.start_screen;
        let logos = cx.new(|_| crate::logo_cache::LogoCache::new(&snapshot.paths.cache_directory));
        let mut subscriptions = vec![
            subscription,
            appearance_subscription,
            activation_subscription,
            key_subscription,
            bounds_subscription,
            quit_subscription,
        ];
        subscriptions.extend(adjustment_subscriptions);
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
            remembered_selection: None,
            source_filter: None,
            collection_filter: None,
            library_filter: LibraryFilter::Installed,
            sort_recent: true,
            appearance: appearance_settings,
            appearance_sliders,
            #[cfg(debug_assertions)]
            stress_case: if std::env::args().any(|argument| argument == "--example-library") {
                std::env::args()
                    .find_map(|argument| match argument.as_str() {
                        "--interface-data=worst" => Some("Worst case"),
                        "--interface-data=empty" => Some("Empty"),
                        "--interface-data=one" => Some("One game"),
                        "--interface-data=large" => Some("1,000 games"),
                        _ => None,
                    })
                    .unwrap_or("Demo data")
            } else {
                "Demo data"
            },
            recents_fade: cx.new(|_| RecentsFade::default()),
            library_artwork: cx.new(|_| RecentsFade::default()),
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
            editor_organization: false,
            launch_advanced: false,
            artwork_online: false,
            detail_closed: false,
            editor_focus: cx.focus_handle(),
            artwork_kind: ArtworkKind::Cover,
            artwork_images: Vec::new(),
            artwork_games: Vec::new(),
            artwork_games_visible: 10,
            artwork_page: 0,
            artwork_has_more: false,
            artwork_page_loaded: false,
            artwork_page_pending: false,
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
            launch_mode: LaunchMode::Default,
            steam_shortcut: None,
            steam_url: text_input("steam://rungameid/…", false, window, cx),
            steam_shortcuts_job: None,
            steam_url_import_job: None,
            launch_save_job: None,
            launch_message: None,
            executable: text_input("Default launcher", false, window, cx),
            arguments: textarea_input("One argument per line", window, cx),
            working_directory: text_input("Executable directory", false, window, cx),
            api_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Your SteamGridDB API key")
                    .masked(true)
            }),
            artwork_search: text_input("Game title, SteamGridDB URL or #ID", false, window, cx),
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
            carousel_elapsed: Duration::ZERO,
            carousel_tick: None,
            carousel_paused: appearance::reduced_motion(),
            game_menu_open: false,
            game_menu_subscription: None,
            carousel_focus: cx.focus_handle(),
            carousel_transition: None,
            carousel_image_ready: false,
            gallery_motion: Default::default(),
            render_power: Default::default(),
            gallery_reflections: cx.new(|_| Default::default()),
            gallery_gpu: cx.new(|_| Default::default()),
            background_clock: Default::default(),
            mosaic_wheel: Default::default(),
            gallery_wheel: Default::default(),
            cinematic_scene: Default::default(),
            cinematic_artwork: cx.new(|_| Default::default()),
            cinematic_motion: Default::default(),
            cinematic_launch_hovered: false,
            art_scenes: Default::default(),
            scene_artwork: cx.new(|_| Default::default()),
            _subscriptions: subscriptions,
        };
        view._subscriptions
            .push(cx.observe(&view.gallery_gpu, |_, _, cx| cx.notify()));
        view._subscriptions
            .push(cx.observe(&view.cinematic_artwork, |_, _, cx| cx.notify()));
        view._subscriptions
            .push(cx.observe(&view.scene_artwork, |_, _, cx| cx.notify()));
        #[cfg(debug_assertions)]
        {
            view.apply_stress_data();
            if view.stress_case != "Demo data" {
                view.library_filter = LibraryFilter::All;
            }
        }
        view.filter(cx);
        view.ensure_selection(window, cx);
        view.focus.focus(window, cx);
        view.reload_themes(window, cx);
        if !crate::automation::active(cx) {
            view.submit(Command::RefreshLibrary, cx);
        }
        cx.spawn_in(window, async move |view, cx| {
            let mut interval = Duration::from_millis(250);
            loop {
                cx.background_executor().timer(interval).await;
                match view.update_in(cx, |view, window, cx| {
                    view.refresh_render_power(window, false, cx);
                    if view.render_power.paused() {
                        return crate::render_power::PROBE_INTERVAL;
                    }
                    if view.tick_art_scenes()
                        || (view.liquid_background_active() && !appearance::reduced_motion())
                    {
                        cx.notify();
                        Duration::from_millis(33)
                    } else if view.advance_carousel(window, cx) {
                        Duration::from_millis(50)
                    } else {
                        crate::render_power::PROBE_INTERVAL
                    }
                }) {
                    Ok(next_interval) => interval = next_interval,
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
        #[cfg(debug_assertions)]
        if self.stress_case != "Demo data"
            && !matches!(
                command,
                Command::SetAppearance { .. } | Command::RefreshLibrary
            )
        {
            self.notice = Some(
                "Interface stress data is read-only. Select Demo data to edit or launch games."
                    .into(),
            );
            self.jobs_open = true;
            cx.notify();
            return None;
        }
        crate::diagnostics::record(command.label());
        match self.api.submit(command) {
            Ok(id) => {
                self.snapshot = self.api.snapshot();
                #[cfg(debug_assertions)]
                self.apply_stress_data();
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
        #[cfg(debug_assertions)]
        self.apply_stress_data();
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
            AppEvent::SteamShortcutUrlRead {
                job_id,
                game_id,
                uri,
            } => {
                if self.steam_url_import_job == Some(job_id)
                    && self.selected.as_ref() == Some(&game_id)
                {
                    self.launch_mode = LaunchMode::SteamUrl;
                    set_value(&self.steam_url, &uri, window, cx);
                    self.launch_message =
                        Some("Steam URL imported. Save launch configuration to use it.".into());
                }
            }
            AppEvent::LibraryChanged => {
                self.filter(cx);
                self.ensure_selection(window, cx);
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
                if self.steam_shortcuts_job == Some(job.id) && !job.state.active() {
                    self.steam_shortcuts_job = None;
                }
                if self.steam_url_import_job == Some(job.id) && !job.state.active() {
                    self.steam_url_import_job = None;
                }
                if self
                    .launch_save_job
                    .as_ref()
                    .is_some_and(|(id, _)| *id == job.id)
                    && !job.state.active()
                    && let Some((_, owner)) = self.launch_save_job.take()
                    && self.selected.as_ref() == Some(&owner)
                {
                    self.launch_message = Some(if job.state == JobState::Completed {
                        "Launch configuration saved.".into()
                    } else {
                        "Launch configuration was not saved. See notifications for details.".into()
                    });
                }
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
                    self.artwork_page_pending = false;
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
                    self.artwork_games_visible = 10;
                }
            }
            AppEvent::ArtworkFound {
                job_id,
                game_id,
                kind,
                page,
                has_more,
                images,
            } => {
                if self.artwork_job == Some(job_id)
                    && self.selected.as_ref() == Some(&game_id)
                    && self.artwork_kind == kind
                {
                    self.artwork_message = if images.is_empty() {
                        Some(
                            if has_more {
                                "No usable images on this page. Try the next page."
                            } else {
                                "No artwork was found on this page."
                            }
                            .into(),
                        )
                    } else {
                        None
                    };
                    self.artwork_images = images;
                    self.artwork_page = page;
                    self.artwork_has_more = has_more;
                    self.artwork_page_loaded = true;
                    self.artwork_page_pending = false;
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
        installed_recents(&self.snapshot.games, self.design().recent_count)
    }

    fn displayed_games(&self) -> Vec<&Game> {
        if self.settings_open {
            return Vec::new();
        }
        match self.screen {
            StartScreen::Library => self.visible.iter().collect(),
            StartScreen::Recents => self.recent_games(),
            StartScreen::Activity => Vec::new(),
        }
    }

    fn ensure_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let had_selection = self.selected.is_some();
        if !reconcile_page_selection(
            self.screen,
            self.settings_open,
            self.detail_tab,
            &mut self.selected,
            &mut self.remembered_selection,
        ) {
            if had_selection {
                self.cancel_artwork();
            }
            return;
        }
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
        if self.selected != selected
            || (!had_selection && !self.detail_tab.is_editor() && selected.is_some())
        {
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

    fn command_game(&self) -> Option<Game> {
        if !page_has_game_context(self.screen, self.settings_open, self.detail_tab) {
            return None;
        }
        self.selected_game().filter(|game| {
            self.detail_tab.is_editor()
                || self
                    .displayed_games()
                    .iter()
                    .any(|displayed| displayed.id == game.id)
        })
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
        self.remembered_selection = None;
        self.detail_closed = false;
        self.settings_open = false;
        self.detail_tab = DetailTab::Overview;
        self.reset_artwork_pages();
        self.artwork_games.clear();
        self.artwork_message = None;
        self.reidentifying = false;
        self.artwork_match = self.selected_game().as_ref().and_then(saved_artwork_match);
        self.editor_organization = false;
        self.artwork_online = false;
        self.launch_advanced = self
            .selected_game()
            .is_some_and(|game| game.has_launch_override());
        self.populate_game_fields(window, cx);
        cx.notify();
    }

    fn populate_game_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(game) = self.selected_game() {
            self.launch_mode = LaunchMode::for_game(&game);
            self.steam_shortcut = game
                .steam_launch
                .clone()
                .filter(|launch| matches!(launch, SteamLaunch::Shortcut { .. }));
            set_value(
                &self.steam_url,
                match &game.steam_launch {
                    Some(SteamLaunch::Url(uri)) => uri,
                    _ => "",
                },
                window,
                cx,
            );
            self.launch_message = None;
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
        self.artwork_page_pending = false;
    }

    fn reset_artwork_pages(&mut self) {
        self.artwork_images.clear();
        self.artwork_page = 0;
        self.artwork_has_more = false;
        self.artwork_page_loaded = false;
        self.artwork_page_pending = false;
    }

    fn search_artwork(&mut self, cx: &mut Context<Self>) {
        if let Some(game_id) = self.selected.clone() {
            self.cancel_artwork();
            self.reset_artwork_pages();
            self.artwork_games.clear();
            self.artwork_games_visible = 10;
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
        self.cancel_artwork();
        self.reset_artwork_pages();
        self.load_artwork_page(0, remote_game_id, cx);
    }

    fn load_artwork_page(
        &mut self,
        page: u32,
        remote_game_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.identity_job.is_some() {
            self.artwork_message =
                Some("Saving the game identity; browse images when it finishes.".into());
            cx.notify();
            return;
        }
        if let Some(game_id) = self.selected.clone() {
            self.cancel_artwork();
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
                    page,
                },
                cx,
            );
            self.artwork_page_pending = self.artwork_job.is_some();
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
                self.reset_artwork_pages();
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
        self.artwork_online = true;
        self.artwork_games.clear();
        self.reset_artwork_pages();
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

    fn block_native_action(&mut self, operation: &str, cx: &mut Context<Self>) -> bool {
        if crate::automation::native_allowed(cx, operation) {
            return false;
        }
        self.notice = Some(format!("Automation blocked native action: {operation}"));
        self.jobs_open = true;
        cx.notify();
        true
    }

    fn pick(&mut self, purpose: PickerPurpose, window: &mut Window, cx: &mut Context<Self>) {
        if self.block_native_action("file dialog", cx) {
            return;
        }
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
                        PickerPurpose::SteamUrl(game_id) => {
                            view.steam_url_import_job =
                                view.submit(Command::ReadSteamShortcutUrl { game_id, path }, cx);
                        }
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

    fn refresh_steam_shortcuts(&mut self, cx: &mut Context<Self>) {
        if self.steam_shortcuts_job.is_none() {
            self.steam_shortcuts_job = self.submit(Command::RefreshSteamShortcuts, cx);
        }
    }

    fn save_launch(&mut self, cx: &mut Context<Self>) {
        let Some(game_id) = self.selected.clone() else {
            return;
        };
        if self.launch_save_job.is_some() {
            return;
        }
        self.launch_message = None;
        let steam_launch = match self.launch_mode {
            LaunchMode::Default => {
                self.launch_save_job = self
                    .submit(
                        Command::SetLaunchOverride {
                            game_id: game_id.clone(),
                            launch: None,
                        },
                        cx,
                    )
                    .map(|job| (job, game_id));
                return;
            }
            LaunchMode::SteamShortcut => {
                let Some(launch) = self.steam_shortcut.clone() else {
                    self.launch_message = Some("Choose a Steam shortcut first.".into());
                    cx.notify();
                    return;
                };
                Some(launch)
            }
            LaunchMode::SteamUrl => Some(SteamLaunch::Url(
                self.steam_url.read(cx).value().trim().to_string(),
            )),
            LaunchMode::Executable => None,
        };
        if let Some(launch) = steam_launch {
            self.launch_save_job = self
                .submit(
                    Command::SetSteamLaunch {
                        game_id: game_id.clone(),
                        launch,
                    },
                    cx,
                )
                .map(|job| (job, game_id));
            return;
        }
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
        if launch.is_none() {
            self.launch_message =
                Some("Choose an executable, or switch Launch via to Default.".into());
            cx.notify();
            return;
        }
        self.launch_save_job = self
            .submit(
                Command::SetLaunchOverride {
                    game_id: game_id.clone(),
                    launch,
                },
                cx,
            )
            .map(|job| (job, game_id));
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
        if self.block_native_action("native confirmation", cx) {
            return;
        }
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
        if self.art_scene_active()
            && self.art_scene_mode() == Some(crate::art_scenes::Mode::Wall)
            && self.art_scenes.focus.is_none()
            && !self.collection_overlay_open()
            && modifiers.number_of_modifiers() == 0
            && matches!(event.keystroke.key.as_str(), "up" | "down")
        {
            let games = self.recent_games();
            if !games.is_empty() {
                let current = games
                    .iter()
                    .position(|game| self.selected.as_ref() == Some(&game.id))
                    .unwrap_or(0);
                let columns = crate::art_scenes::WallLayout::new(
                    self.main_content_width(window),
                    f32::from(window.viewport_size().height),
                )
                .columns;
                let index = if event.keystroke.key == "down" {
                    (current + columns).min(games.len() - 1)
                } else {
                    current.saturating_sub(columns)
                };
                let id = games[index].id.clone();
                self.art_scenes.hover(
                    Some(id.clone()),
                    appearance::reduced_motion(),
                    Instant::now(),
                );
                self.select_recent_game(id, window, cx);
            }
            cx.stop_propagation();
            return;
        }
        if (modifiers.platform || modifiers.control)
            && modifiers.number_of_modifiers() == 1
            && matches!(event.keystroke.key.as_str(), "f" | "k" | "p")
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
        if let Some(focus) = &mut self.art_scenes.focus {
            focus.launch = false;
        }
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
        if let Some(game) = self.command_game() {
            if game.available || game.has_launch_override() {
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
                "Show hidden games".into(),
                IconName::Eye.into(),
                LauncherCommand::HiddenLibrary,
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
                "Refresh metadata and missing artwork for all games".into(),
                Icon::default().path("icons/refresh.svg"),
                LauncherCommand::RefreshMetadata,
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
        commands.retain(|(label, _, command)| {
            self.command_supported(command)
                && query
                    .split_whitespace()
                    .all(|part| label.to_lowercase().contains(part))
        });
        commands
    }

    fn command_supported(&self, command: &LauncherCommand) -> bool {
        let profile = self.design();
        match command {
            LauncherCommand::Appearance(_) => profile.editable_palette(),
            LauncherCommand::DitherRecents => {
                profile.recents == crate::design::RecentPresentation::Carousel
            }
            LauncherCommand::Sidebar => profile.sidebar_allowed(self.screen, self.settings_open),
            _ => true,
        }
    }

    fn run_command(
        &mut self,
        command: LauncherCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A click may come from a command row rendered before a page/selection
        // change. Re-check the context and target before any game mutation.
        let applies = self.command_supported(&command)
            && command_applies_to_game(&command, self.command_game().as_ref());
        self.close_command_bar(window, cx);
        if !applies {
            return;
        }
        match command {
            LauncherCommand::SelectGame(id) => {
                if (self.design().recents == crate::design::RecentPresentation::Cinematic
                    || self.art_scene_mode().is_some())
                    && self.recent_games().iter().any(|game| game.id == id)
                {
                    self.switch_screen(StartScreen::Recents, window, cx);
                    self.select_game(id, window, cx);
                    return;
                }
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
            LauncherCommand::HiddenLibrary => {
                self.library_filter = LibraryFilter::Hidden;
                self.source_filter = None;
                self.collection_filter = None;
                self.show_filtered_library(window, cx);
            }
            LauncherCommand::Settings => {
                if !self.settings_open {
                    self.open_settings(&OpenSettings, window, cx);
                }
            }
            LauncherCommand::Refresh => self.refresh(&RefreshLibrary, window, cx),
            LauncherCommand::RefreshMetadata => {
                self.submit(Command::RefreshMetadata, cx);
            }
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
        } else if self.art_scene_active()
            && self
                .art_scenes
                .dismiss(appearance::reduced_motion(), Instant::now())
        {
            self.focus.focus(window, cx);
            cx.notify();
            return;
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
        self.art_scenes.focus = None;
        if self.command_mode.is_some() {
            self.close_command_bar(window, cx);
        }
        self.settings_open = !self.settings_open;
        self.customization_open = false;
        self.close_theme_editor(window, cx);
        self.jobs_open = false;
        self.ensure_selection(window, cx);
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn launch_selected(&mut self, _: &LaunchSelected, window: &mut Window, cx: &mut Context<Self>) {
        if self.art_scene_active() && !self.collection_overlay_open() {
            self.launch_art_scene(window, cx);
            return;
        }
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
        if self.art_scene_active() && self.art_scenes.focus.is_some() {
            return;
        }
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
        let index = if self.screen == StartScreen::Recents || self.gallery_active() {
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
        if self.art_scene_mode() == Some(crate::art_scenes::Mode::Wall) {
            self.art_scenes.hover(
                Some(id.clone()),
                appearance::reduced_motion(),
                Instant::now(),
            );
        }
        if self.screen == StartScreen::Recents {
            self.select_recent_game(id, window, cx);
        } else {
            self.select_game(id, window, cx);
        }
        if self.screen == StartScreen::Library {
            self.library_reveal = Some(index);
        }
        self.focus.focus(window, cx);
    }

    fn switch_screen(&mut self, screen: StartScreen, window: &mut Window, cx: &mut Context<Self>) {
        self.art_scenes.focus = None;
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
        !matches!(
            self.design().recents,
            crate::design::RecentPresentation::Gallery
                | crate::design::RecentPresentation::Cinematic
                | crate::design::RecentPresentation::InfiniteWall
                | crate::design::RecentPresentation::FilmReel
                | crate::design::RecentPresentation::Mosaic
        ) && self.selected.is_some()
            && self.appearance.library_details
            && !self.detail_closed
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
            self.main_content_width(window) - details,
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
        if tab == DetailTab::Launch {
            self.refresh_steam_shortcuts(cx);
        }
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

    fn visible_sidebar_progress(&self) -> f32 {
        if self
            .design()
            .sidebar_allowed(self.screen, self.settings_open)
        {
            self.sidebar_progress()
        } else {
            0.
        }
    }

    fn main_content_width(&self, window: &Window) -> f32 {
        (f32::from(window.viewport_size().width)
            - self.appearance.sidebar_width * self.visible_sidebar_progress())
        .max(1.)
    }

    fn show_filtered_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter(cx);
        self.switch_screen(StartScreen::Library, window, cx);
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        if !self.snapshot.ready
            || self.command_mode.is_some()
            || !self
                .design()
                .sidebar_allowed(self.screen, self.settings_open)
        {
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

    fn refresh_render_power(&mut self, window: &Window, force: bool, cx: &mut Context<Self>) {
        let changed = self.render_power.refresh(
            window,
            self.appearance.pause_in_background,
            force,
            crate::automation::render_offscreen(cx),
        );
        let paused = self.render_power.paused();
        self.background_clock
            .update(!paused && self.liquid_background_active() && !appearance::reduced_motion());
        self.gallery_motion.set_paused(paused, Instant::now());
        self.cinematic_motion.set_paused(paused, Instant::now());
        self.art_scenes
            .pause(paused || !self.art_scene_active(), Instant::now());
        self.cinematic_scene
            .set_paused(paused || !self.cinematic_active(), Instant::now());
        self.gallery_gpu.update(cx, |gpu, _| {
            gpu.set_paused(
                paused
                    || !(self.liquid_background_active()
                        || self.gallery_active()
                        || (self.art_scene_active()
                            && self.art_scene_mode() == Some(crate::art_scenes::Mode::Reel))),
            )
        });
        if changed {
            self.carousel_tick = None;
            crate::diagnostics::record(if paused {
                "Visual effects suspended"
            } else {
                "Visual effects resumed"
            });
            cx.notify();
        }
    }

    fn set_appearance(
        &mut self,
        settings: AppearanceSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance_save = None;
        let relayout = crate::library_layout::resets_scroll(&self.appearance, &settings);
        let recents_relayout = self.appearance.recents_layout != settings.recents_layout;
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
            if recents_relayout {
                self.gallery_motion = Default::default();
                self.cinematic_motion = Default::default();
                self.cinematic_scene = Default::default();
                self.art_scenes = Default::default();
                self.gallery_wheel = Default::default();
                self.carousel_transition = None;
                self.carousel_tick = None;
                self.carousel_elapsed = Duration::ZERO;
                self.ensure_selection(window, cx);
            }
            if relayout {
                self.library_list_key = None;
                self.library_reveal = None;
                self.library_list.scroll_to(ListOffset {
                    item_ix: 0,
                    offset_in_item: px(0.),
                });
            }
            self.apply_appearance(window, cx);
            self.refresh_render_power(window, false, cx);
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
            if matches!(
                provider.id.as_str(),
                "steam" | "epic" | "gog" | "xbox" | "example"
            ) {
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
                button("all-games", cx)
                    .map(|button| snug_sidebar_button(button, IconName::FolderClosed, cx))
                    .h(px(36.))
                    .ghost()
                    .child(sidebar_label(format!("Installed   {installed}")))
                    .selected(
                        self.screen == StartScreen::Library
                            && self.library_filter == LibraryFilter::Installed
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
                        view.show_filtered_library(window, cx);
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
                                button("favourites", cx)
                                    .map(|button| snug_sidebar_button(button, IconName::Star, cx))
                                    .h(px(36.))
                                    .ghost()
                                    .child(sidebar_label(format!("Favourites   {favourites}")))
                                    .disabled(favourites == 0)
                                    .selected(
                                        self.screen == StartScreen::Library
                                            && self.library_filter == LibraryFilter::Favourites,
                                    )
                                    .border_0()
                                    .w_full()
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.library_filter = LibraryFilter::Favourites;
                                        view.source_filter = None;
                                        view.collection_filter = None;
                                        view.show_filtered_library(window, cx);
                                    })),
                            ),
                    )
                },
            )
            .child(
                button("hidden-games", cx)
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
                    .selected(
                        self.screen == StartScreen::Library
                            && self.library_filter == LibraryFilter::Hidden,
                    )
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.library_filter = LibraryFilter::Hidden;
                        view.source_filter = None;
                        view.collection_filter = None;
                        view.show_filtered_library(window, cx);
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
                        button(SharedString::from(format!("source-{source}")), cx)
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
                                self.screen == StartScreen::Library
                                    && (self.source_filter.as_ref() == Some(&source)
                                        || source == "manual"
                                            && self.library_filter == LibraryFilter::Manual),
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
                                view.show_filtered_library(window, cx);
                            }))
                    }),
            )
            .when(unavailable > 0, |sidebar| {
                sidebar.child(
                    button("unavailable", cx)
                        .map(|button| snug_sidebar_button(button, IconName::FolderOpen, cx))
                        .h(px(36.))
                        .ghost()
                        .child(sidebar_label(format!(
                            "All saved   {}",
                            installed + unavailable
                        )))
                        .selected(
                            self.screen == StartScreen::Library
                                && self.library_filter == LibraryFilter::All,
                        )
                        .border_0()
                        .w_full()
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.library_filter = LibraryFilter::All;
                            view.source_filter = None;
                            view.collection_filter = None;
                            view.show_filtered_library(window, cx);
                        })),
                )
            })
            .when(!self.collection_totals().is_empty(), |sidebar| {
                sidebar
                    .child(div().mt_4().child(eyebrow("COLLECTIONS")))
                    .children(self.collection_totals().into_iter().map(
                        |(name, (completed, total))| {
                            button(SharedString::from(format!("collection-{name}")), cx)
                                .map(|button| {
                                    snug_sidebar_button(button, IconName::FolderClosed, cx)
                                })
                                .h(px(36.))
                                .ghost()
                                .w_full()
                                .border_0()
                                .child(sidebar_label(format!("{name}   {completed}/{total}")))
                                .tooltip(name.clone())
                                .selected(
                                    self.screen == StartScreen::Library
                                        && self.collection_filter.as_ref() == Some(&name),
                                )
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.collection_filter = Some(name.clone());
                                    view.source_filter = None;
                                    view.library_filter = LibraryFilter::All;
                                    view.show_filtered_library(window, cx);
                                }))
                        },
                    ))
            })
            .child(div().flex_1())
            .child(
                button("settings", cx)
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
        let rounding = Rounding::of(cx);
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
            .rounded(rounding.card())
            .cursor_pointer()
            .when(selected, |row| row.bg(cx.theme().secondary_hover.alpha(1.)))
            .hover(|style| style.bg(cx.theme().muted))
            .child(
                div()
                    .w(px(geometry.artwork_height * 1.8125))
                    .h(px(geometry.artwork_height))
                    .flex_shrink_0()
                    .rounded(rounding.artwork)
                    .overflow_hidden()
                    .bg(cx.theme().muted)
                    .child(match cover {
                        Some(asset) => rounded_image(
                            asset.thumbnail_path.clone(),
                            ObjectFit::Cover,
                            rounding.artwork,
                            cx,
                        )
                        .into_any_element(),
                        None => rounded_placeholder("", cx.theme().colors, rounding.artwork)
                            .into_any_element(),
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
                                button("empty-add", cx)
                                    .primary()
                                    .label("Add a game")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.pick(PickerPurpose::AddGame, window, cx)
                                    })),
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
                            .font_family(self.design().display_font)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        button("sort-library", cx)
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
        let rounding = Rounding::of(cx);
        let date = last_played(game);
        let show_date = self.appearance.library_last_played && !date.is_empty();
        let metadata_lines = usize::from(show_date) + usize::from(self.appearance.library_sources);
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
            .rounded(rounding.artwork)
            .bg(cx.theme().muted)
            .h(px(geometry.artwork_height))
            .flex_shrink_0()
            .when(compact, |art| {
                art.w(px(geometry.artwork_height)).flex_shrink_0()
            })
            .when(!compact, |art| art.w_full())
            .child(match cover {
                Some(asset) => rounded_image(
                    asset.thumbnail_path.clone(),
                    ObjectFit::Cover,
                    rounding.artwork,
                    cx,
                )
                .into_any_element(),
                None => {
                    rounded_placeholder("", cx.theme().colors, rounding.artwork).into_any_element()
                }
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
                            button(SharedString::from(format!("card-play-{}", game.id.0)), cx)
                                .secondary()
                                .w(px(28.))
                                .h(px(28.))
                                .p_0()
                                .icon(Icon::default().path("icons/play.svg"))
                                .bg(crate::glass::Palette::of(cx).button)
                                .tooltip("Play game · double-click the card to launch")
                                .disabled(!game.available && !game.has_launch_override())
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
                            button(SharedString::from(format!("card-manage-{}", game.id.0)), cx)
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
        let caption = if compact {
            div().flex_1().flex().flex_col().gap(px(4.))
        } else {
            artwork_card_caption().h(px(geometry.caption_height_for(metadata_lines)))
        }
        .min_w_0()
        .child(single_line_title(
            SharedString::from(format!("library-title-{}", id.0)),
            game.title(),
            px(geometry.title_font_size),
            FontWeight::SEMIBOLD,
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
        .when(!compact && show_date, |caption| {
            caption.child(library_metadata(&date, geometry))
        })
        .when(!compact && self.appearance.library_sources, |caption| {
            caption.child(library_metadata(source_name(&game.provider_id), geometry))
        });
        let card = artwork_card_states(
            artwork_card_frame(rounding.card())
                .id(SharedString::from(format!("library-card-{}", id.0)))
                .group(group.clone())
                .flex_1()
                .min_w_0()
                .h(px(if compact {
                    geometry.row_height - geometry.gap
                } else {
                    geometry.card_height_for(metadata_lines)
                }))
                .flex()
                .gap(px(crate::library_layout::CARD_CAPTION_GAP))
                .when(compact, |card| card.flex_row().items_center().px_2())
                .when(!compact, |card| card.flex_col()),
            selected,
            cx.theme().colors,
        )
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
                    .when(show_date, |column| {
                        column.child(library_metadata(&date, geometry))
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
                if game.available || game.has_launch_override() {
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
            LibraryGameAction::Reveal => {
                if !self.block_native_action("reveal game directory", cx) {
                    cx.reveal_path(&game.install_directory);
                }
            }
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
        button(SharedString::from(format!("star-{}", id.0)), cx)
            .secondary()
            .w(px(26.))
            .h(px(26.))
            .p_0()
            .icon(if game.favourite {
                Icon::default().path("icons/star-filled.svg")
            } else {
                Icon::new(IconName::Star)
            })
            .bg(crate::glass::Palette::of(cx).button)
            .text_color(if game.favourite {
                cx.theme().primary
            } else {
                cx.theme().foreground
            })
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
        toolbar_panel(
            toolbar_island("navigation-controls", toolbar_rounding(cx)).children(
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
                    .disabled(self.command_mode.is_some())
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.switch_screen(screen, window, cx)
                    }))
                }),
            ),
            cx,
        )
        .into_any_element()
    }

    fn start_button(&self, game: &Game, cx: &mut Context<Self>) -> Button {
        button("start-game", cx)
            .primary()
            .min_w(px(128.))
            .h(px(38.))
            .text_size(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .when(!self.appearance.controls.compact_launch, |button| {
                button
                    .w(px(172.))
                    .h(px(44.))
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
            })
            .icon(Icon::default().path("icons/play.svg"))
            .label("Start game")
            .disabled(!game.available && !game.has_launch_override())
            .on_click(cx.listener(|view, _, _, cx| {
                if let Some(game_id) = view.selected.clone() {
                    view.submit(Command::LaunchGame { game_id }, cx);
                }
            }))
    }

    fn recent_card(&self, game: &Game, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let rounding = Rounding::of(cx);
        let date = last_played(game);
        let id = game.id.clone();
        let artwork = game
            .artwork
            .get(&ArtworkKind::Background)
            .or_else(|| game.artwork.get(&ArtworkKind::Cover))
            .or_else(|| game.artwork.get(&ArtworkKind::Icon));
        let selected = self.selected.as_ref() == Some(&id);
        let card = artwork_card_states(
            artwork_card_frame(rounding.card())
                .id(SharedString::from(format!("recent-{}", id.0)))
                .flex_1()
                .min_w_0()
                .max_w(px(280.))
                .flex()
                .flex_col()
                .gap_2()
                .when(!self.appearance.fade_recents, |card| {
                    card.bg(cx.theme().background.alpha(0.94))
                }),
            selected,
            cx.theme().colors,
        )
        .cursor_pointer()
        .child(
            div()
                .relative()
                .w_full()
                .h(px(120.))
                .flex_shrink_0()
                .rounded(rounding.artwork)
                .overflow_hidden()
                .child(match artwork {
                    Some(asset) => rounded_image(
                        asset.thumbnail_path.clone(),
                        ObjectFit::Cover,
                        rounding.artwork,
                        cx,
                    )
                    .absolute()
                    .inset_0()
                    .into_any_element(),
                    None => rounded_placeholder("", cx.theme().colors, rounding.artwork)
                        .into_any_element(),
                }),
        )
        .child(
            artwork_card_caption()
                .gap_2()
                .child(single_line_title(
                    SharedString::from(format!("recent-card-title-{}", id.0)),
                    game.title(),
                    window.rem_size() * 0.875,
                    FontWeight::SEMIBOLD,
                ))
                .when(!date.is_empty(), |caption| {
                    caption.child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .line_clamp(1)
                            .child(date),
                    )
                }),
        )
        .on_click(cx.listener(move |view, _, window, cx| {
            view.select_recent_game(id.clone(), window, cx);
        }));
        self.with_library_context_menu(game, card, cx)
    }

    /// Manual navigation stays on the chosen game until explicitly resumed.
    /// Automatic advancement must keep using `select_game` instead.
    fn select_recent_game(&mut self, id: GameId, window: &mut Window, cx: &mut Context<Self>) {
        self.carousel_paused = true;
        self.select_game(id, window, cx);
        self.focus.focus(window, cx);
    }

    fn carousel_is_paused(&self, window: &Window, cx: &App) -> bool {
        self.carousel_paused
            || self.game_menu_open
            || self.carousel_focus.contains_focused(window, cx)
    }

    fn advance_carousel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.screen != StartScreen::Recents
            || self.design().recents != crate::design::RecentPresentation::Carousel
            || !self.snapshot.ready
            || self.settings_open
            || self.jobs_open
            || self.command_mode.is_some()
            || self.customization_open
            || self.theme_editor.is_some()
            || self.detail_tab != DetailTab::Overview
            || self.carousel_is_paused(window, cx)
            || self.render_power.paused()
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
                if self.appearance.dither_recents {
                    self.appearance.dither_recents_strength
                } else {
                    0.
                },
                self.appearance.dither_recents_size,
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
            .top_0()
            .left_0()
            .w_full()
            .h(window.viewport_size().height)
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
        let has_history = games.iter().any(|game| !last_played(game).is_empty());
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
            .child(div().id("recents-content").size_full().overflow_y_scroll().flex().flex_col()
            .child(div().id("recents-hero").relative().overflow_hidden().w_full().min_h(window.viewport_size().height).flex_shrink_0().flex().flex_col()
            .when(self.liquid_background_active(), |view| view.child(self.liquid_background(cx)))
            .when(!self.liquid_background_active(), |view| view
                .when_some(transition, |view, (previous, _)| view.child(self.recents_background(previous, 1. - opacity, window, cx)))
                .child(self.recents_background(&game, opacity, window, cx)))
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
                                    .child(div().id("selected-recent-title").w_full().min_w_0()
                                        .text_size(px(40.)).line_height(relative(1.2)).font_weight(FontWeight::SEMIBOLD)
                                        .line_clamp(3).child(game.title().to_string())
                                        .tooltip({ let title = game.title().to_string(); move |window, cx| Tooltip::new(title.clone()).build(window, cx) }))
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
                                            .when(!last_played(&game).is_empty(), |row| {
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
                                    .when_some(game.effective_metadata().description, |panel, description| {
                                        panel.child(div().w_full().min_w_0().text_sm().text_color(cx.theme().muted_foreground).line_clamp(2).child(description))
                                    })
                                    .child(div().flex().items_center().gap_2()
                                        .child(self.start_button(&game, cx))
                                        .child(self.library_favourite_button(&game, cx).w(px(38.)).h(px(38.)))
                                        .child(icon_button("recents-game-options", IconName::Ellipsis, "Edit game", cx)
                                            .w(px(38.)).h(px(38.))
                                            .on_click(cx.listener(|view, _, window, cx| view.open_details(DetailTab::Metadata, window, cx))))),
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
                                                button("pause-recents", cx)
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
                                                button("previous-recent", cx)
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
                                                    button(("recent-position", index), cx)
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
                                                                view.select_recent_game(
                                                                    id.clone(),
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        ))
                                                },
                                            ))
                                            .child(
                                                button("next-recent", cx)
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
                                        button("recents-view-library", cx)
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
                                div().w_full().flex().items_start().gap_3().children(
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
                             .when(self.appearance.recents_activity, |widgets| widgets.child(widget_surface(cx).child(self.weekly_activity(cx))))
                             .when(self.appearance.recents_progress, |widgets| widgets.child(widget_surface(cx).child(self.collection_progress(true, cx))))
                             .when(self.appearance.recents_journal, |widgets| widgets.child(widget_surface(cx).child(self.journal_widget(true, cx)))))
                    }),
            )
            .on_mouse_down(MouseButton::Right, cx.listener(|view, _, window, cx| {
                view.carousel_focus.focus(window, cx);
                view.carousel_tick = None;
            }))
            .context_menu(move |menu, _, cx| library_game_menu(menu, &menu_game, owner.clone(), cx))
            .into_any_element()
    }

    fn update_notification_status(&self, cx: &App) -> Option<String> {
        #[cfg(feature = "self-update")]
        {
            self.updater.read(cx).notification_status()
        }
        #[cfg(not(feature = "self-update"))]
        {
            let _ = cx;
            None
        }
    }

    fn activity_button(&self, cx: &mut Context<Self>) -> Button {
        let notification = self
            .notice
            .clone()
            .or_else(|| self.update_notification_status(cx));
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
            cx,
        )
        .tooltip(notification.clone().unwrap_or(tooltip))
        .selected(self.jobs_open)
        .toggled(self.jobs_open)
        .relative()
        .when(notification.is_some(), |button| {
            button.text_color(cx.theme().primary)
        })
        .when(busy || notification.is_some(), |button| {
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
        cx: &App,
    ) -> Button {
        toolbar_icon_button(id, icon, label, cx)
            .disabled(self.command_mode.is_some())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    fn toolbar(&self, sidebar: f32, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let inset = crate::window_chrome::native_left_inset(
            cfg!(target_os = "macos"),
            window.is_fullscreen(),
        );
        let clearance = 12. + (self.appearance.sidebar_width - (142. + inset)).max(0.) * sidebar;
        div()
            .id("window-toolbar")
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(self.design().header_height))
            .block_mouse_except_scroll()
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
                        .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, window, cx| {
                            if view.window_should_move
                                && event.pressed_button == Some(MouseButton::Left)
                            {
                                view.window_should_move = false;
                                if crate::automation::native_allowed(cx, "native window drag") {
                                    appearance::start_window_move(window);
                                }
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
                content: match self.design().shell {
                    crate::design::Shell::ToolbarIslands => {
                        self.toolbar_content(sidebar, inset, clearance, window, cx)
                    }
                },
            })
            .into_any_element()
    }

    fn toolbar_content(
        &self,
        sidebar: f32,
        inset: f32,
        clearance: f32,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .child(
                toolbar_island("window-control-island", toolbar_rounding(cx))
                    .child(toolbar_panel_with_blur(
                        div().absolute().inset_0().opacity(1. - sidebar),
                        crate::glass::BLUR_RADIUS * (1. - sidebar),
                        cx,
                    ))
                    .when(inset > 0., |island| {
                        island.child(div().w(px(inset)).flex_shrink_0())
                    })
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
                                    cx,
                                )
                                .selected(
                                    self.appearance.sidebar_visible
                                        && self
                                            .design()
                                            .sidebar_allowed(self.screen, self.settings_open),
                                )
                                .toggled(
                                    self.appearance.sidebar_visible
                                        && self
                                            .design()
                                            .sidebar_allowed(self.screen, self.settings_open),
                                )
                                .disabled(
                                    !self.snapshot.ready
                                        || self.command_mode.is_some()
                                        || !self
                                            .design()
                                            .sidebar_allowed(self.screen, self.settings_open),
                                )
                                .on_click(cx.listener(
                                    |view, _, window, cx| {
                                        view.toggle_sidebar(&ToggleSidebar, window, cx)
                                    },
                                )),
                            )
                            .child(
                                self.toolbar_button(
                                    "navigation-back",
                                    IconName::ArrowLeft,
                                    "Back",
                                    cx,
                                )
                                .disabled(
                                    self.command_mode.is_some()
                                        || (!self.settings_open
                                            && !self.customization_open
                                            && self.theme_editor.is_none()
                                            && self.detail_tab == DetailTab::Overview
                                            && !self.art_scene_layer_open()),
                                )
                                .on_click(cx.listener(
                                    |view, _, window, cx| view.dismiss(&DismissPanel, window, cx),
                                )),
                            )
                            .child(
                                self.toolbar_button(
                                    "open-commands",
                                    IconName::Search,
                                    "Search games or actions",
                                    cx,
                                )
                                .on_click(cx.listener(
                                    |view, _, window, cx| {
                                        view.open_commands(&OpenCommands, window, cx)
                                    },
                                )),
                            )
                            .child(
                                self.toolbar_button(
                                    "open-settings",
                                    IconName::Settings,
                                    "Settings",
                                    cx,
                                )
                                .selected(self.settings_open)
                                .toggled(self.settings_open)
                                .on_click(cx.listener(
                                    |view, _, window, cx| {
                                        view.open_settings(&OpenSettings, window, cx)
                                    },
                                )),
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
            .child(toolbar_panel(
                toolbar_island("action-toolbar-island", toolbar_rounding(cx))
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
                                    cx,
                                )
                                .selected(self.customization_open)
                                .toggled(self.customization_open)
                                .disabled(!self.snapshot.ready || self.command_mode.is_some())
                                .on_click(cx.listener(
                                    |view, _, window, cx| {
                                        if view.theme_editor.is_some() {
                                            view.close_theme_editor(window, cx);
                                        } else {
                                            view.customization_open = !view.customization_open;
                                        }
                                        view.jobs_open = false;
                                        view.focus.focus(window, cx);
                                        cx.notify();
                                    },
                                )),
                            )
                        },
                    )
                    .child(
                        self.toolbar_button(
                            "refresh-library",
                            Icon::default().path("icons/refresh.svg"),
                            "Refresh installed games",
                            cx,
                        )
                        .disabled(!self.snapshot.ready || self.command_mode.is_some())
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.submit(Command::RefreshLibrary, cx);
                        })),
                    )
                    .child(
                        self.toolbar_button("add-game", IconName::Plus, "Add a game", cx)
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
            .into_any_element()
    }

    fn hero(&self, game: &Game, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let artwork = crate::detail_artwork::background(game);
        let metadata = game.effective_metadata();
        let bounds = size(
            px(DETAIL_WIDTH),
            window.viewport_size().height.max(px(620.)),
        );
        let treated = artwork
            .filter(|_| {
                self.appearance.dither_artwork && self.appearance.dither_artwork_strength > 0.
            })
            .and_then(|asset| {
                self.library_artwork.update(cx, |cache, cx| {
                    cache.artwork(
                        asset.image_path.clone(),
                        bounds,
                        self.appearance.dither_artwork_strength,
                        self.appearance.dither_artwork_size,
                        window,
                        cx,
                    )
                })
            });
        div()
            .image_cache(self.full_resolution_images.clone())
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background.alpha(0.35))
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .absolute()
                    .size_full()
                    .child(match (treated, artwork) {
                        (Some(image), _) => img(image)
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        (None, Some(asset)) => {
                            image(asset.image_path.clone(), ObjectFit::Cover, cx).into_any_element()
                        }
                        (None, None) => div().size_full().bg(cx.theme().muted).into_any_element(),
                    }),
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
                            .rounded(Rounding::of(cx).surface)
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
                                            .w(px(if self.appearance.controls.compact_launch {
                                                38.
                                            } else {
                                                44.
                                            }))
                                            .h(px(if self.appearance.controls.compact_launch {
                                                38.
                                            } else {
                                                44.
                                            })),
                                    )
                                    .child(
                                        icon_button(
                                            "edit-game",
                                            IconName::Ellipsis,
                                            "Edit metadata, artwork or launch options",
                                            cx,
                                        )
                                        .w(px(if self.appearance.controls.compact_launch {
                                            38.
                                        } else {
                                            44.
                                        }))
                                        .h(px(if self.appearance.controls.compact_launch {
                                            38.
                                        } else {
                                            44.
                                        }))
                                        .on_click(
                                            cx.listener(|view, _, window, cx| {
                                                view.open_details(DetailTab::Metadata, window, cx)
                                            }),
                                        ),
                                    ),
                            )
                            .when(!last_played(game).is_empty(), |footer| {
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
                            button("find-detail-background", cx)
                                .secondary()
                                .h(px(36.))
                                .label("Choose background…")
                                .text_color(cx.theme().foreground)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.cancel_artwork();
                                    view.artwork_kind = ArtworkKind::Background;
                                    view.reset_artwork_pages();
                                    view.open_details(DetailTab::Artwork, window, cx)
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    fn detail_overview(
        &self,
        game: &Game,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                        icon_button(
                            "close-detail-pane",
                            IconName::Close,
                            "Close detail pane",
                            cx,
                        )
                        .bg(crate::glass::Palette::of(cx).button)
                        .on_click(
                            cx.listener(|view, _, window, cx| view.close_detail_pane(window, cx)),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn launch_settings(&self, game: &Game, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let saving = self.launch_save_job.is_some();
        let game_id = game.id.clone();
        let import_id = game.id.clone();
        let selected_key = match &self.steam_shortcut {
            Some(SteamLaunch::Shortcut {
                account_id, app_id, ..
            }) => Some((*account_id, *app_id)),
            _ => None,
        };
        let selected_shortcut = self
            .snapshot
            .settings
            .steam_shortcuts
            .iter()
            .find(|shortcut| Some((shortcut.account_id, shortcut.app_id)) == selected_key);
        let mut shortcuts = vec![(None, SharedString::from("Choose a non-Steam shortcut…"))];
        shortcuts.extend(
            self.snapshot
                .settings
                .steam_shortcuts
                .iter()
                .map(|shortcut| {
                    (
                        Some((shortcut.account_id, shortcut.app_id)),
                        SharedString::from(format!(
                            "{} · Steam account {}",
                            shortcut.name, shortcut.account_id
                        )),
                    )
                }),
        );
        if let Some(key) = selected_key
            && selected_shortcut.is_none()
        {
            let name = match &self.steam_shortcut {
                Some(SteamLaunch::Shortcut { name, .. }) => name.as_str(),
                _ => "Saved shortcut",
            };
            shortcuts.push((
                Some(key),
                format!("{name} · account {} · not in current list", key.0).into(),
            ));
        }
        let shortcut_owner = cx.entity().downgrade();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(eyebrow("LAUNCH VIA"))
            .child(choice_dropdown(
                "launch-method",
                self.launch_mode,
                vec![
                    (LaunchMode::Default, "Default launcher".into()),
                    (LaunchMode::SteamShortcut, "Steam non-Steam shortcut".into()),
                    (LaunchMode::SteamUrl, "Steam launch URL".into()),
                    (
                        LaunchMode::Executable,
                        "Executable / application bundle".into(),
                    ),
                ],
                !saving,
                move |mode, _, cx| {
                    let _ = owner.update(cx, |view, cx| {
                        view.launch_mode = mode;
                        view.launch_message = None;
                        if mode == LaunchMode::SteamShortcut
                            && !view.snapshot.settings.steam_shortcuts_loaded
                        {
                            view.refresh_steam_shortcuts(cx);
                        }
                        cx.notify();
                    });
                },
                cx,
            ))
            .when(self.launch_mode == LaunchMode::SteamShortcut, |content| {
                content
                    .child(choice_dropdown(
                        "steam-shortcut-picker",
                        selected_key,
                        shortcuts,
                        !saving,
                        move |key, _, cx| {
                            let _ = shortcut_owner.update(cx, |view, cx| {
                                view.steam_shortcut = view
                                    .snapshot
                                    .settings
                                    .steam_shortcuts
                                    .iter()
                                    .find(|shortcut| {
                                        Some((shortcut.account_id, shortcut.app_id)) == key
                                    })
                                    .map(SteamShortcut::launch);
                                view.launch_message = None;
                                cx.notify();
                            });
                        },
                        cx,
                    ))
                    .child(
                        button("refresh-steam-shortcuts", cx)
                            .label(if self.steam_shortcuts_job.is_some() {
                                "Reading shortcuts…"
                            } else {
                                "Refresh Steam shortcuts"
                            })
                            .disabled(self.steam_shortcuts_job.is_some())
                            .on_click(
                                cx.listener(|view, _, _, cx| view.refresh_steam_shortcuts(cx)),
                            ),
                    )
                    .when(
                        self.snapshot.settings.steam_shortcuts_loaded
                            && self.snapshot.settings.steam_shortcuts.is_empty(),
                        |content| content.child(hint("No saved non-Steam shortcuts found.")),
                    )
                    .when(
                        selected_key.is_some()
                            && selected_shortcut.is_none()
                            && self.snapshot.settings.steam_shortcuts_loaded,
                        |content| {
                            content.child(hint(
                                "Saved shortcut unavailable. Choose another shortcut or refresh.",
                            ))
                        },
                    )
                    .when_some(selected_shortcut, |content, shortcut| {
                        content.child(hint(&format!(
                            "Steam account {}\n{}",
                            shortcut.account_id,
                            shortcut.executable.trim_matches('"')
                        )))
                    })
                    .children(
                        self.snapshot
                            .settings
                            .steam_shortcut_warnings
                            .iter()
                            .map(|warning| hint(warning)),
                    )
            })
            .when(self.launch_mode == LaunchMode::SteamUrl, |content| {
                content
                    .child(field("Steam game URL", &self.steam_url, cx))
                    .child(
                        button("import-steam-url", cx)
                            .label("Import Steam desktop shortcut (.url)…")
                            .disabled(self.steam_url_import_job.is_some() || saving)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.pick(PickerPurpose::SteamUrl(import_id.clone()), window, cx)
                            })),
                    )
            })
            .when(self.launch_mode == LaunchMode::Executable, |content| {
                content
                    .child(field(
                        "Executable / application bundle",
                        &self.executable,
                        cx,
                    ))
                    .child(
                        button("choose-executable", cx)
                            .ghost()
                            .border_0()
                            .icon(IconName::Folder)
                            .label("Choose executable…")
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.pick(
                                    PickerPurpose::LaunchOverride(game_id.clone()),
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(field("Arguments — one per line", &self.arguments, cx))
                    .child(field(
                        if cfg!(target_os = "macos") {
                            "Working directory (executable only)"
                        } else {
                            "Working directory"
                        },
                        &self.working_directory,
                        cx,
                    ))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pt_3()
                    .border_t_1()
                    .border_color(cx.theme().border.alpha(0.5))
                    .child(
                        button("reset-launch", cx)
                            .ghost()
                            .border_0()
                            .label("Use original launcher")
                            .disabled(saving)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.launch_mode = LaunchMode::Default;
                                view.save_launch(cx);
                            })),
                    )
                    .child(
                        button("save-launch", cx)
                            .primary()
                            .label(if saving {
                                "Saving…"
                            } else {
                                "Save launch settings"
                            })
                            .disabled(saving || self.steam_url_import_job.is_some())
                            .on_click(cx.listener(|view, _, _, cx| view.save_launch(cx))),
                    ),
            )
            .when_some(self.launch_message.as_deref(), |content, message| {
                content.child(hint(message))
            })
            .into_any_element()
    }

    fn game_editor(&self, game: &Game, cx: &mut Context<Self>) -> AnyElement {
        let heading = match self.detail_tab {
            DetailTab::Metadata => "Game details",
            DetailTab::Artwork => "Artwork",
            DetailTab::Launch => "Launch configuration",
            DetailTab::Journal => "Journal",
            DetailTab::Overview => return div().into_any_element(),
        };
        let content = match self.detail_tab {
            DetailTab::Overview => return div().into_any_element(),
            DetailTab::Metadata => div()
                .w_full()
                .flex()
                .flex_col()
                .gap_4()
                .child(field("Title", &self.title, cx))
                .child(field("Description", &self.description, cx))
                .child(
                    div()
                        .flex()
                        .gap_4()
                        .child(div().flex_1().min_w_0().child(field(
                            "Developer",
                            &self.developer,
                            cx,
                        )))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(field("Genres", &self.genres, cx)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            button("reidentify-details-game", cx)
                                .ghost()
                                .border_0()
                                .icon(IconName::Search)
                                .label("Re-identify game…")
                                .disabled(self.identity_job.is_some())
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.begin_reidentify(window, cx)
                                })),
                        )
                        .child(
                            button("save-metadata", cx)
                                .h(px(34.))
                                .primary()
                                .label("Save details")
                                .on_click(cx.listener(|view, _, _, cx| view.save_metadata(cx))),
                        ),
                )
                .child(
                    div()
                        .mt_3()
                        .pt_4()
                        .border_t_1()
                        .border_color(cx.theme().border.alpha(0.5))
                        .child(
                            button("toggle-editor-organization", cx)
                                .ghost()
                                .border_0()
                                .px_0()
                                .icon(if self.editor_organization {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .label("Library organization")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.editor_organization = !view.editor_organization;
                                    cx.notify();
                                })),
                        ),
                )
                .when(self.editor_organization, |panel| {
                    panel
                        .child(field("Collections — one per line", &self.collections, cx))
                        .child(
                            Switch::new("game-completed")
                                .checked(self.completed)
                                .label("Completed")
                                .on_click(cx.listener(|view, completed, _, cx| {
                                    view.completed = *completed;
                                    cx.notify();
                                })),
                        )
                        .child(
                            div().flex().justify_end().child(
                                button("save-game-progress", cx)
                                    .label("Save progress")
                                    .on_click(cx.listener(|view, _, _, cx| view.save_progress(cx))),
                            ),
                        )
                        .child(
                            div()
                                .pt_3()
                                .border_t_1()
                                .border_color(cx.theme().border.alpha(0.5))
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    div().flex().child(
                                        button("editor-game-visibility", cx)
                                            .ghost()
                                            .border_0()
                                            .icon(if game.hidden {
                                                IconName::Eye
                                            } else {
                                                IconName::EyeOff
                                            })
                                            .label(if game.hidden {
                                                "Unhide game"
                                            } else {
                                                "Hide from library"
                                            })
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                if let Some(id) = view.selected.clone() {
                                                    view.run_library_game_action(
                                                        &id,
                                                        LibraryGameAction::Hidden,
                                                        window,
                                                        cx,
                                                    );
                                                }
                                            })),
                                    ),
                                )
                                .when(game.provider_id == "manual", |panel| {
                                    panel.child(
                                        div().flex().child(
                                            button("editor-remove-game", cx)
                                                .ghost()
                                                .border_0()
                                                .label("Remove from library…")
                                                .on_click(cx.listener(|view, _, window, cx| {
                                                    view.remove_manual_game(window, cx)
                                                })),
                                        ),
                                    )
                                }),
                        )
                })
                .into_any_element(),
            DetailTab::Launch => div()
                .w_full()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(if game.steam_launch.is_some() {
                            "Launch with Steam".into()
                        } else if game.launch_override.is_some() {
                            "Custom executable".into()
                        } else {
                            format!("Launch with {}", source_name(&game.provider_id))
                        }),
                )
                .when(!self.launch_advanced, |panel| {
                    panel.child(
                        div().flex().child(
                            button("customize-launch", cx)
                                .label("Customize launch…")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.launch_advanced = true;
                                    cx.notify();
                                })),
                        ),
                    )
                })
                .when(self.launch_advanced, |panel| {
                    panel.child(self.launch_settings(game, cx))
                })
                .into_any_element(),
            DetailTab::Artwork => self.artwork_picker(game, cx),
            DetailTab::Journal => self.game_journal(game, cx),
        };
        let artwork = game
            .artwork
            .get(&ArtworkKind::Cover)
            .or_else(|| game.artwork.get(&ArtworkKind::Icon));
        div()
            .id("game-editor-shell")
            .size_full()
            .flex()
            .child(
                div()
                    .w(px(168.))
                    .flex_shrink_0()
                    .h_full()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .bg(cx.theme().background.alpha(0.35))
                    .border_r_1()
                    .border_color(cx.theme().border.alpha(0.5))
                    .child(
                        div()
                            .w_full()
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .pb_2()
                            .when_some(artwork, |identity, asset| {
                                let radius = Rounding::of(cx).artwork;
                                identity.child(
                                    crate::detail_artwork::editor_poster_frame(asset.kind, radius)
                                        .child(rounded_image(
                                            asset.thumbnail_path.clone(),
                                            ObjectFit::Cover,
                                            radius,
                                            cx,
                                        )),
                                )
                            })
                            .child(
                                div()
                                    .id("editor-identity-title")
                                    .role(Role::Label)
                                    .aria_label(game.title().to_string())
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .line_clamp(3)
                                    .child(game.title().to_string())
                                    .tooltip({
                                        let title = game.title().to_string();
                                        move |window, cx| {
                                            Tooltip::new(title.clone()).build(window, cx)
                                        }
                                    }),
                            )
                            .child(hint(source_name(&game.provider_id))),
                    )
                    .child(div().flex().flex_col().gap_1().children(
                        DetailTab::EDITORS.into_iter().map(|(tab, _)| {
                            let (label, icon) = match tab {
                                DetailTab::Metadata => ("Details", IconName::Settings2),
                                DetailTab::Artwork => ("Artwork", IconName::GalleryVerticalEnd),
                                DetailTab::Launch => ("Launch", IconName::ExternalLink),
                                _ => ("Journal", IconName::BookOpen),
                            };
                            button(label, cx)
                                .ghost()
                                .border_0()
                                .w_full()
                                .h(px(34.))
                                .px_3()
                                .icon(Icon::new(icon).small().mr_1())
                                .label(label)
                                .child(div().flex_1())
                                .when(self.detail_tab == tab, |button| {
                                    button.bg(cx.theme().secondary.alpha(0.8))
                                })
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    if view.detail_tab == DetailTab::Artwork
                                        && tab != DetailTab::Artwork
                                    {
                                        view.cancel_artwork();
                                    }
                                    view.open_details(tab, window, cx);
                                }))
                        }),
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .w_full()
                            .flex_shrink_0()
                            .px_6()
                            .pt_5()
                            .pb_5()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xl()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(heading),
                            )
                            .child(
                                icon_button(
                                    "close-game-editor",
                                    IconName::Close,
                                    "Close game editor",
                                    cx,
                                )
                                .ghost()
                                .border_0()
                                .on_click(
                                    cx.listener(|view, _, window, cx| {
                                        view.close_editor(window, cx)
                                    }),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("game-editor-scroll-{heading}")))
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .overflow_y_scroll()
                            .child(div().w_full().p_6().pt_0().child(content)),
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
        modal_backdrop("game-editor-backdrop", cx.theme().overlay)
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
                    .role(Role::Group)
                    .aria_label("Game editor")
                    .track_focus(&self.editor_focus)
                    .tab_group()
                    .w(px(860.).min(window.viewport_size().width - px(64.)))
                    .h((window.viewport_size().height - px(64.)).min(px(720.)))
                    .rounded(Rounding::of(cx).panel)
                    .overflow_hidden()
                    .shadow_lg()
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.game_editor(game, cx)),
                cx,
            ))
            .into_any_element()
    }

    fn game_search_footer(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .when(!self.artwork_games.is_empty(), |footer| {
                footer.child(hint(&format!(
                    "Showing {} of {} returned game matches.",
                    self.artwork_games_visible.min(self.artwork_games.len()),
                    self.artwork_games.len()
                )))
            })
            .when(
                self.artwork_games_visible < self.artwork_games.len(),
                |footer| {
                    footer.child(
                        button("load-more-game-matches", cx)
                            .label("Load more games")
                            .disabled(self.identity_job.is_some())
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.artwork_games_visible = view
                                    .artwork_games_visible
                                    .saturating_add(10)
                                    .min(view.artwork_games.len());
                                cx.notify();
                            })),
                    )
                },
            )
    }

    fn artwork_page_controls(&self, id: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        let busy = self.artwork_page_pending || self.identity_job.is_some();
        div()
            .id(id)
            .flex()
            .items_center()
            .gap_3()
            .when(self.artwork_page_loaded, |row| {
                row.child(
                    button("previous-artwork-page", cx)
                        .label("Previous page")
                        .disabled(busy || self.artwork_page == 0)
                        .on_click(cx.listener(|view, _, _, cx| {
                            if view.artwork_page > 0 {
                                view.load_artwork_page(view.artwork_page - 1, None, cx);
                            }
                        })),
                )
                .child(hint(&format!(
                    "Page {} · {} images",
                    u64::from(self.artwork_page) + 1,
                    self.artwork_images.len()
                )))
                .child(
                    button("next-artwork-page", cx)
                        .label("Next page")
                        .disabled(busy || !self.artwork_has_more || self.artwork_page == u32::MAX)
                        .on_click(cx.listener(|view, _, _, cx| {
                            if view.artwork_has_more
                                && let Some(page) = view.artwork_page.checked_add(1)
                            {
                                view.load_artwork_page(page, None, cx);
                            }
                        })),
                )
            })
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
        let mut saved = game
            .artwork_history
            .iter()
            .filter(|asset| asset.kind == kind)
            .collect::<Vec<_>>();
        if let Some(current) = game.artwork.get(&kind) {
            if !saved.iter().any(|asset| asset.id == current.id) {
                saved.insert(0, current);
            }
        }
        let loading = self.artwork_job.is_some() || self.identity_job.is_some();
        let mut panel = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .pb_3()
                    .border_b_1()
                    .border_color(cx.theme().border.alpha(0.5))
                    .child(
                        div().flex().gap_1().children(
                            [
                                ArtworkKind::Cover,
                                ArtworkKind::Background,
                                ArtworkKind::Logo,
                                ArtworkKind::Icon,
                            ]
                            .into_iter()
                            .map(|kind| {
                                choice_button(
                                    kind.label(),
                                    kind.label(),
                                    kind == self.artwork_kind,
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |view, _, _, cx| {
                                        view.cancel_artwork();
                                        view.artwork_kind = kind;
                                        view.reset_artwork_pages();
                                        view.artwork_message = None;
                                        if view.artwork_online
                                            && view
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
                                                    matches!(
                                                        game.provider_id.as_str(),
                                                        "steam" | "example"
                                                    )
                                                }))
                                        {
                                            view.load_artwork(None, cx);
                                        }
                                        cx.notify();
                                    },
                                ))
                            }),
                        ),
                    )
                    .child(
                        button("import-artwork", cx)
                            .ghost()
                            .border_0()
                            .icon(IconName::Plus)
                            .label("Upload…")
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.pick(PickerPurpose::Artwork(game_id.clone(), kind), window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(
                        if self.artwork_online {
                            "Online artwork"
                        } else {
                            "Your images"
                        },
                    ))
                    .child(
                        button("artwork-source", cx)
                            .ghost()
                            .border_0()
                            .label(if self.artwork_online {
                                "Back to saved images"
                            } else {
                                "Find more artwork"
                            })
                            .icon(if self.artwork_online {
                                IconName::ArrowLeft
                            } else {
                                IconName::ArrowRight
                            })
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.artwork_online = !view.artwork_online;
                                view.cancel_artwork();
                                view.artwork_message = None;
                                cx.notify();
                            })),
                    ),
            );
        if !self.artwork_online {
            panel =
                panel
                    .when(saved.is_empty(), |panel| {
                        panel.child(
                            div().py_8().flex().flex_col().gap_2().child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("No saved images"),
                            ),
                        )
                    })
                    .child(
                        div().flex().flex_wrap().gap_4().children(
                            saved
                                .into_iter()
                                .map(|asset| self.saved_image(game, asset, cx)),
                        ),
                    )
                    .child(
                        div().flex().justify_end().child(
                            button("reidentify-saved-artwork-game", cx)
                                .ghost()
                                .border_0()
                                .icon(IconName::Search)
                                .label("Re-identify game…")
                                .disabled(self.identity_job.is_some())
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.begin_reidentify(window, cx)
                                })),
                        ),
                    );
        } else if !key_configured {
            panel = panel.child(
                div()
                    .py_6()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("SteamGridDB not connected"),
                    )
                    .child(
                        div().flex().child(
                            button("configure-artwork", cx)
                                .label("Connect SteamGridDB")
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.open_settings(&OpenSettings, window, cx)
                                })),
                        ),
                    ),
            );
        } else {
            panel = panel
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div().text_sm().truncate().child(
                                        self.artwork_match
                                            .as_ref()
                                            .map(|matched| matched.name.clone())
                                            .unwrap_or_else(|| game.title().to_string()),
                                    ),
                                )
                                .child(hint("SteamGridDB")),
                        )
                        .child(
                            button("reidentify-artwork-game", cx)
                                .ghost()
                                .border_0()
                                .label("Re-identify game…")
                                .disabled(self.identity_job.is_some())
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.begin_reidentify(window, cx)
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            button("load-artwork", cx)
                                .primary()
                                .label(if self.artwork_images.is_empty() {
                                    "Browse artwork"
                                } else {
                                    "Refresh images"
                                })
                                .disabled(loading)
                                .on_click(
                                    cx.listener(|view, _, _, cx| view.load_artwork(None, cx)),
                                ),
                        )
                        .when(loading, |row| {
                            row.child(Spinner::new().small())
                                .child(hint("Loading artwork…"))
                        }),
                )
                .child(self.artwork_page_controls("artwork-pager-top", cx))
                .child(
                    div().flex().flex_wrap().gap_4().children(
                        self.artwork_images
                            .iter()
                            .enumerate()
                            .map(|(index, candidate)| {
                                let selected = candidate.clone();
                                let current = game.artwork.get(&kind).is_some_and(|asset| {
                                    asset.source_url.as_deref()
                                        == Some(candidate.image_url.as_str())
                                });
                                artwork_option(
                                    ("select-remote", index).into(),
                                    candidate.preview_path.clone(),
                                    &candidate.author,
                                    kind,
                                    current,
                                    cx,
                                )
                                .tooltip(format!(
                                    "{} · {} × {}",
                                    candidate.author, candidate.width, candidate.height
                                ))
                                .disabled(current)
                                .on_click(cx.listener(
                                    move |view, _, _, cx| {
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
                                    },
                                ))
                            }),
                    ),
                )
                .child(self.artwork_page_controls("artwork-pager-bottom", cx));
        }
        panel
            .when_some(self.artwork_message.clone(), |panel, message| {
                panel.child(hint(&message))
            })
            .into_any_element()
    }

    fn reidentify_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let configured = self
            .snapshot
            .settings
            .configured_credentials
            .contains("steamgriddb");
        let saving = self.identity_job.is_some();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Find the right game"),
            )
            .when(!configured, |view| {
                view.child(self.credential_status(cx)).child(
                    button("identify-configure", cx)
                        .label("Configure SteamGridDB")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.open_settings(&OpenSettings, window, cx)
                        })),
                )
            })
            .child({
                let control = input(&self.artwork_search, cx)
                    .large()
                    .disabled(!configured || saving)
                    .into_any_element();
                #[cfg(all(feature = "automation", target_os = "macos"))]
                let control = crate::automation::editor(
                    "artwork-search",
                    "Artwork search",
                    &self.artwork_search,
                    !configured || saving,
                    control,
                    cx,
                );
                control
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button("identify-search", cx)
                            .primary()
                            .label("Search games")
                            .disabled(!configured || saving)
                            .on_click(cx.listener(|view, _, _, cx| view.search_artwork(cx))),
                    )
                    .child(
                        button("identify-cancel", cx)
                            .ghost()
                            .border_0()
                            .label("Back to artwork")
                            .disabled(saving)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.cancel_artwork();
                                view.reidentifying = false;
                                view.artwork_games.clear();
                                view.artwork_message = None;
                                cx.notify();
                            })),
                    ),
            )
            .when(self.artwork_job.is_some() || saving, |view| {
                view.child(Spinner::new().small())
            })
            .when_some(self.artwork_message.clone(), |view, message| {
                view.child(hint(&message))
            })
            .children(
                self.artwork_games
                    .iter()
                    .take(self.artwork_games_visible)
                    .map(|game| {
                        let matched = game.clone();
                        button(SharedString::from(format!("identify-{}", game.id)), cx)
                            .w_full()
                            .label(format!("Use {}", game.name))
                            .disabled(saving)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.choose_artwork_game(matched.clone(), cx)
                            }))
                    }),
            )
            .child(self.game_search_footer(cx))
            .into_any_element()
    }

    fn saved_image(&self, game: &Game, asset: &ArtworkAsset, cx: &mut Context<Self>) -> AnyElement {
        let selected = game
            .artwork
            .get(&asset.kind)
            .is_some_and(|chosen| chosen.id == asset.id);
        let asset_id = asset.id.clone();
        artwork_option(
            SharedString::from(format!("saved-{}", asset.id)).into(),
            Some(asset.thumbnail_path.clone()),
            &asset.author,
            asset.kind,
            selected,
            cx,
        )
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
        }))
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
            cx,
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
                    .label("App background blur")
                    .disabled(
                        !self.snapshot.ready
                            || !self.appearance.window_translucency
                            || !cfg!(any(target_os = "windows", target_os = "macos")),
                    )
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
            .child(self.appearance_sliders[0].clone())
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
            .child(eyebrow("INTERFACE STYLE"))
            .child(self.recents_layout_selector(cx))
            .when(self.background_selectable(), |panel| {
                panel
                    .child(eyebrow("BACKGROUND"))
                    .child(self.background_selector(cx))
            })
            .when(self.design().editable_palette(), |panel| {
                panel
                    .child(eyebrow("COLOUR THEME"))
                    .child(self.theme_selector(cx))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                button("edit-theme", cx)
                                    .icon(IconName::Palette)
                                    .label("Edit theme…")
                                    .disabled(!self.snapshot.ready || self.themes_loading)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.open_theme_editor(window, cx)
                                    })),
                            )
                            .when(custom, |row| {
                                row.child(
                                    button("rename-theme", cx)
                                        .label("Rename…")
                                        .disabled(!self.snapshot.ready || self.themes_loading)
                                        .on_click(cx.listener(|view, _, window, cx| {
                                            view.open_theme_editor(window, cx);
                                            if let Some(editor) = &view.theme_editor {
                                                editor.update(cx, |editor, cx| {
                                                    editor.focus_name(window, cx)
                                                });
                                            }
                                        })),
                                )
                            })
                            .child(
                                icon_button(
                                    "open-themes",
                                    IconName::Folder,
                                    "Open themes directory",
                                    cx,
                                )
                                .on_click(move |_, _, cx| {
                                    if crate::automation::native_allowed(cx, "reveal themes") {
                                        cx.reveal_path(&themes);
                                    }
                                }),
                            )
                            .child(
                                icon_button(
                                    "reload-themes",
                                    Icon::default().path("icons/refresh.svg"),
                                    "Reload themes",
                                    cx,
                                )
                                .disabled(self.themes_loading)
                                .on_click(
                                    cx.listener(|view, _, window, cx| {
                                        view.reload_themes(window, cx)
                                    }),
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
                        cx,
                    ))
            })
            .child(self.control_customization(cx))
            .when(self.design().native_translucency, |panel| {
                panel.child(eyebrow("GLASS")).child(self.glass_controls(cx))
            })
    }

    fn control_customization(&self, cx: &mut Context<Self>) -> Div {
        let panel = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(eyebrow("PRECISION CONTROLS"))
                    .child(
                        button("reset-precision", cx)
                            .ghost()
                            .border_0()
                            .label("Reset")
                            .disabled(!self.snapshot.ready)
                            .tooltip(
                                "Restore Precision controls without changing your theme or layout",
                            )
                            .on_click(cx.listener(|view, _, window, cx| {
                                let mut settings = view.appearance.clone();
                                settings.controls = Default::default();
                                view.set_appearance(settings, window, cx);
                            })),
                    ),
            )
            .children(self.appearance_sliders[1..4].iter().cloned())
            .children(
                [
                    (
                        "control-outlines",
                        "Control outlines",
                        self.appearance.controls.outlines,
                    ),
                    (
                        "filled-secondary",
                        "Filled secondary buttons",
                        self.appearance.controls.filled_secondary,
                    ),
                    (
                        "compact-launch",
                        "Compact launch controls",
                        self.appearance.controls.compact_launch,
                    ),
                ]
                .into_iter()
                .map(|(id, label, checked)| {
                    Switch::new(id)
                        .label(label)
                        .checked(checked)
                        .disabled(!self.snapshot.ready)
                        .on_click(cx.listener(move |view, enabled, window, cx| {
                            let mut settings = view.appearance.clone();
                            match id {
                                "control-outlines" => settings.controls.outlines = *enabled,
                                "filled-secondary" => settings.controls.filled_secondary = *enabled,
                                _ => settings.controls.compact_launch = *enabled,
                            }
                            view.set_appearance(settings, window, cx);
                        }))
                }),
            );
        #[cfg(debug_assertions)]
        let panel = panel.when(
            std::env::args().any(|argument| argument == "--example-library"),
            |panel| {
                let owner = cx.weak_entity();
                panel
                    .child(eyebrow("INTERFACE STRESS DATA · DEVELOPMENT ONLY"))
                    .child(choice_dropdown(
                        "interface-stress-data",
                        self.stress_case,
                        [
                            "Demo data",
                            "Worst case",
                            "Empty",
                            "One game",
                            "1,000 games",
                        ]
                        .into_iter()
                        .map(|name| (name, name.into()))
                        .collect(),
                        self.snapshot.ready,
                        move |selection, window, cx| {
                            let _ = owner.update(cx, |view, cx| {
                                view.stress_case = selection;
                                view.library_filter = LibraryFilter::All;
                                view.source_filter = None;
                                view.collection_filter = None;
                                view.snapshot = view.api.snapshot();
                                view.apply_stress_data();
                                view.detail_tab = DetailTab::Overview;
                                view.filter(cx);
                                view.ensure_selection(window, cx);
                                view.library_list_key = None;
                                view.library_list.scroll_to(ListOffset {
                                    item_ix: 0,
                                    offset_in_item: px(0.),
                                });
                                cx.notify();
                            });
                        },
                        cx,
                    ))
            },
        );
        panel
    }

    #[cfg(debug_assertions)]
    fn apply_stress_data(&mut self) {
        if self.stress_case == "Demo data" {
            return;
        }
        let mut games = self.snapshot.games.as_ref().clone();
        match self.stress_case {
            "Empty" => games.clear(),
            "One game" => games.truncate(1),
            "1,000 games" => {
                if let Some(template) = games.first().cloned() {
                    games = (0..1000)
                        .map(|index| {
                            let mut game = template.clone();
                            game.id = GameId(format!("example:stress-{index}"));
                            game.metadata.title =
                                Some(format!("Library collection — Game {:04}", index + 1));
                            game.artwork.clear();
                            game.available = false;
                            game.launch_override = None;
                            game
                        })
                        .collect();
                }
            }
            _ => {
                let titles = [
                    "The Elder Scrolls V: Skyrim Anniversary Edition — Creation Club Collection",
                    "FINAL FANTASY XIV Online: Dawntrail — Digital Collector’s Edition",
                    "龍が如く８外伝 Pirates in Hawaii — デラックス・エディション",
                    "Ōkami HD — 大神 絶景版 🐺",
                    "X",
                    "NieR:Automata™GameoftheYoRHaEdition",
                ];
                for (index, game) in games.iter_mut().enumerate() {
                    game.metadata.title = Some(titles[index % titles.len()].into());
                    game.metadata.description = if index % 3 == 0 {
                        None
                    } else {
                        Some("Explore a changing world, uncover its forgotten stories, and discover a different route through every encounter. ".repeat(18))
                    };
                    game.catalogue_metadata.description = None;
                    game.metadata.developer =
                        Some("Monolith Soft, Nintendo Entertainment Planning & Development".into());
                    game.collections = [
                        "Role-playing adventures — complete editions and expansions".into(),
                        "Playing at my own pace".into(),
                    ]
                    .into_iter()
                    .collect();
                    game.last_launched = (index % 3 != 0).then_some(0);
                    game.available = index % 3 != 0;
                    game.favourite = index % 2 == 0;
                    game.completed = index % 3 == 0;
                    if index % 2 == 0 {
                        game.artwork.clear();
                        game.artwork_history.clear();
                    } else {
                        let alternatives = game
                            .artwork
                            .values()
                            .map(|asset| {
                                let mut alternative = asset.clone();
                                alternative.id = format!("stress-alternative-{}", asset.id);
                                alternative.author = "Aleksandra Wiśniewska-Kowalczyk".into();
                                alternative
                            })
                            .collect::<Vec<_>>();
                        game.artwork_history.extend(alternatives);
                    }
                }
                let created_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                self.snapshot.journal = Arc::new(games.iter().take(3).enumerate().map(|(index, game)| startpoint_sdk::JournalEntry {
                    id: format!("interface-stress-note-{index}"), game_id: game.id.clone(), created_at,
                    text: "Reached the northern observatory. The entrance is behind the old library; return with the brass key and check the map before leaving. ".repeat(14),
                }).collect());
            }
        }
        self.snapshot.games = Arc::new(games);
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
                            cx,
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
            panel = panel.child(self.appearance_controls(cx)).when(
                self.design().recents == crate::design::RecentPresentation::Carousel,
                |panel| {
                    panel
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
                        .child(self.appearance_sliders[6].clone())
                        .child(self.appearance_sliders[4].clone())
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
                                .label("Weekly activity")
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
                                .label("Journal preview")
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
                        )
                },
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
                            cx,
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
                            cx,
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
                    .child(self.appearance_sliders[7].clone())
                    .child(self.appearance_sliders[5].clone())
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

    fn weekly_activity(&self, cx: &mut Context<Self>) -> AnyElement {
        let today = Local::now().date_naive();
        let start = today
            .checked_sub_days(chrono::Days::new(u64::from(
                today.weekday().num_days_from_monday(),
            )))
            .unwrap_or(today);
        let days = (0..7)
            .filter_map(|offset| {
                let date = start.checked_add_days(chrono::Days::new(offset))?;
                let key = date.format("%Y-%m-%d").to_string();
                let launches = self
                    .snapshot
                    .activity
                    .iter()
                    .find(|day| day.date == key)
                    .map(|day| day.launches)
                    .unwrap_or(0);
                Some((date, launches))
            })
            .collect::<Vec<_>>();
        let launches = days
            .iter()
            .map(|(_, count)| *count)
            .fold(0u32, u32::saturating_add);
        let active_days = days.iter().filter(|(_, count)| *count > 0).count();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .h(px(28.))
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("This week"),
                    )
                    .child(hint(&start.format("%b %-d").to_string())),
            )
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(FontWeight::MEDIUM)
                            .child(launches.to_string()),
                    )
                    .child(hint(if launches == 1 {
                        "game launch"
                    } else {
                        "game launches"
                    })),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap_1()
                    .children(days.into_iter().enumerate().map(|(index, (date, count))| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                button(("week-day", index), cx)
                                    .ghost()
                                    .border_0()
                                    .p_0()
                                    .w_full()
                                    .h(px(28.))
                                    .label(["M", "T", "W", "T", "F", "S", "S"][index])
                                    .tooltip(format!(
                                        "{} · {}",
                                        date.format("%A, %b %-d"),
                                        launch_count(count)
                                    ))
                                    .when(date == today, |button| button.bg(cx.theme().secondary))
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.activity_day = date;
                                        view.activity_month = date.with_day(1).unwrap_or(date);
                                        view.switch_screen(StartScreen::Activity, window, cx);
                                    })),
                            )
                            .child(div().mx_1().h(px(3.)).rounded_full().bg(if count == 0 {
                                cx.theme().secondary
                            } else {
                                cx.theme().primary.alpha(if count < 3 { 0.45 } else { 0.9 })
                            }))
                    })),
            )
            .child(hint(&format!(
                "{active_days} active day{}",
                if active_days == 1 { "" } else { "s" }
            )))
            .into_any_element()
    }

    fn collection_progress(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let games = self
            .snapshot
            .games
            .iter()
            .filter(|game| !game.hidden)
            .collect::<Vec<_>>();
        let completed = games.iter().filter(|game| game.completed).count();
        let total = games.len();
        let collections = self.collection_totals();
        let overall = if total == 0 {
            0.
        } else {
            completed as f32 / total as f32
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Making progress"),
            )
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(FontWeight::MEDIUM)
                            .child(completed.to_string()),
                    )
                    .child(hint(&format!(
                        "of {total} game{} completed",
                        if total == 1 { "" } else { "s" }
                    ))),
            )
            .child(
                div()
                    .w_full()
                    .h(px(3.))
                    .rounded_full()
                    .overflow_hidden()
                    .bg(cx.theme().secondary)
                    .child(div().h_full().w(relative(overall)).bg(cx.theme().primary)),
            )
            .child(
                div()
                    .id(if compact {
                        "recent-collections"
                    } else {
                        "activity-collections"
                    })
                    .w_full()
                    .max_h(px(if compact { 132. } else { 260. }))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(
                        collections
                            .iter()
                            .take(if compact { 3 } else { usize::MAX })
                            .enumerate()
                            .map(|(index, (name, (completed, total)))| {
                                let name = name.clone();
                                with_accessible_label(
                                    button(("collection-progress", index), cx),
                                    format!("Open {name} · {completed} of {total} completed"),
                                )
                                .ghost()
                                .border_0()
                                .w_full()
                                .h(px(30.))
                                .px_1()
                                .tooltip(format!("Open {name} · {completed} of {total} completed"))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_left()
                                        .truncate()
                                        .child(name.clone()),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{completed} / {total}")),
                                )
                                .on_click(cx.listener(
                                    move |view, _, window, cx| {
                                        view.collection_filter = Some(name.clone());
                                        view.source_filter = None;
                                        view.library_filter = LibraryFilter::All;
                                        view.filter(cx);
                                        view.switch_screen(StartScreen::Library, window, cx);
                                    },
                                ))
                            }),
                    ),
            )
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
            .fold(0u32, u32::saturating_add);
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
                            button(
                                if forward {
                                    "activity-next-month"
                                } else {
                                    "activity-previous-month"
                                },
                                cx,
                            )
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
                            button(SharedString::from(format!("activity-day-{key}")), cx)
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
                                button(
                                    SharedString::from(format!("activity-game-{}", game_id.0)),
                                    cx,
                                )
                                .secondary()
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
            .into_any_element()
    }

    fn journal_widget(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let entries = self
            .snapshot
            .journal
            .iter()
            .filter_map(|entry| {
                let game = self
                    .snapshot
                    .games
                    .iter()
                    .find(|game| game.id == entry.game_id)?;
                Some((entry, game))
            })
            .take(if compact { 1 } else { 3 })
            .collect::<Vec<_>>();
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
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(if compact { "Last note" } else { "Your journal" }),
                    )
                    .child(
                        button("write-journal-note", cx)
                            .ghost()
                            .border_0()
                            .h(px(28.))
                            .label("Write a note")
                            .disabled(self.selected.is_none())
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.open_details(DetailTab::Journal, window, cx)
                            })),
                    ),
            )
            .when(entries.is_empty(), |panel| {
                panel.child(hint("No notes yet"))
            })
            .children(entries.into_iter().map(|(entry, game)| {
                let id = game.id.clone();
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .line_height(relative(1.65))
                            .line_clamp(3)
                            .child(entry.text.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(game.title().to_string()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(timestamp_label(entry.created_at)),
                            ),
                    )
                    .child(
                        button(
                            SharedString::from(format!("journal-widget-{}", entry.id)),
                            cx,
                        )
                        .ghost()
                        .border_0()
                        .h(px(28.))
                        .label("Open journal")
                        .icon(IconName::ArrowRight)
                        .on_click(cx.listener(
                            move |view, _, window, cx| {
                                view.open_game_journal(id.clone(), window, cx)
                            },
                        )),
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
                        button("save-journal-note", cx)
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
                            button("cancel-journal-edit", cx)
                                .ghost()
                                .border_0()
                                .h(px(28.))
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
                panel.child(hint("No notes yet"))
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
                    .rounded(Rounding::of(cx).surface)
                    .bg(crate::glass::Palette::of(cx).input.alpha(0.3))
                    .child(hint(&timestamp_label(entry.created_at)))
                    .child(div().text_sm().child(entry.text.clone()))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                button(SharedString::from(format!("edit-note-{}", entry.id)), cx)
                                    .ghost()
                                    .border_0()
                                    .h(px(28.))
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
                                button(SharedString::from(format!("delete-note-{}", entry.id)), cx)
                                    .ghost()
                                    .border_0()
                                    .h(px(28.))
                                    .text_color(
                                        if self.journal_delete.as_ref() == Some(&entry.id) {
                                            cx.theme().danger
                                        } else {
                                            cx.theme().muted_foreground
                                        },
                                    )
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
                                    button(
                                        SharedString::from(format!(
                                            "cancel-delete-note-{}",
                                            entry.id
                                        )),
                                        cx,
                                    )
                                    .secondary()
                                    .h(px(36.))
                                    .label("Keep note")
                                    .on_click(cx.listener(
                                        |view, _, _, cx| {
                                            view.journal_delete = None;
                                            cx.notify();
                                        },
                                    )),
                                )
                            }),
                    )
            }))
            .into_any_element()
    }

    fn activity_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("library-activity")
            .size_full()
            .overflow_y_scroll()
            .px_8()
            .pb_8()
            .child(
                div()
                    .w_full()
                    .max_w(px(1100.))
                    .mx_auto()
                    .pt(px(CONTENT_TOP))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(crate::components::page_title("Your activity", cx))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_wrap()
                            .items_start()
                            .gap_4()
                            .child(widget_surface(cx).child(self.activity_calendar(cx)))
                            .child(widget_surface(cx).child(self.collection_progress(false, cx))),
                    )
                    .child(
                        widget_surface(cx)
                            .w_full()
                            .child(self.journal_widget(false, cx)),
                    ),
            )
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
                                cx,
                            )
                            .on_click(move |_, _, cx| {
                                if crate::automation::native_allowed(cx, "reveal directory") {
                                    cx.reveal_path(&path);
                                }
                            }),
                        )
                    })
                    .when(index == 0, |actions| {
                        actions
                            .child(
                                button(SharedString::from(format!("choose-{provider}")), cx)
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
                                    button(SharedString::from(format!("reset-{provider}")), cx)
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
        let content_width = px(self.main_content_width(window)) - window.rem_size() * 4.;
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
                    .child(crate::components::page_title("Settings", cx))
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
                                    cx,
                                )
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .child(div().flex_1().text_sm().child("Interface style"))
                            .child(div().w(px(200.)).child(self.recents_layout_selector(cx))),
                    )
                    .child(div().mt_4().child(eyebrow("PERFORMANCE")))
                    .child(
                        Switch::new("pause-in-background")
                            .checked(self.appearance.pause_in_background)
                            .label("Pause visual effects in background")
                            .disabled(!self.snapshot.ready)
                            .on_click(cx.listener(|view, enabled, window, cx| {
                                view.set_appearance(AppearanceSettings {
                                    pause_in_background: *enabled,
                                    ..view.appearance.clone()
                                }, window, cx);
                            })),
                    )
                    .child(div().text_xs().text_color(cx.theme().muted_foreground)
                        .child("Freezes artwork motion and automatic browsing when unfocused. Minimized windows and other fullscreen apps always pause visual effects on Windows and macOS."))
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
                                    .when(!crate::automation::active(cx), |link| link.href("https://www.steamgriddb.com/profile/preferences")),
                            ),
                    )
                    .when(!key_configured || self.credential_editing, |view| {
                        view.child(
                            div().flex().child({
                                let control = input(&self.api_key, cx).large().w_auto().flex_1().min_w_0().mask_toggle().into_any_element();
                                #[cfg(all(feature = "automation", target_os = "macos"))]
                                let control = crate::automation::editor("artwork-api-key", "Artwork API key", &self.api_key, false, control, cx);
                                control
                            }),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .when(!key_configured || self.credential_editing, |row| {
                                row.child(
                                    button("save-api-key", cx)
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
                                    button("edit-api-key", cx)
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
                                    button("api-key-options", cx)
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
                                    button("cancel-api-key", cx)
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
                            button("open-data", cx)
                                .icon(IconName::Folder)
                                .label("Open directory")
                                .on_click(move |_, _, cx| { if crate::automation::native_allowed(cx, "reveal data directory") { cx.reveal_path(&data_directory); } }),
                        ),
                        cx,
                    ))
                    .child(path_row(
                        "Thumbnail cache",
                        cache_directory.display().to_string(),
                        div().child(
                            button("open-cache", cx)
                                .icon(IconName::Folder)
                                .label("Open directory")
                                .on_click(move |_, _, cx| { if crate::automation::native_allowed(cx, "reveal cache directory") { cx.reveal_path(&cache_directory); } }),
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
        let search = input(&self.command_search, cx)
            .large()
            .w_full()
            .prefix(Icon::new(IconName::Search))
            .cleanable(true)
            .into_any_element();
        #[cfg(all(feature = "automation", target_os = "macos"))]
        let search = crate::automation::editor(
            "command-search",
            "Command search",
            &self.command_search,
            false,
            search,
            cx,
        );
        let panel = div()
            .id("command-panel")
            .role(Role::Group)
            .aria_label("Command panel")
            .w(px(560.))
            .max_w(relative(0.9))
            .rounded(Rounding::of(cx).panel)
            .shadow_lg()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(search)
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
                                                rounded_image(
                                                    asset.thumbnail_path.clone(),
                                                    ObjectFit::Cover,
                                                    Rounding::of(cx).artwork,
                                                    cx,
                                                )
                                                .size(px(28.))
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
        let update_notification = self.update_notification_status(cx).is_some();
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
            .rounded(Rounding::of(cx).panel)
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
                        icon_button("close-activity", IconName::Close, "Close activity", cx)
                            .tooltip("Close activity")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.jobs_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .map(|panel| {
                #[cfg(feature = "self-update")]
                {
                    if update_notification {
                        let restart_allowed = self.can_restart_for_update();
                        return panel.child(
                            self.updater.update(cx, |updater, cx| {
                                updater.notification(restart_allowed, cx)
                            }),
                        );
                    }
                }
                panel
            })
            .when_some(self.notice.clone(), |panel, notice| {
                panel.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_3()
                        .rounded(Rounding::of(cx).surface)
                        .bg(cx.theme().accent)
                        .child(div().text_sm().child(notice))
                        .child(
                            button("dismiss-message", cx)
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
                self.snapshot.ready
                    && self.snapshot.jobs.is_empty()
                    && self.notice.is_none()
                    && !update_notification,
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
                                    button(("cancel-job", id), cx)
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
        self.refresh_render_power(window, false, cx);
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
            } else if !self.render_power.paused() {
                window.request_animation_frame();
            }
        }
        self.prepare_library(window);
        self.prepare_gallery(window, cx);
        self.prepare_cinematic(window, cx);
        self.prepare_background(window, cx);
        self.prepare_art_scenes(window, cx);
        let slider_values = [
            (
                self.appearance.translucency_amount,
                self.appearance.window_translucency
                    && cfg!(any(target_os = "windows", target_os = "macos")),
            ),
            (self.appearance.controls.radius, true),
            (self.appearance.controls.horizontal_padding, true),
            (
                self.appearance.controls.outline_strength,
                self.appearance.controls.outlines,
            ),
            (
                self.appearance.dither_recents_strength,
                self.appearance.dither_recents,
            ),
            (
                self.appearance.dither_artwork_strength,
                self.appearance.dither_artwork,
            ),
            (
                self.appearance.dither_recents_size,
                self.appearance.dither_recents,
            ),
            (
                self.appearance.dither_artwork_size,
                self.appearance.dither_artwork,
            ),
        ];
        for (slider, (value, enabled)) in self.appearance_sliders.iter().zip(slider_values) {
            slider.update(cx, |slider, cx| {
                slider.sync(value, self.snapshot.ready && enabled, window, cx)
            });
        }
        if self.screen == StartScreen::Recents
            && !self.settings_open
            && self.design().recents == crate::design::RecentPresentation::Carousel
        {
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
            } else if !self.render_power.paused() {
                window.request_animation_frame();
            }
        }
        if let Some((_, started)) = &self.carousel_transition {
            if started.elapsed() >= RECENTS_TRANSITION
                || appearance::reduced_motion()
                || self.screen != StartScreen::Recents
                || self.design().recents != crate::design::RecentPresentation::Carousel
                || self.settings_open
            {
                self.carousel_transition = None;
            } else if !self.render_power.paused() {
                window.request_animation_frame();
            }
        }
        let sidebar = self.visible_sidebar_progress();
        let recents_backdrop = self.screen == StartScreen::Recents
            && self.design().recents == crate::design::RecentPresentation::Carousel
            && !self.settings_open
            && self.snapshot.ready
            && self.selected_game().is_some();
        let content = if self.settings_open {
            self.settings(window, cx)
        } else if self.art_scene_active() {
            self.art_scene(window, cx)
        } else if self.cinematic_active() {
            self.cinematic(window, cx)
        } else if self.gallery_active() {
            self.gallery(window, cx)
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
            .id("launcher-root")
            .role(Role::Group)
            .aria_label("Startpoint")
            .key_context("Startpoint")
            .track_focus(&self.focus)
            .when(crate::automation::active(cx), |root| {
                root.capture_action(|_: &gpui_component::input::Copy, _, cx| {
                    if !crate::automation::native_allowed(cx, "clipboard copy") {
                        cx.stop_propagation();
                    }
                })
                .capture_action(|_: &gpui_component::input::Cut, _, cx| {
                    if !crate::automation::native_allowed(cx, "clipboard cut") {
                        cx.stop_propagation();
                    }
                })
                .capture_action(|_: &gpui_component::input::Paste, _, cx| {
                    if !crate::automation::native_allowed(cx, "clipboard paste") {
                        cx.stop_propagation();
                    }
                })
                .capture_action(
                    |_: &gpui_component::input::ShowCharacterPalette, _, cx| {
                        if !crate::automation::native_allowed(cx, "character palette") {
                            cx.stop_propagation();
                        }
                    },
                )
            })
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

fn artwork_option(
    id: ElementId,
    preview: Option<PathBuf>,
    author: &str,
    kind: ArtworkKind,
    current: bool,
    cx: &App,
) -> Button {
    let radius = Rounding::of(cx).artwork;
    let (width, height) = match kind {
        ArtworkKind::Cover => (148., 210.),
        ArtworkKind::Background => (252., 142.),
        ArtworkKind::Logo => (210., 132.),
        ArtworkKind::Icon => (116., 116.),
    };
    let group = SharedString::from(format!("artwork-option-{id:?}"));
    with_accessible_label(
        button(id, cx),
        format!(
            "{} {} by {author}",
            if current { "Current" } else { "Use" },
            kind.label().to_lowercase()
        ),
    )
    .ghost()
    .border_0()
    .p_0()
    .w(px(width))
    .h(px(height + 36.))
    .group(group.clone())
    .tooltip(format!(
        "{} {} by {author}",
        if current { "Current" } else { "Use" },
        kind.label().to_lowercase()
    ))
    .child(
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(height))
                    .flex_shrink_0()
                    .rounded(radius)
                    .overflow_hidden()
                    .bg(cx.theme().secondary.alpha(0.4))
                    .when(!current, |figure| {
                        figure.group_hover(group, |style| style.opacity(0.88))
                    })
                    .child(match preview {
                        Some(path) => {
                            rounded_image(path, ObjectFit::Contain, radius, cx).into_any_element()
                        }
                        None => {
                            rounded_placeholder("Preview unavailable", cx.theme().colors, radius)
                                .into_any_element()
                        }
                    }),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .px_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("artwork-author")
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .truncate()
                            .role(Role::Label)
                            .aria_label(format!("{} by {author}", kind.label()))
                            .child(author.to_string()),
                    )
                    .when(current, |caption| {
                        caption.child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    Icon::new(IconName::Check)
                                        .small()
                                        .text_color(cx.theme().primary),
                                )
                                .child("Current"),
                        )
                    }),
            ),
    )
}

fn page_has_game_context(screen: StartScreen, settings_open: bool, tab: DetailTab) -> bool {
    !settings_open
        && (matches!(screen, StartScreen::Library | StartScreen::Recents) || tab.is_editor())
}

/// Suspend active selection on non-game pages, retaining it only as navigation
/// memory. Refresh events must not reselect a game behind Settings/Activity.
fn reconcile_page_selection(
    screen: StartScreen,
    settings_open: bool,
    tab: DetailTab,
    selected: &mut Option<GameId>,
    remembered: &mut Option<GameId>,
) -> bool {
    if !page_has_game_context(screen, settings_open, tab) {
        if let Some(id) = selected.take() {
            *remembered = Some(id);
        }
        return false;
    }
    if selected.is_none() {
        *selected = remembered.take();
    }
    true
}

fn command_applies_to_game(command: &LauncherCommand, selected: Option<&Game>) -> bool {
    match command {
        LauncherCommand::LaunchGame(id) => selected
            .is_some_and(|game| game.id == *id && (game.available || game.has_launch_override())),
        LauncherCommand::Favourite(id, _) | LauncherCommand::Hidden(id, _) => {
            selected.is_some_and(|game| game.id == *id)
        }
        LauncherCommand::Details(_) | LauncherCommand::Reidentify => selected.is_some(),
        _ => true,
    }
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
            !game.available && !game.has_launch_override(),
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
    track_launcher_menu(owner.clone(), cx);
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

fn track_launcher_menu(owner: WeakEntity<LauncherView>, cx: &mut Context<PopupMenu>) {
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
}

/// Recents always shows the five most recently launched visible installed games,
/// regardless of the Library's saved filter, source, collection or sort order.
fn installed_recents(games: &[Game], limit: usize) -> Vec<&Game> {
    let mut recent = games
        .iter()
        .filter(|game| game.available && !game.hidden)
        .collect::<Vec<_>>();
    recent.sort_by_key(|game| std::cmp::Reverse(game.last_launched.unwrap_or(0)));
    recent.truncate(limit);
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
fn non_game_pages_suspend_selection_and_refresh_cannot_restore_it() {
    for (screen, settings) in [
        (StartScreen::Library, true),
        (StartScreen::Recents, true),
        (StartScreen::Activity, true),
        (StartScreen::Activity, false),
    ] {
        let id = GameId("steam:42".into());
        let mut selected = Some(id.clone());
        let mut remembered = None;
        for _ in 0..2 {
            assert!(!reconcile_page_selection(
                screen,
                settings,
                DetailTab::Overview,
                &mut selected,
                &mut remembered
            ));
            assert!(selected.is_none());
            assert_eq!(remembered.as_ref(), Some(&id));
        }
        assert!(reconcile_page_selection(
            StartScreen::Library,
            false,
            DetailTab::Overview,
            &mut selected,
            &mut remembered
        ));
        assert_eq!(selected, Some(id));
        assert!(remembered.is_none());
    }
}

#[cfg(test)]
#[::core::prelude::v1::test]
fn game_context_requires_a_game_page_or_an_explicit_editor_and_never_settings() {
    for tab in
        std::iter::once(DetailTab::Overview).chain(DetailTab::EDITORS.iter().map(|(tab, _)| *tab))
    {
        for screen in [
            StartScreen::Library,
            StartScreen::Recents,
            StartScreen::Activity,
        ] {
            assert!(!page_has_game_context(screen, true, tab));
            assert_eq!(
                page_has_game_context(screen, false, tab),
                screen != StartScreen::Activity || tab.is_editor()
            );
        }
    }
}

#[cfg(test)]
#[::core::prelude::v1::test]
fn game_commands_reject_missing_or_changed_selection_but_global_commands_still_work() {
    let game = sample_library_game();
    let mut other = game.clone();
    other.id = GameId("steam:other".into());
    for command in [
        LauncherCommand::LaunchGame(game.id.clone()),
        LauncherCommand::Favourite(game.id.clone(), true),
        LauncherCommand::Hidden(game.id.clone(), true),
        LauncherCommand::Details(DetailTab::Metadata),
        LauncherCommand::Details(DetailTab::Artwork),
        LauncherCommand::Details(DetailTab::Launch),
        LauncherCommand::Details(DetailTab::Journal),
        LauncherCommand::Reidentify,
    ] {
        assert!(!command_applies_to_game(&command, None));
        assert!(command_applies_to_game(&command, Some(&game)));
        if matches!(
            command,
            LauncherCommand::LaunchGame(_)
                | LauncherCommand::Favourite(_, _)
                | LauncherCommand::Hidden(_, _)
        ) {
            assert!(!command_applies_to_game(&command, Some(&other)));
        }
    }
    for command in [
        LauncherCommand::SelectGame(game.id),
        LauncherCommand::Settings,
        LauncherCommand::Screen(StartScreen::Library),
        LauncherCommand::Refresh,
        LauncherCommand::RefreshMetadata,
        LauncherCommand::AddGame,
    ] {
        assert!(command_applies_to_game(&command, None));
    }
}

#[cfg(test)]
#[::core::prelude::v1::test]
fn steam_launch_routes_select_the_right_editor_mode_and_enable_game_actions() {
    let mut game = sample_library_game();
    game.available = false;
    assert_eq!(LaunchMode::for_game(&game), LaunchMode::Default);
    assert!(!command_applies_to_game(
        &LauncherCommand::LaunchGame(game.id.clone()),
        Some(&game)
    ));
    game.steam_launch = Some(SteamLaunch::Shortcut {
        account_id: 42,
        app_id: 0x9123_4567,
        name: "Shortcut".into(),
    });
    assert_eq!(LaunchMode::for_game(&game), LaunchMode::SteamShortcut);
    assert!(command_applies_to_game(
        &LauncherCommand::LaunchGame(game.id.clone()),
        Some(&game)
    ));
    game.steam_launch = Some(SteamLaunch::Url("steam://rungameid/123".into()));
    assert_eq!(LaunchMode::for_game(&game), LaunchMode::SteamUrl);
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
        installed_recents(&games, 5)
            .into_iter()
            .map(|game| &game.id.0)
            .collect::<Vec<_>>(),
        vec!["steam:6", "steam:5", "steam:4", "steam:3", "steam:2"]
    );
    assert!(installed_recents(&[], 6).is_empty());
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
    game.last_launched = Some(0);
    assert!(last_played(&game).is_empty());
    assert_eq!(library_subtitle(&game, true, true), "Steam");
    game.last_launched = Some(1);
    assert!(last_played(&game).contains("1970"));
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

fn single_line_title(
    id: SharedString,
    title: &str,
    font_size: Pixels,
    weight: FontWeight,
) -> impl IntoElement {
    let title = SharedString::from(title.to_string());
    let tooltip = title.clone();
    div()
        .id(id)
        .w_full()
        .min_w_0()
        .h((font_size * 1.2).ceil())
        .line_height(relative(1.2))
        .text_size(font_size)
        .font_weight(weight)
        .truncate()
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(title)
}

fn source_name(provider_id: &str) -> &str {
    match provider_id {
        "steam" => "Steam",
        "epic" => "Epic Games",
        "gog" => "GOG",
        "xbox" => "Xbox",
        "manual" => "Manually added",
        "example" => "Example library",
        other => other,
    }
}

fn last_played(game: &Game) -> String {
    let Some(timestamp) = game.last_launched.filter(|timestamp| *timestamp > 0) else {
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
        days if days < 28 => format!("Last launched {} weeks ago", days / 7),
        _ => i64::try_from(timestamp)
            .ok()
            .and_then(|timestamp| Local.timestamp_opt(timestamp, 0).single())
            .map(|date| format!("Last launched {}", date.format("%b %-d, %Y")))
            .unwrap_or_default(),
    }
}
