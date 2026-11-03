use crate::{artwork, catalogue, platform, steam, steam_shortcuts, storage::Store};
use async_channel::{Receiver, Sender};
use startpoint_sdk::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub struct LauncherBuilder {
    discovery: Vec<Arc<dyn DiscoveryProvider>>,
    artwork: Vec<Arc<dyn ArtworkProvider>>,
}

impl LauncherBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn discovery_provider(mut self, provider: impl DiscoveryProvider + 'static) -> Self {
        self.discovery.push(Arc::new(provider));
        self
    }

    pub fn artwork_provider(mut self, provider: impl ArtworkProvider + 'static) -> Self {
        self.artwork.push(Arc::new(provider));
        self
    }

    /// Opens the cached library on a worker before accepting any queued operations.
    pub fn build(self, paths: ApplicationPaths) -> Result<Application, ApiError> {
        let providers: Vec<_> = self
            .discovery
            .iter()
            .map(|provider| provider.descriptor())
            .chain(self.artwork.iter().map(|provider| provider.descriptor()))
            .collect();
        let mut ids = BTreeSet::new();
        for provider in &providers {
            if provider.id.is_empty()
                || provider.id.len() > 80
                || !provider.id.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
                })
                || !ids.insert(provider.id.clone())
            {
                return Err(ApiError::Validation(
                    "Provider identifiers must be unique, short ASCII names".into(),
                ));
            }
        }
        let snapshot = Snapshot {
            ready: false,
            games: Arc::new(Vec::new()),
            activity: Arc::new(Vec::new()),
            journal: Arc::new(Vec::new()),
            jobs: Vec::new(),
            settings: SettingsSnapshot::default(),
            paths,
            initialization_error: None,
            providers,
        };
        let shared = Arc::new(Shared {
            image_client: reqwest::blocking::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(artwork::image_error)?,
            state: Mutex::new(State {
                snapshot,
                games: BTreeMap::new(),
            }),
            store: Mutex::new(None),
            subscribers: Mutex::new(Vec::new()),
            cancellations: Mutex::new(BTreeMap::new()),
            credentials: Mutex::new(BTreeMap::new()),
            discovery: self.discovery,
            artwork: self.artwork,
            refreshing: AtomicBool::new(false),
        });
        let (interactive, interactive_receiver) = async_channel::bounded(64);
        let (background, background_receiver) = async_channel::bounded(64);
        let initialize = shared.clone();
        std::thread::Builder::new()
            .name("startpoint-initialization".into())
            .spawn(move || {
                if let Err(error) = initialize.initialize() {
                    initialize
                        .state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .snapshot
                        .initialization_error = Some(error.to_string());
                    initialize.publish(AppEvent::LibraryChanged);
                    for job in initialize.snapshot().jobs {
                        if job.state.active() {
                            initialize.job(
                                job.id,
                                job.label,
                                JobState::Failed,
                                Some(error.to_string()),
                            );
                        }
                    }
                    return;
                }
                for (name, receiver) in [
                    ("startpoint-interactive", interactive_receiver),
                    ("startpoint-artwork-1", background_receiver.clone()),
                    ("startpoint-artwork-2", background_receiver),
                ] {
                    let worker = initialize.clone();
                    if let Err(error) =
                        std::thread::Builder::new()
                            .name(name.into())
                            .spawn(move || {
                                while let Ok(work) = receiver.recv_blocking() {
                                    worker.run(work);
                                }
                            })
                    {
                        initialize
                            .state
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .snapshot
                            .initialization_error = Some(error.to_string());
                        initialize.publish(AppEvent::LibraryChanged);
                    }
                }
            })
            .map_err(|error| ApiError::Io(error.to_string()))?;
        Ok(Arc::new(Launcher {
            shared,
            interactive,
            background,
            next_job: AtomicU64::new(1),
        }))
    }
}

struct State {
    snapshot: Snapshot,
    games: BTreeMap<GameId, Game>,
}
struct Work {
    id: JobId,
    command: Command,
    context: JobContext,
}
struct Launcher {
    shared: Arc<Shared>,
    interactive: Sender<Work>,
    background: Sender<Work>,
    next_job: AtomicU64,
}

struct Shared {
    image_client: reqwest::blocking::Client,
    state: Mutex<State>,
    store: Mutex<Option<Store>>,
    subscribers: Mutex<Vec<Sender<AppEvent>>>,
    cancellations: Mutex<BTreeMap<JobId, Arc<AtomicBool>>>,
    credentials: Mutex<BTreeMap<String, Option<String>>>,
    discovery: Vec<Arc<dyn DiscoveryProvider>>,
    artwork: Vec<Arc<dyn ArtworkProvider>>,
    refreshing: AtomicBool,
}

impl ApplicationApi for Launcher {
    fn snapshot(&self) -> Snapshot {
        self.shared.snapshot()
    }

    fn subscribe(&self) -> Receiver<AppEvent> {
        // ponytail: trusted in-process subscribers must drain events; use bounded IPC when hosting external plugins.
        let (sender, receiver) = async_channel::unbounded();
        self.shared
            .subscribers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(sender);
        receiver
    }

    fn submit(&self, command: Command) -> Result<JobId, ApiError> {
        if self.snapshot().initialization_error.is_some() {
            return Err(ApiError::Storage(
                "The saved library could not be opened".into(),
            ));
        }
        let id = self.next_job.fetch_add(1, Ordering::Relaxed);
        let cancelled = Arc::new(AtomicBool::new(false));
        self.shared
            .cancellations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id, cancelled.clone());
        self.shared
            .job(id, command.label().into(), JobState::Queued, None);
        let shared = self.shared.clone();
        let context = JobContext::new(cancelled, move |label| {
            shared.job(id, label, JobState::Running, None)
        });
        let sender = if command.background() {
            &self.background
        } else {
            &self.interactive
        };
        if sender
            .try_send(Work {
                id,
                command,
                context,
            })
            .is_err()
        {
            self.shared.job(
                id,
                "Operation not queued".into(),
                JobState::Failed,
                Some(ApiError::Busy.to_string()),
            );
            self.shared
                .cancellations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&id);
            return Err(ApiError::Busy);
        }
        Ok(id)
    }

    fn cancel(&self, job_id: JobId) {
        if let Some(cancellation) = self
            .shared
            .cancellations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&job_id)
        {
            cancellation.store(true, Ordering::Relaxed);
        }
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        for cancellation in self
            .shared
            .cancellations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
        {
            cancellation.store(true, Ordering::Relaxed);
        }
    }
}

impl Shared {
    fn snapshot(&self) -> Snapshot {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot
            .clone()
    }

    fn publish(&self, event: AppEvent) {
        self.subscribers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|subscriber| subscriber.try_send(event.clone()).is_ok());
    }

    fn store<T>(
        &self,
        operation: impl FnOnce(&mut Store) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        let mut guard = self
            .store
            .lock()
            .map_err(|_| ApiError::Storage("The database worker failed".into()))?;
        operation(
            guard
                .as_mut()
                .ok_or_else(|| ApiError::Storage("The library is not ready".into()))?,
        )
    }

    fn initialize(&self) -> Result<(), ApiError> {
        let paths = self.snapshot().paths;
        let store = Store::open(&paths)?;
        let games: BTreeMap<_, _> = store
            .games()?
            .into_iter()
            .map(|game| (game.id.clone(), game))
            .collect();
        let library = sorted_library(games.values().cloned().collect());
        let activity = store.activity()?;
        let journal = store.journal()?;
        let steam_directory = store.setting("steam_directory")?.flatten();
        let epic_directory = store.setting("epic_directory")?.flatten();
        let gog_directory = store.setting("gog_directory")?.flatten();
        let configured_credentials: BTreeSet<String> =
            store.setting("configured_credentials")?.unwrap_or_default();
        let credential_status = self
            .artwork
            .iter()
            .filter(|provider| provider.requires_credential())
            .map(|provider| {
                let id = provider.descriptor().id;
                let status = if configured_credentials.contains(&id) {
                    CredentialStatus::Stored
                } else {
                    CredentialStatus::Missing
                };
                (id, status)
            })
            .collect();
        let (mut appearance, migrated) = restore_appearance_settings(store.setting("appearance")?)?;
        if !appearance.sidebar_width.is_finite() {
            appearance.sidebar_width = AppearanceSettings::default().sidebar_width;
        }
        appearance.sidebar_width = appearance.sidebar_width.clamp(176., 400.);
        if migrated {
            store.save_setting("appearance", &appearance)?;
        }
        *self
            .store
            .lock()
            .map_err(|_| ApiError::Storage("The database worker failed".into()))? = Some(store);
        let mut settings = SettingsSnapshot {
            steam_directory,
            epic_directory,
            gog_directory,
            configured_credentials,
            credential_status,
            appearance,
            discovery_directories: BTreeMap::new(),
            ..SettingsSnapshot::default()
        };
        // Resolve paths outside the state lock: Steam location may read platform settings.
        settings.discovery_directories = crate::discovery_paths::resolve(&settings, &library);
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.games = games;
        state.snapshot.settings = settings;
        state.snapshot.ready = true;
        state.snapshot.activity = Arc::new(activity);
        state.snapshot.journal = Arc::new(journal);
        state.snapshot.games = library;
        drop(state);
        self.publish(AppEvent::LibraryChanged);
        let mut settings = self.snapshot().settings;
        for provider in self
            .artwork
            .iter()
            .filter(|provider| provider.requires_credential())
        {
            let id = provider.descriptor().id;
            let status = match platform::read_api_key(&id, false) {
                Ok(credential) => {
                    let credential = credential.filter(|key| !key.trim().is_empty());
                    let status = if credential.is_some() {
                        settings.configured_credentials.insert(id.clone());
                        CredentialStatus::Stored
                    } else {
                        settings.configured_credentials.remove(&id);
                        CredentialStatus::Missing
                    };
                    self.credentials
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .insert(id.clone(), credential);
                    status
                }
                Err(ApiError::CredentialLocked) => CredentialStatus::Locked,
                Err(error) => CredentialStatus::Unavailable(error.to_string()),
            };
            settings.credential_status.insert(id, status);
        }
        self.store(|store| {
            store.save_setting("configured_credentials", &settings.configured_credentials)
        })?;
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot
            .settings = settings;
        self.publish(AppEvent::SettingsChanged);
        Ok(())
    }

    fn job(&self, id: JobId, label: String, job_state: JobState, error: Option<String>) {
        let job = JobStatus {
            id,
            label,
            state: job_state,
            error,
        };
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let jobs = &mut state.snapshot.jobs;
        if let Some(previous) = jobs.iter_mut().find(|previous| previous.id == id) {
            *previous = job.clone();
        } else {
            jobs.push(job.clone());
        }
        while jobs.iter().filter(|job| !job.state.active()).count() > 16 {
            if let Some(index) = jobs.iter().position(|job| !job.state.active()) {
                jobs.remove(index);
            }
        }
        drop(state);
        self.publish(AppEvent::JobChanged(job));
    }

    fn run(&self, work: Work) {
        self.job(
            work.id,
            work.command.label().into(),
            JobState::Running,
            None,
        );
        let result = catch_unwind(AssertUnwindSafe(|| {
            work.context.checkpoint()?;
            self.execute(work.id, work.command, &work.context)
        }))
        .unwrap_or_else(|_| {
            Err(ApiError::Provider(
                "The background operation failed unexpectedly; saved data has been retained".into(),
            ))
        });
        let label = self
            .snapshot()
            .jobs
            .iter()
            .find(|job| job.id == work.id)
            .map(|job| job.label.clone())
            .unwrap_or_default();
        match result {
            Ok(()) => self.job(work.id, label, JobState::Completed, None),
            Err(ApiError::Cancelled) => self.job(work.id, label, JobState::Cancelled, None),
            Err(error) => self.job(work.id, label, JobState::Failed, Some(error.to_string())),
        }
        self.cancellations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&work.id);
    }

    fn game(&self, id: &GameId) -> Result<Game, ApiError> {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .games
            .get(id)
            .cloned()
            .ok_or(ApiError::NotFound)
    }

    fn update_game(
        &self,
        id: &GameId,
        update: impl FnOnce(&mut Game) -> Result<(), ApiError>,
    ) -> Result<(), ApiError> {
        let changed = self.store(|store| {
            let previous = self.snapshot().games;
            let original = previous
                .iter()
                .find(|game| &game.id == id)
                .ok_or(ApiError::NotFound)?;
            let mut game = original.clone();
            update(&mut game)?;
            if game == *original {
                return Ok(false);
            }
            store.save_games(std::slice::from_ref(&game))?;
            let library = updated_library(&previous, &game);
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            let replaced = state.games.insert(id.clone(), game);
            state.snapshot.games = library;
            drop(state);
            drop(replaced);
            Ok(true)
        })?;
        if changed {
            self.publish(AppEvent::LibraryChanged);
        }
        Ok(())
    }

    fn artwork_provider(&self, id: &str) -> Result<&Arc<dyn ArtworkProvider>, ApiError> {
        self.artwork
            .iter()
            .find(|provider| provider.descriptor().id == id)
            .ok_or_else(|| ApiError::Validation("The artwork provider is not registered".into()))
    }

    fn credential(&self, provider_id: &str, allow_interaction: bool) -> Result<String, ApiError> {
        let provider = self.artwork_provider(provider_id)?;
        let mut credentials = self
            .credentials
            .lock()
            .map_err(|_| ApiError::Provider("The credential worker failed".into()))?;
        if !credentials.contains_key(provider_id) {
            credentials.insert(
                provider_id.into(),
                platform::read_api_key(provider_id, allow_interaction)?,
            );
        }
        let credential = credentials
            .get(provider_id)
            .cloned()
            .flatten()
            .unwrap_or_default();
        if credential.is_empty() && provider.requires_credential() {
            return Err(ApiError::Validation(
                "Add your artwork provider API key in Settings first".into(),
            ));
        }
        Ok(credential)
    }

    fn set_credential_status(&self, provider_id: &str, credential: &str, status: CredentialStatus) {
        let credentials = self
            .credentials
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if credentials
            .get(provider_id)
            .and_then(|value| value.as_deref())
            .unwrap_or_default()
            != credential
        {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.snapshot.settings.credential_status.get(provider_id) == Some(&status) {
            return;
        }
        state
            .snapshot
            .settings
            .credential_status
            .insert(provider_id.into(), status);
        drop(state);
        drop(credentials);
        self.publish(AppEvent::SettingsChanged);
    }

    fn provider_result<T>(
        &self,
        provider_id: &str,
        credential: &str,
        result: Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        match &result {
            Ok(_) => {
                self.set_credential_status(provider_id, credential, CredentialStatus::Verified)
            }
            Err(ApiError::InvalidCredential(_)) => {
                self.set_credential_status(provider_id, credential, CredentialStatus::Invalid)
            }
            _ => {}
        }
        result
    }

    fn verify_credential(
        &self,
        provider_id: &str,
        context: &JobContext,
        allow_interaction: bool,
    ) -> Result<(), ApiError> {
        let credential = match self.credential(provider_id, allow_interaction) {
            Ok(credential) => credential,
            Err(error) => {
                let status = match &error {
                    ApiError::CredentialLocked => CredentialStatus::Locked,
                    ApiError::Validation(_) => CredentialStatus::Missing,
                    _ => CredentialStatus::Unavailable(error.to_string()),
                };
                self.set_credential_status(provider_id, "", status);
                return Err(error);
            }
        };
        self.set_credential_status(provider_id, &credential, CredentialStatus::Checking);
        context.report("Checking the saved SteamGridDB API key");
        let result = self
            .artwork_provider(provider_id)?
            .validate_credential(&credential, context);
        let status = match &result {
            Ok(()) => CredentialStatus::Verified,
            Err(ApiError::InvalidCredential(_)) => CredentialStatus::Invalid,
            Err(ApiError::Cancelled) => CredentialStatus::Stored,
            Err(error) => CredentialStatus::Unavailable(error.to_string()),
        };
        self.set_credential_status(provider_id, &credential, status);
        result
    }

    fn execute(
        &self,
        job_id: JobId,
        command: Command,
        context: &JobContext,
    ) -> Result<(), ApiError> {
        match command {
            Command::RefreshLibrary => self.refresh(context),
            Command::RefreshSteamShortcuts => self.refresh_steam_shortcuts(context),
            Command::ReadSteamShortcutUrl { game_id, path } => {
                self.game(&game_id)?;
                let uri = steam_shortcuts::read_url(&path)?;
                context.checkpoint()?;
                self.publish(AppEvent::SteamShortcutUrlRead {
                    job_id,
                    game_id,
                    uri,
                });
                Ok(())
            }
            Command::AddManualGame { title, launch } => {
                let title = title.trim().to_string();
                if title.is_empty() || title.len() > 500 {
                    return Err(ApiError::Validation(
                        "A game title must contain 1–500 bytes".into(),
                    ));
                }
                platform::validate_launch(&launch)?;
                let source_game_id = uuid::Uuid::new_v4().to_string();
                let id = GameId(format!("manual:{source_game_id}"));
                let install_directory = launch
                    .executable
                    .parent()
                    .unwrap_or(&launch.executable)
                    .to_path_buf();
                let default_launch = if launch
                    .executable
                    .extension()
                    .is_some_and(|extension| extension == "app")
                {
                    LaunchTarget::ApplicationBundle(launch)
                } else {
                    LaunchTarget::Executable(launch)
                };
                let game = Game {
                    id: id.clone(),
                    provider_id: "manual".into(),
                    source_game_id,
                    discovered_title: title,
                    install_directory,
                    default_launch,
                    launch_override: None,
                    steam_launch: None,
                    metadata: UserMetadata::default(),
                    catalogue_metadata: UserMetadata::default(),
                    favourite: false,
                    hidden: false,
                    available: true,
                    last_launched: None,
                    collections: BTreeSet::new(),
                    completed: false,
                    artwork: BTreeMap::new(),
                    artwork_history: Vec::new(),
                    artwork_game_ids: BTreeMap::new(),
                };
                self.store(|store| {
                    store.save_games(std::slice::from_ref(&game))?;
                    let previous = self.snapshot().games;
                    let library = updated_library(&previous, &game);
                    let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                    state.games.insert(id.clone(), game);
                    state.snapshot.games = library;
                    Ok(())
                })?;
                self.publish(AppEvent::LibraryChanged);
                self.publish(AppEvent::GameAdded {
                    job_id,
                    game_id: id,
                });
                Ok(())
            }
            Command::RemoveManualGame { game_id } => {
                self.store(|store| {
                    if self.game(&game_id)?.provider_id != "manual" {
                        return Err(ApiError::Validation(
                            "Only manually added games can be removed".into(),
                        ));
                    }
                    store.remove_game(&game_id.0)?;
                    let previous = self.snapshot().games;
                    let mut games = previous.as_ref().clone();
                    games.retain(|game| game.id != game_id);
                    let journal = Arc::new(store.journal()?);
                    let activity = Arc::new(store.activity()?);
                    let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                    let removed = state.games.remove(&game_id);
                    state.snapshot.games = Arc::new(games);
                    state.snapshot.journal = journal;
                    state.snapshot.activity = activity;
                    drop(state);
                    drop(removed);
                    Ok(())
                })?;
                self.publish(AppEvent::LibraryChanged);
                Ok(())
            }
            Command::UpdateMetadata {
                game_id,
                mut metadata,
            } => {
                for value in [
                    &mut metadata.title,
                    &mut metadata.description,
                    &mut metadata.developer,
                    &mut metadata.genres,
                ] {
                    *value = value
                        .take()
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty());
                }
                if metadata
                    .title
                    .as_ref()
                    .is_some_and(|value| value.len() > 500)
                    || [&metadata.description, &metadata.developer, &metadata.genres]
                        .into_iter()
                        .flatten()
                        .any(|value| value.len() > 10_000)
                {
                    return Err(ApiError::Validation("Metadata is too long".into()));
                }
                self.update_game(&game_id, |game| {
                    game.metadata = metadata;
                    Ok(())
                })
            }
            Command::SetFavourite { game_id, favourite } => self.update_game(&game_id, |game| {
                game.favourite = favourite;
                Ok(())
            }),
            Command::SetHidden { game_id, hidden } => self.update_game(&game_id, |game| {
                game.hidden = hidden;
                Ok(())
            }),
            Command::ReidentifyGame { game_id, matched } => {
                if matched.id.parse::<u64>().is_err()
                    || matched.name.trim().is_empty()
                    || matched.name.len() > 500
                    || matched.name.chars().any(char::is_control)
                {
                    return Err(ApiError::Validation(
                        "Choose a valid SteamGridDB game match".into(),
                    ));
                }
                self.update_game(&game_id, |game| {
                    apply_reidentification(game, &matched);
                    Ok(())
                })?;
                // The selected identity is saved even if subsequent artwork downloads fail.
                if self.artwork_provider("steamgriddb").is_ok()
                    && self
                        .snapshot()
                        .settings
                        .configured_credentials
                        .contains("steamgriddb")
                {
                    self.populate_game(&game_id, context)?;
                }
                Ok(())
            }
            Command::SetGameProgress {
                game_id,
                collections,
                completed,
            } => {
                if collections.len() > 32
                    || collections.iter().any(|name| {
                        name.trim().is_empty()
                            || name.len() > 80
                            || name.chars().any(char::is_control)
                    })
                {
                    return Err(ApiError::Validation(
                        "Use at most 32 collection names of 1–80 bytes without control characters"
                            .into(),
                    ));
                }
                self.update_game(&game_id, |game| {
                    game.collections = collections
                        .into_iter()
                        .map(|name| name.trim().to_string())
                        .collect();
                    game.completed = completed;
                    Ok(())
                })
            }
            Command::SaveJournalEntry {
                game_id,
                entry_id,
                text,
            } => {
                self.game(&game_id)?;
                let text = text.trim().to_string();
                if text.is_empty() || text.len() > 10_000 || text.contains('\0') {
                    return Err(ApiError::Validation(
                        "Journal entries must contain 1–10,000 bytes without NUL characters".into(),
                    ));
                }
                self.store(|store| {
                    let existing = entry_id.is_some();
                    let entry = JournalEntry {
                        id: entry_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                        game_id,
                        created_at: now(),
                        text,
                    };
                    store.save_journal_entry(&entry, existing)?;
                    let journal = store.journal()?;
                    self.state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .snapshot
                        .journal = Arc::new(journal);
                    Ok(())
                })?;
                self.publish(AppEvent::LibraryChanged);
                Ok(())
            }
            Command::DeleteJournalEntry { game_id, entry_id } => {
                self.store(|store| {
                    store.delete_journal_entry(&game_id, &entry_id)?;
                    let journal = store.journal()?;
                    self.state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .snapshot
                        .journal = Arc::new(journal);
                    Ok(())
                })?;
                self.publish(AppEvent::LibraryChanged);
                Ok(())
            }
            Command::SetLaunchOverride { game_id, launch } => {
                if let Some(launch) = &launch {
                    platform::validate_launch(launch)?;
                }
                self.update_game(&game_id, |game| {
                    game.launch_override = launch;
                    game.steam_launch = None;
                    Ok(())
                })
            }
            Command::SetSteamLaunch { game_id, launch } => {
                self.game(&game_id)?;
                let launch = steam_shortcuts::validate(
                    launch,
                    self.snapshot().settings.steam_directory.as_deref(),
                    context,
                    false,
                )?;
                context.checkpoint()?;
                self.update_game(&game_id, |game| {
                    game.steam_launch = Some(launch);
                    game.launch_override = None;
                    Ok(())
                })
            }
            Command::LaunchGame { game_id } => {
                let game = self.game(&game_id)?;
                if game.provider_id == "example" {
                    context.report(format!(
                        "Example launch · {} · no program started",
                        game.title()
                    ));
                    return self.record_launch(&game_id);
                }
                if !game.available && !game.has_launch_override() {
                    return Err(ApiError::Validation(
                        "This installation is currently unavailable".into(),
                    ));
                }
                let mut schemes = self
                    .discovery
                    .iter()
                    .find(|provider| provider.descriptor().id == game.provider_id)
                    .map(|provider| provider.uri_schemes())
                    .unwrap_or_default();
                let target = if let Some(launch) = game.steam_launch.clone() {
                    let launch = steam_shortcuts::validate(
                        launch,
                        self.snapshot().settings.steam_directory.as_deref(),
                        context,
                        true,
                    )?;
                    schemes = vec!["steam".into()];
                    LaunchTarget::Uri(launch.uri())
                } else {
                    game.effective_launch()
                };
                platform::launch(target, &schemes)?;
                self.record_launch(&game_id)
            }
            Command::SetArtworkCredential {
                provider_id,
                credential,
            } => {
                self.artwork_provider(&provider_id)?;
                let credential = credential
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
                if credential
                    .as_ref()
                    .is_some_and(|value| value.len() > 1024 || value.chars().any(char::is_control))
                {
                    return Err(ApiError::Validation(
                        "The API key contains invalid characters".into(),
                    ));
                }
                platform::write_api_key(&provider_id, credential.as_deref())?;
                self.credentials
                    .lock()
                    .map_err(|_| ApiError::Provider("The credential worker failed".into()))?
                    .insert(provider_id.clone(), credential.clone());
                self.store(|store| {
                    let mut settings = self.snapshot().settings;
                    if credential.is_some() {
                        settings.configured_credentials.insert(provider_id.clone());
                    } else {
                        settings.configured_credentials.remove(&provider_id);
                    }
                    store
                        .save_setting("configured_credentials", &settings.configured_credentials)?;
                    self.state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .snapshot
                        .settings = settings;
                    Ok(())
                })?;
                self.set_credential_status(
                    &provider_id,
                    credential.as_deref().unwrap_or_default(),
                    if credential.is_some() {
                        CredentialStatus::Stored
                    } else {
                        CredentialStatus::Missing
                    },
                );
                self.publish(AppEvent::SettingsChanged);
                if credential.is_some() {
                    self.verify_credential(&provider_id, context, false)
                } else {
                    Ok(())
                }
            }
            Command::VerifyArtworkCredential { provider_id } => {
                self.verify_credential(&provider_id, context, true)
            }
            Command::SetArtworkGame {
                provider_id,
                game_id,
                remote_game_id,
            } => {
                self.artwork_provider(&provider_id)?;
                if remote_game_id.is_empty()
                    || remote_game_id.len() > 200
                    || remote_game_id.chars().any(char::is_control)
                    || (provider_id == "steamgriddb" && remote_game_id.parse::<u64>().is_err())
                {
                    return Err(ApiError::Validation(
                        "Invalid artwork game identifier".into(),
                    ));
                }
                self.update_game(&game_id, |game| {
                    game.artwork_game_ids.insert(provider_id, remote_game_id);
                    Ok(())
                })
            }
            Command::SetSteamDirectory { directory } => {
                if directory.as_ref().is_some_and(|path| {
                    !path.is_absolute() || !path.join("steamapps/libraryfolders.vdf").is_file()
                }) {
                    return Err(ApiError::Validation("Choose the Steam installation directory containing steamapps/libraryfolders.vdf".into()));
                }
                self.store(|store| {
                    store.save_setting("steam_directory", &directory)?;
                    self.state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .snapshot
                        .settings
                        .steam_directory = directory;
                    Ok(())
                })?;
                self.refresh_discovery_directories();
                self.refresh_steam_shortcuts(context)?;
                self.publish(AppEvent::SettingsChanged);
                Ok(())
            }
            Command::SetEpicDirectory { directory } => {
                if directory
                    .as_ref()
                    .is_some_and(|path| !path.is_absolute() || !path.is_dir())
                {
                    return Err(ApiError::Validation(
                        "Choose an existing Epic manifest directory".into(),
                    ));
                }
                self.store(|store| {
                    store.save_setting("epic_directory", &directory)?;
                    self.state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .snapshot
                        .settings
                        .epic_directory = directory;
                    Ok(())
                })?;
                self.refresh_discovery_directories();
                self.publish(AppEvent::SettingsChanged);
                Ok(())
            }
            Command::SetGogDirectory { directory } => {
                if directory
                    .as_ref()
                    .is_some_and(|path| !path.is_absolute() || !path.is_dir())
                {
                    return Err(ApiError::Validation(
                        "Choose an existing GOG games directory".into(),
                    ));
                }
                self.store(|store| {
                    store.save_setting("gog_directory", &directory)?;
                    self.state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .snapshot
                        .settings
                        .gog_directory = directory;
                    Ok(())
                })?;
                self.refresh_discovery_directories();
                self.publish(AppEvent::SettingsChanged);
                Ok(())
            }
            Command::SetAppearance { settings } => {
                if settings.theme.trim().is_empty()
                    || settings.theme.len() > 80
                    || settings.theme.chars().any(char::is_control)
                {
                    return Err(ApiError::Validation(
                        "Theme names must contain 1–80 bytes without control characters".into(),
                    ));
                }
                if !settings.sidebar_width.is_finite()
                    || !(176. ..=400.).contains(&settings.sidebar_width)
                {
                    return Err(ApiError::Validation(
                        "Sidebar width must be between 176 and 400 pixels".into(),
                    ));
                }
                if !settings.translucency_amount.is_finite()
                    || !(0. ..=1.).contains(&settings.translucency_amount)
                {
                    return Err(ApiError::Validation(
                        "Translucency must be between 0 and 100 percent".into(),
                    ));
                }
                if !settings.controls.is_valid() {
                    return Err(ApiError::Validation(
                        "Control rounding must be 0–12px, padding 8–20px and outline strength 25–100%".into(),
                    ));
                }
                if [
                    settings.dither_recents_strength,
                    settings.dither_artwork_strength,
                ]
                .into_iter()
                .any(|value| !value.is_finite() || !(0. ..=1.).contains(&value))
                {
                    return Err(ApiError::Validation(
                        "Dithering strength must be between 0 and 100 percent".into(),
                    ));
                }
                if [settings.dither_recents_size, settings.dither_artwork_size]
                    .into_iter()
                    .any(|value| {
                        !value.is_finite()
                            || !(1. ..=startpoint_sdk::MAX_DITHER_SIZE).contains(&value)
                    })
                {
                    return Err(ApiError::Validation(
                        "Dithering size must be between 1 and 12 pixels".into(),
                    ));
                }
                self.store(|store| {
                    store.save_setting("appearance", &settings)?;
                    self.state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .snapshot
                        .settings
                        .appearance = settings;
                    Ok(())
                })?;
                self.publish(AppEvent::SettingsChanged);
                Ok(())
            }
            Command::SearchArtworkGames {
                provider_id,
                game_id,
                query,
            } => {
                self.game(&game_id)?;
                let cache_key = format!("{provider_id}/search/{}", query.trim().to_lowercase());
                let cached = self.store(|store| store.cached(&cache_key, now()))?;
                let games = match cached {
                    Some(games) => games,
                    None => {
                        let credential = self.credential(&provider_id, false)?;
                        let games = self.provider_result(
                            &provider_id,
                            &credential,
                            self.artwork_provider(&provider_id)?.search(
                                &query,
                                &credential,
                                context,
                            ),
                        )?;
                        self.store(|store| store.save_cache(&cache_key, &games, now()))?;
                        games
                    }
                };
                context.checkpoint()?;
                self.publish(AppEvent::ArtworkGamesFound {
                    job_id,
                    game_id,
                    games,
                });
                Ok(())
            }
            Command::ListArtwork {
                provider_id,
                game_id,
                remote_game_id,
                kind,
                page,
            } => {
                let game = self.game(&game_id)?;
                let mut result = self.artwork_choices_page(
                    &provider_id,
                    &game,
                    remote_game_id.as_deref(),
                    kind,
                    page,
                    context,
                )?;
                let hosts = self.artwork_provider(&provider_id)?.image_hosts();
                let key = artwork_page_cache_key(
                    &provider_id,
                    &game,
                    remote_game_id.as_deref(),
                    kind,
                    page,
                );
                self.publish(AppEvent::ArtworkFound {
                    job_id,
                    game_id: game_id.clone(),
                    kind,
                    page,
                    has_more: result.has_more,
                    images: result.images.clone(),
                });
                for index in 0..result.images.len() {
                    let image = &mut result.images[index];
                    context.checkpoint()?;
                    context.report(format!("Loading artwork preview {}", index + 1));
                    if !image
                        .preview_path
                        .as_ref()
                        .is_some_and(|path| path.is_file())
                        && let Ok(bytes) = artwork::download(
                            &self.image_client,
                            &image.thumbnail_url,
                            &hosts,
                            context,
                        )
                    {
                        image.preview_path =
                            artwork::save_preview(&bytes, &self.snapshot().paths).ok();
                    }
                    self.store(|store| store.save_cache(&key, &result, now()))?;
                    self.publish(AppEvent::ArtworkFound {
                        job_id,
                        game_id: game_id.clone(),
                        kind,
                        page,
                        has_more: result.has_more,
                        images: result.images.clone(),
                    });
                }
                context.checkpoint()?;
                self.store(|store| store.save_cache(&key, &result, now()))?;
                self.publish(AppEvent::ArtworkFound {
                    job_id,
                    game_id,
                    kind,
                    page,
                    has_more: result.has_more,
                    images: result.images,
                });
                Ok(())
            }
            Command::SelectArtwork { game_id, candidate } => {
                self.game(&game_id)?;
                let hosts = self.artwork_provider(&candidate.provider_id)?.image_hosts();
                let bytes =
                    artwork::download(&self.image_client, &candidate.image_url, &hosts, context)?;
                let asset = artwork::save_asset(
                    &bytes,
                    candidate.kind,
                    &candidate.provider_id,
                    Some(candidate.image_url),
                    candidate.author,
                    &self.snapshot().paths,
                )?;
                context.checkpoint()?;
                self.update_game(&game_id, |game| {
                    select_asset(game, asset);
                    Ok(())
                })
            }
            Command::ImportArtwork {
                game_id,
                kind,
                path,
            } => {
                self.game(&game_id)?;
                let bytes = artwork::read_local(&path)?;
                let asset = artwork::save_asset(
                    &bytes,
                    kind,
                    "local",
                    None,
                    "Your image".into(),
                    &self.snapshot().paths,
                )?;
                context.checkpoint()?;
                self.update_game(&game_id, |game| {
                    select_asset(game, asset);
                    Ok(())
                })
            }
            Command::SelectLocalArtwork { game_id, asset_id } => {
                self.update_game(&game_id, |game| {
                    let asset = game
                        .artwork_history
                        .iter()
                        .find(|asset| asset.id == asset_id)
                        .cloned()
                        .ok_or(ApiError::NotFound)?;
                    select_asset(game, asset);
                    Ok(())
                })
            }
        }
    }

    fn artwork_choices(
        &self,
        provider_id: &str,
        game: &Game,
        remote_id: Option<&str>,
        kind: ArtworkKind,
        context: &JobContext,
    ) -> Result<Vec<ArtworkCandidate>, ApiError> {
        self.artwork_choices_page(provider_id, game, remote_id, kind, 0, context)
            .map(|page| page.images)
    }

    fn artwork_choices_page(
        &self,
        provider_id: &str,
        game: &Game,
        remote_id: Option<&str>,
        kind: ArtworkKind,
        page: u32,
        context: &JobContext,
    ) -> Result<ArtworkPage, ApiError> {
        let key = artwork_page_cache_key(provider_id, game, remote_id, kind, page);
        if let Some(result) = self.store(|store| store.cached(&key, now()))? {
            return Ok(result);
        }
        let credential = self.credential(provider_id, false)?;
        let result = match self.provider_result(
            provider_id,
            &credential,
            self.artwork_provider(provider_id)?.artwork_page(
                game,
                remote_id,
                kind,
                page,
                &credential,
                context,
            ),
        ) {
            Ok(result) => result,
            Err(ApiError::NoArtwork) => ArtworkPage::default(),
            Err(error) => return Err(error),
        };
        context.checkpoint()?;
        self.store(|store| store.save_cache(&key, &result, now()))?;
        Ok(result)
    }

    fn refresh(&self, context: &JobContext) -> Result<(), ApiError> {
        if self.refreshing.swap(true, Ordering::Relaxed) {
            return Err(ApiError::Validation(
                "A library refresh is already running".into(),
            ));
        }
        let _refresh = RefreshGuard(&self.refreshing);
        let settings = self.snapshot().settings;
        let settings = DiscoverySettings {
            steam_directory: settings.steam_directory,
            epic_directory: settings.epic_directory,
            gog_directory: settings.gog_directory,
        };
        let mut local_covers = Vec::new();
        let mut warnings = Vec::new();
        for provider in &self.discovery {
            context.checkpoint()?;
            let descriptor = provider.descriptor();
            let report =
                match catch_unwind(AssertUnwindSafe(|| provider.discover(&settings, context)))
                    .unwrap_or_else(|_| {
                        Err(ApiError::Provider("Discovery failed unexpectedly".into()))
                    }) {
                    Ok(report) => report,
                    Err(ApiError::Cancelled) => return Err(ApiError::Cancelled),
                    Err(error) => {
                        warnings.push(format!(
                            "{} discovery failed; saved games retained: {error}",
                            descriptor.name
                        ));
                        continue;
                    }
                };
            if report
                .games
                .iter()
                .any(|game| game.source_game_id.is_empty() || game.source_game_id.len() > 512)
            {
                warnings.push(format!(
                    "{} returned an invalid game identifier; saved games retained",
                    descriptor.name
                ));
                continue;
            }
            warnings.extend(report.warnings);
            context.checkpoint()?;
            let changed = self.store(|store| {
                let previous = self.snapshot().games;
                let mut games = previous.as_ref().clone();
                let mut positions: BTreeMap<_, _> = games
                    .iter()
                    .enumerate()
                    .map(|(index, game)| (game.id.clone(), index))
                    .collect();
                if report.complete {
                    for game in games
                        .iter_mut()
                        .filter(|game| game.provider_id == descriptor.id)
                    {
                        game.available = false;
                    }
                }
                for (index, discovered) in report.games.into_iter().enumerate() {
                    context.checkpoint()?;
                    let id = GameId(format!("{}:{}", descriptor.id, discovered.source_game_id));
                    if let Some(path) = discovered.local_cover {
                        local_covers.push((id.clone(), path));
                    }
                    if let Some(position) = positions.get(&id) {
                        let game = &mut games[*position];
                        game.discovered_title = discovered.title;
                        game.install_directory = discovered.install_directory;
                        game.default_launch = discovered.launch_target;
                        game.available = true;
                    } else {
                        positions.insert(id.clone(), games.len());
                        games.push(Game {
                            id,
                            provider_id: descriptor.id.clone(),
                            source_game_id: discovered.source_game_id,
                            discovered_title: discovered.title,
                            install_directory: discovered.install_directory,
                            default_launch: discovered.launch_target,
                            launch_override: None,
                            steam_launch: None,
                            metadata: UserMetadata::default(),
                            catalogue_metadata: UserMetadata::default(),
                            favourite: false,
                            hidden: false,
                            available: true,
                            collections: BTreeSet::new(),
                            completed: false,
                            last_launched: if descriptor.id == "example" {
                                Some(now().saturating_sub(
                                    (u64::try_from(index).unwrap_or_default() + 1) * 86_400,
                                ))
                            } else {
                                None
                            },
                            artwork: BTreeMap::new(),
                            artwork_history: Vec::new(),
                            artwork_game_ids: BTreeMap::new(),
                        });
                    }
                }
                context.checkpoint()?;
                if games == *previous {
                    return Ok(false);
                }
                let changes: Vec<_> = games
                    .iter()
                    .enumerate()
                    .filter(|(index, game)| previous.get(*index) != Some(*game))
                    .map(|(_, game)| game.clone())
                    .collect();
                store.save_games(&changes)?;
                let library = sorted_library(games);
                let games = library
                    .iter()
                    .cloned()
                    .map(|game| (game.id.clone(), game))
                    .collect();
                let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                let replaced = std::mem::replace(&mut state.games, games);
                state.snapshot.games = library;
                drop(state);
                drop(replaced);
                Ok(true)
            })?;
            if changed {
                self.publish(AppEvent::LibraryChanged);
            }
        }
        context.checkpoint()?;
        let artwork_enabled = self.artwork_provider("steamgriddb").is_ok()
            && self
                .credentials
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get("steamgriddb")
                .is_some_and(Option::is_some);
        let credential_check = if artwork_enabled
            && self
                .snapshot()
                .settings
                .credential_status
                .get("steamgriddb")
                != Some(&CredentialStatus::Verified)
        {
            self.verify_credential("steamgriddb", context, false)
        } else {
            Ok(())
        };
        self.refresh_discovery_directories();
        self.publish(AppEvent::SettingsChanged);
        for (id, path) in local_covers {
            context.checkpoint()?;
            if self.game(&id)?.artwork.contains_key(&ArtworkKind::Cover) {
                continue;
            }
            if let Ok(bytes) = artwork::read_local(&path)
                && let Ok(asset) = artwork::save_asset(
                    &bytes,
                    ArtworkKind::Cover,
                    "steam-cache",
                    None,
                    "Steam".into(),
                    &self.snapshot().paths,
                )
            {
                self.update_game(&id, |game| {
                    if !game.artwork.contains_key(&ArtworkKind::Cover) {
                        select_asset(game, asset);
                    }
                    Ok(())
                })?;
            }
        }
        for game in self
            .snapshot()
            .games
            .iter()
            .filter(|game| game.provider_id == "manual")
        {
            context.checkpoint()?;
            let available = match &game.default_launch {
                LaunchTarget::Executable(launch) | LaunchTarget::ApplicationBundle(launch) => {
                    launch.executable.exists()
                }
                LaunchTarget::Uri(_) => true,
            };
            if game.available != available {
                self.update_game(&game.id, |game| {
                    game.available = available;
                    Ok(())
                })?;
            }
        }
        let mut repaired = 0;
        let mut games = self.snapshot().games.as_ref().clone();
        games.sort_by_key(|game| std::cmp::Reverse(game.last_launched.unwrap_or(0)));
        for game in &games {
            for asset in game.artwork.values() {
                context.checkpoint()?;
                if artwork::restore_thumbnail(asset, &self.snapshot().paths).unwrap_or(false) {
                    repaired += 1;
                    context.report(format!("Restoring cached thumbnails · {repaired}"));
                    if repaired <= 16 || repaired % 32 == 0 {
                        self.publish(AppEvent::ArtworkCacheChanged);
                    }
                }
            }
        }
        if repaired > 0 {
            self.publish(AppEvent::ArtworkCacheChanged);
        }
        credential_check?;
        if artwork_enabled {
            self.populate_library(context)?;
        }
        context.checkpoint()?;
        if !warnings.is_empty() {
            context.report(warnings.join("\n"));
        } else {
            context.report("Installed games are up to date");
        }
        Ok(())
    }

    fn refresh_steam_shortcuts(&self, context: &JobContext) -> Result<(), ApiError> {
        context.report("Reading saved non-Steam shortcuts from Steam");
        let directory = self.snapshot().settings.steam_directory;
        let (shortcuts, warnings) = match steam_shortcuts::root(directory.as_deref())
            .and_then(|root| steam_shortcuts::scan(&root, context))
        {
            Ok(result) => result,
            Err(ApiError::Cancelled) => return Err(ApiError::Cancelled),
            Err(error) => (Vec::new(), vec![error.to_string()]),
        };
        context.checkpoint()?;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let settings = &mut state.snapshot.settings;
        settings.steam_shortcuts = shortcuts;
        settings.steam_shortcut_warnings = warnings;
        settings.steam_shortcuts_loaded = true;
        drop(state);
        self.publish(AppEvent::SettingsChanged);
        Ok(())
    }

    fn refresh_discovery_directories(&self) {
        let snapshot = self.snapshot();
        let directories = crate::discovery_paths::resolve(&snapshot.settings, &snapshot.games);
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot
            .settings
            .discovery_directories = directories;
    }

    fn record_launch(&self, game_id: &GameId) -> Result<(), ApiError> {
        self.store(|store| {
            let mut game = self.game(game_id)?;
            let timestamp = now();
            game.last_launched = Some(timestamp);
            let activity = store.record_launch(&game, timestamp)?;
            let previous = self.snapshot().games;
            let library = updated_library(&previous, &game);
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            let replaced = state.games.insert(game_id.clone(), game);
            state.snapshot.activity = Arc::new(activity);
            state.snapshot.games = library;
            drop(state);
            drop(replaced);
            Ok(())
        })?;
        self.publish(AppEvent::LibraryChanged);
        Ok(())
    }

    fn populate_library(&self, context: &JobContext) -> Result<(), ApiError> {
        if self.artwork_provider("steamgriddb").is_err() {
            return Ok(());
        }
        let mut games: Vec<_> = self
            .snapshot()
            .games
            .iter()
            .filter(|game| game.available && !game.hidden)
            .cloned()
            .collect();
        games.sort_by_key(|game| std::cmp::Reverse(game.last_launched.unwrap_or(0)));
        let mut first_error = None;
        for (index, game) in games.iter().enumerate() {
            context.checkpoint()?;
            context.report(format!(
                "Filling metadata and artwork {}/{} · {}",
                index + 1,
                games.len(),
                game.title()
            ));
            match self.populate_game(&game.id, context) {
                Ok(()) | Err(ApiError::NoArtwork) => {}
                Err(error @ (ApiError::Cancelled | ApiError::InvalidCredential(_))) => {
                    return Err(error);
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    fn populate_game(&self, game_id: &GameId, context: &JobContext) -> Result<(), ApiError> {
        let mut game = self.game(game_id)?;
        let credential = self.credential("steamgriddb", false)?;
        let mut catalogue_error = None;
        if matches!(game.provider_id.as_str(), "steam" | "example") {
            match self.populate_catalogue_metadata(&game, context) {
                Ok(()) | Err(ApiError::NoArtwork) => {}
                Err(error @ ApiError::Cancelled) => return Err(error),
                Err(error) => catalogue_error = Some(error),
            }
            game = self.game(game_id)?;
        }
        if !game.artwork_game_ids.contains_key("steamgriddb")
            || game.catalogue_metadata.title.is_none()
        {
            let key = format!(
                "steamgriddb/game/{}/{}",
                game.id.0,
                game.artwork_game_ids
                    .get("steamgriddb")
                    .map(String::as_str)
                    .unwrap_or("source")
            );
            let remote = match self.store(|store| store.cached(&key, now()))? {
                Some(remote) => remote,
                None => {
                    let remote = self.provider_result(
                        "steamgriddb",
                        &credential,
                        self.artwork_provider("steamgriddb")?
                            .game(&game, &credential, context),
                    )?;
                    self.store(|store| store.save_cache(&key, &remote, now()))?;
                    remote
                }
            };
            let remote: ArtworkGame = remote;
            context.checkpoint()?;
            self.update_game(game_id, |game| {
                game.artwork_game_ids
                    .entry("steamgriddb".into())
                    .or_insert(remote.id);
                if game.catalogue_metadata.title.is_none() {
                    game.catalogue_metadata.title = Some(remote.name);
                }
                Ok(())
            })?;
            game = self.game(game_id)?;
        }
        if !matches!(game.provider_id.as_str(), "steam" | "example") {
            match self.populate_catalogue_metadata(&game, context) {
                Ok(()) | Err(ApiError::NoArtwork) => {}
                Err(error @ ApiError::Cancelled) => return Err(error),
                Err(error) => catalogue_error = Some(error),
            }
            game = self.game(game_id)?;
        }
        for kind in [
            ArtworkKind::Background,
            ArtworkKind::Cover,
            ArtworkKind::Logo,
            ArtworkKind::Icon,
        ] {
            context.checkpoint()?;
            if self.game(game_id)?.artwork.contains_key(&kind) {
                continue;
            }
            context.report(format!(
                "Choosing popular {} artwork · {}",
                kind.label().to_lowercase(),
                game.title()
            ));
            let images = self.artwork_choices("steamgriddb", &game, None, kind, context)?;
            let Some(candidate) = images.first() else {
                continue;
            };
            let hosts = self.artwork_provider("steamgriddb")?.image_hosts();
            let bytes =
                artwork::download(&self.image_client, &candidate.image_url, &hosts, context)?;
            let asset = artwork::save_asset(
                &bytes,
                kind,
                "steamgriddb",
                Some(candidate.image_url.clone()),
                candidate.author.clone(),
                &self.snapshot().paths,
            )?;
            context.checkpoint()?;
            self.update_game(game_id, |current| {
                if !current.artwork.contains_key(&kind)
                    && current.artwork_game_ids.get("steamgriddb")
                        == game.artwork_game_ids.get("steamgriddb")
                {
                    select_asset(current, asset);
                }
                Ok(())
            })?;
        }
        catalogue_error.map_or(Ok(()), Err)
    }

    fn populate_catalogue_metadata(
        &self,
        game: &Game,
        context: &JobContext,
    ) -> Result<(), ApiError> {
        if [
            &game.catalogue_metadata.title,
            &game.catalogue_metadata.description,
            &game.catalogue_metadata.developer,
            &game.catalogue_metadata.genres,
        ]
        .iter()
        .all(|field| field.is_some())
        {
            return Ok(());
        }
        let Some(target) = catalogue_target(game) else {
            return Ok(());
        };
        let mut native_error = None;
        if let CatalogueTarget::Identified { title, .. } = &target
            && let Some(key) = native_catalogue_key(game, title)
        {
            let cached = self.store(|store| store.cached::<Option<UserMetadata>>(&key, now()))?;
            let native = match cached {
                Some(metadata) => metadata,
                None => {
                    let result = match game.provider_id.as_str() {
                        "gog" => catalogue::gog_metadata(
                            &self.image_client,
                            &game.source_game_id,
                            context,
                        ),
                        "epic" => catalogue::epic_metadata(
                            &self.image_client,
                            title,
                            catalogue::epic_namespace(game).as_deref(),
                            context,
                        ),
                        _ => unreachable!(),
                    };
                    match result {
                        Ok(metadata) => {
                            self.store(|store| store.save_cache(&key, &Some(&metadata), now()))?;
                            Some(metadata)
                        }
                        Err(ApiError::NoArtwork) => {
                            self.store(|store| {
                                store.save_cache(&key, &None::<UserMetadata>, now())
                            })?;
                            None
                        }
                        Err(error @ ApiError::Cancelled) => return Err(error),
                        Err(error) => {
                            native_error = Some(error);
                            None
                        }
                    }
                }
            };
            // The GOG response has already been verified against this exact
            // product ID, and native_catalogue_key checked the selected identity
            // against the installed game. Storefront names can change (e.g.
            // Witcher 3 GOTY is now marketed as Remastered); do not reject the
            // same product's text just because its current display name differs.
            // Epic searches by title, so it still needs the title check below.
            if let Some(metadata) = native
                && metadata.title.as_deref().is_some_and(|name| {
                    game.provider_id == "gog" || catalogue::titles_match(title, name)
                })
                && (metadata.description.is_some()
                    || metadata.developer.is_some()
                    || metadata.genres.is_some())
            {
                context.checkpoint()?;
                return self.update_game(&game.id, |current| {
                    merge_catalogue_metadata(current, &target, metadata);
                    Ok(())
                });
            }
        }
        let app_id = match &target {
            CatalogueTarget::Steam(id) => id.clone(),
            CatalogueTarget::Identified { title, .. } => {
                // Cache misses as well as matches to avoid repeating searches for
                // games with no Steam listing during every library refresh.
                let key = format!("steam/catalogue-title-v2/{}", title.trim().to_lowercase());
                let app_id = match self
                    .store(|store| store.cached::<Option<String>>(&key, now()))?
                {
                    Some(app_id) => app_id,
                    None => {
                        let app_id = steam::app_id_for_title(&self.image_client, title, context)?;
                        self.store(|store| store.save_cache(&key, &app_id, now()))?;
                        app_id
                    }
                };
                let Some(app_id) = app_id else {
                    return native_error.map_or(Ok(()), Err);
                };
                app_id
            }
        };
        let key = format!("steam/catalogue/{app_id}");
        let metadata: UserMetadata = match self.store(|store| store.cached(&key, now()))? {
            Some(metadata) => metadata,
            None => {
                let metadata = steam::metadata(&self.image_client, &app_id, context)?;
                self.store(|store| store.save_cache(&key, &metadata, now()))?;
                metadata
            }
        };
        // Verify the catalogue result too: a stale search cache must never mix
        // an expansion/sequel's text into the selected identity.
        if let CatalogueTarget::Identified { title, .. } = &target
            && !metadata
                .title
                .as_deref()
                .is_some_and(|name| catalogue::titles_match(title, name))
        {
            return Ok(());
        }
        context.checkpoint()?;
        self.update_game(&game.id, |current| {
            merge_catalogue_metadata(current, &target, metadata);
            Ok(())
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CatalogueTarget {
    Steam(String),
    Identified { title: String, artwork_id: String },
}

fn native_catalogue_key(game: &Game, title: &str) -> Option<String> {
    // Do not borrow the installed game's native metadata after the user picked
    // a different game or expansion. The explicit identity still wins.
    if !catalogue::titles_match(title, &game.discovered_title) {
        return None;
    }
    match game.provider_id.as_str() {
        "gog" => Some(format!("gog/catalogue-v1/{}", game.source_game_id)),
        "epic" => Some(format!(
            "epic/catalogue-v1/{}/{}",
            catalogue::epic_namespace(game).unwrap_or_else(|| game.source_game_id.clone()),
            title.trim().to_lowercase()
        )),
        _ => None,
    }
}

fn catalogue_target(game: &Game) -> Option<CatalogueTarget> {
    if matches!(game.provider_id.as_str(), "steam" | "example") {
        Some(CatalogueTarget::Steam(game.source_game_id.clone()))
    } else {
        Some(CatalogueTarget::Identified {
            title: game.catalogue_metadata.title.clone()?,
            artwork_id: game.artwork_game_ids.get("steamgriddb")?.clone(),
        })
    }
}

fn merge_catalogue_metadata(game: &mut Game, target: &CatalogueTarget, metadata: UserMetadata) {
    // Refresh and re-identification can run on different workers. Do not publish
    // a result belonging to the previous selection. User overrides stay separate.
    if catalogue_target(game).as_ref() != Some(target) {
        return;
    }
    let current = &mut game.catalogue_metadata;
    current.title = current.title.take().or(metadata.title);
    current.description = current.description.take().or(metadata.description);
    current.developer = current.developer.take().or(metadata.developer);
    current.genres = current.genres.take().or(metadata.genres);
}

struct RefreshGuard<'a>(&'a AtomicBool);
impl Drop for RefreshGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

fn restore_appearance_settings(
    document: Option<serde_json::Value>,
) -> Result<(AppearanceSettings, bool), ApiError> {
    let Some(document) = document else {
        return Ok((AppearanceSettings::default(), false));
    };
    let experimental = [
        "recents_fade_length",
        "recents_fade_style",
        "recents_fade_dithering",
        "recents_fade_dither_style",
    ]
    .iter()
    .any(|key| document.get(key).is_some());
    let combined_glass_switch = document.get("backdrop_blur").is_none();
    let mut settings: AppearanceSettings = serde_json::from_value(document)
        .map_err(|error| ApiError::Storage(format!("Invalid appearance settings: {error}")))?;
    if combined_glass_switch {
        settings.backdrop_blur = settings.window_translucency;
    }
    let invalid_amount = !settings.translucency_amount.is_finite()
        || !(0. ..=1.).contains(&settings.translucency_amount);
    if invalid_amount {
        settings.translucency_amount = DEFAULT_TRANSLUCENCY_AMOUNT;
    }
    let mut invalid_dithering = false;
    for value in [
        &mut settings.dither_recents_strength,
        &mut settings.dither_artwork_strength,
    ] {
        if !value.is_finite() || !(0. ..=1.).contains(value) {
            *value = startpoint_sdk::DEFAULT_DITHER_STRENGTH;
            invalid_dithering = true;
        }
    }
    let invalid_controls = !settings.controls.is_valid();
    for value in [
        &mut settings.dither_recents_size,
        &mut settings.dither_artwork_size,
    ] {
        if !value.is_finite() || !(1. ..=startpoint_sdk::MAX_DITHER_SIZE).contains(value) {
            *value = startpoint_sdk::DEFAULT_DITHER_SIZE;
            invalid_dithering = true;
        }
    }
    if invalid_controls {
        let defaults = AppearanceSettings::default().controls;
        for (value, min, max, default) in [
            (&mut settings.controls.radius, 0., 12., defaults.radius),
            (
                &mut settings.controls.horizontal_padding,
                8.,
                20.,
                defaults.horizontal_padding,
            ),
            (
                &mut settings.controls.outline_strength,
                0.25,
                1.,
                defaults.outline_strength,
            ),
        ] {
            *value = if value.is_finite() {
                value.clamp(min, max)
            } else {
                default
            };
        }
    }
    if experimental {
        // Adopt the settled Recents defaults once, retaining theme/mode/widgets
        // and all other preferences. Saving removes the experimental keys, so
        // subsequent changes to the retained toggles/start page are respected.
        let defaults = AppearanceSettings::default();
        settings.start_screen = defaults.start_screen;
        settings.dither_recents = defaults.dither_recents;
        settings.fade_recents = defaults.fade_recents;
    }
    Ok((
        settings,
        experimental
            || combined_glass_switch
            || invalid_amount
            || invalid_controls
            || invalid_dithering,
    ))
}

fn apply_reidentification(game: &mut Game, matched: &ArtworkGame) {
    if game.artwork_game_ids.get("steamgriddb") != Some(&matched.id)
        || game.catalogue_metadata.title.as_deref() != Some(matched.name.trim())
    {
        // Automatic text belongs to the previous identity; hand-written fields
        // in game.metadata must survive a correction.
        game.catalogue_metadata = UserMetadata::default();
    }
    game.metadata.title = Some(matched.name.trim().to_string());
    game.catalogue_metadata.title = Some(matched.name.trim().to_string());
    game.artwork_game_ids
        .insert("steamgriddb".into(), matched.id.clone());
    // Keep user imports and every saved image; discard only the old automatic selections.
    game.artwork
        .retain(|_, asset| asset.provider_id != "steamgriddb");
}

fn sorted_library(mut games: Vec<Game>) -> Arc<Vec<Game>> {
    games.sort_by_cached_key(|game| (game.title().to_lowercase(), game.id.clone()));
    Arc::new(games)
}

fn updated_library(previous: &[Game], game: &Game) -> Arc<Vec<Game>> {
    // ponytail: value-based SDK snapshots require O(n) copying; use per-game Arc handles if profiling warrants an API change.
    let mut games = previous.to_vec();
    let needs_sort = if let Some(index) = games.iter().position(|current| current.id == game.id) {
        let title_changed = games[index].title() != game.title();
        games[index] = game.clone();
        title_changed
    } else {
        games.push(game.clone());
        true
    };
    if needs_sort {
        sorted_library(games)
    } else {
        Arc::new(games)
    }
}

fn select_asset(game: &mut Game, mut asset: ArtworkAsset) {
    asset.id = format!(
        "{:?}:{}",
        asset.kind,
        asset.id.split(':').next_back().unwrap_or(&asset.id)
    );
    if let Some(previous) = game
        .artwork_history
        .iter_mut()
        .find(|previous| previous.id == asset.id)
    {
        *previous = asset.clone();
    } else {
        game.artwork_history.push(asset.clone());
    }
    game.artwork.insert(asset.kind, asset);
}

#[cfg(test)]
fn artwork_cache_key(
    provider_id: &str,
    game: &Game,
    remote_id: Option<&str>,
    kind: ArtworkKind,
) -> String {
    artwork_page_cache_key(provider_id, game, remote_id, kind, 0)
}

fn artwork_page_cache_key(
    provider_id: &str,
    game: &Game,
    remote_id: Option<&str>,
    kind: ArtworkKind,
    page: u32,
) -> String {
    format!(
        "{provider_id}/images-v4/{}/{kind:?}/{}/page/{page}",
        game.id.0,
        remote_id
            .or_else(|| game.artwork_game_ids.get(provider_id).map(String::as_str))
            .unwrap_or("source")
    )
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn identified_nonsteam_game(provider: &str) -> Game {
        serde_json::from_value(serde_json::json!({
            "id": format!("{provider}:1423049311"), "provider_id": provider,
            "source_game_id": "1423049311", "discovered_title": "Cyberpunk 2077",
            "install_directory": "game", "default_launch": {"Uri":"steam://rungameid/1091500"},
            "launch_override": null, "metadata": {"title":"My custom title"},
            "catalogue_metadata": {"title":"Cyberpunk 2077: Phantom Liberty"},
            "favourite": true, "available": true, "last_launched": 123,
            "artwork": {}, "artwork_history": [], "artwork_game_ids": {"steamgriddb":"5461202"}
        }))
        .unwrap()
    }

    #[test]
    fn steam_launch_routes_persist_on_the_original_game_and_reset_without_losing_data() {
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/verification")
            .join(uuid::Uuid::new_v4().to_string());
        let paths = ApplicationPaths {
            data_directory: directory.join("data"),
            cache_directory: directory.join("cache"),
        };
        let steam = directory.join("Steam");
        let config = steam.join("userdata/42/config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("shortcuts.vdf"),
            steam_shortcuts::fixture_bytes(&[(0x9123_4567, "Cyberpunk via Steam")]),
        )
        .unwrap();
        let game = identified_nonsteam_game("gog");
        let mut store = Store::open(&paths).unwrap();
        store.save_games(std::slice::from_ref(&game)).unwrap();
        store.save_setting("steam_directory", &Some(steam)).unwrap();
        drop(store);
        let api = LauncherBuilder::new().build(paths.clone()).unwrap();
        let events = api.subscribe();
        let submit = |command: Command, expected| {
            let job_id = api.submit(command).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                assert!(Instant::now() < deadline, "Steam launch command timed out");
                match events.try_recv() {
                    Ok(AppEvent::JobChanged(job)) if job.id == job_id && !job.state.active() => {
                        assert_eq!(job.state, expected, "{:?}", job.error);
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
        };
        submit(Command::RefreshSteamShortcuts, JobState::Completed);
        let settings = api.snapshot().settings;
        assert!(settings.steam_shortcuts_loaded);
        assert_eq!(settings.steam_shortcuts.len(), 1);
        assert_eq!(settings.steam_shortcuts[0].account_id, 42);
        let launch = settings.steam_shortcuts[0].launch();
        submit(
            Command::SetSteamLaunch {
                game_id: game.id.clone(),
                launch: launch.clone(),
            },
            JobState::Completed,
        );
        let saved = api.snapshot().games[0].clone();
        assert_eq!(saved.effective_launch(), LaunchTarget::Uri(launch.uri()));
        assert!(saved.has_launch_override());
        let mut original = saved.clone();
        original.steam_launch = None;
        assert_eq!(
            original, game,
            "A new launch route must not create or re-identify a library entry"
        );
        let mut corrected = saved.clone();
        apply_reidentification(
            &mut corrected,
            &ArtworkGame {
                id: "999".into(),
                name: "Corrected title".into(),
            },
        );
        assert_eq!(corrected.steam_launch, saved.steam_launch);
        submit(
            Command::SetSteamLaunch {
                game_id: game.id.clone(),
                launch: SteamLaunch::Url("steam://uninstall/123".into()),
            },
            JobState::Failed,
        );
        assert_eq!(api.snapshot().games[0], saved);
        submit(
            Command::SetSteamLaunch {
                game_id: game.id.clone(),
                launch: SteamLaunch::Url("steam://rungameid/000123".into()),
            },
            JobState::Completed,
        );
        assert_eq!(
            api.snapshot().games[0].steam_launch,
            Some(SteamLaunch::Url("steam://rungameid/123".into()))
        );
        let reopened = LauncherBuilder::new().build(paths).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !reopened.snapshot().ready {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(reopened.snapshot().games[0], api.snapshot().games[0]);
        let executable = ExecutableLaunch {
            executable: std::env::current_exe().unwrap(),
            arguments: vec!["--list".into()],
            working_directory: None,
        };
        submit(
            Command::SetLaunchOverride {
                game_id: game.id.clone(),
                launch: Some(executable.clone()),
            },
            JobState::Completed,
        );
        assert!(api.snapshot().games[0].steam_launch.is_none());
        assert_eq!(
            api.snapshot().games[0].effective_launch(),
            LaunchTarget::Executable(executable)
        );
        submit(
            Command::SetLaunchOverride {
                game_id: game.id.clone(),
                launch: None,
            },
            JobState::Completed,
        );
        assert_eq!(api.snapshot().games[0], game);
        let mut old_document = serde_json::to_value(&game).unwrap();
        old_document.as_object_mut().unwrap().remove("steam_launch");
        let restored: Game = serde_json::from_value(old_document).unwrap();
        assert_eq!(restored, game);
    }

    struct EmptyArtwork;
    impl ArtworkProvider for EmptyArtwork {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: "steamgriddb".into(),
                name: "Fixture artwork".into(),
            }
        }
        fn image_hosts(&self) -> Vec<String> {
            Vec::new()
        }
        fn requires_credential(&self) -> bool {
            false
        }
        fn search(&self, _: &str, _: &str, _: &JobContext) -> Result<Vec<ArtworkGame>, ApiError> {
            panic!("An identified game must not be searched again")
        }
        fn artwork(
            &self,
            _: &Game,
            _: Option<&str>,
            _: ArtworkKind,
            _: &str,
            _: &JobContext,
        ) -> Result<Vec<ArtworkCandidate>, ApiError> {
            Ok(Vec::new())
        }
    }

    struct PagedArtwork {
        calls: Arc<Mutex<Vec<u32>>>,
        preview: std::path::PathBuf,
    }

    impl ArtworkProvider for PagedArtwork {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: "fixture".into(),
                name: "Paged fixture".into(),
            }
        }
        fn image_hosts(&self) -> Vec<String> {
            Vec::new()
        }
        fn requires_credential(&self) -> bool {
            false
        }
        fn search(&self, _: &str, _: &str, _: &JobContext) -> Result<Vec<ArtworkGame>, ApiError> {
            Ok(Vec::new())
        }
        fn artwork(
            &self,
            _: &Game,
            _: Option<&str>,
            _: ArtworkKind,
            _: &str,
            _: &JobContext,
        ) -> Result<Vec<ArtworkCandidate>, ApiError> {
            panic!("Paged requests must use artwork_page")
        }
        fn artwork_page(
            &self,
            _: &Game,
            _: Option<&str>,
            kind: ArtworkKind,
            page: u32,
            _: &str,
            _: &JobContext,
        ) -> Result<ArtworkPage, ApiError> {
            self.calls.lock().unwrap().push(page);
            Ok(ArtworkPage {
                images: vec![ArtworkCandidate {
                    id: format!("fixture:{page}"),
                    provider_id: "fixture".into(),
                    kind,
                    image_url: "https://example.invalid/full".into(),
                    thumbnail_url: "https://example.invalid/thumb".into(),
                    author: "Fixture".into(),
                    preview_path: Some(self.preview.clone()),
                    score: 0,
                    width: 600,
                    height: 900,
                }],
                has_more: page == 0,
            })
        }
    }

    #[test]
    fn artwork_page_requests_cache_separately_and_keep_pagination_in_preview_events() {
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/verification")
            .join(uuid::Uuid::new_v4().to_string());
        let paths = ApplicationPaths {
            data_directory: directory.join("data"),
            cache_directory: directory.join("cache"),
        };
        let game = identified_nonsteam_game("gog");
        let mut store = Store::open(&paths).unwrap();
        store.save_games(std::slice::from_ref(&game)).unwrap();
        let preview = paths.cache_directory.join("preview.png");
        std::fs::write(&preview, b"cached preview, never decoded in this test").unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (sender, events) = async_channel::unbounded();
        let shared = Shared {
            image_client: reqwest::blocking::Client::new(),
            state: Mutex::new(State {
                games: BTreeMap::from([(game.id.clone(), game.clone())]),
                snapshot: Snapshot {
                    ready: true,
                    games: Arc::new(vec![game.clone()]),
                    activity: Arc::new(Vec::new()),
                    journal: Arc::new(Vec::new()),
                    jobs: Vec::new(),
                    settings: SettingsSnapshot::default(),
                    paths,
                    initialization_error: None,
                    providers: Vec::new(),
                },
            }),
            store: Mutex::new(Some(store)),
            subscribers: Mutex::new(vec![sender]),
            cancellations: Mutex::new(BTreeMap::new()),
            credentials: Mutex::new(BTreeMap::from([("fixture".into(), Some(String::new()))])),
            discovery: Vec::new(),
            artwork: vec![Arc::new(PagedArtwork {
                calls: calls.clone(),
                preview,
            })],
            refreshing: AtomicBool::new(false),
        };
        let context = JobContext::new(Arc::new(AtomicBool::new(false)), |_| {});
        for (job_id, page) in [(100, 0), (101, 1), (102, 0)] {
            shared
                .execute(
                    job_id,
                    Command::ListArtwork {
                        provider_id: "fixture".into(),
                        game_id: game.id.clone(),
                        remote_game_id: Some("123".into()),
                        kind: ArtworkKind::Cover,
                        page,
                    },
                    &context,
                )
                .unwrap();
            let mut updates = 0;
            while let Ok(event) = events.try_recv() {
                if let AppEvent::ArtworkFound {
                    job_id: owner,
                    game_id,
                    kind,
                    page: received_page,
                    has_more,
                    images,
                } = event
                {
                    assert_eq!(owner, job_id);
                    assert_eq!(game_id, game.id);
                    assert_eq!(kind, ArtworkKind::Cover);
                    assert_eq!(received_page, page);
                    assert_eq!(has_more, page == 0);
                    assert_eq!(images[0].id, format!("fixture:{page}"));
                    updates += 1;
                }
            }
            assert!(
                updates >= 2,
                "Initial results and preview updates must carry the same page context"
            );
        }
        assert_eq!(
            *calls.lock().unwrap(),
            vec![0, 1],
            "Going back must reuse the cached page"
        );
        assert_eq!(
            shared.snapshot().games[0],
            game,
            "Browsing pages must not change the game identity or selected images"
        );
        let first = artwork_page_cache_key("fixture", &game, Some("123"), ArtworkKind::Cover, 0);
        assert_ne!(
            first,
            artwork_page_cache_key("fixture", &game, Some("123"), ArtworkKind::Cover, 1)
        );
        assert_ne!(
            first,
            artwork_page_cache_key("fixture", &game, Some("456"), ArtworkKind::Cover, 0)
        );
        assert_ne!(
            first,
            artwork_page_cache_key("fixture", &game, Some("123"), ArtworkKind::Logo, 0)
        );
    }

    #[test]
    fn identified_nonsteam_games_backfill_cached_text_without_changing_user_data() {
        for provider in ["gog", "epic", "manual"] {
            let game = identified_nonsteam_game(provider);
            let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/verification")
                .join(uuid::Uuid::new_v4().to_string());
            let paths = ApplicationPaths {
                data_directory: directory.join("data"),
                cache_directory: directory.join("cache"),
            };
            let mut store = Store::open(&paths).unwrap();
            store.save_games(std::slice::from_ref(&game)).unwrap();
            store
                .save_cache(
                    "steam/catalogue-title-v2/cyberpunk 2077: phantom liberty",
                    &Some("2138330"),
                    now(),
                )
                .unwrap();
            let metadata = UserMetadata {
                title: Some("Cyberpunk 2077: Phantom Liberty".into()),
                description: Some("Expansion description".into()),
                developer: Some("CD PROJEKT RED".into()),
                genres: Some("RPG".into()),
            };
            store
                .save_cache("steam/catalogue/2138330", &metadata, now())
                .unwrap();
            let shared = Shared {
                image_client: reqwest::blocking::Client::new(),
                state: Mutex::new(State {
                    games: BTreeMap::from([(game.id.clone(), game.clone())]),
                    snapshot: Snapshot {
                        ready: true,
                        games: Arc::new(vec![game.clone()]),
                        activity: Arc::new(Vec::new()),
                        journal: Arc::new(Vec::new()),
                        jobs: Vec::new(),
                        settings: SettingsSnapshot::default(),
                        paths,
                        initialization_error: None,
                        providers: Vec::new(),
                    },
                }),
                store: Mutex::new(Some(store)),
                subscribers: Mutex::new(Vec::new()),
                cancellations: Mutex::new(BTreeMap::new()),
                credentials: Mutex::new(BTreeMap::from([(
                    "steamgriddb".into(),
                    Some(String::new()),
                )])),
                discovery: Vec::new(),
                artwork: vec![Arc::new(EmptyArtwork)],
                refreshing: AtomicBool::new(false),
            };
            let context = JobContext::new(Arc::new(AtomicBool::new(false)), |_| {});
            shared.populate_game(&game.id, &context).unwrap();
            let enriched = shared.game(&game.id).unwrap();
            assert_eq!(enriched.catalogue_metadata, metadata);
            assert_eq!(
                enriched.effective_metadata().description.as_deref(),
                Some("Expansion description")
            );
            let mut unchanged = enriched.clone();
            unchanged.catalogue_metadata = game.catalogue_metadata.clone();
            assert_eq!(
                unchanged, game,
                "{provider}: enrichment changed user-owned data"
            );
            // It is persisted and repeated refreshes do not need another lookup.
            assert_eq!(shared.store(|store| store.games()).unwrap()[0], enriched);
            shared.populate_game(&game.id, &context).unwrap();
            assert_eq!(shared.game(&game.id).unwrap(), enriched);

            // A stale/mismatched catalogue record and a cached miss must both
            // leave this identified game intact, without substituting base-game text.
            shared
                .update_game(&game.id, |current| {
                    current.catalogue_metadata = game.catalogue_metadata.clone();
                    Ok(())
                })
                .unwrap();
            let mut wrong = metadata.clone();
            wrong.title = Some("Cyberpunk 2077".into());
            shared
                .store(|store| store.save_cache("steam/catalogue/2138330", &wrong, now()))
                .unwrap();
            shared.populate_game(&game.id, &context).unwrap();
            assert_eq!(shared.game(&game.id).unwrap(), game);
            shared
                .store(|store| {
                    store.save_cache(
                        "steam/catalogue-title-v2/cyberpunk 2077: phantom liberty",
                        &None::<String>,
                        now(),
                    )
                })
                .unwrap();
            shared.populate_game(&game.id, &context).unwrap();
            assert_eq!(shared.game(&game.id).unwrap(), game);
        }
    }

    #[test]
    fn reidentification_drops_old_automatic_text_but_retains_manual_fields() {
        let mut game = identified_nonsteam_game("gog");
        game.metadata.description = Some("My description".into());
        game.catalogue_metadata.description = Some("Old expansion description".into());
        game.catalogue_metadata.developer = Some("Old developer".into());
        game.catalogue_metadata.genres = Some("Old genres".into());
        let old_target = catalogue_target(&game).unwrap();
        let old_metadata = game.catalogue_metadata.clone();
        apply_reidentification(
            &mut game,
            &ArtworkGame {
                id: "5460000".into(),
                name: "Cyberpunk 2077".into(),
            },
        );
        assert_eq!(game.metadata.description.as_deref(), Some("My description"));
        assert_eq!(
            game.catalogue_metadata,
            UserMetadata {
                title: Some("Cyberpunk 2077".into()),
                ..UserMetadata::default()
            }
        );
        let corrected = game.clone();
        merge_catalogue_metadata(&mut game, &old_target, old_metadata);
        assert_eq!(
            game, corrected,
            "An old worker result must not restore stale metadata"
        );
        let target = catalogue_target(&game).unwrap();
        merge_catalogue_metadata(
            &mut game,
            &target,
            UserMetadata {
                description: Some("New automatic description".into()),
                ..UserMetadata::default()
            },
        );
        assert_eq!(
            game.effective_metadata().description.as_deref(),
            Some("My description")
        );
    }

    #[test]
    fn metadata_lookup_uses_store_ids_only_for_steam_and_selected_names_for_other_sources() {
        let mut game = identified_nonsteam_game("gog");
        assert_eq!(
            catalogue_target(&game),
            Some(CatalogueTarget::Identified {
                title: "Cyberpunk 2077: Phantom Liberty".into(),
                artwork_id: "5461202".into(),
            })
        );
        game.artwork_game_ids.clear();
        assert_eq!(catalogue_target(&game), None);
        game.provider_id = "steam".into();
        game.source_game_id = "1091500".into();
        assert_eq!(
            catalogue_target(&game),
            Some(CatalogueTarget::Steam("1091500".into()))
        );
    }

    #[test]
    fn native_metadata_uses_verified_store_identity_despite_renames_and_steam_misses() {
        for (provider, discovered, selected, catalogue_title, source_id, description, developer) in [
            (
                "gog",
                "Disco Elysium - The Final Cut",
                "Disco Elysium",
                "Disco Elysium - The Final Cut",
                "1771589310",
                "A detective RPG.",
                "ZA/UM",
            ),
            (
                "epic",
                "Alan Wake 2",
                "Alan Wake 2",
                "Alan Wake 2",
                "epic-app",
                "Two heroes in a horror story.",
                "Remedy Entertainment",
            ),
            (
                "gog",
                "The Witcher 3: Wild Hunt - Game of the Year Edition",
                "The Witcher 3: Wild Hunt",
                "The Witcher 3: Wild Hunt — Remastered",
                "1495134320",
                "Become a professional monster slayer.",
                "CD PROJEKT RED",
            ),
        ] {
            let mut game = identified_nonsteam_game(provider);
            game.discovered_title = discovered.into();
            game.source_game_id = source_id.into();
            game.metadata.title = Some(selected.into());
            game.catalogue_metadata = UserMetadata {
                title: Some(selected.into()),
                ..UserMetadata::default()
            };
            if provider == "epic" {
                game.default_launch = LaunchTarget::Uri("com.epicgames.launcher://apps/namespace%3Aitem%3Aepic-app?action=launch&silent=true".into());
                assert_eq!(
                    catalogue::epic_namespace(&game).as_deref(),
                    Some("namespace")
                );
            }
            game.steam_launch = Some(SteamLaunch::Url("steam://rungameid/123".into()));
            let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/verification")
                .join(uuid::Uuid::new_v4().to_string());
            let paths = ApplicationPaths {
                data_directory: directory.join("data"),
                cache_directory: directory.join("cache"),
            };
            let mut store = Store::open(&paths).unwrap();
            store.save_games(std::slice::from_ref(&game)).unwrap();
            let metadata = UserMetadata {
                title: Some(catalogue_title.into()),
                description: Some(description.into()),
                developer: Some(developer.into()),
                genres: Some("RPG / Horror fixture".into()),
            };
            store
                .save_cache(
                    &native_catalogue_key(&game, selected).unwrap(),
                    &Some(&metadata),
                    now(),
                )
                .unwrap();
            for version in ["v1", "v2"] {
                store
                    .save_cache(
                        &format!(
                            "steam/catalogue-title-{version}/{}",
                            selected.to_lowercase()
                        ),
                        &None::<String>,
                        now(),
                    )
                    .unwrap();
            }
            let shared = Shared {
                image_client: reqwest::blocking::Client::builder()
                    .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
                    .build()
                    .unwrap(),
                state: Mutex::new(State {
                    games: BTreeMap::from([(game.id.clone(), game.clone())]),
                    snapshot: Snapshot {
                        ready: true,
                        games: Arc::new(vec![game.clone()]),
                        activity: Arc::new(Vec::new()),
                        journal: Arc::new(Vec::new()),
                        jobs: Vec::new(),
                        settings: SettingsSnapshot::default(),
                        paths,
                        initialization_error: None,
                        providers: Vec::new(),
                    },
                }),
                store: Mutex::new(Some(store)),
                subscribers: Mutex::new(Vec::new()),
                cancellations: Mutex::new(BTreeMap::new()),
                credentials: Mutex::new(BTreeMap::new()),
                discovery: Vec::new(),
                artwork: Vec::new(),
                refreshing: AtomicBool::new(false),
            };
            let context = JobContext::new(Arc::new(AtomicBool::new(false)), |_| {});
            shared.populate_catalogue_metadata(&game, &context).unwrap();
            let enriched = shared.game(&game.id).unwrap();
            assert_eq!(enriched.catalogue_metadata.title.as_deref(), Some(selected));
            assert_eq!(
                enriched.effective_metadata().description.as_deref(),
                Some(description)
            );
            assert_eq!(
                enriched.effective_metadata().developer.as_deref(),
                Some(developer)
            );
            assert!(enriched.effective_metadata().genres.is_some());
            let mut unchanged = enriched;
            unchanged.catalogue_metadata = game.catalogue_metadata.clone();
            assert_eq!(
                unchanged, game,
                "Native metadata must not replace manual fields, identity, artwork or launch routes"
            );
            let mut wrong_identity = game.clone();
            wrong_identity.catalogue_metadata.title = Some("Another game: expansion".into());
            assert!(native_catalogue_key(&wrong_identity, "Another game: expansion").is_none());
            // Even a cached, verified installed-product response must not leak
            // into an unrelated re-identification or a different expansion.
            shared
                .store(|store| {
                    store.save_cache(
                        "steam/catalogue-title-v2/another game: expansion",
                        &None::<String>,
                        now(),
                    )
                })
                .unwrap();
            shared
                .update_game(&game.id, |current| {
                    *current = wrong_identity.clone();
                    Ok(())
                })
                .unwrap();
            shared
                .populate_catalogue_metadata(&wrong_identity, &context)
                .unwrap();
            assert_eq!(shared.game(&game.id).unwrap(), wrong_identity);
        }
    }

    #[test]
    fn local_store_discovery_preserves_edits_and_handles_partial_scans() {
        use crate::{EpicProvider, GogProvider, SteamProvider};
        let directory = std::env::temp_dir()
            .join("startpoint-discovery")
            .join(uuid::Uuid::new_v4().to_string());
        let epic_directory = directory.join("Manifests");
        let epic_installation = directory.join("Epic game");
        let gog_directory = directory.join("GOG Games");
        let gog_installation = gog_directory.join("GOG game");
        let steam_directory = directory.join("Steam");
        for path in [
            &epic_directory,
            &epic_installation,
            &gog_installation.join("bin"),
            &steam_directory.join("steamapps/common/Steam game"),
        ] {
            std::fs::create_dir_all(path).expect("installation directory");
        }
        let steam_libraries = steam_directory.join("steamapps/libraryfolders.vdf");
        std::fs::write(
            &steam_libraries,
            format!(
                "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} }}",
                steam_directory.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .expect("Steam libraries");
        std::fs::write(
            steam_directory.join("steamapps/appmanifest_123.acf"),
            r#""AppState" { "appid" "123" "name" "Steam game" "installdir" "Steam game" "StateFlags" "4" }"#,
        )
        .expect("Steam installation record");
        std::fs::write(gog_installation.join("bin/game.exe"), b"").expect("local executable");
        let epic_record = epic_directory.join("game.item");
        let mut epic_document = serde_json::json!({
            "AppName": "ExampleGame", "CatalogNamespace": "namespace", "CatalogItemId": "catalogue",
            "DisplayName": "Epic game", "InstallLocation": epic_installation,
            "AppCategories": ["games"], "bIsIncompleteInstall": false
        });
        std::fs::write(
            &epic_record,
            serde_json::to_vec(&epic_document).expect("Epic record"),
        )
        .expect("Epic manifest");
        let mut ignored = epic_document.clone();
        ignored["AppName"] = "PendingGame".into();
        ignored["bIsIncompleteInstall"] = true.into();
        std::fs::write(
            epic_directory.join("pending.item"),
            serde_json::to_vec(&ignored).expect("incomplete Epic record"),
        )
        .expect("pending manifest");
        ignored["bIsIncompleteInstall"] = false.into();
        ignored["AppCategories"] = serde_json::json!(["addons"]);
        std::fs::write(
            epic_directory.join("addon.item"),
            serde_json::to_vec(&ignored).expect("Epic add-on"),
        )
        .expect("add-on manifest");
        let gog_record = gog_installation.join("goggame-123456.info");
        let gog_document = serde_json::json!({
            "gameId": "123456", "rootGameId": "123456", "name": "GOG game",
            "playTasks": [
                { "type": "FileTask", "path": "modelviewer.exe", "isPrimary": false },
                { "type": "FileTask", "path": "bin\\game.exe", "isPrimary": true, "workingDir": "bin", "arguments": "--profile \"First save\" \"\" --option=\"quoted value\" $UNCHANGED" }
            ]
        });
        let mut bytes = b"\xef\xbb\xbf".to_vec();
        bytes.extend(serde_json::to_vec(&gog_document).expect("GOG record"));
        std::fs::write(&gog_record, bytes).expect("GOG manifest with BOM");
        let mut addon = gog_document.clone();
        addon["gameId"] = "123457".into();
        std::fs::write(
            gog_installation.join("goggame-123457.info"),
            serde_json::to_vec(&addon).expect("GOG add-on"),
        )
        .expect("GOG add-on manifest");
        let settings = DiscoverySettings {
            epic_directory: Some(epic_directory.clone()),
            gog_directory: Some(gog_directory.clone()),
            ..DiscoverySettings::default()
        };
        let cancellation = Arc::new(AtomicBool::new(false));
        let context = JobContext::new(cancellation.clone(), |_| {});
        let epic = EpicProvider
            .discover(&settings, &context)
            .expect("Epic discovery");
        assert!(epic.complete && epic.warnings.is_empty());
        assert_eq!(epic.games.len(), 1);
        assert!(
            matches!(&epic.games[0].launch_target, LaunchTarget::Uri(uri) if uri == "com.epicgames.launcher://apps/namespace%3Acatalogue%3AExampleGame?action=launch&silent=true")
        );
        let gog = GogProvider
            .discover(&settings, &context)
            .expect("GOG discovery");
        assert!(
            gog.complete && gog.warnings.is_empty(),
            "{:?}",
            gog.warnings
        );
        assert_eq!(gog.games.len(), 1);
        let LaunchTarget::Executable(launch) = &gog.games[0].launch_target else {
            panic!("expected primary executable")
        };
        assert_eq!(
            launch.executable,
            gog_installation
                .join("bin/game.exe")
                .canonicalize()
                .expect("executable path")
        );
        assert_eq!(
            launch.working_directory,
            Some(
                gog_installation
                    .join("bin")
                    .canonicalize()
                    .expect("working directory")
            )
        );
        assert_eq!(
            launch.arguments,
            [
                "--profile",
                "First save",
                "",
                "--option=quoted value",
                "$UNCHANGED"
            ]
        );
        let invalid_record = gog_installation.join("goggame-invalid.info");
        let mut invalid = gog_document.clone();
        invalid["playTasks"][1]["path"] = "../outside.exe".into();
        std::fs::write(
            &invalid_record,
            serde_json::to_vec(&invalid).expect("invalid task"),
        )
        .expect("invalid manifest");
        let partial = GogProvider
            .discover(&settings, &context)
            .expect("partial GOG discovery");
        assert!(!partial.complete && !partial.warnings.is_empty() && partial.games.len() == 1);
        std::fs::remove_file(&invalid_record).expect("remove invalid task");
        let oversized = epic_directory.join("oversized.item");
        std::fs::write(&oversized, vec![b' '; 1_000_001]).expect("oversized record");
        assert!(
            !EpicProvider
                .discover(&settings, &context)
                .expect("bounded manifest read")
                .complete
        );
        std::fs::remove_file(oversized).expect("remove oversized record");
        let directory_record = epic_directory.join("directory.item");
        std::fs::create_dir(&directory_record).expect("non-file installation record");
        assert!(
            !EpicProvider
                .discover(&settings, &context)
                .expect("regular-file validation")
                .complete
        );
        std::fs::remove_dir(directory_record).expect("remove non-file record");
        cancellation.store(true, Ordering::Relaxed);
        assert!(matches!(
            EpicProvider.discover(&settings, &context),
            Err(ApiError::Cancelled)
        ));
        assert!(matches!(
            GogProvider.discover(&settings, &context),
            Err(ApiError::Cancelled)
        ));
        cancellation.store(false, Ordering::Relaxed);
        let missing = DiscoverySettings {
            epic_directory: Some(directory.join("missing")),
            ..settings.clone()
        };
        assert!(
            !EpicProvider
                .discover(&missing, &context)
                .expect("missing manifest folder")
                .complete
        );
        #[cfg(target_os = "macos")]
        {
            let resources = gog_directory.join("Native game.app/Contents/Resources/game");
            std::fs::create_dir_all(&resources).expect("bundle resources");
            std::fs::write(
                resources.join("goggame-123458.info"),
                br#"{"gameId":"123458","name":"Native game","playTasks":[]}"#,
            )
            .expect("bundle manifest");
            let native = GogProvider
                .discover(&settings, &context)
                .expect("native bundle discovery");
            assert!(native.complete && native.games.len() == 2);
            assert!(native.games.iter().any(|game| matches!(&game.launch_target, LaunchTarget::ApplicationBundle(launch) if launch.working_directory.is_none())));
            std::fs::remove_file(resources.join("goggame-123458.info"))
                .expect("remove native record");
        }
        let artwork = directory.join("artwork.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([30, 50, 70, 255]))
            .save(&artwork)
            .expect("local artwork");
        let paths = ApplicationPaths {
            data_directory: directory.join("data"),
            cache_directory: directory.join("cache"),
        };
        let store = Store::open(&paths).expect("discovery settings store");
        store
            .save_setting("steam_directory", &Some(steam_directory))
            .expect("isolated Steam directory");
        drop(store);
        let api = LauncherBuilder::new()
            .discovery_provider(SteamProvider)
            .discovery_provider(EpicProvider)
            .discovery_provider(GogProvider)
            .build(paths.clone())
            .expect("store application");
        let events = api.subscribe();
        let game_id = GameId("epic:ExampleGame".into());
        let manual_executable = directory.join("manual.exe");
        std::fs::write(&manual_executable, b"").expect("manual installation");
        for step in 0..19 {
            if step == 9 {
                std::fs::write(&steam_libraries, b"{").expect("unreadable Steam libraries");
                let snapshot = api.snapshot();
                let cover = snapshot
                    .games
                    .iter()
                    .find(|game| game.id == game_id)
                    .and_then(|game| game.artwork.get(&ArtworkKind::Cover))
                    .expect("saved cover");
                std::fs::remove_file(&cover.thumbnail_path).expect("discard disposable thumbnail");
                epic_document["DisplayName"] = "Updated Epic title".into();
                std::fs::write(
                    &epic_record,
                    serde_json::to_vec(&epic_document).expect("updated manifest"),
                )
                .expect("update discovered title");
            }
            if step == 10 {
                std::fs::remove_file(&epic_record).expect("remove game manifest");
                std::fs::remove_file(&gog_record).expect("remove GOG manifest");
                std::fs::write(epic_directory.join("broken.item"), b"{")
                    .expect("malformed Epic sibling");
                std::fs::write(gog_installation.join("goggame-broken.info"), b"{")
                    .expect("malformed GOG sibling");
            }
            if step == 11 {
                assert!(api.snapshot().games.iter().all(|game| game.available));
                std::fs::remove_file(epic_directory.join("broken.item"))
                    .expect("remove malformed Epic record");
                std::fs::remove_file(gog_installation.join("goggame-broken.info"))
                    .expect("remove malformed GOG record");
            }
            if step == 15 {
                std::fs::remove_file(&manual_executable).expect("remove manual installation");
            }
            let previous_library = api.snapshot().games;
            let command = match step {
                0 => Command::SetEpicDirectory {
                    directory: Some(epic_directory.clone()),
                },
                1 => Command::SetGogDirectory {
                    directory: Some(gog_directory.clone()),
                },
                2 | 9..=11 | 15 => Command::RefreshLibrary,
                3 => Command::UpdateMetadata {
                    game_id: game_id.clone(),
                    metadata: UserMetadata {
                        title: Some("Local title".into()),
                        ..UserMetadata::default()
                    },
                },
                4 => Command::SetFavourite {
                    game_id: game_id.clone(),
                    favourite: true,
                },
                5 => Command::SetGameProgress {
                    game_id: game_id.clone(),
                    collections: BTreeSet::from(["Collection".into()]),
                    completed: true,
                },
                6 => Command::SetLaunchOverride {
                    game_id: game_id.clone(),
                    launch: Some(ExecutableLaunch {
                        executable: std::env::current_exe().expect("executable"),
                        arguments: vec!["--list".into()],
                        working_directory: None,
                    }),
                },
                7 => Command::SaveJournalEntry {
                    game_id: game_id.clone(),
                    entry_id: None,
                    text: "Local game note".into(),
                },
                8 => Command::ImportArtwork {
                    game_id: game_id.clone(),
                    kind: ArtworkKind::Cover,
                    path: artwork.clone(),
                },
                12 => Command::SetEpicDirectory {
                    directory: Some(std::path::PathBuf::from("relative")),
                },
                13 => Command::SetGogDirectory {
                    directory: Some(directory.join("missing")),
                },
                14 => Command::AddManualGame {
                    title: "Manual game".into(),
                    launch: ExecutableLaunch {
                        executable: manual_executable.clone(),
                        arguments: Vec::new(),
                        working_directory: None,
                    },
                },
                17 => Command::SetHidden {
                    game_id: game_id.clone(),
                    hidden: true,
                },
                18 => Command::RefreshLibrary,
                _ => Command::RemoveManualGame {
                    game_id: api
                        .snapshot()
                        .games
                        .iter()
                        .find(|game| game.provider_id == "manual")
                        .expect("saved manual game")
                        .id
                        .clone(),
                },
            };
            let job_id = api.submit(command).expect("queued discovery command");
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                assert!(Instant::now() < deadline, "discovery command timed out");
                match events.try_recv() {
                    Ok(AppEvent::JobChanged(job)) if job.id == job_id && !job.state.active() => {
                        assert_eq!(
                            job.state,
                            if !matches!(step, 12..=13) {
                                JobState::Completed
                            } else {
                                JobState::Failed
                            },
                            "{:?}",
                            job.error
                        );
                        if matches!(step, 9..=11 | 15 | 18) {
                            assert!(
                                job.label.contains("Steam discovery failed"),
                                "{}",
                                job.label
                            );
                        }
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
            if step == 9 {
                let snapshot = api.snapshot();
                let game = snapshot
                    .games
                    .iter()
                    .find(|game| game.id == game_id)
                    .expect("updated Epic game after Steam failed");
                assert_eq!(game.discovered_title, "Updated Epic title");
                assert!(game.artwork[&ArtworkKind::Cover].thumbnail_path.is_file());
                assert_eq!(
                    snapshot
                        .games
                        .iter()
                        .find(|game| game.provider_id == "steam"),
                    previous_library
                        .iter()
                        .find(|game| game.provider_id == "steam")
                );
            }
            if step == 15 {
                assert!(
                    !api.snapshot()
                        .games
                        .iter()
                        .find(|game| game.provider_id == "manual")
                        .expect("manual maintenance after Steam failed")
                        .available
                );
            }
        }
        let snapshot = api.snapshot();
        assert_eq!(snapshot.games.len(), 3);
        assert!(
            snapshot
                .games
                .iter()
                .all(|game| game.available == (game.provider_id == "steam"))
        );
        let game = snapshot
            .games
            .iter()
            .find(|game| game.id == game_id)
            .expect("retained Epic game");
        assert_eq!(game.title(), "Local title");
        assert_eq!(game.discovered_title, "Updated Epic title");
        assert!(game.favourite && game.hidden && game.completed && game.launch_override.is_some());
        assert_eq!(game.collections, BTreeSet::from(["Collection".into()]));
        assert!(game.artwork.contains_key(&ArtworkKind::Cover));
        assert_eq!(snapshot.journal[0].text, "Local game note");
        let reopened = LauncherBuilder::new()
            .build(paths)
            .expect("reopened store settings");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !reopened.snapshot().ready {
            assert!(
                Instant::now() < deadline,
                "reopening discovery settings timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            reopened.snapshot().settings.epic_directory,
            Some(epic_directory)
        );
        assert_eq!(
            reopened.snapshot().settings.gog_directory,
            Some(gog_directory)
        );
        assert_eq!(reopened.snapshot().games.len(), 3);
        assert!(
            reopened
                .snapshot()
                .games
                .iter()
                .find(|game| game.id == game_id)
                .unwrap()
                .hidden
        );
    }

    #[test]
    fn settled_recents_defaults_migrate_experiments_without_resetting_other_preferences() {
        let defaults = AppearanceSettings::default();
        assert_eq!(StartScreen::default(), StartScreen::Recents);
        assert_eq!(defaults.start_screen, StartScreen::Recents);
        assert!(defaults.dither_recents && defaults.fade_recents);
        let paths = ApplicationPaths {
            data_directory: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/verification")
                .join(uuid::Uuid::new_v4().to_string()),
            cache_directory: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/verification")
                .join(uuid::Uuid::new_v4().to_string()),
        };
        let store = Store::open(&paths).expect("appearance migration store");
        store
            .save_setting(
                "appearance",
                &serde_json::json!({
                    "theme": "Custom", "mode": "dark", "dither_artwork": false,
                    "start_screen": "library", "dither_recents": false, "fade_recents": false,
                    "recents_activity": true, "recents_progress": false, "recents_journal": true,
                    "sidebar_visible": true, "sidebar_width": 320.,
                    "recents_fade_length": 35., "recents_fade_style": "frosted",
                    "recents_fade_dithering": true, "recents_fade_dither_style": "stepped"
                }),
            )
            .expect("experimental settings");
        drop(store);
        let api = LauncherBuilder::new()
            .build(paths.clone())
            .expect("migrated application");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !api.snapshot().ready {
            assert!(Instant::now() < deadline, "appearance migration timed out");
            assert!(api.snapshot().initialization_error.is_none());
            std::thread::sleep(Duration::from_millis(2));
        }
        let settings = api.snapshot().settings.appearance;
        assert_eq!(settings.start_screen, StartScreen::Recents);
        assert!(settings.dither_recents && settings.fade_recents);
        assert_eq!(settings.theme, "Custom");
        assert_eq!(settings.mode, AppearanceMode::Dark);
        assert!(!settings.dither_artwork);
        assert!(
            settings.recents_activity && !settings.recents_progress && settings.recents_journal
        );
        assert!(settings.sidebar_visible && settings.sidebar_width == 320.);
        let store = Store::open(&paths).expect("cleaned appearance store");
        let document: serde_json::Value = store.setting("appearance").expect("appearance").unwrap();
        assert!(
            !document
                .as_object()
                .unwrap()
                .keys()
                .any(|key| key.starts_with("recents_fade_"))
        );
        let (restored, migrated) =
            restore_appearance_settings(Some(document)).expect("cleaned settings");
        assert!(!migrated);
        assert_eq!(restored, settings);
        let chosen = AppearanceSettings {
            start_screen: StartScreen::Library,
            dither_recents: false,
            fade_recents: false,
            ..settings
        };
        let (restored, migrated) = restore_appearance_settings(Some(
            serde_json::to_value(&chosen).expect("retained preferences"),
        ))
        .expect("subsequent startup");
        assert!(!migrated);
        assert_eq!(
            restored, chosen,
            "retained controls remain customizable after migration"
        );
    }

    #[test]
    fn glass_preferences_migrate_and_round_trip_without_resetting_other_preferences() {
        let old: AppearanceSettings =
            serde_json::from_str(r#"{"theme":"Custom","mode":"dark","sidebar_width":320}"#)
                .unwrap();
        assert!(old.window_translucency);
        assert!(old.backdrop_blur);
        assert_eq!(old.translucency_amount, DEFAULT_TRANSLUCENCY_AMOUNT);
        assert_eq!(old.controls, startpoint_sdk::ControlAppearance::default());
        assert_eq!(
            old.dither_recents_strength,
            startpoint_sdk::DEFAULT_DITHER_STRENGTH
        );
        assert_eq!(
            old.dither_artwork_strength,
            startpoint_sdk::DEFAULT_DITHER_STRENGTH
        );
        for enabled in [false, true] {
            let (combined, changed) = restore_appearance_settings(Some(serde_json::json!({
                "window_translucency": enabled, "theme": "Custom"
            })))
            .unwrap();
            assert!(changed);
            assert_eq!(combined.window_translucency, enabled);
            assert_eq!(combined.backdrop_blur, enabled);
            assert_eq!(combined.translucency_amount, DEFAULT_TRANSLUCENCY_AMOUNT);
            let (migrated, changed) = restore_appearance_settings(Some(serde_json::json!({
                "theme": "Custom", "window_translucency": enabled, "backdrop_blur": !enabled,
                "fade_recents": false, "dither_recents": false, "start_screen": "library"
            })))
            .unwrap();
            assert!(!changed);
            assert_eq!(migrated.window_translucency, enabled);
            assert_eq!(migrated.backdrop_blur, !enabled);
            assert!(!migrated.fade_recents && !migrated.dither_recents);
            assert_eq!(migrated.start_screen, StartScreen::Library);
        }
        for window_translucency in [false, true] {
            for backdrop_blur in [false, true] {
                let chosen = AppearanceSettings {
                    window_translucency,
                    backdrop_blur,
                    translucency_amount: 0.35,
                    dither_recents_strength: 0.07,
                    dither_artwork_strength: 0.3,
                    controls: startpoint_sdk::ControlAppearance {
                        radius: 9.,
                        horizontal_padding: 18.,
                        outline_strength: 0.8,
                        outlines: false,
                        filled_secondary: true,
                        compact_launch: false,
                    },
                    ..old.clone()
                };
                let (restored, _) =
                    restore_appearance_settings(Some(serde_json::to_value(&chosen).unwrap()))
                        .unwrap();
                assert_eq!(restored, chosen);
                assert_eq!(restored.theme, "Custom");
                assert_eq!(restored.sidebar_width, 320.);
            }
        }
        for amount in [-1., 2.] {
            let (restored, changed) = restore_appearance_settings(Some(serde_json::json!({
                "window_translucency": true, "backdrop_blur": false, "translucency_amount": amount
            })))
            .unwrap();
            assert!(changed);
            assert_eq!(restored.translucency_amount, DEFAULT_TRANSLUCENCY_AMOUNT);
        }
        for (radius, padding, strength) in [(-5_f32, 4_f32, -1_f32), (80., 60., 8.)] {
            let (restored, changed) = restore_appearance_settings(Some(serde_json::json!({
                "backdrop_blur": true,
                "controls": {"radius": radius, "horizontal_padding": padding, "outline_strength": strength, "filled_secondary": true}
            }))).unwrap();
            assert!(changed);
            assert!(restored.controls.is_valid());
            assert!(restored.controls.filled_secondary);
            assert_eq!(restored.controls.radius, radius.clamp(0., 12.));
            assert_eq!(restored.controls.horizontal_padding, padding.clamp(8., 20.));
            assert_eq!(restored.controls.outline_strength, strength.clamp(0.25, 1.));
        }
        for (recents, artwork) in [(-0.1, 0.35), (0.08, 1.1)] {
            let (restored, changed) = restore_appearance_settings(Some(serde_json::json!({
                "backdrop_blur": true, "dither_recents": false, "dither_artwork": true,
                "dither_recents_strength": recents, "dither_artwork_strength": artwork
            })))
            .unwrap();
            assert!(changed);
            assert!(!restored.dither_recents && restored.dither_artwork);
            assert_eq!(
                restored.dither_recents_strength,
                if (0. ..=1.).contains(&recents) {
                    recents
                } else {
                    startpoint_sdk::DEFAULT_DITHER_STRENGTH
                }
            );
            assert_eq!(
                restored.dither_artwork_strength,
                if (0. ..=1.).contains(&artwork) {
                    artwork
                } else {
                    startpoint_sdk::DEFAULT_DITHER_STRENGTH
                }
            );
        }
    }

    #[test]
    fn background_pause_defaults_on_and_persists_both_choices() {
        let (old, _) = restore_appearance_settings(Some(serde_json::json!({
            "theme": "Custom", "recents_layout": "gallery", "sidebar_width": 320
        })))
        .unwrap();
        assert!(old.pause_in_background);
        for pause_in_background in [true, false] {
            let chosen = AppearanceSettings {
                pause_in_background,
                ..old.clone()
            };
            let (restored, _) =
                restore_appearance_settings(Some(serde_json::to_value(&chosen).unwrap())).unwrap();
            assert_eq!(restored, chosen);
        }
    }

    #[test]
    fn editorial_recents_defaults_compatibly_and_preserves_carousel_preferences() {
        use startpoint_sdk::RecentsLayout;
        let old: AppearanceSettings = serde_json::from_value(serde_json::json!({
            "theme": "Custom", "mode": "dark", "recents_activity": true,
            "fade_recents": false, "dither_recents": false, "sidebar_visible": true
        }))
        .unwrap();
        assert_eq!(old.recents_layout, RecentsLayout::Carousel);
        for recents_layout in [
            RecentsLayout::Editorial,
            RecentsLayout::Carousel,
            RecentsLayout::Gallery,
            RecentsLayout::Cinematic,
        ] {
            let chosen = AppearanceSettings {
                recents_layout,
                ..old.clone()
            };
            let document = serde_json::to_value(&chosen).unwrap();
            assert_eq!(
                document["recents_layout"],
                match recents_layout {
                    RecentsLayout::Editorial => "editorial",
                    RecentsLayout::Carousel => "carousel",
                    RecentsLayout::Gallery => "gallery",
                    RecentsLayout::Cinematic => "cinematic",
                }
            );
            let (restored, migrated) = restore_appearance_settings(Some(document)).unwrap();
            assert!(!migrated);
            assert_eq!(restored, chosen);
        }
    }

    #[test]
    fn dither_defaults_are_three_pixels_and_twenty_percent_without_overwriting_saved_values() {
        for settings in [
            AppearanceSettings::default(),
            serde_json::from_str("{}").unwrap(),
        ] {
            assert_eq!(settings.dither_recents_size, 3.);
            assert_eq!(settings.dither_artwork_size, 3.);
            assert_eq!(settings.dither_recents_strength, 0.2);
            assert_eq!(settings.dither_artwork_strength, 0.2);
        }
        let (saved, changed) = restore_appearance_settings(Some(serde_json::json!({
            "backdrop_blur": true,
            "dither_recents_size": 2., "dither_artwork_size": 6.,
            "dither_recents_strength": 0.12, "dither_artwork_strength": 0.65
        })))
        .unwrap();
        assert!(!changed);
        assert_eq!(saved.dither_recents_size, 2.);
        assert_eq!(saved.dither_artwork_size, 6.);
        assert_eq!(saved.dither_recents_strength, 0.12);
        assert_eq!(saved.dither_artwork_strength, 0.65);
    }

    #[test]
    fn dither_size_preferences_default_and_round_trip_independently_of_intensity() {
        let old: AppearanceSettings = serde_json::from_value(serde_json::json!({
            "theme": "Custom", "dither_recents_strength": 0.07, "dither_artwork_strength": 0.3
        }))
        .unwrap();
        assert_eq!(old.dither_recents_size, startpoint_sdk::DEFAULT_DITHER_SIZE);
        assert_eq!(old.dither_artwork_size, startpoint_sdk::DEFAULT_DITHER_SIZE);
        for size in 1..=startpoint_sdk::MAX_DITHER_SIZE as u32 {
            let chosen = AppearanceSettings {
                dither_recents_size: size as f32,
                dither_artwork_size: startpoint_sdk::MAX_DITHER_SIZE + 1. - size as f32,
                ..old.clone()
            };
            let (restored, _) =
                restore_appearance_settings(Some(serde_json::to_value(&chosen).unwrap())).unwrap();
            assert_eq!(restored, chosen);
        }
        for (recents, artwork) in [(0., 8.), (6., 13.)] {
            let chosen = AppearanceSettings {
                dither_recents_size: recents,
                dither_artwork_size: artwork,
                ..old.clone()
            };
            let (restored, changed) =
                restore_appearance_settings(Some(serde_json::to_value(&chosen).unwrap())).unwrap();
            assert!(changed);
            let expected = AppearanceSettings {
                dither_recents_size: if recents < 1. {
                    startpoint_sdk::DEFAULT_DITHER_SIZE
                } else {
                    recents
                },
                dither_artwork_size: if artwork > startpoint_sdk::MAX_DITHER_SIZE {
                    startpoint_sdk::DEFAULT_DITHER_SIZE
                } else {
                    artwork
                },
                ..chosen
            };
            assert_eq!(restored, expected);
        }
    }

    #[test]
    fn application_api_preserves_edits_launch_configuration_and_persistence() {
        let previous_settings: AppearanceSettings =
            serde_json::from_str(r#"{"mode":"dark","dither_artwork":true}"#)
                .expect("previous appearance settings");
        assert_eq!(previous_settings.start_screen, StartScreen::Recents);
        assert!(previous_settings.window_translucency);
        assert!(previous_settings.dither_recents && previous_settings.fade_recents);
        assert!(
            !previous_settings.recents_activity
                && !previous_settings.recents_progress
                && !previous_settings.recents_journal
        );
        assert!(!previous_settings.sidebar_visible);
        assert_eq!(previous_settings.sidebar_width, 200.);
        assert_eq!(previous_settings.theme, "Startpoint");
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/verification")
            .join(uuid::Uuid::new_v4().to_string());
        let paths = ApplicationPaths {
            data_directory: directory.join("data"),
            cache_directory: directory.join("cache"),
        };
        std::fs::create_dir_all(&paths.data_directory).expect("legacy data directory");
        let legacy = rusqlite::Connection::open(paths.data_directory.join("library.sqlite3"))
            .expect("legacy database");
        legacy.execute_batch("CREATE TABLE games (id TEXT PRIMARY KEY, document TEXT NOT NULL); CREATE TABLE settings (name TEXT PRIMARY KEY, document TEXT NOT NULL); CREATE TABLE metadata_cache (id TEXT PRIMARY KEY, document TEXT NOT NULL, updated INTEGER NOT NULL); PRAGMA user_version=1;").expect("legacy schema");
        drop(legacy);
        let api = LauncherBuilder::new()
            .build(paths.clone())
            .expect("application");
        let events = api.subscribe();
        let executable = std::env::current_exe().expect("test executable");
        for step in 0..61 {
            let previous_library = api.snapshot().games;
            let game_id = api
                .snapshot()
                .games
                .first()
                .map(|game| game.id.clone())
                .unwrap_or(GameId(String::new()));
            let command = match step {
                0 => Command::AddManualGame {
                    title: "Original title".into(),
                    launch: ExecutableLaunch {
                        executable: executable.clone(),
                        arguments: vec!["--list".into()],
                        working_directory: None,
                    },
                },
                1 => Command::UpdateMetadata {
                    game_id,
                    metadata: UserMetadata {
                        title: Some("Edited title".into()),
                        description: Some("Local description".into()),
                        ..UserMetadata::default()
                    },
                },
                2 | 23 => Command::SetFavourite {
                    game_id,
                    favourite: true,
                },
                3 => Command::SetLaunchOverride {
                    game_id,
                    launch: Some(ExecutableLaunch {
                        executable: executable.clone(),
                        arguments: vec!["--list".into(), "--format=terse".into()],
                        working_directory: None,
                    }),
                },
                4 => Command::RefreshLibrary,
                5 => Command::SetAppearance {
                    settings: AppearanceSettings {
                        theme: "Custom".into(),
                        mode: AppearanceMode::Light,
                        window_translucency: false,
                        backdrop_blur: true,
                        translucency_amount: 0.35,
                        dither_artwork: false,
                        dither_artwork_strength: 0.35,
                        dither_artwork_size: 3.,
                        start_screen: StartScreen::Recents,
                        recents_layout: startpoint_sdk::RecentsLayout::Editorial,
                        pause_in_background: false,
                        dither_recents: true,
                        dither_recents_strength: 0.08,
                        dither_recents_size: 8.,
                        fade_recents: true,
                        recents_activity: true,
                        recents_progress: true,
                        recents_journal: true,
                        sidebar_visible: true,
                        sidebar_width: 320.,
                        library_layout: LibraryLayout::Covers,
                        library_density: LibraryDensity::Large,
                        library_details: false,
                        library_last_played: false,
                        library_sources: true,
                        library_favourite_badges: false,
                        controls: Default::default(),
                    },
                },
                6 => Command::LaunchGame { game_id },
                7..=10 => Command::SetAppearance {
                    settings: AppearanceSettings {
                        sidebar_width: [175., 401., f32::NAN, f32::INFINITY][step - 7],
                        ..AppearanceSettings::default()
                    },
                },
                11..=14 => Command::SetAppearance {
                    settings: AppearanceSettings {
                        theme: [
                            String::new(),
                            " ".into(),
                            "Line\nbreak".into(),
                            "x".repeat(81),
                        ][step - 11]
                            .clone(),
                        ..AppearanceSettings::default()
                    },
                },
                29..=32 => Command::SetAppearance {
                    settings: AppearanceSettings {
                        translucency_amount: [-0.1, 1.1, f32::NAN, f32::INFINITY][step - 29],
                        ..AppearanceSettings::default()
                    },
                },
                33..=44 => {
                    let mut settings = AppearanceSettings::default();
                    match step {
                        33..=36 => {
                            settings.controls.radius =
                                [-1., 13., f32::NAN, f32::INFINITY][step - 33]
                        }
                        37..=40 => {
                            settings.controls.horizontal_padding =
                                [7., 21., f32::NAN, f32::INFINITY][step - 37]
                        }
                        _ => {
                            settings.controls.outline_strength =
                                [0.24, 1.01, f32::NAN, f32::INFINITY][step - 41]
                        }
                    }
                    Command::SetAppearance { settings }
                }
                45..=52 => {
                    let mut settings = AppearanceSettings::default();
                    let value = [-0.1, 1.1, f32::NAN, f32::INFINITY][(step - 45) % 4];
                    if step < 49 {
                        settings.dither_recents_strength = value;
                    } else {
                        settings.dither_artwork_strength = value;
                    }
                    Command::SetAppearance { settings }
                }
                53..=60 => {
                    let mut settings = AppearanceSettings::default();
                    let value = [0., 13., f32::NAN, f32::INFINITY][(step - 53) % 4];
                    if step < 57 {
                        settings.dither_recents_size = value;
                    } else {
                        settings.dither_artwork_size = value;
                    }
                    Command::SetAppearance { settings }
                }
                24 => Command::SetHidden {
                    game_id,
                    hidden: true,
                },
                25 => Command::RefreshLibrary,
                26 => Command::SetHidden {
                    game_id,
                    hidden: false,
                },
                27 => Command::ReidentifyGame {
                    game_id,
                    matched: ArtworkGame {
                        id: "987".into(),
                        name: "Edited title".into(),
                    },
                },
                28 => Command::ReidentifyGame {
                    game_id,
                    matched: ArtworkGame {
                        id: "invalid".into(),
                        name: "Invalid match".into(),
                    },
                },
                15 => Command::SetGameProgress {
                    game_id,
                    collections: BTreeSet::from([
                        "Role playing".into(),
                        "  Completed games  ".into(),
                    ]),
                    completed: true,
                },
                16 | 22 => Command::SaveJournalEntry {
                    game_id,
                    entry_id: None,
                    text: "  Finished the first chapter.  ".into(),
                },
                17..=19 => Command::SaveJournalEntry {
                    game_id: if step == 19 {
                        GameId("missing:game".into())
                    } else {
                        game_id
                    },
                    entry_id: api.snapshot().journal.first().map(|entry| entry.id.clone()),
                    text: if step == 18 {
                        " ".into()
                    } else {
                        "Updated journal note.".into()
                    },
                },
                20 => Command::SetGameProgress {
                    game_id,
                    collections: BTreeSet::from(["Invalid\ncollection".into()]),
                    completed: false,
                },
                _ => Command::DeleteJournalEntry {
                    game_id,
                    entry_id: api
                        .snapshot()
                        .journal
                        .first()
                        .expect("saved journal note")
                        .id
                        .clone(),
                },
            };
            let job_id = api.submit(command).expect("queued command");
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                assert!(Instant::now() < deadline, "application command timed out");
                match events.try_recv() {
                    Ok(AppEvent::JobChanged(job)) if job.id == job_id && !job.state.active() => {
                        assert_eq!(
                            job.state,
                            if step < 7 || matches!(step, 15..=17 | 21..=27) {
                                JobState::Completed
                            } else {
                                JobState::Failed
                            },
                            "{:?}",
                            job.error
                        );
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
            if matches!(step, 4 | 23) {
                assert!(Arc::ptr_eq(&previous_library, &api.snapshot().games));
            }
            if step == 1 {
                assert_eq!(previous_library[0].title(), "Original title");
                assert_eq!(api.snapshot().games[0].title(), "Edited title");
            }
            if matches!(step, 24..=25) {
                assert!(api.snapshot().games[0].hidden);
            }
        }
        let saved = api.snapshot().games.first().cloned().expect("saved game");
        assert_eq!(saved.title(), "Edited title");
        assert_eq!(saved.discovered_title, "Original title");
        assert!(!saved.hidden);
        assert_eq!(
            saved
                .artwork_game_ids
                .get("steamgriddb")
                .map(String::as_str),
            Some("987")
        );
        let mut corrected = saved.clone();
        let uploaded = ArtworkAsset {
            id: "user-cover".into(),
            kind: ArtworkKind::Cover,
            image_path: "cover.png".into(),
            thumbnail_path: "thumb.png".into(),
            provider_id: "local".into(),
            source_url: None,
            author: "You".into(),
        };
        let wrong = ArtworkAsset {
            id: "wrong-background".into(),
            kind: ArtworkKind::Background,
            provider_id: "steamgriddb".into(),
            ..uploaded.clone()
        };
        corrected
            .artwork
            .insert(ArtworkKind::Cover, uploaded.clone());
        corrected
            .artwork
            .insert(ArtworkKind::Background, wrong.clone());
        corrected.artwork_history = vec![uploaded.clone(), wrong];
        let history = corrected.artwork_history.clone();
        apply_reidentification(
            &mut corrected,
            &ArtworkGame {
                id: "456".into(),
                name: "Metro Exodus Enhanced Edition".into(),
            },
        );
        assert_eq!(corrected.title(), "Metro Exodus Enhanced Edition");
        assert_eq!(corrected.artwork.get(&ArtworkKind::Cover), Some(&uploaded));
        assert!(!corrected.artwork.contains_key(&ArtworkKind::Background));
        assert_eq!(corrected.artwork_history, history);
        assert_eq!(corrected.default_launch, saved.default_launch);
        assert_eq!(corrected.launch_override, saved.launch_override);
        assert_eq!(corrected.metadata.description, saved.metadata.description);
        assert_eq!(corrected.last_launched, saved.last_launched);
        for kind in [
            ArtworkKind::Cover,
            ArtworkKind::Background,
            ArtworkKind::Logo,
            ArtworkKind::Icon,
        ] {
            let key = artwork_cache_key("steamgriddb", &corrected, None, kind);
            assert_eq!(
                key,
                artwork_cache_key("steamgriddb", &corrected, Some("456"), kind)
            );
            assert_ne!(key, artwork_cache_key("steamgriddb", &saved, None, kind));
        }
        let mut enriched = saved.clone();
        enriched.id = GameId("manual:enriched".into());
        enriched.metadata.title = Some("A first game".into());
        let library = updated_library(std::slice::from_ref(&saved), &enriched);
        assert_eq!(library[0].id, enriched.id);
        assert_eq!(library[1], saved);
        enriched.metadata.title = Some("Z last game".into());
        let renamed = updated_library(&library, &enriched);
        assert_eq!(renamed[0], saved);
        assert_eq!(renamed[1].id, enriched.id);
        assert_eq!(library[0].title(), "A first game");
        enriched.metadata = saved.metadata.clone();
        enriched.catalogue_metadata = UserMetadata {
            title: Some("Catalogue title".into()),
            description: Some("Catalogue description".into()),
            developer: Some("Catalogue developer".into()),
            genres: Some("Adventure".into()),
        };
        assert_eq!(enriched.title(), "Edited title");
        assert_eq!(
            enriched.effective_metadata().description.as_deref(),
            Some("Local description")
        );
        assert_eq!(
            enriched.effective_metadata().developer.as_deref(),
            Some("Catalogue developer")
        );
        let mut previous_document = serde_json::to_value(&saved).expect("game document");
        previous_document
            .as_object_mut()
            .expect("game object")
            .remove("catalogue_metadata");
        for field in ["collections", "completed", "hidden"] {
            previous_document
                .as_object_mut()
                .expect("game object")
                .remove(field);
        }
        let previous_game: Game =
            serde_json::from_value(previous_document).expect("previous game document");
        assert_eq!(previous_game.title(), "Edited title");
        assert!(previous_game.catalogue_metadata.developer.is_none());
        assert!(previous_game.collections.is_empty() && !previous_game.completed);
        assert!(!previous_game.hidden);
        assert!(saved.favourite && saved.available && saved.last_launched.is_some());
        assert!(saved.completed);
        assert_eq!(
            saved.collections,
            BTreeSet::from(["Role playing".into(), "Completed games".into()])
        );
        assert_eq!(
            api.snapshot()
                .activity
                .iter()
                .map(|day| day.launches)
                .sum::<u32>(),
            1
        );
        assert_eq!(api.snapshot().journal.len(), 1);
        assert_eq!(
            api.snapshot().journal[0].text,
            "Finished the first chapter."
        );
        assert_eq!(
            saved
                .launch_override
                .as_ref()
                .expect("launch override")
                .arguments,
            ["--list", "--format=terse"]
        );
        drop(api);
        let reopened = LauncherBuilder::new()
            .build(paths.clone())
            .expect("reopened application");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !reopened.snapshot().ready {
            assert!(Instant::now() < deadline, "reopening the library timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        let loaded = reopened
            .snapshot()
            .games
            .first()
            .cloned()
            .expect("persisted game");
        assert_eq!(
            loaded
                .artwork_game_ids
                .get("steamgriddb")
                .map(String::as_str),
            Some("987")
        );
        assert_eq!(loaded.id, saved.id);
        assert_eq!(loaded.title(), "Edited title");
        assert_eq!(
            loaded.metadata.description.as_deref(),
            Some("Local description")
        );
        assert!(loaded.favourite && loaded.launch_override.is_some());
        assert!(loaded.completed && loaded.collections == saved.collections);
        assert_eq!(
            reopened
                .snapshot()
                .activity
                .iter()
                .map(|day| day.launches)
                .sum::<u32>(),
            1
        );
        assert_eq!(
            reopened.snapshot().journal[0].text,
            "Finished the first chapter."
        );
        assert_eq!(
            reopened.snapshot().settings.appearance,
            AppearanceSettings {
                theme: "Custom".into(),
                mode: AppearanceMode::Light,
                window_translucency: false,
                backdrop_blur: true,
                translucency_amount: 0.35,
                dither_artwork: false,
                dither_artwork_strength: 0.35,
                dither_artwork_size: 3.,
                start_screen: StartScreen::Recents,
                recents_layout: startpoint_sdk::RecentsLayout::Editorial,
                pause_in_background: false,
                dither_recents: true,
                dither_recents_strength: 0.08,
                dither_recents_size: 8.,
                fade_recents: true,
                recents_activity: true,
                recents_progress: true,
                recents_journal: true,
                sidebar_visible: true,
                sidebar_width: 320.,
                library_layout: LibraryLayout::Covers,
                library_density: LibraryDensity::Large,
                library_details: false,
                library_last_played: false,
                library_sources: true,
                library_favourite_badges: false,
                controls: Default::default(),
            }
        );
        let mut store = Store::open(&paths).expect("migrated store");
        let mut other = saved.clone();
        other.id = GameId("manual:other".into());
        store
            .save_games(std::slice::from_ref(&other))
            .expect("another game");
        let note = store.journal().expect("journal")[0].clone();
        let invalid_owner = JournalEntry {
            game_id: other.id.clone(),
            text: "Should not replace the note".into(),
            ..note.clone()
        };
        assert!(matches!(
            store.save_journal_entry(&invalid_owner, true),
            Err(ApiError::NotFound)
        ));
        assert!(matches!(
            store.delete_journal_entry(&other.id, &note.id),
            Err(ApiError::NotFound)
        ));
        assert_eq!(store.journal().expect("preserved note")[0], note);
        other.last_launched = Some(0);
        store.record_launch(&other, 0).expect("epoch launch");
        other.last_launched = Some(86_400);
        store
            .record_launch(&other, 86_400)
            .expect("following day launch");
        let activity = store.activity().expect("grouped activity");
        assert_eq!(activity.iter().map(|day| day.launches).sum::<u32>(), 3);
        assert_eq!(
            activity
                .iter()
                .flat_map(|day| day.games.values())
                .sum::<u32>(),
            3
        );
        store.remove_game(&saved.id.0).expect("remove manual game");
        assert!(store.journal().expect("cascade journal removal").is_empty());
        assert_eq!(
            store
                .activity()
                .expect("cascade launch removal")
                .iter()
                .map(|day| day.launches)
                .sum::<u32>(),
            2
        );
        store.remove_game(&other.id.0).expect("remove second game");
        assert!(store.activity().expect("removed activity").is_empty());
        assert!(matches!(
            store.record_launch(&other, now()),
            Err(ApiError::NotFound)
        ));
        assert!(
            store
                .activity()
                .expect("failed launch not recorded")
                .is_empty()
        );
    }
}
