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

/// A launch route, independent of the game's original store and identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SteamLaunch {
    Shortcut {
        account_id: u32,
        app_id: u32,
        name: String,
    },
    Url(String),
}

impl SteamLaunch {
    pub fn uri(&self) -> String {
        match self {
            Self::Shortcut { app_id, .. } => {
                // Use Steam's saved appid, not a hash reconstructed from the name.
                let game_id = (u64::from(*app_id) << 32) | 0x0200_0000;
                format!("steam://rungameid/{game_id}")
            }
            Self::Url(uri) => uri.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamShortcut {
    pub account_id: u32,
    pub app_id: u32,
    pub name: String,
    pub executable: String,
    pub start_directory: String,
}

impl SteamShortcut {
    pub fn launch(&self) -> SteamLaunch {
        SteamLaunch::Shortcut {
            account_id: self.account_id,
            app_id: self.app_id,
            name: self.name.clone(),
        }
    }
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
    #[serde(default)]
    pub steam_launch: Option<SteamLaunch>,
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
        if let Some(launch) = &self.steam_launch {
            return LaunchTarget::Uri(launch.uri());
        }
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

    pub fn has_launch_override(&self) -> bool {
        self.launch_override.is_some() || self.steam_launch.is_some()
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ArtworkPage {
    pub images: Vec<ArtworkCandidate>,
    pub has_more: bool,
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
pub enum RecentsLayout {
    #[default]
    Carousel,
    Editorial,
    Gallery,
    Cinematic,
    InfiniteWall,
    FilmReel,
    ArtBook,
    Diagonal,
    Mosaic,
}

impl RecentsLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Carousel => "Carousel",
            Self::Editorial => "Editorial Recents",
            Self::Gallery => "Gallery",
            Self::Cinematic => "Cinematic",
            Self::InfiniteWall => "Infinite Wall",
            Self::FilmReel => "Film Reel",
            Self::ArtBook => "Art Book",
            Self::Diagonal => "Diagonal",
            Self::Mosaic => "Mosaic",
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
    pub window_translucency: bool,
    pub backdrop_blur: bool,
    pub translucency_amount: f32,
    pub controls: ControlAppearance,
    pub dither_artwork: bool,
    pub dither_artwork_strength: f32,
    pub dither_artwork_size: f32,
    pub start_screen: StartScreen,
    pub recents_layout: RecentsLayout,
    pub pause_in_background: bool,
    pub dither_recents: bool,
    pub dither_recents_strength: f32,
    pub dither_recents_size: f32,
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
            window_translucency: true,
            backdrop_blur: true,
            translucency_amount: DEFAULT_TRANSLUCENCY_AMOUNT,
            controls: ControlAppearance::default(),
            dither_artwork: true,
            dither_artwork_strength: DEFAULT_DITHER_STRENGTH,
            dither_artwork_size: DEFAULT_DITHER_SIZE,
            start_screen: StartScreen::Recents,
            recents_layout: RecentsLayout::default(),
            pause_in_background: true,
            dither_recents: true,
            dither_recents_strength: DEFAULT_DITHER_STRENGTH,
            dither_recents_size: DEFAULT_DITHER_SIZE,
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

/// Shared Precision control geometry and emphasis, independent of panel rounding.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlAppearance {
    pub radius: f32,
    pub horizontal_padding: f32,
    pub outline_strength: f32,
    pub outlines: bool,
    pub filled_secondary: bool,
    pub compact_launch: bool,
}

impl Default for ControlAppearance {
    fn default() -> Self {
        Self {
            radius: 6.,
            horizontal_padding: 12.,
            outline_strength: 0.65,
            outlines: true,
            filled_secondary: false,
            compact_launch: true,
        }
    }
}

impl ControlAppearance {
    /// Checks finite values against the supported rounding, padding and outline ranges.
    pub fn is_valid(self) -> bool {
        [
            (self.radius, 0., 12.),
            (self.horizontal_padding, 8., 20.),
            (self.outline_strength, 0.25, 1.),
        ]
        .into_iter()
        .all(|(value, min, max)| value.is_finite() && (min..=max).contains(&value))
    }
}

/// The settled glass appearance is the slider's default detent.
pub const DEFAULT_TRANSLUCENCY_AMOUNT: f32 = 0.2;

/// Default blend between the original artwork and Bayer-quantized colors.
pub const DEFAULT_DITHER_STRENGTH: f32 = 0.2;
pub const DEFAULT_DITHER_SIZE: f32 = 3.;
pub const MAX_DITHER_SIZE: f32 = 12.;

#[derive(Clone, Debug, Default)]
pub struct SettingsSnapshot {
    pub configured_credentials: BTreeSet<String>,
    pub credential_status: BTreeMap<String, CredentialStatus>,
    pub steam_directory: Option<PathBuf>,
    pub epic_directory: Option<PathBuf>,
    pub gog_directory: Option<PathBuf>,
    /// Resolved discovery locations for display; never persisted as overrides.
    pub discovery_directories: BTreeMap<String, Vec<PathBuf>>,
    /// Read-only, worker-resolved non-Steam shortcuts, with their Steam account IDs.
    pub steam_shortcuts: Vec<SteamShortcut>,
    pub steam_shortcut_warnings: Vec<String>,
    pub steam_shortcuts_loaded: bool,
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
    RefreshMetadata,
    RefreshSteamShortcuts,
    ReadSteamShortcutUrl {
        game_id: GameId,
        path: PathBuf,
    },
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
    SetSteamLaunch {
        game_id: GameId,
        launch: SteamLaunch,
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
        page: u32,
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
                | Self::RefreshMetadata
                | Self::RefreshSteamShortcuts
                | Self::ReadSteamShortcutUrl { .. }
                | Self::SetSteamLaunch { .. }
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
            Self::RefreshMetadata => "Refreshing metadata and missing artwork",
            Self::RefreshSteamShortcuts => "Reading Steam shortcuts",
            Self::ReadSteamShortcutUrl { .. } => "Reading Steam desktop shortcut",
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
            Self::SetSteamLaunch { .. } => "Saving Steam launch configuration",
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
    SteamShortcutUrlRead {
        job_id: JobId,
        game_id: GameId,
        uri: String,
    },
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
        page: u32,
        has_more: bool,
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
