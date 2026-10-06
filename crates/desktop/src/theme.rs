use gpui::{Global, SharedString};
use gpui_component::{ThemeConfig, ThemeMode, ThemeSet};
use serde_json::Value;
use std::{fs, io::Read, path::Path, rc::Rc};

const DEFAULT_THEME: &str = include_str!("../assets/themes/startpoint.json");

pub(crate) struct Catalog {
    pub themes: Vec<ThemeSet>,
    pub warnings: Vec<String>,
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
                Ok(theme) => catalog.themes.push(theme),
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
}

fn parse(source: &str) -> Result<ThemeSet, String> {
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
                let first = luminance(background);
                let second = luminance(foreground);
                assert!(
                    (first.max(second) + 0.05) / (first.min(second) + 0.05) >= 4.5,
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
}
