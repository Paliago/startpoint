use crate::{artwork, platform, steam, storage::Store};
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
        let games = store.games()?;
        let steam_directory = store.setting("steam_directory")?.flatten();
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
        let mut appearance = store
            .setting("appearance")?
            .unwrap_or_else(|| AppearanceSettings {
                start_screen: if self
                    .discovery
                    .iter()
                    .any(|provider| provider.descriptor().id == "example")
                {
                    StartScreen::Recents
                } else {
                    StartScreen::Library
                },
                ..AppearanceSettings::default()
            });
        if !appearance.sidebar_width.is_finite() {
            appearance.sidebar_width = AppearanceSettings::default().sidebar_width;
        }
        appearance.sidebar_width = appearance.sidebar_width.clamp(192., 400.);
        *self
            .store
            .lock()
            .map_err(|_| ApiError::Storage("The database worker failed".into()))? = Some(store);
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.games = games
            .into_iter()
            .map(|game| (game.id.clone(), game))
            .collect();
        state.snapshot.settings = SettingsSnapshot {
            steam_directory,
            configured_credentials,
            credential_status,
            appearance,
        };
        state.snapshot.ready = true;
        rebuild_library(&mut state);
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
        self.store(|store| {
            let mut game = self.game(id)?;
            update(&mut game)?;
            store.save_games(std::slice::from_ref(&game))?;
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.games.insert(id.clone(), game);
            rebuild_library(&mut state);
            Ok(())
        })?;
        self.publish(AppEvent::LibraryChanged);
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
                    metadata: UserMetadata::default(),
                    catalogue_metadata: UserMetadata::default(),
                    favourite: false,
                    available: true,
                    last_launched: None,
                    artwork: BTreeMap::new(),
                    artwork_history: Vec::new(),
                    artwork_game_ids: BTreeMap::new(),
                };
                self.store(|store| {
                    store.save_games(std::slice::from_ref(&game))?;
                    let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                    state.games.insert(id.clone(), game);
                    rebuild_library(&mut state);
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
                    let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                    state.games.remove(&game_id);
                    rebuild_library(&mut state);
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
            Command::SetLaunchOverride { game_id, launch } => {
                if let Some(launch) = &launch {
                    platform::validate_launch(launch)?;
                }
                self.update_game(&game_id, |game| {
                    game.launch_override = launch;
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
                    return self.update_game(&game_id, |game| {
                        game.last_launched = Some(now());
                        Ok(())
                    });
                }
                if !game.available && game.launch_override.is_none() {
                    return Err(ApiError::Validation(
                        "This installation is currently unavailable".into(),
                    ));
                }
                let schemes = self
                    .discovery
                    .iter()
                    .find(|provider| provider.descriptor().id == game.provider_id)
                    .map(|provider| provider.uri_schemes())
                    .unwrap_or_default();
                platform::launch(game.effective_launch(), &schemes)?;
                self.update_game(&game_id, |game| {
                    game.last_launched = Some(now());
                    Ok(())
                })
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
                self.publish(AppEvent::SettingsChanged);
                Ok(())
            }
            Command::SetAppearance { settings } => {
                if !settings.sidebar_width.is_finite()
                    || !(192. ..=400.).contains(&settings.sidebar_width)
                {
                    return Err(ApiError::Validation(
                        "Sidebar width must be between 192 and 400 pixels".into(),
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
            } => {
                let game = self.game(&game_id)?;
                let mut images = self.artwork_choices(
                    &provider_id,
                    &game,
                    remote_game_id.as_deref(),
                    kind,
                    context,
                )?;
                let hosts = self.artwork_provider(&provider_id)?.image_hosts();
                let key = artwork_cache_key(&provider_id, &game, remote_game_id.as_deref(), kind);
                self.publish(AppEvent::ArtworkFound {
                    job_id,
                    game_id: game_id.clone(),
                    kind,
                    images: images.clone(),
                });
                for index in 0..images.len() {
                    let image = &mut images[index];
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
                    self.store(|store| store.save_cache(&key, &images, now()))?;
                    self.publish(AppEvent::ArtworkFound {
                        job_id,
                        game_id: game_id.clone(),
                        kind,
                        images: images.clone(),
                    });
                }
                context.checkpoint()?;
                self.store(|store| store.save_cache(&key, &images, now()))?;
                self.publish(AppEvent::ArtworkFound {
                    job_id,
                    game_id,
                    kind,
                    images,
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
        let key = artwork_cache_key(provider_id, game, remote_id, kind);
        if let Some(images) = self.store(|store| store.cached(&key, now()))? {
            return Ok(images);
        }
        let credential = self.credential(provider_id, false)?;
        let images = match self.provider_result(
            provider_id,
            &credential,
            self.artwork_provider(provider_id)?.artwork(
                game,
                remote_id,
                kind,
                &credential,
                context,
            ),
        ) {
            Ok(images) => images,
            Err(ApiError::NoArtwork) => Vec::new(),
            Err(error) => return Err(error),
        };
        context.checkpoint()?;
        self.store(|store| store.save_cache(&key, &images, now()))?;
        Ok(images)
    }

    fn refresh(&self, context: &JobContext) -> Result<(), ApiError> {
        if self.refreshing.swap(true, Ordering::Relaxed) {
            return Err(ApiError::Validation(
                "A library refresh is already running".into(),
            ));
        }
        let _refresh = RefreshGuard(&self.refreshing);
        let settings = DiscoverySettings {
            steam_directory: self.snapshot().settings.steam_directory,
        };
        let mut local_covers = Vec::new();
        let mut warnings = Vec::new();
        for provider in &self.discovery {
            context.checkpoint()?;
            let descriptor = provider.descriptor();
            let report = provider.discover(&settings, context)?;
            warnings.extend(report.warnings);
            context.checkpoint()?;
            self.store(|store| {
                let mut games = self
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .games
                    .clone();
                if report.complete {
                    for game in games
                        .values_mut()
                        .filter(|game| game.provider_id == descriptor.id)
                    {
                        game.available = false;
                    }
                }
                for (index, discovered) in report.games.into_iter().enumerate() {
                    if discovered.source_game_id.is_empty() || discovered.source_game_id.len() > 512
                    {
                        return Err(ApiError::Validation(
                            "A discovery provider returned an invalid game identifier".into(),
                        ));
                    }
                    let id = GameId(format!("{}:{}", descriptor.id, discovered.source_game_id));
                    if let Some(path) = discovered.local_cover {
                        local_covers.push((id.clone(), path));
                    }
                    if let Some(game) = games.get_mut(&id) {
                        game.discovered_title = discovered.title;
                        game.install_directory = discovered.install_directory;
                        game.default_launch = discovered.launch_target;
                        game.available = true;
                    } else {
                        games.insert(
                            id.clone(),
                            Game {
                                id,
                                provider_id: descriptor.id.clone(),
                                source_game_id: discovered.source_game_id,
                                discovered_title: discovered.title,
                                install_directory: discovered.install_directory,
                                default_launch: discovered.launch_target,
                                launch_override: None,
                                metadata: UserMetadata::default(),
                                catalogue_metadata: UserMetadata::default(),
                                favourite: false,
                                available: true,
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
                            },
                        );
                    }
                }
                store.save_games(&games.values().cloned().collect::<Vec<_>>())?;
                let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                state.games = games;
                rebuild_library(&mut state);
                Ok(())
            })?;
            self.publish(AppEvent::LibraryChanged);
        }
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
        if let Some(warning) = warnings.first() {
            context.report(warning.clone());
        } else {
            context.report("Installed games are up to date");
        }
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
            .filter(|game| game.available)
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
        if matches!(game.provider_id.as_str(), "steam" | "example")
            && (game.catalogue_metadata.title.is_none()
                || game.catalogue_metadata.description.is_none()
                || game.catalogue_metadata.developer.is_none()
                || game.catalogue_metadata.genres.is_none())
        {
            let key = format!("steam/catalogue/{}", game.source_game_id);
            let metadata = match self.store(|store| store.cached(&key, now()))? {
                Some(metadata) => metadata,
                None => match steam::metadata(&self.image_client, &game.source_game_id, context) {
                    Ok(metadata) => {
                        self.store(|store| store.save_cache(&key, &metadata, now()))?;
                        metadata
                    }
                    Err(ApiError::NoArtwork) => game.catalogue_metadata.clone(),
                    Err(error @ ApiError::Cancelled) => return Err(error),
                    Err(error) => {
                        catalogue_error = Some(error);
                        game.catalogue_metadata.clone()
                    }
                },
            };
            context.checkpoint()?;
            self.update_game(game_id, |game| {
                game.catalogue_metadata = metadata;
                Ok(())
            })?;
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
}

struct RefreshGuard<'a>(&'a AtomicBool);
impl Drop for RefreshGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

fn rebuild_library(state: &mut State) {
    let mut games: Vec<_> = state.games.values().cloned().collect();
    games.sort_by_cached_key(|game| game.title().to_lowercase());
    state.snapshot.games = Arc::new(games);
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

fn artwork_cache_key(
    provider_id: &str,
    game: &Game,
    remote_id: Option<&str>,
    kind: ArtworkKind,
) -> String {
    format!(
        "{provider_id}/images-v3/{}/{kind:?}/{}",
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

    #[test]
    fn application_api_preserves_edits_launch_configuration_and_persistence() {
        let previous_settings: AppearanceSettings =
            serde_json::from_str(r#"{"mode":"dark","dither_artwork":true}"#)
                .expect("previous appearance settings");
        assert_eq!(previous_settings.start_screen, StartScreen::Library);
        assert!(!previous_settings.dither_recents);
        assert!(!previous_settings.sidebar_visible);
        assert_eq!(previous_settings.sidebar_width, 216.);
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/verification")
            .join(uuid::Uuid::new_v4().to_string());
        let paths = ApplicationPaths {
            data_directory: directory.join("data"),
            cache_directory: directory.join("cache"),
        };
        let api = LauncherBuilder::new()
            .build(paths.clone())
            .expect("application");
        let events = api.subscribe();
        let executable = std::env::current_exe().expect("test executable");
        for step in 0..11 {
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
                2 => Command::SetFavourite {
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
                        mode: AppearanceMode::Light,
                        dither_artwork: false,
                        start_screen: StartScreen::Recents,
                        dither_recents: true,
                        sidebar_visible: true,
                        sidebar_width: 320.,
                    },
                },
                6 => Command::LaunchGame { game_id },
                _ => Command::SetAppearance {
                    settings: AppearanceSettings {
                        sidebar_width: [191., 401., f32::NAN, f32::INFINITY][step - 7],
                        ..AppearanceSettings::default()
                    },
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
                            if step < 7 {
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
        }
        let saved = api.snapshot().games.first().cloned().expect("saved game");
        assert_eq!(saved.title(), "Edited title");
        assert_eq!(saved.discovered_title, "Original title");
        let mut enriched = saved.clone();
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
        let previous_game: Game =
            serde_json::from_value(previous_document).expect("previous game document");
        assert_eq!(previous_game.title(), "Edited title");
        assert!(previous_game.catalogue_metadata.developer.is_none());
        assert!(saved.favourite && saved.available && saved.last_launched.is_some());
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
            .build(paths)
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
        assert_eq!(loaded.id, saved.id);
        assert_eq!(loaded.title(), "Edited title");
        assert_eq!(
            loaded.metadata.description.as_deref(),
            Some("Local description")
        );
        assert!(loaded.favourite && loaded.launch_override.is_some());
        assert_eq!(
            reopened.snapshot().settings.appearance,
            AppearanceSettings {
                mode: AppearanceMode::Light,
                dither_artwork: false,
                start_screen: StartScreen::Recents,
                dither_recents: true,
                sidebar_visible: true,
                sidebar_width: 320.,
            }
        );
    }
}
