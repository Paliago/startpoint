use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};

pub type JobId = u64;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GameId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutableLaunch {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaunchTarget {
    Uri(String),
    Executable(ExecutableLaunch),
    ApplicationBundle(ExecutableLaunch),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub developer: Option<String>,
    pub genres: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ArtworkKind {
    Cover,
    Background,
    Logo,
    Icon,
}

impl ArtworkKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cover => "Cover",
            Self::Background => "Background",
            Self::Logo => "Logo",
            Self::Icon => "Icon",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkAsset {
    pub id: String,
    pub kind: ArtworkKind,
    pub image_path: PathBuf,
    pub thumbnail_path: PathBuf,
    pub provider_id: String,
    pub source_url: Option<String>,
    pub author: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveredGame {
    pub source_game_id: String,
    pub title: String,
    pub install_directory: PathBuf,
    pub launch_target: LaunchTarget,
    pub local_cover: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub id: GameId,
    pub provider_id: String,
    pub source_game_id: String,
    pub discovered_title: String,
    pub install_directory: PathBuf,
    pub default_launch: LaunchTarget,
    pub launch_override: Option<ExecutableLaunch>,
    pub metadata: UserMetadata,
    #[serde(default)]
    pub catalogue_metadata: UserMetadata,
    pub favourite: bool,
    #[serde(default)]
    pub hidden: bool,
    pub available: bool,
    pub last_launched: Option<u64>,
    #[serde(default)]
    pub collections: BTreeSet<String>,
    #[serde(default)]
    pub completed: bool,
    pub artwork: BTreeMap<ArtworkKind, ArtworkAsset>,
    pub artwork_history: Vec<ArtworkAsset>,
    pub artwork_game_ids: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityDay {
    pub date: String,
    pub launches: u32,
    pub games: BTreeMap<GameId, u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub id: String,
    pub game_id: GameId,
    pub created_at: u64,
    pub text: String,
}

impl Game {
    pub fn title(&self) -> &str {
        self.metadata
            .title
            .as_deref()
            .or(self.catalogue_metadata.title.as_deref())
            .unwrap_or(&self.discovered_title)
    }

    pub fn effective_metadata(&self) -> UserMetadata {
        UserMetadata {
            title: self
                .metadata
                .title
                .clone()
                .or_else(|| self.catalogue_metadata.title.clone()),
            description: self
                .metadata
                .description
                .clone()
                .or_else(|| self.catalogue_metadata.description.clone()),
            developer: self
                .metadata
                .developer
                .clone()
                .or_else(|| self.catalogue_metadata.developer.clone()),
            genres: self
                .metadata
                .genres
                .clone()
                .or_else(|| self.catalogue_metadata.genres.clone()),
        }
    }

    pub fn effective_launch(&self) -> LaunchTarget {
        match &self.launch_override {
            Some(launch)
                if launch
                    .executable
                    .extension()
                    .is_some_and(|value| value == "app") =>
            {
                LaunchTarget::ApplicationBundle(launch.clone())
            }
            Some(launch) => LaunchTarget::Executable(launch.clone()),
            None => self.default_launch.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtworkGame {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtworkCandidate {
    pub id: String,
    pub provider_id: String,
    pub kind: ArtworkKind,
    pub image_url: String,
    pub thumbnail_url: String,
    pub author: String,
    pub preview_path: Option<PathBuf>,
    #[serde(default)]
    pub score: i32,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppearanceMode {
    #[default]
    System,
    Light,
    Dark,
}

impl AppearanceMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartScreen {
    Library,
    #[default]
    Recents,
    Activity,
}

impl StartScreen {
    pub fn label(self) -> &'static str {
        match self {
            Self::Library => "Library",
            Self::Recents => "Recents",
            Self::Activity => "Activity",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryLayout {
    Pills,
    #[default]
    Covers,
    Tiles,
    Compact,
}

impl LibraryLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pills => "Pills",
            Self::Covers => "Cover grid",
            Self::Tiles => "Landscape tiles",
            Self::Compact => "Compact list",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryDensity {
    Small,
    #[default]
    Medium,
    Large,
}

impl LibraryDensity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceSettings {
    pub theme: String,
    pub mode: AppearanceMode,
    pub dither_artwork: bool,
    pub start_screen: StartScreen,
    pub dither_recents: bool,
    pub fade_recents: bool,
    pub recents_activity: bool,
    pub recents_progress: bool,
    pub recents_journal: bool,
    pub sidebar_visible: bool,
    pub sidebar_width: f32,
    pub library_layout: LibraryLayout,
    pub library_density: LibraryDensity,
    pub library_details: bool,
    pub library_last_played: bool,
    pub library_sources: bool,
    pub library_favourite_badges: bool,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: "Startpoint".into(),
            mode: AppearanceMode::System,
            dither_artwork: true,
            start_screen: StartScreen::Recents,
            dither_recents: true,
            fade_recents: true,
            recents_activity: false,
            recents_progress: false,
            recents_journal: false,
            sidebar_visible: false,
            sidebar_width: 200.,
            library_layout: LibraryLayout::Covers,
            library_density: LibraryDensity::Medium,
            library_details: true,
            library_last_played: true,
            library_sources: false,
            library_favourite_badges: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SettingsSnapshot {
    pub configured_credentials: BTreeSet<String>,
    pub credential_status: BTreeMap<String, CredentialStatus>,
    pub steam_directory: Option<PathBuf>,
    pub epic_directory: Option<PathBuf>,
    pub gog_directory: Option<PathBuf>,
    pub appearance: AppearanceSettings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CredentialStatus {
    Missing,
    Stored,
    Checking,
    Verified,
    Invalid,
    Locked,
    Unavailable(String),
}

impl CredentialStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Missing => "No API key supplied",
            Self::Stored => "API key saved · not checked yet",
            Self::Checking => "API key saved · checking connection…",
            Self::Verified => "Connected · API key saved and working",
            Self::Invalid => "API key saved · rejected by SteamGridDB",
            Self::Locked => "API key saved · access approval required",
            Self::Unavailable(_) => "Credential verification unavailable",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ApplicationPaths {
    pub data_directory: PathBuf,
    pub cache_directory: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Clone, Debug)]
pub struct JobStatus {
    pub id: JobId,
    pub label: String,
    pub state: JobState,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub ready: bool,
    pub games: Arc<Vec<Game>>,
    pub activity: Arc<Vec<ActivityDay>>,
    pub journal: Arc<Vec<JournalEntry>>,
    pub jobs: Vec<JobStatus>,
    pub settings: SettingsSnapshot,
    pub paths: ApplicationPaths,
    pub initialization_error: Option<String>,
    pub providers: Vec<crate::ProviderDescriptor>,
}

/// Arguments are individual values, not a shell command. Credentials are never logged.
#[derive(Clone)]
pub enum Command {
    RefreshLibrary,
    AddManualGame {
        title: String,
        launch: ExecutableLaunch,
    },
    RemoveManualGame {
        game_id: GameId,
    },
    UpdateMetadata {
        game_id: GameId,
        metadata: UserMetadata,
    },
    SetFavourite {
        game_id: GameId,
        favourite: bool,
    },
    SetHidden {
        game_id: GameId,
        hidden: bool,
    },
    ReidentifyGame {
        game_id: GameId,
        matched: ArtworkGame,
    },
    SetGameProgress {
        game_id: GameId,
        collections: BTreeSet<String>,
        completed: bool,
    },
    SaveJournalEntry {
        game_id: GameId,
        entry_id: Option<String>,
        text: String,
    },
    DeleteJournalEntry {
        game_id: GameId,
        entry_id: String,
    },
    SetLaunchOverride {
        game_id: GameId,
        launch: Option<ExecutableLaunch>,
    },
    LaunchGame {
        game_id: GameId,
    },
    SetArtworkCredential {
        provider_id: String,
        credential: Option<String>,
    },
    VerifyArtworkCredential {
        provider_id: String,
    },
    SetArtworkGame {
        provider_id: String,
        game_id: GameId,
        remote_game_id: String,
    },
    SetSteamDirectory {
        directory: Option<PathBuf>,
    },
    SetEpicDirectory {
        directory: Option<PathBuf>,
    },
    SetGogDirectory {
        directory: Option<PathBuf>,
    },
    SetAppearance {
        settings: AppearanceSettings,
    },
    SearchArtworkGames {
        provider_id: String,
        game_id: GameId,
        query: String,
    },
    ListArtwork {
        provider_id: String,
        game_id: GameId,
        remote_game_id: Option<String>,
        kind: ArtworkKind,
    },
    SelectArtwork {
        game_id: GameId,
        candidate: ArtworkCandidate,
    },
    ImportArtwork {
        game_id: GameId,
        kind: ArtworkKind,
        path: PathBuf,
    },
    SelectLocalArtwork {
        game_id: GameId,
        asset_id: String,
    },
}

impl Command {
    pub fn background(&self) -> bool {
        matches!(
            self,
            Self::RefreshLibrary
                | Self::ReidentifyGame { .. }
                | Self::SetArtworkCredential { .. }
                | Self::VerifyArtworkCredential { .. }
                | Self::SearchArtworkGames { .. }
                | Self::ListArtwork { .. }
                | Self::SelectArtwork { .. }
                | Self::ImportArtwork { .. }
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::RefreshLibrary => "Refreshing installed games",
            Self::AddManualGame { .. } => "Adding game",
            Self::RemoveManualGame { .. } => "Removing game",
            Self::UpdateMetadata { .. } => "Saving metadata",
            Self::SetFavourite { .. } => "Saving favourite",
            Self::SetHidden { .. } => "Saving game visibility",
            Self::ReidentifyGame { .. } => "Re-identifying game",
            Self::SetGameProgress { .. } => "Saving collections and completion",
            Self::SaveJournalEntry { .. } => "Saving journal entry",
            Self::DeleteJournalEntry { .. } => "Deleting journal entry",
            Self::SetLaunchOverride { .. } => "Saving launch configuration",
            Self::LaunchGame { .. } => "Launching game",
            Self::SetArtworkCredential { .. } => "Saving artwork provider credentials",
            Self::VerifyArtworkCredential { .. } => "Checking artwork provider connection",
            Self::SetArtworkGame { .. } => "Saving artwork game match",
            Self::SetSteamDirectory { .. } => "Saving Steam directory",
            Self::SetEpicDirectory { .. } => "Saving Epic manifest directory",
            Self::SetGogDirectory { .. } => "Saving GOG games directory",
            Self::SetAppearance { .. } => "Saving appearance",
            Self::SearchArtworkGames { .. } => "Searching SteamGridDB",
            Self::ListArtwork { .. } => "Loading artwork choices",
            Self::SelectArtwork { .. } => "Downloading selected artwork",
            Self::ImportArtwork { .. } => "Importing artwork",
            Self::SelectLocalArtwork { .. } => "Selecting saved artwork",
        }
    }
}

#[derive(Clone, Debug)]
pub enum AppEvent {
    LibraryChanged,
    ArtworkCacheChanged,
    SettingsChanged,
    JobChanged(JobStatus),
    GameAdded {
        job_id: JobId,
        game_id: GameId,
    },
    ArtworkGamesFound {
        job_id: JobId,
        game_id: GameId,
        games: Vec<ArtworkGame>,
    },
    ArtworkFound {
        job_id: JobId,
        game_id: GameId,
        kind: ArtworkKind,
        images: Vec<ArtworkCandidate>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    Validation(String),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("{0}")]
    Provider(String),
    #[error("{0}")]
    InvalidCredential(String),
    #[error(
        "The saved API key requires OS credential-store approval. Choose Unlock saved key in Settings."
    )]
    CredentialLocked,
    #[error("File or launch error: {0}")]
    Io(String),
    #[error("Game or asset not found")]
    NotFound,
    #[error("No matching game or artwork was found on this provider")]
    NoArtwork,
    #[error("The work queue is full; try again shortly")]
    Busy,
    #[error("Operation cancelled")]
    Cancelled,
}
