use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{
    editor::{EditorDraft, EditorSection, ManualDraft},
    theme::{self, ThemeDocument, ThemeDraft},
};
use rune::{Image, ImageCache, PointerPress, TextEdit, Tx, Waker};
use rune_kit::Kit;
use startpoint_sdk::*;

pub const PAGE_SIZE: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Recents,
    Library,
    Activity,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    Journal,
    None,
    Editor,
    AddGame,
    Customize,
    Theme,
    Commands,
    Notifications,
    Confirm,
    WallFocus,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LibraryFilter {
    All,
    Favourites,
    Manual,
    Hidden,
    Completed,
    Unfinished,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Title,
    Recent,
    Source,
}

#[derive(Clone, PartialEq, Eq)]
struct NavigationLocation {
    screen: Screen,
    filter: LibraryFilter,
    source: Option<String>,
    collection: Option<String>,
    query: String,
    page: usize,
    selected: Option<GameId>,
    sort: SortOrder,
    sort_reverse: bool,
    activity_date: Option<String>,
    detail_closed: bool,
}

#[derive(Clone)]
pub struct Confirmation {
    pub title: String,
    pub message: String,
    pub command: Command,
    pub return_to: Overlay,
}

pub enum HostEvent {
    Result(Result<String, String>),
    Themes(Vec<ThemeDocument>),
    ThemeSaved(Result<(ThemeDocument, Option<String>), String>),
    ThemeImported(Result<ThemeDocument, String>),
}

pub struct State {
    pub api: Application,
    pub snapshot: Snapshot,
    pub appearance: AppearanceSettings,
    pub appearance_dirty: bool,
    pub screen: Screen,
    pub overlay: Overlay,
    pub selected: Option<GameId>,
    pub search: TextEdit,
    pub command_query: TextEdit,
    pub filter: LibraryFilter,
    pub source: Option<String>,
    pub collection: Option<String>,
    pub sort: SortOrder,
    pub sort_reverse: bool,
    pub table: rune_kit::TableState,
    pub library_order: Vec<GameId>,
    pub page: usize,
    pub status: String,
    pub error: Option<String>,
    pub example: bool,
    pub headless: bool,
    pub images: BTreeMap<PathBuf, Image>,
    image_changed: Arc<AtomicBool>,
    image_window: (usize, usize),
    pub sidebar_fold: crate::motion::PanelFold,
    pub overview_fold: crate::motion::PanelFold,
    pub library_start: Arc<AtomicUsize>,
    pub window_width: f32,
    pub dismissed_notifications: Vec<JobId>,
    pub command_index: usize,
    pub command_pointer: bool,
    pub presented_background: Option<PathBuf>,
    pub previous_background: Option<PathBuf>,
    pub presented_detail: Option<GameId>,
    pub wall_focus: Option<GameId>,
    pub wall_hover: Option<GameId>,
    pub wall_start: Arc<AtomicUsize>,
    pub editor: EditorDraft,
    pub manual: ManualDraft,
    pub open_select: Option<String>,
    pub context: Option<(GameId, PointerPress)>,
    pub confirmation: Option<Confirmation>,
    pub credential: TextEdit,
    pub themes: Vec<ThemeDocument>,
    pub theme_draft: ThemeDraft,
    pub system_dark: bool,
    pub reduced_motion: bool,
    pub window_focused: bool,
    credential_job: Option<JobId>,
    artwork_refresh_pending: bool,
    pub activity_date: Option<String>,
    pub detail_closed: bool,
    pub host_sender: async_channel::Sender<HostEvent>,
    pub wake: Option<Waker>,
    events: Option<async_channel::Receiver<AppEvent>>,
    pending_events: Arc<Mutex<Vec<AppEvent>>>,
    host_events: async_channel::Receiver<HostEvent>,
    changed: Arc<AtomicBool>,
    image_cache: Option<ImageCache>,
    pub last_job: Option<JobId>,
    appearance_job: Option<JobId>,
    initialized: bool,
    previous_query: String,
    navigation_back: Vec<NavigationLocation>,
    navigation_forward: Vec<NavigationLocation>,
    _temporary_directory: Option<tempfile::TempDir>,
}

impl State {
    pub fn new() -> Self {
        Self::try_new().unwrap_or_else(|error| {
            eprintln!("Startpoint Rune could not start: {error}");
            std::process::exit(1);
        })
    }

    fn try_new() -> Result<Self, Box<dyn std::error::Error>> {
        let headless = rune::tooling::headless_run().is_some();
        let example = headless
            || rune::tooling::extra_args()
                .iter()
                .any(|arg| arg == "--example-library");
        let temporary_directory = if headless {
            Some(tempfile::tempdir()?)
        } else {
            None
        };
        let mut paths = startpoint_core::application_paths()?;
        if let Some(directory) = &temporary_directory {
            paths.data_directory = directory.path().join("data");
            paths.cache_directory = directory.path().join("cache");
        } else {
            let name = if example {
                "rune-preview/examples"
            } else {
                "rune-preview"
            };
            paths.data_directory = paths.data_directory.join(name);
            paths.cache_directory = paths.cache_directory.join(name);
        }
        let api = if example {
            let builder = startpoint_core::LauncherBuilder::new()
                .discovery_provider(startpoint_core::ExampleProvider);
            let builder = if headless {
                builder
                    .without_system_credentials()
                    .without_automatic_metadata()
            } else {
                builder.artwork_provider(startpoint_core::SteamGridDbProvider::new()?)
            };
            builder.build(paths)?
        } else {
            startpoint_core::create_application_at(paths)?
        };
        let events = api.subscribe();
        let job = api.submit(Command::RefreshLibrary)?;
        if headless {
            wait_for_job(&api, job)?;
        }
        let snapshot = api.snapshot();
        let appearance = snapshot.settings.appearance.clone();
        let themes = theme::builtins();
        let document = themes.first().cloned().unwrap_or_default();
        let (host_sender, host_events) = async_channel::unbounded();
        let mut state = Self {
            api,
            snapshot,
            appearance,
            appearance_dirty: false,
            screen: Screen::Recents,
            overlay: Overlay::None,
            selected: None,
            search: TextEdit::new(""),
            command_query: TextEdit::new(""),
            filter: LibraryFilter::All,
            source: None,
            collection: None,
            sort: SortOrder::Title,
            sort_reverse: false,
            table: rune_kit::TableState::new().key("library-table"),
            library_order: Vec::new(),
            page: 0,
            status: String::new(),
            error: None,
            example,
            headless,
            images: BTreeMap::new(),
            image_changed: Arc::new(AtomicBool::new(false)),
            image_window: (0, 0),
            sidebar_fold: crate::motion::PanelFold::new(false),
            overview_fold: crate::motion::PanelFold::new(true),
            library_start: Arc::new(AtomicUsize::new(0)),
            window_width: 1280.0,
            dismissed_notifications: Vec::new(),
            command_index: 0,
            command_pointer: false,
            presented_background: None,
            previous_background: None,
            presented_detail: None,
            wall_focus: None,
            wall_hover: None,
            wall_start: Arc::new(AtomicUsize::new(0)),
            editor: EditorDraft::default(),
            manual: ManualDraft::default(),
            open_select: None,
            context: None,
            confirmation: None,
            credential: TextEdit::new(""),
            themes,
            theme_draft: ThemeDraft::new(document, true),
            system_dark: true,
            reduced_motion: false,
            window_focused: true,
            credential_job: None,
            artwork_refresh_pending: false,
            activity_date: None,
            detail_closed: false,
            host_sender,
            wake: None,
            events: Some(events),
            pending_events: Arc::new(Mutex::new(Vec::new())),
            host_events,
            changed: Arc::new(AtomicBool::new(true)),
            image_cache: None,
            last_job: Some(job),
            appearance_job: None,
            initialized: false,
            previous_query: String::new(),
            navigation_back: Vec::new(),
            navigation_forward: Vec::new(),
            _temporary_directory: temporary_directory,
        };
        state.credential.set_secure(true);
        state.apply_start_screen();
        state.reconcile_selection();
        state.refresh_library_order();
        Ok(state)
    }

    pub fn setup(state: &mut Self, wake: Waker) {
        state.wake = Some(wake.clone());
        let image_changed = state.image_changed.clone();
        let image_wake = wake.clone();
        state.image_cache = Some(ImageCache::new(Arc::new(move || {
            image_changed.store(true, Ordering::Release);
            image_wake();
        })));
        if let Some(events) = state.events.take() {
            let changed = state.changed.clone();
            let pending = state.pending_events.clone();
            let event_wake = wake.clone();
            std::thread::Builder::new()
                .name("startpoint-rune-events".into())
                .spawn(move || {
                    while let Ok(event) = events.recv_blocking() {
                        pending
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .push(event);
                        changed.store(true, Ordering::Release);
                        event_wake();
                    }
                })
                .unwrap_or_else(|error| {
                    eprintln!("Could not start Rune event delivery: {error}");
                    std::process::exit(1);
                });
        }
        if !state.headless {
            let directory = state.snapshot.paths.data_directory.join("themes");
            let sender = state.host_sender.clone();
            state.spawn("startpoint-rune-themes", move || {
                let mut themes = Vec::new();
                if let Ok(entries) = std::fs::read_dir(directory) {
                    for entry in entries.flatten().take(128) {
                        let path = entry.path();
                        if path
                            .extension()
                            .is_some_and(|extension| extension == "json")
                            && let Ok(metadata) = std::fs::metadata(&path)
                            && metadata.len() <= 262_144
                            && let Ok(source) = std::fs::read_to_string(path)
                            && let Ok(document) = serde_json::from_str::<ThemeDocument>(&source)
                            && document.validate().is_ok()
                        {
                            themes.push(document);
                        }
                    }
                }
                let _ = sender.try_send(HostEvent::Themes(themes));
            });
        }
        state.update_images();
    }

    pub fn poll(state: &mut Self, now: f64, tx: &mut Tx) -> bool {
        let changed = state.changed.swap(false, Ordering::AcqRel);
        let events = std::mem::take(
            &mut *state
                .pending_events
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        let mut dirty = changed || !events.is_empty();
        if changed || !events.is_empty() {
            state.snapshot = state.api.snapshot();
            state.apply_start_screen();
            if !state.appearance_dirty
                && state.appearance_job.is_none_or(|id| !state.job_active(id))
            {
                state.appearance = state.snapshot.settings.appearance.clone();
                state.normalize_recents();
                state.appearance_job = None;
            }
            if let Some(job) = state.snapshot.jobs.iter().find(|job| {
                Some(job.id) == state.last_job && !state.dismissed_notifications.contains(&job.id)
            }) {
                state.status = job.error.clone().unwrap_or_else(|| job.label.clone());
                if let Some(error) = &job.error {
                    state.error = Some(error.clone());
                }
            }
        }
        for event in events {
            state.handle_event(event);
        }
        while let Ok(event) = state.host_events.try_recv() {
            dirty = true;
            match event {
                HostEvent::Result(Ok(message)) => state.status = message,
                HostEvent::Result(Err(error)) => state.error = Some(error),
                HostEvent::Themes(themes) => {
                    for theme in themes {
                        state.insert_theme(theme);
                    }
                }
                HostEvent::ThemeSaved(Ok((theme, original))) => {
                    if let Some(original) = original
                        .filter(|original| !theme::is_builtin(original) && original != &theme.name)
                    {
                        state.themes.retain(|document| document.name != original);
                    }
                    state.appearance.theme = theme.name.clone();
                    state.insert_theme(theme);
                    state.save_appearance();
                    state.overlay = Overlay::Customize;
                }
                HostEvent::ThemeSaved(Err(error)) => state.error = Some(error),
                HostEvent::ThemeImported(Ok(document)) => state.store_theme(document, None),
                HostEvent::ThemeImported(Err(error)) => state.error = Some(error),
            }
        }
        if state.search.text != state.previous_query {
            state.previous_query = state.search.text.clone();
            state.page = 0;
            dirty = true;
        }
        let image_dirty = state.image_changed.swap(false, Ordering::AcqRel);
        let image_window = (
            state.library_start.load(Ordering::Relaxed),
            state.wall_start.load(Ordering::Relaxed),
        );
        if dirty {
            dirty |= state.reconcile_selection();
        }
        if dirty || image_dirty || image_window != state.image_window {
            state.update_images();
            state.image_window = image_window;
            dirty = true;
        }
        if state.artwork_refresh_pending && !state.headless && !state.busy() {
            state.artwork_refresh_pending = false;
            if state.artwork_connected() {
                state.submit(Command::RefreshMetadata);
                dirty = true;
            }
        }
        dirty |= state.sidebar_fold.update(
            state.appearance.sidebar_visible,
            now,
            state.reduced_motion,
            tx,
        );
        dirty |= state.overview_fold.update(
            state.appearance.library_details && !state.detail_closed,
            now,
            state.reduced_motion,
            tx,
        );
        dirty
    }

    fn apply_start_screen(&mut self) {
        if self.snapshot.ready && !self.initialized {
            self.appearance = self.snapshot.settings.appearance.clone();
            self.normalize_recents();
            self.screen = match self.appearance.start_screen {
                StartScreen::Recents => Screen::Recents,
                StartScreen::Library => Screen::Library,
                StartScreen::Activity => Screen::Activity,
            };
            self.initialized = true;
            self.sidebar_fold = crate::motion::PanelFold::new(self.appearance.sidebar_visible);
            self.overview_fold = crate::motion::PanelFold::new(
                self.appearance.library_details && !self.detail_closed,
            );
        }
    }

    fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::JobChanged(job)
                if self.editor.task_job == Some(job.id) && !job.state.active() =>
            {
                self.editor.task_job = None;
                if job.state == JobState::Completed {
                    self.editor.task = TextEdit::new("");
                    self.editor.task_edit = None;
                }
            }
            AppEvent::JobChanged(job)
                if self.credential_job == Some(job.id) && !job.state.active() =>
            {
                self.credential_job = None;
                self.artwork_refresh_pending =
                    job.state == JobState::Completed && self.artwork_connected();
            }
            AppEvent::ArtworkCacheChanged => {
                self.images.retain(|_, image| image.error().is_none());
            }
            AppEvent::JobChanged(job)
                if self.editor.journal_job == Some(job.id) && !job.state.active() =>
            {
                self.editor.journal_job = None;
                if job.state == JobState::Completed {
                    self.editor.journal_entry = None;
                    self.editor.journal = TextEdit::new("");
                    self.editor.journal_preview = false;
                }
            }
            AppEvent::JobChanged(job)
                if self.editor.reload_job == Some(job.id) && !job.state.active() =>
            {
                self.editor.reload_job = None;
                if job.state == JobState::Completed
                    && let Some(id) = self.editor.game_id.clone()
                {
                    self.open_editor(id, self.editor.section);
                }
            }
            AppEvent::ArtworkGamesFound {
                job_id,
                game_id,
                games,
            } if self.editor.game_id.as_ref() == Some(&game_id)
                && self.editor.artwork.search_job == Some(job_id) =>
            {
                self.editor.artwork.matches = games;
                self.editor.artwork.search_job = None;
            }
            AppEvent::ArtworkFound {
                job_id,
                game_id,
                kind,
                page,
                has_more,
                images,
            } if self.editor.game_id.as_ref() == Some(&game_id)
                && self.editor.artwork.page_job == Some(job_id)
                && self.editor.artwork.kind == kind
                && self.editor.artwork.page == page =>
            {
                self.editor.artwork.candidates = images;
                self.editor.artwork.has_more = has_more;
                self.editor.artwork.page_job = None;
            }
            AppEvent::SteamShortcutUrlRead {
                job_id,
                game_id,
                uri,
            } if self.editor.game_id.as_ref() == Some(&game_id)
                && self.editor.import_job == Some(job_id) =>
            {
                self.editor.steam_url = TextEdit::new(uri);
                self.editor.import_job = None;
            }
            AppEvent::GameAdded { job_id, game_id } if self.manual.pending_job == Some(job_id) => {
                self.manual.pending_job = None;
                self.open_editor(game_id, EditorSection::Details);
            }
            _ => {}
        }
    }

    pub fn visible_games(&self) -> Vec<&Game> {
        let query = self.search.text.trim().to_lowercase();
        let mut games: Vec<_> = self
            .snapshot
            .games
            .iter()
            .filter(|game| {
                let matches = query.is_empty() || game.title().to_lowercase().contains(&query);
                if self.screen == Screen::Recents {
                    return game.available && !game.hidden;
                }
                let filter = match self.filter {
                    LibraryFilter::All => !game.hidden,
                    LibraryFilter::Favourites => game.favourite && !game.hidden,
                    LibraryFilter::Manual => game.provider_id == "manual" && !game.hidden,
                    LibraryFilter::Hidden => game.hidden,
                    LibraryFilter::Completed => game.completed && !game.hidden,
                    LibraryFilter::Unfinished => !game.completed && !game.hidden,
                };
                game.available
                    && filter
                    && matches
                    && self
                        .source
                        .as_ref()
                        .is_none_or(|source| &game.provider_id == source)
                    && self
                        .collection
                        .as_ref()
                        .is_none_or(|collection| game.collections.contains(collection))
            })
            .collect();
        if self.screen == Screen::Recents || self.sort == SortOrder::Recent {
            games.sort_by(|a, b| {
                b.last_launched
                    .cmp(&a.last_launched)
                    .then_with(|| a.title().cmp(b.title()))
            });
            if self.screen == Screen::Recents
                && matches!(
                    self.appearance.recents_layout,
                    RecentsLayout::Carousel | RecentsLayout::Gallery
                )
            {
                games.truncate(5);
            }
        } else if self.sort == SortOrder::Source {
            games.sort_by(|a, b| {
                a.provider_id
                    .cmp(&b.provider_id)
                    .then_with(|| a.title().cmp(b.title()))
            });
        } else {
            games.sort_by_key(|game| game.title().to_lowercase());
        }
        if self.sort_reverse && self.screen != Screen::Recents {
            games.reverse();
        }
        games
    }

    pub fn game(&self, id: &GameId) -> Option<&Game> {
        self.snapshot.games.iter().find(|game| &game.id == id)
    }

    pub fn selected_game(&self) -> Option<&Game> {
        if !matches!(self.screen, Screen::Library | Screen::Recents) {
            return None;
        }
        self.visible_games()
            .into_iter()
            .find(|game| Some(&game.id) == self.selected.as_ref())
    }

    pub fn select(&mut self, id: GameId) {
        self.table.select(id.0.clone());
        self.selected = Some(id);
        self.detail_closed = false;
        self.update_images();
    }

    pub fn refresh_library_order(&mut self) {
        self.library_order = self
            .visible_games()
            .into_iter()
            .map(|game| game.id.clone())
            .collect();
        let column = match self.sort {
            SortOrder::Title => "title",
            SortOrder::Recent => "date",
            SortOrder::Source => "source",
        };
        let descending = if self.sort == SortOrder::Recent {
            !self.sort_reverse
        } else {
            self.sort_reverse
        };
        self.table.set_sort(
            column,
            if descending {
                rune_kit::SortDir::Descending
            } else {
                rune_kit::SortDir::Ascending
            },
        );
    }

    fn reconcile_selection(&mut self) -> bool {
        let games = self.visible_games();
        let selected = if self
            .selected
            .as_ref()
            .is_some_and(|id| games.iter().any(|game| &game.id == id))
        {
            self.selected.clone()
        } else {
            games.first().map(|game| game.id.clone())
        };
        let last_page = games.len().saturating_sub(1) / PAGE_SIZE;
        let changed = self.selected != selected || self.page > last_page;
        self.selected = selected;
        if changed && let Some(id) = &self.selected {
            self.table.select(id.0.clone());
        }
        self.page = self.page.min(last_page);
        self.refresh_library_order();
        changed
    }

    pub fn navigate(&mut self, screen: Screen) {
        let previous = self.navigation_location();
        self.reset_navigation(screen);
        self.reconcile_selection();
        self.record_navigation(previous);
    }

    fn reset_navigation(&mut self, screen: Screen) {
        self.screen = screen;
        self.page = 0;
        self.search = TextEdit::new("");
        self.open_select = None;
        self.context = None;
        self.activity_date = None;
    }

    pub fn set_filter(&mut self, filter: LibraryFilter) {
        let previous = self.navigation_location();
        self.reset_navigation(Screen::Library);
        self.filter = filter;
        self.source = None;
        self.collection = None;
        self.reconcile_selection();
        self.record_navigation(previous);
    }

    pub fn set_source(&mut self, source: String) {
        let previous = self.navigation_location();
        self.reset_navigation(Screen::Library);
        self.filter = LibraryFilter::All;
        self.source = Some(source);
        self.collection = None;
        self.reconcile_selection();
        self.record_navigation(previous);
    }

    pub fn set_collection(&mut self, collection: String) {
        let previous = self.navigation_location();
        self.reset_navigation(Screen::Library);
        self.filter = LibraryFilter::All;
        self.source = None;
        self.collection = Some(collection);
        self.reconcile_selection();
        self.record_navigation(previous);
    }

    pub fn open_activity_date(&mut self, date: String) {
        let previous = self.navigation_location();
        self.reset_navigation(Screen::Activity);
        self.activity_date = Some(date);
        self.record_navigation(previous);
    }

    pub fn can_go_back(&self) -> bool {
        !self.navigation_back.is_empty()
    }
    pub fn can_go_forward(&self) -> bool {
        !self.navigation_forward.is_empty()
    }

    pub fn go_back(&mut self, tx: &mut Tx) {
        if let Some(previous) = self.navigation_back.pop() {
            self.navigation_forward.push(self.navigation_location());
            self.restore_navigation(previous, tx);
        }
    }

    pub fn go_forward(&mut self, tx: &mut Tx) {
        if let Some(next) = self.navigation_forward.pop() {
            self.navigation_back.push(self.navigation_location());
            self.restore_navigation(next, tx);
        }
    }

    fn navigation_location(&self) -> NavigationLocation {
        NavigationLocation {
            screen: self.screen,
            filter: self.filter,
            source: self.source.clone(),
            collection: self.collection.clone(),
            query: self.search.text.clone(),
            page: self.page,
            selected: self.selected.clone(),
            sort: self.sort,
            sort_reverse: self.sort_reverse,
            activity_date: self.activity_date.clone(),
            detail_closed: self.detail_closed,
        }
    }

    fn record_navigation(&mut self, previous: NavigationLocation) {
        self.update_images();
        if previous == self.navigation_location() {
            return;
        }
        if self.navigation_back.last() != Some(&previous) {
            self.navigation_back.push(previous);
            if self.navigation_back.len() > 100 {
                self.navigation_back.remove(0);
            }
        }
        self.navigation_forward.clear();
    }

    fn restore_navigation(&mut self, location: NavigationLocation, tx: &mut Tx) {
        self.close_overlay();
        self.screen = location.screen;
        self.filter = location.filter;
        self.source = location.source;
        self.collection = location.collection;
        self.search = TextEdit::new(location.query);
        self.previous_query = self.search.text.clone();
        self.page = location.page;
        self.selected = location.selected;
        self.sort = location.sort;
        self.sort_reverse = location.sort_reverse;
        self.activity_date = location.activity_date;
        self.detail_closed = location.detail_closed;
        self.reconcile_selection();
        self.refresh_library_order();
        self.update_images();
        if let Some(id) = &self.selected {
            self.table.select(id.0.clone());
            self.reveal_game(id, tx);
        }
    }

    pub fn submit(&mut self, command: Command) -> Option<JobId> {
        self.error = None;
        if self.example
            && let Command::LaunchGame { game_id } = &command
            && self
                .game(game_id)
                .is_none_or(|game| game.provider_id != "example")
        {
            self.error =
                Some("Example sessions only simulate launches of example-provider games.".into());
            return None;
        }
        match self.api.submit(command) {
            Ok(job) => {
                self.last_job = Some(job);
                if self.headless
                    && let Err(error) = wait_for_job(&self.api, job)
                {
                    self.error = Some(error);
                }
                self.snapshot = self.api.snapshot();
                self.changed.store(true, Ordering::Release);
                Some(job)
            }
            Err(error) => {
                self.error = Some(error.to_string());
                None
            }
        }
    }

    pub fn job_active(&self, id: JobId) -> bool {
        self.snapshot
            .jobs
            .iter()
            .any(|job| job.id == id && job.state.active())
    }
    pub fn busy(&self) -> bool {
        self.snapshot.jobs.iter().any(|job| job.state.active())
    }

    pub fn launch(&mut self, id: GameId) {
        if self
            .game(&id)
            .is_some_and(|game| game.available && !game.hidden)
        {
            self.submit(Command::LaunchGame { game_id: id });
        }
    }

    pub fn launch_selected(&mut self) {
        if let Some(game) = self.selected_game() {
            self.launch(game.id.clone());
        }
    }

    pub fn toggle_favourite(&mut self) {
        if let Some(game) = self.selected_game() {
            self.submit(Command::SetFavourite {
                game_id: game.id.clone(),
                favourite: !game.favourite,
            });
        }
    }

    pub fn save_appearance(&mut self) {
        self.appearance_job = self.submit(Command::SetAppearance {
            settings: self.appearance.clone(),
        });
        self.appearance_dirty = false;
    }

    pub fn artwork_connected(&self) -> bool {
        self.snapshot.settings.credential_status.get("steamgriddb")
            == Some(&CredentialStatus::Verified)
    }

    pub fn connect_artwork(&mut self) {
        let mut empty = TextEdit::new("");
        empty.set_secure(true);
        let mut credential = std::mem::replace(&mut self.credential, empty);
        let secret = std::mem::take(&mut credential.text);
        self.credential_job = self.submit(Command::SetArtworkCredential {
            provider_id: "steamgriddb".into(),
            credential: Some(secret),
        });
    }

    pub fn verify_artwork(&mut self) {
        self.credential_job = self.submit(Command::VerifyArtworkCredential {
            provider_id: "steamgriddb".into(),
        });
    }

    pub fn fetch_artwork(&mut self) {
        if self.headless {
            self.error = Some("Online artwork is disabled in isolated headless sessions.".into());
            return;
        }
        if self.artwork_connected() {
            self.artwork_refresh_pending = true;
        } else if self
            .snapshot
            .settings
            .configured_credentials
            .contains("steamgriddb")
        {
            self.verify_artwork();
        } else {
            self.close_overlay();
            self.navigate(Screen::Settings);
        }
    }

    pub fn sidebar_open(&self) -> bool {
        self.appearance.sidebar_visible
    }

    pub fn open_commands(&mut self, query: String, pointer: bool) {
        self.search = TextEdit::new("");
        self.command_query = TextEdit::new(query);
        self.command_query.focused = true;
        self.command_index = 0;
        self.command_pointer = pointer;
        self.show_overlay(Overlay::Commands);
        self.update_images();
    }

    pub fn close_overlay(&mut self) {
        self.search.focused = false;
        self.command_query.focused = false;
        self.overlay = Overlay::None;
        self.open_select = None;
        self.error = None;
    }
    pub fn dismiss_overlay(&mut self) {
        if self.overlay == Overlay::Confirm {
            let return_to = self
                .confirmation
                .take()
                .map_or(Overlay::None, |confirmation| confirmation.return_to);
            self.show_overlay(return_to);
        } else {
            self.close_overlay();
        }
    }
    pub fn show_overlay(&mut self, overlay: Overlay) {
        self.wall_hover = None;
        self.overlay = overlay;
        self.open_select = None;
        self.context = None;
        self.error = None;
        self.update_images();
    }

    pub fn preview_style(&mut self, style: RecentsLayout) {
        self.screen = Screen::Recents;
        self.appearance.recents_layout = match style {
            RecentsLayout::Gallery
            | RecentsLayout::Cinematic
            | RecentsLayout::FilmReel
            | RecentsLayout::Mosaic => RecentsLayout::Carousel,
            other => other,
        };
        self.wall_focus = None;
        self.appearance_dirty = true;
        self.reconcile_selection();
        self.update_images();
    }

    fn normalize_recents(&mut self) {
        if self.appearance.recents_layout != RecentsLayout::InfiniteWall {
            self.appearance.recents_layout = RecentsLayout::Carousel;
        }
    }

    pub fn confirm(&mut self, title: &str, message: &str, command: Command) {
        self.confirmation = Some(Confirmation {
            title: title.into(),
            message: message.into(),
            command,
            return_to: self.overlay,
        });
        self.show_overlay(Overlay::Confirm);
    }

    pub fn accept_confirmation(&mut self) {
        if let Some(confirmation) = self.confirmation.clone() {
            let reidentify = matches!(confirmation.command, Command::ReidentifyGame { .. });
            let remove_game = matches!(confirmation.command, Command::RemoveManualGame { .. });
            let clear_journal = match &confirmation.command {
                Command::DeleteJournalEntry { entry_id, .. } => {
                    self.editor.journal_entry.as_ref() == Some(entry_id)
                }
                _ => false,
            };
            let job = self.submit(confirmation.command);
            if self.error.is_none() && job.is_some() {
                self.confirmation = None;
                self.overlay = if remove_game {
                    Overlay::None
                } else {
                    confirmation.return_to
                };
                if reidentify {
                    self.editor.reload_job = job;
                }
                if clear_journal {
                    self.editor.journal_entry = None;
                    self.editor.journal = TextEdit::new("");
                }
            }
        }
    }

    pub fn move_selection(&mut self, forward: bool, tx: &mut Tx) -> bool {
        if self.overlay != Overlay::None
            || !matches!(self.screen, Screen::Library | Screen::Recents)
        {
            return false;
        }
        let games = self.visible_games();
        if games.is_empty() {
            return false;
        }
        let current = games
            .iter()
            .position(|game| Some(&game.id) == self.selected.as_ref())
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % games.len()
        } else {
            (current + games.len() - 1) % games.len()
        };
        let id = games[next].id.clone();
        self.select(id.clone());
        self.reveal_game(&id, tx);
        true
    }

    pub fn reveal_game(&self, id: &GameId, tx: &mut Tx) {
        if self.screen == Screen::Library
            && self.appearance.library_layout != LibraryLayout::Compact
        {
            let games = self.visible_games();
            let (columns, _, _) = crate::library::grid_geometry(self, self.window_width);
            if let Some(index) = games.iter().position(|game| &game.id == id) {
                tx.scroll_into_view(format!(
                    "library-row-{}",
                    games[index - index % columns].id.0
                ));
            }
        } else {
            tx.scroll_into_view(format!("game-{}", id.0));
        }
    }

    pub fn kit(&self, system_dark: bool) -> Kit {
        let mut appearance = self.appearance.clone();
        if self.screen == Screen::Recents {
            appearance.mode = AppearanceMode::Dark;
        }
        if self.overlay == Overlay::Theme
            && let Ok(document) = self.theme_draft.resolve()
        {
            return document.kit(&appearance, system_dark);
        }
        self.themes
            .iter()
            .find(|theme| theme.name == self.appearance.theme)
            .or_else(|| self.themes.first())
            .map_or_else(
                || {
                    Kit::neutral(match appearance.mode {
                        AppearanceMode::System => system_dark,
                        AppearanceMode::Light => false,
                        AppearanceMode::Dark => true,
                    })
                },
                |theme| theme.kit(&appearance, system_dark),
            )
    }

    fn insert_theme(&mut self, document: ThemeDocument) {
        if theme::is_builtin(&document.name) {
            return;
        }
        if let Some(theme) = self
            .themes
            .iter_mut()
            .find(|theme| theme.name.eq_ignore_ascii_case(&document.name))
        {
            *theme = document;
        } else {
            self.themes.push(document);
        }
    }

    pub fn store_theme(&mut self, document: ThemeDocument, original: Option<String>) {
        if theme::is_builtin(&document.name) {
            self.error = Some("Choose a different name to save a copy of a built-in theme.".into());
            return;
        }
        let directory = self.snapshot.paths.data_directory.join("themes");
        let sender = self.host_sender.clone();
        self.spawn("startpoint-rune-save-theme", move || {
            let result = (|| {
                document.validate()?;
                std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
                let path = directory.join(theme::file_name(&document.name));
                let temporary = directory.join(format!("pending-{}.json", uuid::Uuid::new_v4()));
                let source =
                    serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
                std::fs::write(&temporary, source).map_err(|error| error.to_string())?;
                std::fs::rename(&temporary, &path).map_err(|error| error.to_string())?;
                if let Some(original) = original.as_ref().filter(|original| {
                    !theme::is_builtin(original) && !original.eq_ignore_ascii_case(&document.name)
                }) {
                    let previous = directory.join(theme::file_name(original));
                    match std::fs::remove_file(previous) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(format!(
                                "Theme saved, but the previous name could not be removed: {error}"
                            ));
                        }
                    }
                }
                Ok((document, original))
            })();
            let _ = sender.try_send(HostEvent::ThemeSaved(result));
        });
    }

    pub fn spawn(&mut self, name: &str, operation: impl FnOnce() + Send + 'static) {
        let wake = self.wake.clone();
        if let Err(error) = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                operation();
                if let Some(wake) = wake {
                    wake();
                }
            })
        {
            self.error = Some(error.to_string());
        }
    }

    pub fn open_path(&mut self, path: PathBuf) {
        if self.headless {
            self.error = Some("Opening external applications is disabled headlessly.".into());
            return;
        }
        let sender = self.host_sender.clone();
        self.spawn("startpoint-rune-open-path", move || {
            let result = open::that(path)
                .map(|_| "Opened folder".into())
                .map_err(|error| error.to_string());
            let _ = sender.try_send(HostEvent::Result(result));
        });
    }

    pub fn open_key_page(&mut self) {
        if self.headless {
            self.error = Some("Opening external applications is disabled headlessly.".into());
            return;
        }
        let sender = self.host_sender.clone();
        self.spawn("startpoint-rune-open-website", move || {
            let result = open::that("https://www.steamgriddb.com/profile/preferences/api")
                .map(|_| "Opened SteamGridDB".into())
                .map_err(|error| error.to_string());
            let _ = sender.try_send(HostEvent::Result(result));
        });
    }

    pub(crate) fn update_images(&mut self) {
        let Some(cache) = &self.image_cache else {
            return;
        };
        let games = self.visible_games();
        let mut paths: Vec<_> = games
            .iter()
            .skip(if self.screen == Screen::Library {
                self.library_start
                    .load(Ordering::Relaxed)
                    .saturating_sub(48)
            } else {
                0
            })
            .take(if self.screen == Screen::Library {
                192
            } else {
                PAGE_SIZE
            })
            .flat_map(|game| {
                [
                    ArtworkKind::Cover,
                    ArtworkKind::Background,
                    ArtworkKind::Logo,
                ]
                .into_iter()
                .filter_map(|kind| game.artwork.get(&kind))
                .map(|asset| asset.thumbnail_path.clone())
            })
            .collect();
        if matches!(self.screen, Screen::Recents | Screen::Library) {
            if self.appearance.recents_layout == RecentsLayout::InfiniteWall {
                paths.extend(
                    games
                        .iter()
                        .skip(self.wall_start.load(Ordering::Relaxed).saturating_sub(48))
                        .take(192)
                        .filter_map(|game| game.artwork.get(&ArtworkKind::Cover))
                        .map(|asset| asset.thumbnail_path.clone()),
                );
            }
            let selected = games
                .iter()
                .position(|game| Some(&game.id) == self.selected.as_ref())
                .unwrap_or(0);
            for game in games
                .iter()
                .enumerate()
                .filter(|(index, _)| index.abs_diff(selected) <= 2)
                .map(|(_, game)| game)
            {
                paths.extend(
                    [ArtworkKind::Background, ArtworkKind::Logo]
                        .into_iter()
                        .filter_map(|kind| game.artwork.get(&kind))
                        .map(|asset| asset.image_path.clone()),
                );
            }
        }
        paths.extend(self.presented_background.iter().cloned());
        paths.extend(self.previous_background.iter().cloned());
        if self.overlay == Overlay::Commands {
            let query = self.command_query.text.trim().to_lowercase();
            paths.extend(
                self.snapshot
                    .games
                    .iter()
                    .filter(|game| {
                        game.available
                            && !game.hidden
                            && (query.is_empty() || game.title().to_lowercase().contains(&query))
                    })
                    .take(20)
                    .filter_map(|game| game.artwork.get(&ArtworkKind::Cover))
                    .map(|asset| asset.thumbnail_path.clone()),
            );
        }
        if let Some(game) = self.presented_detail.as_ref().and_then(|id| self.game(id)) {
            paths.extend(game.artwork.values().map(|asset| asset.image_path.clone()));
        }
        if let Some(game) = self.wall_focus.as_ref().and_then(|id| self.game(id)) {
            paths.extend(
                game.artwork
                    .get(&ArtworkKind::Cover)
                    .map(|asset| asset.image_path.clone()),
            );
        }
        if let Some(game) = self.selected_game() {
            paths.extend(game.artwork.values().map(|asset| asset.image_path.clone()));
        }
        if self.overlay == Overlay::Editor
            && let Some(game) = self.editor.game_id.as_ref().and_then(|id| self.game(id))
        {
            paths.extend(game.artwork.values().map(|asset| asset.image_path.clone()));
            paths.extend(
                crate::artwork::saved_assets(self)
                    .into_iter()
                    .skip(self.editor.artwork.saved_page * 24)
                    .take(24)
                    .map(|asset| asset.thumbnail_path.clone()),
            );
            paths.extend(
                self.editor
                    .artwork
                    .candidates
                    .iter()
                    .filter_map(|candidate| candidate.preview_path.clone()),
            );
        }
        paths.sort();
        paths.dedup();
        self.images
            .retain(|path, _| paths.binary_search(path).is_ok());
        for path in paths {
            let image = self
                .images
                .entry(path.clone())
                .or_insert_with(|| cache.file(path, 640.0, 2.0));
            if self.headless {
                let _ = image.wait();
            }
        }
        if let Some(game) = self.selected_game()
            && [ArtworkKind::Background, ArtworkKind::Logo]
                .into_iter()
                .filter_map(|kind| game.artwork.get(&kind))
                .all(|asset| {
                    self.images
                        .get(&asset.image_path)
                        .is_some_and(|image| image.is_ready() || image.error().is_some())
                })
        {
            self.presented_detail = Some(game.id.clone());
        }
        if let Some(path) = self
            .selected_game()
            .and_then(|game| game.artwork.get(&ArtworkKind::Background))
            .map(|asset| asset.image_path.clone())
            && self
                .images
                .get(&path)
                .is_some_and(|image| image.is_ready() && image.error().is_none())
            && self.presented_background.as_ref() != Some(&path)
        {
            self.previous_background = self.presented_background.replace(path);
        }
    }
}

fn wait_for_job(api: &Application, id: JobId) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let snapshot = api.snapshot();
        if let Some(error) = snapshot.initialization_error {
            return Err(error);
        }
        if let Some(job) = snapshot
            .jobs
            .iter()
            .find(|job| job.id == id && !job.state.active())
        {
            return job.error.clone().map_or(Ok(()), Err);
        }
        if Instant::now() >= deadline {
            return Err("Library operation timed out".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
