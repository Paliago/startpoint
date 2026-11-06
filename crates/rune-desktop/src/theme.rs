use rune::{Color, TextEdit};
use rune_kit::{Kit, PaletteSeed};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use startpoint_sdk::{AppearanceMode, AppearanceSettings};
use std::collections::BTreeMap;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ThemeDocument {
    pub name: String,
    pub themes: Vec<ThemeVariant>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ThemeVariant {
    pub name: String,
    pub mode: String,
    #[serde(default = "default_radius")]
    pub radius: f32,
    pub colors: BTreeMap<String, String>,
}

fn default_radius() -> f32 {
    8.0
}

pub struct ThemeDraft {
    pub name: TextEdit,
    pub background: TextEdit,
    pub foreground: TextEdit,
    pub accent: TextEdit,
    pub border: TextEdit,
    pub radius: TextEdit,
    pub document: ThemeDocument,
    pub dark: bool,
}

impl ThemeDraft {
    pub fn new(document: ThemeDocument, dark: bool) -> Self {
        let variant = document.variant(dark);
        Self {
            name: TextEdit::new(&document.name),
            background: TextEdit::new(
                variant
                    .and_then(|v| v.colors.get("background"))
                    .cloned()
                    .unwrap_or_else(|| "#1b1e22".into()),
            ),
            foreground: TextEdit::new(
                variant
                    .and_then(|v| v.colors.get("foreground"))
                    .cloned()
                    .unwrap_or_else(|| "#f1f0ec".into()),
            ),
            accent: TextEdit::new(
                variant
                    .and_then(|v| v.colors.get("primary.background"))
                    .cloned()
                    .unwrap_or_else(|| "#75caba".into()),
            ),
            border: TextEdit::new(
                variant
                    .and_then(|v| v.colors.get("border"))
                    .cloned()
                    .unwrap_or_else(|| "#393c40".into()),
            ),
            radius: TextEdit::new(variant.map_or(8.0, |v| v.radius).to_string()),
            document,
            dark,
        }
    }

    pub fn resolve(&self) -> Result<ThemeDocument, String> {
        let name = self.name.text.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err("Use a theme name between 1 and 80 characters.".into());
        }
        let radius: f32 = self
            .radius
            .text
            .parse()
            .map_err(|_| "Corner radius must be a number.")?;
        if !radius.is_finite() || !(0.0..=24.0).contains(&radius) {
            return Err("Corner radius must be between 0 and 24.".into());
        }
        let mut document = self.document.clone();
        document.name = name.into();
        let variant = document
            .themes
            .iter_mut()
            .find(|v| (v.mode == "dark") == self.dark)
            .ok_or("Theme has no matching colour mode.")?;
        for (key, value) in [
            ("background", &self.background),
            ("foreground", &self.foreground),
            ("primary.background", &self.accent),
            ("border", &self.border),
        ] {
            if rune::color::hex(value.text.trim()).is_none() {
                return Err(format!(
                    "Invalid colour for {key}. Use a hexadecimal colour."
                ));
            }
            variant.colors.insert(key.into(), value.text.trim().into());
        }
        variant.radius = radius;
        Ok(document)
    }
}

impl ThemeDocument {
    fn variant(&self, dark: bool) -> Option<&ThemeVariant> {
        self.themes
            .iter()
            .find(|variant| (variant.mode == "dark") == dark)
            .or_else(|| self.themes.first())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.chars().count() > 80 || self.themes.is_empty() {
            return Err("Invalid theme name or missing variants.".into());
        }
        for variant in &self.themes {
            if !["light", "dark"].contains(&variant.mode.as_str())
                || !variant.radius.is_finite()
                || !(0.0..=24.0).contains(&variant.radius)
            {
                return Err("Invalid theme mode or radius.".into());
            }
            for key in ["background", "foreground", "primary.background"] {
                if variant
                    .colors
                    .get(key)
                    .and_then(|value| rune::color::hex(value))
                    .is_none()
                {
                    return Err(format!("Missing or invalid theme colour: {key}"));
                }
            }
        }
        Ok(())
    }

    pub fn kit(&self, appearance: &AppearanceSettings, system_dark: bool) -> Kit {
        let dark = match appearance.mode {
            AppearanceMode::System => system_dark,
            AppearanceMode::Light => false,
            AppearanceMode::Dark => true,
        };
        let fallback = rune::tok::theme(dark);
        let color = |key: &str, fallback: Color| {
            self.variant(dark)
                .and_then(|variant| variant.colors.get(key))
                .and_then(|value| rune::color::hex(value))
                .unwrap_or(fallback)
        };
        let mut kit = Kit::from_palette(PaletteSeed {
            bg: color("background", fallback.bg),
            fg: color("foreground", fallback.text),
            accent: color("primary.background", rune::tok::accent(0, dark)),
        });
        kit.c.border_base = color("border", kit.c.border_base);
        kit.c.border_muted = kit.c.border_base.alpha(0.55);
        kit.c.std_border = kit.c.border_base;
        kit.c.border_focus = color("ring", kit.c.border_focus);
        kit.c.layer_01 = color("popover.background", kit.c.layer_01);
        kit.c.layer_02 = color("muted.background", kit.c.layer_02);
        kit.c.layer_03 = color("secondary.background", kit.c.layer_03);
        kit.c.layer_04 = color("secondary.hover.background", kit.c.layer_04);
        kit.c.surface_base = kit.c.layer_02;
        kit.c.surface_weak = kit.c.layer_01;
        kit.c.surface_raised = kit.c.layer_01;
        kit.c.surface_raised_base = kit.c.layer_01;
        kit.c.surface_raised_hover = kit.c.layer_03;
        kit.c.surface_disabled = kit.c.layer_02;
        kit.c.text_muted = color("muted.foreground", kit.c.text_muted);
        kit.c.text_faint = kit.c.text_muted;
        kit.c.icon_muted = kit.c.text_muted;
        kit.c.icon_faint = kit.c.text_muted;
        kit.c.text_weak = kit.c.text_muted;
        kit.c.icon_weaker = kit.c.text_muted;
        kit.radius.md = appearance.controls.radius;
        kit.radius.lg = self.variant(dark).map_or(8.0, |variant| variant.radius);
        kit.elevation.button_neutral.clear();
        kit.elevation.button_contrast.clear();
        kit.elevation.elements.clear();
        kit.c.bg_button_neutral = kit.c.layer_02;
        kit.c.submit_highlight = Color::TRANSPARENT;
        kit.c.depth_top = Color::TRANSPARENT;
        kit.c.depth_bottom = Color::TRANSPARENT;
        kit.c.bg_contrast = color("primary.background", kit.c.text_accent);
        kit.c.text_contrast = color(
            "primary.foreground",
            if dark { Color::BLACK } else { Color::WHITE },
        );
        kit.c.icon_contrast = kit.c.text_contrast;
        kit.c.bg_icon_button_contrast = kit.c.bg_contrast;
        kit.c.submit_fg = kit.c.text_contrast;
        kit.c.submit_icon = kit.c.text_contrast;
        kit.c.scrim = Color::BLACK.alpha(if dark { 0.4 } else { 0.2 });
        kit
    }
}

pub fn builtins() -> Vec<ThemeDocument> {
    [
        include_str!("../../desktop/assets/themes/startpoint.json"),
        include_str!("../../desktop/assets/themes/forest.json"),
        include_str!("../../desktop/assets/themes/ocean.json"),
        include_str!("../../desktop/assets/themes/violet.json"),
    ]
    .into_iter()
    .filter_map(|source| serde_json::from_str(source).ok())
    .collect()
}

pub fn file_name(name: &str) -> String {
    format!(
        "theme-{:x}.json",
        Sha256::digest(name.to_lowercase().as_bytes())
    )
}

pub fn is_builtin(name: &str) -> bool {
    builtins()
        .iter()
        .any(|theme| theme.name.eq_ignore_ascii_case(name))
}
