use serde::{Deserialize, de::DeserializeOwned};
use startpoint_sdk::{
    ApiError, DiscoveredGame, DiscoveryProvider, DiscoveryReport, DiscoverySettings,
    ExecutableLaunch, JobContext, LaunchTarget, ProviderDescriptor,
};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

pub struct EpicProvider;
pub struct GogProvider;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct EpicManifest {
    app_name: String,
    display_name: String,
    install_location: PathBuf,
    #[serde(default)]
    catalog_namespace: String,
    #[serde(default)]
    catalog_item_id: String,
    #[serde(default)]
    app_categories: Vec<String>,
    #[serde(default)]
    compatible_apps: Vec<String>,
    #[serde(default)]
    technical_type: String,
    #[serde(default, rename = "bIsIncompleteInstall")]
    incomplete: bool,
    #[serde(rename = "bIsExecutable")]
    executable: Option<bool>,
    #[serde(rename = "bIsApplication")]
    application: Option<bool>,
}

impl EpicManifest {
    fn game(self) -> Result<Option<DiscoveredGame>, String> {
        if self.incomplete
            || self.executable == Some(false)
            || self.application == Some(false)
            || self.app_name.starts_with("UE_")
            || self
                .app_categories
                .iter()
                .any(|category| category.starts_with("plugins"))
            || self
                .compatible_apps
                .iter()
                .any(|name| name.starts_with("UE_"))
            || self.technical_type.starts_with("plugins")
            || (self
                .app_categories
                .iter()
                .any(|category| category == "addons")
                && !self
                    .app_categories
                    .iter()
                    .any(|category| category == "addons/launchable"))
        {
            return Ok(None);
        }
        if !valid_epic_id(&self.app_name)
            || !valid_title(&self.display_name)
            || !self.install_location.is_absolute()
        {
            return Err("Invalid game identifier, title or installation path".into());
        }
        if !is_directory(&self.install_location)? {
            return Ok(None);
        }
        let app = if self.catalog_namespace.is_empty() && self.catalog_item_id.is_empty() {
            self.app_name.clone()
        } else if valid_epic_id(&self.catalog_namespace) && valid_epic_id(&self.catalog_item_id) {
            format!(
                "{}%3A{}%3A{}",
                self.catalog_namespace, self.catalog_item_id, self.app_name
            )
        } else {
            return Err("Invalid Epic catalogue identifier".into());
        };
        Ok(Some(DiscoveredGame {
            source_game_id: self.app_name,
            title: self.display_name.trim().to_string(),
            install_directory: self.install_location,
            launch_target: LaunchTarget::Uri(format!(
                "com.epicgames.launcher://apps/{app}?action=launch&silent=true"
            )),
            local_cover: None,
        }))
    }
}

impl DiscoveryProvider for EpicProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "epic".into(),
            name: "Epic Games".into(),
        }
    }

    fn uri_schemes(&self) -> Vec<String> {
        vec!["com.epicgames.launcher".into()]
    }

    fn discover(
        &self,
        settings: &DiscoverySettings,
        context: &JobContext,
    ) -> Result<DiscoveryReport, ApiError> {
        context.report("Reading installed Epic games");
        let directories = epic_directories(&settings.epic_directory);
        let mut report = empty_report();
        let mut found = false;
        let mut games = BTreeMap::new();
        for directory in directories {
            let Some(entries) = read_directory(
                &directory,
                settings.epic_directory.is_none(),
                context,
                &mut report,
            )?
            else {
                continue;
            };
            found = true;
            for path in entries {
                context.checkpoint()?;
                if !path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("item"))
                {
                    continue;
                }
                match read_json::<EpicManifest>(&path).and_then(EpicManifest::game) {
                    Ok(Some(game)) => {
                        games.insert(game.source_game_id.clone(), game);
                    }
                    Ok(None) => {}
                    Err(error) => warning(&mut report, &path, error),
                }
            }
        }
        report.complete &= found;
        report.games = games.into_values().collect();
        Ok(report)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GogManifest {
    game_id: String,
    name: String,
    #[serde(default)]
    root_game_id: String,
    standalone: Option<bool>,
    #[serde(default)]
    play_tasks: Vec<GogTask>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GogTask {
    #[serde(default)]
    is_primary: bool,
    #[serde(rename = "type")]
    task_type: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    working_dir: String,
    #[serde(default)]
    arguments: String,
}

impl GogManifest {
    fn game(
        self,
        directory: &Path,
        bundle: Option<&Path>,
    ) -> Result<Option<DiscoveredGame>, String> {
        if self.standalone != Some(true)
            && !self.root_game_id.is_empty()
            && self.root_game_id != self.game_id
        {
            return Ok(None);
        }
        if self.game_id.parse::<u64>().is_err()
            || self.game_id.len() > 20
            || !self.game_id.bytes().all(|byte| byte.is_ascii_digit())
            || !valid_title(&self.name)
        {
            return Err("Invalid GOG product identifier or title".into());
        }
        let launch_target = if let Some(bundle) = bundle {
            LaunchTarget::ApplicationBundle(ExecutableLaunch {
                executable: bundle.to_path_buf(),
                arguments: Vec::new(),
                working_directory: None,
            })
        } else {
            let task = self
                .play_tasks
                .iter()
                .find(|task| task.is_primary)
                .or_else(|| self.play_tasks.first());
            let Some(task) = task.filter(|task| task.task_type == "FileTask") else {
                return Ok(None);
            };
            let executable = local_path(directory, &task.path)?;
            if native_bundle(&executable) && is_directory(&executable)? {
                LaunchTarget::ApplicationBundle(ExecutableLaunch {
                    executable,
                    arguments: arguments(&task.arguments)?,
                    working_directory: None,
                })
            } else {
                if !executable.is_file() {
                    return Err("The primary GOG executable is missing or unreadable".into());
                }
                let working_directory = local_path(directory, &task.working_dir)?;
                if !is_directory(&working_directory)? {
                    return Err("The GOG working directory is missing".into());
                }
                LaunchTarget::Executable(ExecutableLaunch {
                    executable,
                    arguments: arguments(&task.arguments)?,
                    working_directory: Some(working_directory),
                })
            }
        };
        Ok(Some(DiscoveredGame {
            source_game_id: self.game_id,
            title: self.name.trim().to_string(),
            install_directory: bundle.unwrap_or(directory).to_path_buf(),
            launch_target,
            local_cover: None,
        }))
    }
}

impl DiscoveryProvider for GogProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "gog".into(),
            name: "GOG".into(),
        }
    }

    fn discover(
        &self,
        settings: &DiscoverySettings,
        context: &JobContext,
    ) -> Result<DiscoveryReport, ApiError> {
        context.report("Reading installed GOG games");
        let mut report = empty_report();
        let mut games = BTreeMap::new();
        let mut found = false;
        let directories = gog_directories(&settings.gog_directory);
        if settings.gog_directory.is_none() {
            #[cfg(target_os = "windows")]
            {
                let (registered, present) = gog_registry(context, &mut report)?;
                found = present;
                for directory in registered {
                    context.checkpoint()?;
                    scan_gog_installation(&directory, context, &mut report, &mut games)?;
                }
            }
        }
        // ponytail: inspect only the selected folder, its children and native bundle resources; select a closer folder for deeper layouts.
        for directory in directories {
            let Some(entries) = read_directory(
                &directory,
                settings.gog_directory.is_none(),
                context,
                &mut report,
            )?
            else {
                continue;
            };
            found = true;
            scan_gog_installation(&directory, context, &mut report, &mut games)?;
            for path in entries {
                context.checkpoint()?;
                if !native_bundle(&path)
                    && is_directory(&path).unwrap_or_else(|error| {
                        warning(&mut report, &path, error);
                        false
                    })
                {
                    scan_gog_installation(&path, context, &mut report, &mut games)?;
                }
            }
        }
        report.complete &= found;
        report.games = games.into_values().collect();
        Ok(report)
    }
}

pub(crate) fn epic_directories(selected: &Option<PathBuf>) -> Vec<PathBuf> {
    if let Some(directory) = selected {
        return vec![directory.clone()];
    }
    if cfg!(target_os = "windows") {
        std::env::var_os("PROGRAMDATA")
            .map(PathBuf::from)
            .into_iter()
            .map(|directory| directory.join("Epic/EpicGamesLauncher/Data/Manifests"))
            .collect()
    } else if cfg!(target_os = "macos") {
        directories::BaseDirs::new()
            .map(|base| {
                base.home_dir()
                    .join("Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests")
            })
            .into_iter()
            .collect()
    } else {
        Vec::new()
    }
}

pub(crate) fn gog_directories(selected: &Option<PathBuf>) -> Vec<PathBuf> {
    if let Some(directory) = selected {
        return vec![directory.clone()];
    }
    let mut paths = Vec::new();
    #[cfg(target_os = "windows")]
    for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(directory) = std::env::var_os(variable) {
            paths.push(PathBuf::from(directory).join("GOG Galaxy/Games"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from("/Applications"));
        if let Some(base) = directories::BaseDirs::new() {
            paths.push(base.home_dir().join("Applications"));
        }
    }
    paths
}

fn scan_gog_installation(
    directory: &Path,
    context: &JobContext,
    report: &mut DiscoveryReport,
    games: &mut BTreeMap<String, DiscoveredGame>,
) -> Result<(), ApiError> {
    let bundle = native_bundle(directory).then_some(directory);
    let mut locations = vec![directory.to_path_buf()];
    if bundle.is_some() {
        locations.extend(
            ["Contents/Resources", "Contents/Resources/game"].map(|path| directory.join(path)),
        );
    }
    for location in locations {
        let Some(entries) = read_directory(&location, true, context, report)? else {
            continue;
        };
        for path in entries {
            context.checkpoint()?;
            if bundle.is_none()
                && native_bundle(&path)
                && is_directory(&path).unwrap_or_else(|error| {
                    warning(report, &path, error);
                    false
                })
            {
                scan_gog_installation(&path, context, report, games)?;
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let name = name.to_ascii_lowercase();
            if !name.starts_with("goggame-") || !name.ends_with(".info") {
                continue;
            }
            match read_json::<GogManifest>(&path)
                .and_then(|manifest| manifest.game(&location, bundle))
            {
                Ok(Some(game)) => {
                    games.insert(game.source_game_id.clone(), game);
                }
                Ok(None) => {}
                Err(error) => warning(report, &path, error),
            }
        }
    }
    Ok(())
}

fn empty_report() -> DiscoveryReport {
    DiscoveryReport {
        games: Vec::new(),
        complete: true,
        warnings: Vec::new(),
    }
}

fn warning(report: &mut DiscoveryReport, path: &Path, error: impl std::fmt::Display) {
    report.complete = false;
    report.warnings.push(format!("{}: {error}", path.display()));
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    if !fs::metadata(path)
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Installation records must be regular files".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(1_000_001).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    if bytes.len() > 1_000_000 {
        return Err("Installation record exceeds the 1 MB limit".into());
    }
    serde_json::from_slice(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes))
        .map_err(|error| error.to_string())
}

fn read_directory(
    path: &Path,
    optional: bool,
    context: &JobContext,
    report: &mut DiscoveryReport,
) -> Result<Option<Vec<PathBuf>>, ApiError> {
    context.checkpoint()?;
    if !path.is_absolute() {
        warning(report, path, "Discovery directories must be absolute");
        return Ok(None);
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if optional && error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            warning(report, path, error);
            return Ok(None);
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        context.checkpoint()?;
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(error) => warning(report, path, error),
        }
    }
    paths.sort();
    Ok(Some(paths))
}

fn is_directory(path: &Path) -> Result<bool, String> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn valid_title(title: &str) -> bool {
    !title.trim().is_empty() && title.len() <= 500 && !title.chars().any(char::is_control)
}

fn native_bundle(path: &Path) -> bool {
    cfg!(target_os = "macos")
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
}

fn valid_epic_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn local_path(directory: &Path, relative: &str) -> Result<PathBuf, String> {
    let normalized = relative.replace('\\', "/");
    let path = Path::new(&normalized);
    if relative.contains('\0')
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err("GOG task paths must remain inside the installation directory".into());
    }
    let root = directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let resolved = directory
        .join(path)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !resolved.starts_with(root) {
        return Err("GOG task paths must remain inside the installation directory".into());
    }
    Ok(resolved)
}

fn arguments(value: &str) -> Result<Vec<String>, String> {
    if value.len() > 32_000 || value.contains('\0') {
        return Err("Invalid GOG launch arguments".into());
    }
    split_arguments(value)
}

#[cfg(not(target_os = "windows"))]
fn split_arguments(value: &str) -> Result<Vec<String>, String> {
    shlex::split(value).ok_or_else(|| "Unterminated quote in GOG launch arguments".into())
}

#[cfg(target_os = "windows")]
fn split_arguments(value: &str) -> Result<Vec<String>, String> {
    use windows_sys::Win32::{Foundation::LocalFree, UI::Shell::CommandLineToArgvW};
    let command = format!("startpoint {value}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut count = 0;
    // Windows owns the returned, NUL-terminated argument array; free it once after copying.
    unsafe {
        let parsed = CommandLineToArgvW(command.as_ptr(), &mut count);
        if parsed.is_null() {
            return Err(io::Error::last_os_error().to_string());
        }
        let mut result = Vec::new();
        for index in 1..usize::try_from(count).unwrap_or_default() {
            let argument = *parsed.add(index);
            let mut length = 0;
            while *argument.add(length) != 0 {
                length += 1;
            }
            result.push(String::from_utf16_lossy(std::slice::from_raw_parts(
                argument, length,
            )));
        }
        LocalFree(parsed.cast());
        Ok(result)
    }
}

#[cfg(target_os = "windows")]
fn gog_registry(
    context: &JobContext,
    report: &mut DiscoveryReport,
) -> Result<(Vec<PathBuf>, bool), ApiError> {
    use std::collections::BTreeSet;
    use winreg::{
        RegKey,
        enums::{
            HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
        },
    };
    let mut directories = BTreeSet::new();
    let mut found = false;
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
            for (key_path, value) in [
                (r"SOFTWARE\GOG.com\Games", "path"),
                (
                    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
                    "InstallLocation",
                ),
            ] {
                context.checkpoint()?;
                let key =
                    match RegKey::predef(hive).open_subkey_with_flags(key_path, KEY_READ | view) {
                        Ok(key) => key,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                        Err(error) => {
                            warning(report, Path::new(key_path), error);
                            continue;
                        }
                    };
                if value == "path" {
                    found = true;
                }
                for name in key.enum_keys() {
                    context.checkpoint()?;
                    let name = match name {
                        Ok(name) => name,
                        Err(error) => {
                            warning(report, Path::new(key_path), error);
                            continue;
                        }
                    };
                    let identifier = if value == "path" {
                        name.as_str()
                    } else {
                        name.strip_suffix("_is1").unwrap_or("")
                    };
                    if identifier.is_empty()
                        || !identifier.bytes().all(|byte| byte.is_ascii_digit())
                    {
                        continue;
                    }
                    let entry = match key.open_subkey(&name) {
                        Ok(entry) => entry,
                        Err(error) => {
                            warning(report, Path::new(key_path), error);
                            continue;
                        }
                    };
                    if value == "InstallLocation"
                        && !entry
                            .get_value::<String, _>("Publisher")
                            .is_ok_and(|publisher| publisher.eq_ignore_ascii_case("GOG.com"))
                    {
                        continue;
                    }
                    found = true;
                    match entry.get_value::<String, _>(value) {
                        Ok(path) if Path::new(&path).is_absolute() => {
                            directories.insert(PathBuf::from(path));
                        }
                        Ok(_) => {
                            warning(report, Path::new(key_path), "Invalid GOG installation path")
                        }
                        Err(error) => warning(report, Path::new(key_path), error),
                    }
                }
            }
        }
    }
    Ok((directories.into_iter().collect(), found))
}
