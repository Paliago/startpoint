use serde::Deserialize;
use startpoint_sdk::{
    ApiError, DiscoveredGame, DiscoveryProvider, DiscoveryReport, DiscoverySettings, JobContext,
    LaunchTarget, ProviderDescriptor, UserMetadata,
};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};
use steamlocate::{SteamDir, app::StateFlag};

pub struct SteamProvider;

#[derive(Deserialize)]
struct CatalogueResponse {
    success: bool,
    data: Option<CatalogueGame>,
}

#[derive(Deserialize)]
struct CatalogueGame {
    name: String,
    #[serde(default)]
    short_description: String,
    #[serde(default)]
    developers: Vec<String>,
    #[serde(default)]
    genres: Vec<CatalogueGenre>,
}

#[derive(Deserialize)]
struct CatalogueGenre {
    description: String,
}

pub(crate) fn metadata(
    client: &reqwest::blocking::Client,
    app_id: &str,
    context: &JobContext,
) -> Result<UserMetadata, ApiError> {
    let app_id = app_id
        .parse::<u32>()
        .map_err(|_| ApiError::Validation("Invalid Steam application identifier".into()))?;
    context.checkpoint()?;
    let response = client
        .get("https://store.steampowered.com/api/appdetails")
        .query(&[("appids", app_id.to_string()), ("l", "english".into())])
        .send()
        .map_err(|error| ApiError::Provider(format!("Steam catalogue: {error}")))?;
    if !response.status().is_success() {
        return Err(ApiError::Provider(format!(
            "Steam catalogue returned HTTP {}",
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    response
        .take(2_000_001)
        .read_to_end(&mut bytes)
        .map_err(|error| ApiError::Provider(error.to_string()))?;
    if bytes.len() > 2_000_000 {
        return Err(ApiError::Provider(
            "Steam catalogue response exceeded the size limit".into(),
        ));
    }
    context.checkpoint()?;
    let mut response: BTreeMap<String, CatalogueResponse> = serde_json::from_slice(&bytes)
        .map_err(|error| {
            ApiError::Provider(format!("Invalid Steam catalogue response: {error}"))
        })?;
    let response = response
        .remove(&app_id.to_string())
        .filter(|response| response.success)
        .ok_or(ApiError::NoArtwork)?;
    let game = response.data.ok_or(ApiError::NoArtwork)?;
    // ponytail: Steam short descriptions use common HTML entities; add an HTML parser only if richer markup is needed.
    let description = game
        .short_description
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#039;", "'")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("<br>", "\n")
        .replace("<br />", "\n");
    let mut metadata = UserMetadata {
        title: Some(game.name),
        description: Some(description),
        developer: Some(game.developers.join(", ")),
        genres: Some(
            game.genres
                .into_iter()
                .map(|genre| genre.description)
                .collect::<Vec<_>>()
                .join(", "),
        ),
    };
    for field in [
        &mut metadata.title,
        &mut metadata.description,
        &mut metadata.developer,
        &mut metadata.genres,
    ] {
        *field = field
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if field
            .as_ref()
            .is_some_and(|value| value.len() > 10_000 || value.contains('\0'))
        {
            return Err(ApiError::Provider(
                "Steam catalogue returned invalid metadata".into(),
            ));
        }
    }
    if metadata
        .title
        .as_ref()
        .is_some_and(|title| title.len() > 500)
    {
        return Err(ApiError::Provider(
            "Steam catalogue returned an invalid title".into(),
        ));
    }
    Ok(metadata)
}

impl DiscoveryProvider for SteamProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "steam".into(),
            name: "Steam".into(),
        }
    }

    fn uri_schemes(&self) -> Vec<String> {
        vec!["steam".into()]
    }

    fn discover(
        &self,
        settings: &DiscoverySettings,
        context: &JobContext,
    ) -> Result<DiscoveryReport, ApiError> {
        context.report("Reading installed Steam games");
        let steam = match &settings.steam_directory {
            Some(path) => SteamDir::from_dir(path).map_err(|error| ApiError::Provider(error.to_string()))?,
            None => match SteamDir::locate() {
                Ok(steam) => steam,
                Err(_) => return Ok(DiscoveryReport { games: Vec::new(), complete: false, warnings: vec!["Steam was not found. Add games manually or choose the Steam directory in Settings.".into()] }),
            },
        };
        let mut report = DiscoveryReport {
            games: Vec::new(),
            complete: true,
            warnings: Vec::new(),
        };
        let libraries = steam
            .libraries()
            .map_err(|error| ApiError::Provider(error.to_string()))?;
        for library in libraries {
            context.checkpoint()?;
            let library = match library {
                Ok(library) => library,
                Err(error) => {
                    report.complete = false;
                    report.warnings.push(error.to_string());
                    continue;
                }
            };
            for app in library.apps() {
                context.checkpoint()?;
                let app = match app {
                    Ok(app) => app,
                    Err(error) => {
                        report.complete = false;
                        report.warnings.push(error.to_string());
                        continue;
                    }
                };
                if !app.state_flags.is_some_and(|flags| {
                    flags.flags().any(|flag| flag == StateFlag::FullyInstalled)
                }) {
                    continue;
                }
                if !Path::new(&app.install_dir)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
                {
                    report.complete = false;
                    report.warnings.push(format!(
                        "Ignored an invalid installation directory for Steam app {}",
                        app.app_id
                    ));
                    continue;
                }
                let install_directory = library.resolve_app_dir(&app);
                if !install_directory.is_dir() {
                    continue;
                }
                let title = app
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Steam app {}", app.app_id));
                if app.app_id == 228_980
                    || title.starts_with("Steamworks Common")
                    || title.starts_with("Steam Linux Runtime")
                    || title.starts_with("Proton ")
                {
                    continue;
                }
                report.games.push(DiscoveredGame {
                    source_game_id: app.app_id.to_string(),
                    title,
                    install_directory,
                    launch_target: LaunchTarget::Uri(format!("steam://rungameid/{}", app.app_id)),
                    local_cover: cached_cover(steam.path(), app.app_id),
                });
            }
        }
        Ok(report)
    }
}

fn cached_cover(steam: &Path, app_id: u32) -> Option<PathBuf> {
    let cache = steam.join("appcache/librarycache");
    for suffix in [
        "library_600x900_2x.jpg",
        "library_600x900.jpg",
        "library_600x900.png",
    ] {
        let path = cache.join(format!("{app_id}_{suffix}"));
        if path.is_file() {
            return Some(path);
        }
    }
    fs::read_dir(cache.join(app_id.to_string()))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains("library_600x900"))
        })
}
