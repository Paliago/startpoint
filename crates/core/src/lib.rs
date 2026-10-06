//! Application services, persistence, discovery and platform integrations.

mod artwork;
mod examples;
mod platform;
mod runtime;
mod steam;
mod steamgriddb;
mod storage;
mod stores;

pub use platform::application_paths;
pub use runtime::LauncherBuilder;
pub use steam::SteamProvider;
pub use steamgriddb::SteamGridDbProvider;
pub use stores::{EpicProvider, GogProvider};

use startpoint_sdk::{ApiError, Application};

pub fn create_application() -> Result<Application, ApiError> {
    LauncherBuilder::new()
        .discovery_provider(SteamProvider)
        .discovery_provider(EpicProvider)
        .discovery_provider(GogProvider)
        .artwork_provider(SteamGridDbProvider::new()?)
        .build(application_paths()?)
}

/// Opens a separate, persistent 15-game example library. Game launches are simulated.
pub fn create_example_application() -> Result<Application, ApiError> {
    let mut paths = application_paths()?;
    paths.data_directory = paths.data_directory.join("examples");
    paths.cache_directory = paths.cache_directory.join("examples");
    LauncherBuilder::new()
        .discovery_provider(examples::ExampleProvider)
        .artwork_provider(SteamGridDbProvider::new()?)
        .build(paths)
}
