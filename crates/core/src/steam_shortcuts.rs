//! Read Steam's per-account shortcuts without modifying Steam's configuration.
//! All filesystem/registry work happens on the runtime worker.
use startpoint_sdk::{ApiError, JobContext, SteamLaunch, SteamShortcut};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_FIELDS: usize = 100_000;

fn error(message: impl Into<String>) -> ApiError {
    ApiError::Validation(message.into())
}

pub(crate) fn root(directory: Option<&Path>) -> Result<PathBuf, ApiError> {
    match directory {
        Some(path) => Ok(path.to_path_buf()),
        None => steamlocate::SteamDir::locate().map(|steam| steam.path().to_path_buf())
            .map_err(|_| error("Steam was not found. Set its installation directory in Settings, or paste a Steam launch URL.")),
    }
}

fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>, ApiError> {
    let file = fs::File::open(path).map_err(|e| ApiError::Io(e.to_string()))?;
    if !file
        .metadata()
        .map_err(|e| ApiError::Io(e.to_string()))?
        .is_file()
    {
        return Err(error(
            "The Steam shortcut configuration is not a regular file.",
        ));
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ApiError::Io(e.to_string()))?;
    if bytes.len() as u64 > maximum {
        return Err(error(
            "Steam shortcut configuration exceeded the size limit.",
        ));
    }
    Ok(bytes)
}

pub(crate) fn scan(
    root: &Path,
    context: &JobContext,
) -> Result<(Vec<SteamShortcut>, Vec<String>), ApiError> {
    context.checkpoint()?;
    let userdata = root.join("userdata");
    let directories = match fs::read_dir(&userdata) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), Vec::new())),
        Err(e) => return Err(ApiError::Io(e.to_string())),
    };
    let mut shortcuts = Vec::new();
    let mut warnings = Vec::new();
    for entry in directories {
        context.checkpoint()?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                warnings.push("A Steam account directory could not be read.".into());
                continue;
            }
        };
        let Some(account_id) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
            .filter(|id| *id != 0)
        else {
            continue;
        };
        let file = entry.path().join("config/shortcuts.vdf");
        if !file.exists() {
            continue;
        }
        match read_bounded(&file, MAX_FILE_BYTES).and_then(|bytes| parse(&bytes, account_id)) {
            Ok(mut entries) => shortcuts.append(&mut entries),
            Err(_) => warnings.push(format!("Could not read saved shortcuts for Steam account {account_id}. Restart Steam and refresh the list.")),
        }
    }
    shortcuts.sort_by_key(|shortcut| {
        (
            shortcut.name.to_lowercase(),
            shortcut.account_id,
            shortcut.app_id,
        )
    });
    Ok((shortcuts, warnings))
}

/// Only accept the launch URI we advertise, never Steam's other command URLs.
pub(crate) fn normalize_uri(uri: &str) -> Result<String, ApiError> {
    let invalid = || {
        error(
            "Enter a Steam game URL such as steam://rungameid/123456789. Other URL commands are not supported.",
        )
    };
    if uri.trim().chars().any(char::is_control) {
        return Err(invalid());
    }
    let parsed = reqwest::Url::parse(uri.trim()).map_err(|_| invalid())?;
    if parsed.scheme() != "steam"
        || parsed.host_str() != Some("rungameid")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(invalid());
    }
    let id = parsed.path().strip_prefix('/').ok_or_else(invalid)?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    let id = id
        .parse::<u64>()
        .ok()
        .filter(|id| *id != 0)
        .ok_or_else(invalid)?;
    Ok(format!("steam://rungameid/{id}"))
}

pub(crate) fn read_url(path: &Path) -> Result<String, ApiError> {
    let bytes = read_bounded(path, 64 * 1024)?;
    let text = if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        if bytes.len() % 2 != 0 {
            return Err(error("Invalid Steam desktop shortcut encoding."));
        }
        let units = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units).map_err(|_| error("Invalid Steam desktop shortcut encoding."))?
    } else {
        String::from_utf8(bytes)
            .map_err(|_| error("The selected file is not a Steam .url shortcut."))?
    };
    parse_url(&text)
}

fn parse_url(text: &str) -> Result<String, ApiError> {
    let mut section = false;
    let mut url = None;
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if line.starts_with('[') {
            section = line.eq_ignore_ascii_case("[InternetShortcut]");
            continue;
        }
        if section
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("URL")
        {
            if url.is_some() {
                return Err(error("The desktop shortcut contains multiple launch URLs."));
            }
            url = Some(normalize_uri(value)?);
        }
    }
    url.ok_or_else(|| {
        error("Choose a Steam desktop .url shortcut containing a steam://rungameid/ URL.")
    })
}

pub(crate) fn validate(
    launch: SteamLaunch,
    root_override: Option<&Path>,
    context: &JobContext,
    launching: bool,
) -> Result<SteamLaunch, ApiError> {
    match launch {
        SteamLaunch::Url(uri) => Ok(SteamLaunch::Url(normalize_uri(&uri)?)),
        SteamLaunch::Shortcut {
            account_id, app_id, ..
        } => {
            let root = root(root_override)?;
            // Read just the linked account: a broken unrelated profile must not
            // prevent launching a valid shortcut.
            let file = root.join(format!("userdata/{account_id}/config/shortcuts.vdf"));
            context.checkpoint()?;
            let entries = read_bounded(&file, MAX_FILE_BYTES).and_then(|bytes| parse(&bytes, account_id))
                .map_err(|_| error("The linked Steam shortcut is unavailable. Refresh the shortcut list and relink it; the executable was not launched."))?;
            let shortcut = entries.into_iter().find(|shortcut| shortcut.app_id == app_id)
                .ok_or_else(|| error("The linked Steam shortcut was removed or changed. Relink it in Launch settings; the executable was not launched."))?;
            if launching
                && let Some(active) = active_account(&root)
                && active != account_id
            {
                return Err(error(format!(
                    "This shortcut belongs to Steam account {account_id}, but account {active} is signed in. Switch accounts in Steam, then try again."
                )));
            }
            Ok(shortcut.launch())
        }
    }
}

#[cfg(target_os = "windows")]
fn active_account(root: &Path) -> Option<u32> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let steam = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Valve\\Steam")
        .ok()?;
    let installed: String = steam.get_value("SteamPath").ok()?;
    if !Path::new(&installed)
        .canonicalize()
        .ok()?
        .to_string_lossy()
        .eq_ignore_ascii_case(&root.canonicalize().ok()?.to_string_lossy())
    {
        return None;
    }
    steam
        .open_subkey("ActiveProcess")
        .ok()?
        .get_value::<u32, _>("ActiveUser")
        .ok()
        .filter(|id| *id != 0)
}

#[cfg(not(target_os = "windows"))]
fn active_account(_: &Path) -> Option<u32> {
    None
}

enum Value {
    Object(Vec<(String, Value)>),
    Text(String),
    Number(u32),
    Other,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    fields: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], ApiError> {
        let end = self
            .at
            .checked_add(length)
            .ok_or_else(|| error("Invalid Steam shortcut file."))?;
        let bytes = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| error("Truncated Steam shortcut file."))?;
        self.at = end;
        Ok(bytes)
    }
    fn string(&mut self) -> Result<String, ApiError> {
        let length = self.bytes[self.at..]
            .iter()
            .position(|b| *b == 0)
            .filter(|len| *len <= 16_384)
            .ok_or_else(|| error("Invalid Steam shortcut string."))?;
        let text = std::str::from_utf8(self.take(length)?)
            .map_err(|_| error("Invalid Steam shortcut text."))?
            .to_string();
        self.take(1)?;
        Ok(text)
    }
    fn object(&mut self, depth: usize) -> Result<Vec<(String, Value)>, ApiError> {
        if depth > 16 {
            return Err(error("Steam shortcut nesting exceeded the limit."));
        }
        let mut values = Vec::new();
        loop {
            let kind = self.take(1)?[0];
            if kind == 8 {
                return Ok(values);
            }
            self.fields += 1;
            if self.fields > MAX_FIELDS {
                return Err(error("Too many Steam shortcut fields."));
            }
            let key = self.string()?;
            let value = match kind {
                0 => Value::Object(self.object(depth + 1)?),
                1 => Value::Text(self.string()?),
                2 => Value::Number(u32::from_le_bytes(self.take(4)?.try_into().unwrap())),
                3 | 4 | 6 => {
                    self.take(4)?;
                    Value::Other
                }
                7 | 10 => {
                    self.take(8)?;
                    Value::Other
                }
                5 => {
                    let mut length = 0;
                    while self.take(2)? != [0, 0] {
                        length += 2;
                        if length > 32_768 {
                            return Err(error("Steam shortcut string exceeded the limit."));
                        }
                    }
                    Value::Other
                }
                _ => return Err(error("Unsupported Steam shortcut field type.")),
            };
            values.push((key, value));
        }
    }
}

fn get<'a>(fields: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    fields
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, value)| value)
}

fn parse(bytes: &[u8], account_id: u32) -> Result<Vec<SteamShortcut>, ApiError> {
    let mut reader = Reader {
        bytes,
        at: 0,
        fields: 0,
    };
    let root = reader.object(0)?;
    if reader.at != bytes.len() {
        return Err(error("Unexpected data after Steam shortcuts."));
    }
    let Some(Value::Object(entries)) = get(&root, "shortcuts") else {
        return Err(error("Invalid Steam shortcut root."));
    };
    if entries.len() > 4096 {
        return Err(error("Too many Steam shortcuts."));
    }
    let mut ids = BTreeSet::new();
    let mut shortcuts = Vec::new();
    for (_, value) in entries {
        let Value::Object(fields) = value else {
            continue;
        };
        let (Some(Value::Number(app_id)), Some(Value::Text(name)), Some(Value::Text(executable))) = (
            get(fields, "appid"),
            get(fields, "appname"),
            get(fields, "exe"),
        ) else {
            return Err(error("Incomplete Steam shortcut."));
        };
        if *app_id < 0x8000_0000
            || name.trim().is_empty()
            || name.len() > 500
            || name.chars().any(char::is_control)
            || !ids.insert(*app_id)
        {
            return Err(error("Invalid or duplicate Steam shortcut identifier."));
        }
        let start_directory = match get(fields, "startdir") {
            Some(Value::Text(value)) => value.clone(),
            _ => String::new(),
        };
        shortcuts.push(SteamShortcut {
            account_id,
            app_id: *app_id,
            name: name.clone(),
            executable: executable.clone(),
            start_directory,
        });
    }
    Ok(shortcuts)
}

#[cfg(test)]
pub(crate) fn fixture_bytes(entries: &[(u32, &str)]) -> Vec<u8> {
    fn key(bytes: &mut Vec<u8>, kind: u8, name: &str) {
        bytes.push(kind);
        bytes.extend(name.as_bytes());
        bytes.push(0);
    }
    fn text(bytes: &mut Vec<u8>, name: &str, value: &str) {
        key(bytes, 1, name);
        bytes.extend(value.as_bytes());
        bytes.push(0);
    }
    let mut bytes = Vec::new();
    key(&mut bytes, 0, "shortcuts");
    for (index, (app_id, name)) in entries.iter().enumerate() {
        key(&mut bytes, 0, &index.to_string());
        // Unknown/reordered fields and nested tags must not confuse parsing.
        text(&mut bytes, "LaunchOptions", "--controller");
        text(&mut bytes, "appname", name);
        key(&mut bytes, 0, "tags");
        text(&mut bytes, "0", "Controller games");
        bytes.push(8);
        key(&mut bytes, 2, "appid");
        bytes.extend(app_id.to_le_bytes());
        text(&mut bytes, "Exe", "\"C:\\Games\\Game\\game.exe\"");
        text(&mut bytes, "StartDir", "\"C:\\Games\\Game\"");
        key(&mut bytes, 2, "AllowOverlay");
        bytes.extend(1_u32.to_le_bytes());
        bytes.push(8);
    }
    bytes.extend([8, 8]);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::AtomicBool};

    fn context() -> JobContext {
        JobContext::new(Arc::new(AtomicBool::new(false)), |_| {})
    }
    fn directory() -> PathBuf {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/verification")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn binary_shortcuts_keep_account_and_saved_app_id_instead_of_rehashing_names() {
        let app_id = 0x9123_4567;
        let original = parse(&fixture_bytes(&[(app_id, "Cyberpunk 2077")]), 42).unwrap();
        assert_eq!(original[0].account_id, 42);
        assert_eq!(original[0].app_id, app_id);
        assert_eq!(original[0].name, "Cyberpunk 2077");
        let renamed = parse(
            &fixture_bytes(&[(app_id, "A completely different name")]),
            42,
        )
        .unwrap();
        assert_eq!(original[0].launch().uri(), renamed[0].launch().uri());
        assert_eq!(
            original[0].launch().uri(),
            format!(
                "steam://rungameid/{}",
                (u64::from(app_id) << 32) | 0x0200_0000
            )
        );
    }

    #[test]
    fn scans_accounts_separately_and_retains_good_profiles_when_another_is_broken() {
        let root = directory();
        for (account, bytes) in [
            (42, fixture_bytes(&[(0x9123_4567, "Game")])),
            (43, fixture_bytes(&[(0x9123_4567, "Game")])),
            (44, b"broken".to_vec()),
        ] {
            let config = root.join(format!("userdata/{account}/config"));
            fs::create_dir_all(&config).unwrap();
            fs::write(config.join("shortcuts.vdf"), bytes).unwrap();
        }
        let before = fs::read(root.join("userdata/42/config/shortcuts.vdf")).unwrap();
        let (shortcuts, warnings) = scan(&root, &context()).unwrap();
        assert_eq!(shortcuts.len(), 2);
        assert_ne!(shortcuts[0].account_id, shortcuts[1].account_id);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            fs::read(root.join("userdata/42/config/shortcuts.vdf")).unwrap(),
            before
        );
        assert!(scan(&directory(), &context()).unwrap().0.is_empty());
    }

    #[test]
    fn missing_or_changed_shortcut_is_an_error_not_an_executable_fallback() {
        let root = directory();
        let config = root.join("userdata/42/config");
        fs::create_dir_all(&config).unwrap();
        fs::write(
            config.join("shortcuts.vdf"),
            fixture_bytes(&[(0x9123_4567, "Current name")]),
        )
        .unwrap();
        let saved = SteamLaunch::Shortcut {
            account_id: 42,
            app_id: 0x9123_4567,
            name: "Old name".into(),
        };
        let validated = validate(saved.clone(), Some(&root), &context(), false).unwrap();
        assert_eq!(saved.uri(), validated.uri());
        assert!(matches!(validated, SteamLaunch::Shortcut { name, .. } if name == "Current name"));
        fs::write(config.join("shortcuts.vdf"), fixture_bytes(&[])).unwrap();
        assert!(validate(saved, Some(&root), &context(), true).is_err());
    }

    #[test]
    fn malformed_duplicate_and_truncated_binary_data_is_rejected_without_panicking() {
        let bytes = fixture_bytes(&[(0x9123_4567, "Game")]);
        for length in 0..bytes.len() {
            assert!(parse(&bytes[..length], 42).is_err());
        }
        assert!(
            parse(
                &fixture_bytes(&[(0x9123_4567, "First"), (0x9123_4567, "Duplicate")]),
                42
            )
            .is_err()
        );
        assert!(parse(&fixture_bytes(&[(12, "Not a shortcut")]), 42).is_err());
        let mut trailing = bytes;
        trailing.push(0);
        assert!(parse(&trailing, 42).is_err());
    }

    #[test]
    fn steam_urls_only_allow_game_launches_and_reject_other_commands() {
        assert_eq!(
            normalize_uri(" steam://rungameid/000123 ").unwrap(),
            "steam://rungameid/123"
        );
        for uri in [
            "https://example.com",
            "file:///C:/game.exe",
            "steam://install/123",
            "steam://uninstall/123",
            "steam://rungameid/0",
            "steam://rungameid/-1",
            "steam://rungameid/18446744073709551616",
            "steam://rungameid/123?x=1",
            "steam://rungameid/123#fragment",
            "steam://user@rungameid/123",
            "steam://rungameid:80/123",
            "steam://rungameid/123/456",
            "steam://rungameid/%31",
            "steam://rungameid/12\n3",
        ] {
            assert!(normalize_uri(uri).is_err(), "{uri}");
        }
    }

    #[test]
    fn imports_utf8_and_utf16_desktop_shortcuts_as_data_without_executing_them() {
        let root = directory();
        let file = root.join("Game.url");
        let text =
            "[InternetShortcut]\r\nURL=steam://rungameid/123456789\r\nIconFile=ignored.exe\r\n";
        fs::write(&file, text).unwrap();
        assert_eq!(read_url(&file).unwrap(), "steam://rungameid/123456789");
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        fs::write(&file, bytes).unwrap();
        assert_eq!(read_url(&file).unwrap(), "steam://rungameid/123456789");
        assert!(parse_url("[Other]\nURL=steam://rungameid/123").is_err());
        assert!(
            parse_url("[InternetShortcut]\nURL=steam://rungameid/123\nURL=steam://rungameid/456")
                .is_err()
        );
        fs::write(&file, vec![b'x'; 65_537]).unwrap();
        assert!(read_url(&file).is_err());
    }

    #[test]
    fn discovery_respects_cancellation() {
        let context = JobContext::new(Arc::new(AtomicBool::new(true)), |_| {});
        assert!(matches!(
            scan(&directory(), &context),
            Err(ApiError::Cancelled)
        ));
    }
}
