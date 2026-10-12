use gpui::{Global, SharedString};
use gpui_component::{ThemeConfig, ThemeMode, ThemeSet};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};

const DEFAULT_THEME: &str = include_str!("../assets/themes/startpoint.json");

#[derive(Clone)]
pub(crate) struct Catalog {
    pub themes: Vec<ThemeSet>,
    pub warnings: Vec<String>,
    pub files: BTreeMap<String, PathBuf>,
}

impl Global for Catalog {}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            themes: std::iter::once(
                serde_json::from_str(DEFAULT_THEME).expect("built-in Startpoint theme"),
            )
            .chain(
                [
                    include_str!("../assets/themes/forest.json"),
                    include_str!("../assets/themes/ocean.json"),
                    include_str!("../assets/themes/violet.json"),
                ]
                .into_iter()
                .map(|source| parse(source).expect("validated built-in theme")),
            )
            .collect(),
            warnings: Vec::new(),
            files: BTreeMap::new(),
        }
    }
}

impl Catalog {
    pub fn load(directory: &Path) -> Self {
        let mut catalog = Self::default();
        let entries = match fs::create_dir_all(directory).and_then(|_| fs::read_dir(directory)) {
            Ok(entries) => entries,
            Err(error) => {
                catalog
                    .warnings
                    .push(format!("{}: {error}", directory.display()));
                return catalog;
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    if path.is_file()
                        && path
                            .extension()
                            .and_then(|extension| extension.to_str())
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
                    {
                        paths.push(path);
                    }
                }
                Err(error) => catalog.warnings.push(error.to_string()),
            }
        }
        paths.sort();
        for path in paths {
            let result = fs::File::open(&path)
                .and_then(|file| {
                    let mut source = String::new();
                    file.take(256 * 1024 + 1).read_to_string(&mut source)?;
                    Ok(source)
                })
                .map_err(|error| error.to_string())
                .and_then(|source| parse(&source));
            match result {
                Ok(theme) if catalog.contains(&theme.name) => catalog.warnings.push(format!(
                    "{}: duplicate or reserved theme name {}",
                    path.display(),
                    theme.name
                )),
                Ok(theme) => {
                    catalog.files.insert(theme.name.to_ascii_lowercase(), path);
                    catalog.themes.push(theme);
                }
                Err(error) => catalog
                    .warnings
                    .push(format!("{}: {error}", path.display())),
            }
        }
        catalog.themes[1..].sort_by_key(|theme| theme.name.to_lowercase());
        catalog
    }

    pub fn contains(&self, name: &str) -> bool {
        self.themes
            .iter()
            .any(|theme| theme.name.eq_ignore_ascii_case(name))
    }

    pub fn configuration(&self, name: &str, mode: ThemeMode) -> Rc<ThemeConfig> {
        let theme = self
            .themes
            .iter()
            .find(|theme| theme.name.eq_ignore_ascii_case(name))
            .unwrap_or(&self.themes[0]);
        Rc::new(
            theme
                .themes
                .iter()
                .find(|theme| theme.mode == mode)
                .expect("validated light and dark theme variants")
                .clone(),
        )
    }

    pub fn save(
        &self,
        directory: &Path,
        source: &str,
        original_name: &str,
    ) -> Result<String, String> {
        let theme = parse(source)?;
        let key = theme.name.to_ascii_lowercase();
        if self.contains(&theme.name)
            && (!self.files.contains_key(&key) || !theme.name.eq_ignore_ascii_case(original_name))
        {
            return Err("Choose a new name; another theme already uses this name".into());
        }
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?;
        // A custom theme's name can change without leaving its old file behind.
        // Built-ins have no file and are always saved as new custom copies.
        let path = self
            .files
            .get(&original_name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| {
                directory.join(format!(
                    "theme-{}-{:09}.json",
                    timestamp.as_secs(),
                    timestamp.subsec_nanos()
                ))
            });
        let temporary = path.with_extension(format!("{}.temporary", timestamp.subsec_nanos()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        let result = file
            .write_all(source.as_bytes())
            .and_then(|_| file.sync_all());
        drop(file);
        if let Err(error) = result.and_then(|_| fs::rename(&temporary, &path)) {
            let _ = fs::remove_file(&temporary);
            return Err(error.to_string());
        }
        Ok(theme.name.to_string())
    }
}

pub(crate) fn source(theme: &ThemeSet) -> Result<String, String> {
    let mut document = serde_json::to_value(theme).map_err(|error| error.to_string())?;
    if let Some(variants) = document.get_mut("themes").and_then(Value::as_array_mut) {
        for variant in variants {
            if let Some(colors) = variant.get_mut("colors").and_then(Value::as_object_mut) {
                colors.retain(|_, value| !value.is_null());
            }
        }
    }
    serde_json::to_string_pretty(&document).map_err(|error| error.to_string())
}

pub(crate) fn parse(source: &str) -> Result<ThemeSet, String> {
    if source.len() > 256 * 1024 {
        return Err("Theme files must not exceed 256 KiB".into());
    }
    let mut document: Value = serde_json::from_str(source).map_err(|error| error.to_string())?;
    let name = document
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| {
            !name.trim().is_empty() && name.len() <= 80 && !name.chars().any(char::is_control)
        })
        .ok_or("A theme needs a nonempty name of at most 80 bytes")?
        .to_string();
    let variants = document
        .get_mut("themes")
        .and_then(Value::as_array_mut)
        .filter(|variants| variants.len() == 2)
        .ok_or("A theme must contain one light and one dark variant")?;
    let defaults: ThemeSet =
        serde_json::from_str(DEFAULT_THEME).expect("built-in Startpoint theme");
    let mut modes = Vec::new();
    for variant in variants {
        let mode = match variant.get("mode").and_then(Value::as_str) {
            Some("light") => ThemeMode::Light,
            Some("dark") => ThemeMode::Dark,
            _ => return Err("Variant mode must be light or dark".into()),
        };
        if modes.contains(&mode) {
            return Err("A theme must contain one light and one dark variant".into());
        }
        modes.push(mode);
        let default = defaults
            .themes
            .iter()
            .find(|theme| theme.mode == mode)
            .expect("built-in theme variants");
        let mut merged = serde_json::to_value(default).map_err(|error| error.to_string())?;
        let overrides = variant
            .as_object()
            .ok_or("Theme variants must be objects")?;
        for (key, value) in overrides {
            let target = merged
                .get_mut(key)
                .ok_or_else(|| format!("Unknown theme property: {key}"))?;
            if key != "colors" {
                *target = value.clone();
                continue;
            }
            for (token, color) in value.as_object().ok_or("Theme colors must be an object")? {
                let target = target
                    .get_mut(token)
                    .ok_or_else(|| format!("Unknown color token: {token}"))?;
                let hex = color
                    .as_str()
                    .and_then(|color| color.strip_prefix('#'))
                    .filter(|hex| {
                        matches!(hex.len(), 6 | 8)
                            && hex.bytes().all(|digit| digit.is_ascii_hexdigit())
                    })
                    .ok_or_else(|| format!("{token} must be #RRGGBB or #RRGGBBAA"))?;
                if matches!(
                    token.as_str(),
                    "background" | "foreground" | "popover.background" | "popover.foreground"
                ) && hex.len() == 8
                    && !hex[6..].eq_ignore_ascii_case("ff")
                {
                    return Err(format!("{token} must be opaque"));
                }
                *target = color.clone();
            }
        }
        let mut configuration: ThemeConfig =
            serde_json::from_value(merged).map_err(|error| error.to_string())?;
        if configuration
            .font_size
            .is_some_and(|size| !(10. ..=32.).contains(&size))
            || configuration
                .mono_font_size
                .is_some_and(|size| !(10. ..=32.).contains(&size))
            || configuration.radius.is_some_and(|radius| radius > 48)
            || configuration.radius_lg.is_some_and(|radius| radius > 48)
        {
            return Err("Font sizes must be 10–32 pixels and radii at most 48 pixels".into());
        }
        configuration.name = SharedString::from(format!("{name} {}", mode.name()));
        *variant = serde_json::to_value(configuration).map_err(|error| error.to_string())?;
    }
    serde_json::from_value(document).map_err(|error| error.to_string())
}

pub(crate) fn contrast(background: gpui::Hsla, foreground: gpui::Hsla) -> f32 {
    let luminance = |color: gpui::Hsla| {
        let color = color.to_rgb();
        [color.r, color.g, color.b]
            .into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(channel, weight)| {
                weight
                    * if channel <= 0.04045 {
                        channel / 12.92
                    } else {
                        ((channel + 0.055) / 1.055).powf(2.4)
                    }
            })
            .sum::<f32>()
    };
    let first = luminance(background);
    let second = luminance(foreground);
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
}

#[cfg(test)]
#[test]
fn custom_themes_inherit_defaults_and_validate_colors() {
    use gpui_component::Theme;

    let theme = parse(r##"{"name":"Custom","themes":[{"mode":"light","colors":{"primary.background":"#124578","overlay":"#00000080"}},{"mode":"dark"}]}"##)
        .expect("custom theme");
    let mut applied = Theme::default();
    applied.apply_config(&Rc::new(theme.themes[0].clone()));
    assert_eq!(applied.primary, gpui::Hsla::from(gpui::rgb(0x124578)));
    assert_eq!(applied.background, gpui::Hsla::from(gpui::rgb(0xf6f5f1)));
    assert_eq!(applied.radius, gpui::px(8.));
    assert!((applied.overlay.a - 128. / 255.).abs() < 0.0001);
    assert_eq!(
        Catalog::default()
            .configuration("Unavailable", ThemeMode::Light)
            .name,
        "Startpoint Light"
    );
    let catalog = Catalog::default();
    for theme in &catalog.themes {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            applied.apply_config(&catalog.configuration(&theme.name, mode));
            for (background, foreground) in [
                (applied.background, applied.foreground),
                (applied.muted, applied.muted_foreground),
                (applied.primary, applied.primary_foreground),
                (applied.primary_hover, applied.primary_foreground),
                (applied.primary_active, applied.primary_foreground),
                (applied.secondary, applied.secondary_foreground),
                (applied.accent, applied.accent_foreground),
            ] {
                assert!(
                    contrast(background, foreground) >= 4.5,
                    "{} {} has insufficient text contrast",
                    theme.name,
                    mode.name()
                );
            }
        }
    }
    for invalid in ["#ééé", "#00000g", "orange", "#123", "#123456789"] {
        let source = format!(
            r#"{{"name":"Custom","themes":[{{"mode":"light","colors":{{"primary.background":"{invalid}"}}}},{{"mode":"dark"}}]}}"#
        );
        assert!(parse(&source).is_err(), "accepted {invalid}");
    }
    for invalid in [
        r#"{"name":"Custom","themes":[{"mode":"light"},{"mode":"light"}]}"#,
        r#"{"name":"Custom","themes":[{"mode":"dark"}]}"#,
        r##"{"name":"Custom","themes":[{"mode":"light","colors":{"background":"#00000080"}},{"mode":"dark"}]}"##,
        r##"{"name":"Custom","themes":[{"mode":"light","colors":{"unknown":"#000000"}},{"mode":"dark"}]}"##,
        r#"{"name":"Custom","themes":[{"mode":"light","font.size":-1},{"mode":"dark"}]}"#,
    ] {
        assert!(parse(invalid).is_err(), "accepted {invalid}");
    }
    let directory = std::env::temp_dir().join(format!(
        "startpoint-theme-editor-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let mut edited = theme.clone();
    edited.name = "Edited theme".into();
    edited.themes[1].font_size = Some(18.);
    edited.themes[1].radius = Some(0);
    let document = source(&edited).expect("editor serialization");
    assert_eq!(
        catalog
            .save(&directory, &document, "Startpoint")
            .expect("save copy"),
        "Edited theme"
    );
    let loaded = Catalog::load(&directory);
    assert!(loaded.warnings.is_empty());
    assert_eq!(
        loaded
            .configuration("Edited theme", ThemeMode::Dark)
            .font_size,
        Some(18.)
    );
    assert_eq!(
        loaded.configuration("Edited theme", ThemeMode::Dark).radius,
        Some(0)
    );
    assert!(loaded.save(&directory, &document, "Other theme").is_err());
    edited.themes[1].font_size = Some(20.);
    loaded
        .save(
            &directory,
            &source(&edited).expect("serialize edit"),
            "Edited theme",
        )
        .expect("update saved theme");
    assert_eq!(
        Catalog::load(&directory)
            .configuration("Edited theme", ThemeMode::Dark)
            .font_size,
        Some(20.)
    );
    let before_rename = Catalog::load(&directory);
    let original_path = before_rename.files["edited theme"].clone();
    edited.name = "Renamed theme".into();
    before_rename
        .save(&directory, &source(&edited).unwrap(), "Edited theme")
        .expect("rename custom theme");
    let renamed = Catalog::load(&directory);
    assert!(!renamed.contains("Edited theme"));
    assert_eq!(renamed.files["renamed theme"], original_path);
    assert_eq!(
        renamed
            .configuration("Renamed theme", ThemeMode::Dark)
            .font_size,
        Some(20.)
    );
    edited.name = "Startpoint".into();
    assert!(
        renamed
            .save(&directory, &source(&edited).unwrap(), "Renamed theme")
            .is_err()
    );
    assert!(Catalog::load(&directory).contains("Renamed theme"));
    assert!(
        catalog
            .save(&directory, DEFAULT_THEME, "Startpoint")
            .is_err()
    );
    for (name, size) in [("Å", 16.), ("å", 22.), ("K", 17.), ("K", 23.)] {
        edited.name = name.into();
        edited.themes[1].font_size = Some(size);
        Catalog::load(&directory)
            .save(
                &directory,
                &source(&edited).expect("serialize Unicode theme"),
                "Startpoint",
            )
            .expect("save distinct Unicode theme");
    }
    let loaded = Catalog::load(&directory);
    assert!(loaded.warnings.is_empty());
    for (name, size) in [("Å", 16.), ("å", 22.), ("K", 17.), ("K", 23.)] {
        assert_eq!(
            loaded.configuration(name, ThemeMode::Dark).font_size,
            Some(size)
        );
    }
    edited.name = "å".into();
    edited.themes[1].font_size = Some(24.);
    let document = source(&edited).expect("serialize Unicode edit");
    assert!(loaded.save(&directory, &document, "Å").is_err());
    loaded
        .save(&directory, &document, "å")
        .expect("update the original Unicode theme");
    let loaded = Catalog::load(&directory);
    assert_eq!(
        loaded.configuration("Å", ThemeMode::Dark).font_size,
        Some(16.)
    );
    assert_eq!(
        loaded.configuration("å", ThemeMode::Dark).font_size,
        Some(24.)
    );
    edited.name = "k".into();
    assert!(
        loaded
            .save(
                &directory,
                &source(&edited).expect("serialize ASCII collision"),
                "Startpoint"
            )
            .is_err()
    );
    fs::remove_dir_all(&directory).expect("remove test theme directory");
}
