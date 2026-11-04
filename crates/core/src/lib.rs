//! Application services, persistence, discovery and platform integrations.

mod artwork;
mod catalogue;
mod discovery_paths;
mod examples;
mod platform;
mod runtime;
mod steam;
mod steam_shortcuts;
mod steamgriddb;
mod storage;
mod stores;
mod xbox;

pub use platform::application_paths;
pub use runtime::LauncherBuilder;
pub use steam::SteamProvider;
pub use steamgriddb::SteamGridDbProvider;
pub use stores::{EpicProvider, GogProvider};
pub use xbox::XboxProvider;

use startpoint_sdk::{ApiError, Application};

/// Copies game records from a read-only library into a new automation database.
/// Credentials, preferences, journals and launch history are deliberately excluded.
#[cfg(feature = "automation")]
pub fn seed_automation_library(
    source: &std::path::Path,
    paths: &startpoint_sdk::ApplicationPaths,
) -> Result<usize, ApiError> {
    if paths.data_directory.join("library.sqlite3").exists() {
        return Err(ApiError::Validation(
            "Automation seeding requires a new database".into(),
        ));
    }
    let connection =
        rusqlite::Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| ApiError::Storage(error.to_string()))?;
    let mut query = connection
        .prepare("SELECT document FROM games")
        .map_err(|error| ApiError::Storage(error.to_string()))?;
    let documents = query
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| ApiError::Storage(error.to_string()))?;
    let games = documents
        .map(|document| {
            let document = document.map_err(|error| ApiError::Storage(error.to_string()))?;
            serde_json::from_str::<startpoint_sdk::Game>(&document)
                .map_err(|error| ApiError::Storage(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut store = storage::Store::open(paths)?;
    store.save_games(&games)?;
    Ok(games.len())
}

pub fn create_application() -> Result<Application, ApiError> {
    let builder = LauncherBuilder::new()
        .discovery_provider(SteamProvider)
        .discovery_provider(EpicProvider)
        .discovery_provider(GogProvider);
    #[cfg(target_os = "windows")]
    let builder = builder.discovery_provider(XboxProvider);
    builder
        .artwork_provider(SteamGridDbProvider::new()?)
        .build(application_paths()?)
}

/// Opens a separate, persistent 45-game example library. Game launches are simulated.
pub fn create_example_application() -> Result<Application, ApiError> {
    let mut paths = application_paths()?;
    paths.data_directory = paths.data_directory.join("examples");
    paths.cache_directory = paths.cache_directory.join("examples");
    LauncherBuilder::new()
        .discovery_provider(examples::ExampleProvider)
        .artwork_provider(SteamGridDbProvider::new()?)
        .build(paths)
}
