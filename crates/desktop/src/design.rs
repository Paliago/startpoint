//! App-wide visual design, separate from authored colour themes and page state.
//! Add a profile here, then implement new shell/material/presentation strategies
//! only at their shared rendering boundaries; pages consume the resolved roles.
use gpui::{App, Global};
use gpui_component::{ThemeConfig, ThemeMode};
use startpoint_sdk::{AppearanceSettings, RecentsLayout, StartScreen};
use std::{rc::Rc, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shell {
    ToolbarIslands,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Material {
    SmokedGlass,
    Paper,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecentPresentation {
    Carousel,
    Organic,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ToolbarMaterial {
    pub opacity: f32,
    pub blur_radius: f32,
    pub radius: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PaletteSource {
    Authored,
    Builtin {
        source: &'static str,
        mode: ThemeMode,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Profile {
    pub name: &'static str,
    pub shell: Shell,
    pub material: Material,
    pub recents: RecentPresentation,
    pub recent_count: usize,
    pub palette: PaletteSource,
    pub body_font: &'static str,
    pub display_font: &'static str,
    pub ambient_motion: bool,
    pub motion_interval: Duration,
    pub native_translucency: bool,
    pub sidebar_pages: &'static [StartScreen],
    pub sidebar_in_settings: bool,
    pub toolbar_material: Option<ToolbarMaterial>,
}

const NATIVE_FONT: &str = if cfg!(target_os = "windows") {
    "Bahnschrift"
} else {
    "Avenir Next"
};
const EDITORIAL_BODY: &str = if cfg!(target_os = "windows") {
    "Segoe UI"
} else {
    "Avenir Next"
};
const EDITORIAL_DISPLAY: &str = if cfg!(target_os = "windows") {
    "Georgia"
} else {
    "Baskerville"
};

pub(crate) const CLASSIC: Profile = Profile {
    name: "Classic",
    shell: Shell::ToolbarIslands,
    material: Material::SmokedGlass,
    recents: RecentPresentation::Carousel,
    recent_count: 5,
    palette: PaletteSource::Authored,
    body_font: NATIVE_FONT,
    display_font: NATIVE_FONT,
    ambient_motion: false,
    motion_interval: Duration::from_millis(50),
    native_translucency: true,
    sidebar_pages: &[
        StartScreen::Recents,
        StartScreen::Library,
        StartScreen::Activity,
    ],
    sidebar_in_settings: true,
    toolbar_material: None,
};

pub(crate) const EDITORIAL: Profile = Profile {
    name: "Editorial",
    shell: Shell::ToolbarIslands,
    material: Material::Paper,
    recents: RecentPresentation::Organic,
    recent_count: 6,
    palette: PaletteSource::Builtin {
        source: include_str!("../assets/themes/editorial.json"),
        mode: ThemeMode::Light,
    },
    body_font: EDITORIAL_BODY,
    display_font: EDITORIAL_DISPLAY,
    ambient_motion: true,
    motion_interval: Duration::from_millis(33),
    native_translucency: false,
    sidebar_pages: &[
        StartScreen::Recents,
        StartScreen::Library,
        StartScreen::Activity,
    ],
    sidebar_in_settings: false,
    toolbar_material: Some(ToolbarMaterial {
        opacity: 0.82,
        blur_radius: 12.,
        radius: 12.,
    }),
};

impl Profile {
    pub fn editable_palette(&self) -> bool {
        matches!(self.palette, PaletteSource::Authored)
    }

    pub fn fixed_palette(&self) -> Option<(Rc<ThemeConfig>, ThemeMode)> {
        let PaletteSource::Builtin { source, mode } = self.palette else {
            return None;
        };
        let set = crate::theme::parse(source).expect("validated design palette");
        let configuration = set
            .themes
            .into_iter()
            .find(|variant| variant.mode == mode)
            .expect("design palette mode");
        Some((Rc::new(configuration), mode))
    }

    pub fn sidebar_allowed(&self, screen: StartScreen, settings_open: bool) -> bool {
        self.sidebar_pages.contains(&screen) && (!settings_open || self.sidebar_in_settings)
    }

    pub fn ambient_enabled(
        &self,
        recents_visible: bool,
        ready: bool,
        game_count: usize,
        reduced_motion: bool,
    ) -> bool {
        self.ambient_motion && recents_visible && ready && game_count > 0 && !reduced_motion
    }
}

/// Keep the existing serialized choice compatible. This is the single mapping
/// from persisted selection to a design; no screen name participates in it.
pub(crate) fn resolve(settings: &AppearanceSettings) -> &'static Profile {
    match settings.recents_layout {
        RecentsLayout::Carousel => &CLASSIC,
        RecentsLayout::Editorial => &EDITORIAL,
    }
}

#[derive(Clone, Copy)]
struct Resolved(&'static Profile);
impl Global for Resolved {}

pub(crate) fn install(settings: &AppearanceSettings, cx: &mut App) {
    cx.set_global(Resolved(resolve(settings)));
}

pub(crate) fn current(cx: &App) -> &'static Profile {
    cx.try_global::<Resolved>()
        .map_or(&CLASSIC, |resolved| resolved.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_resolution_is_page_independent_and_does_not_mutate_preferences() {
        let settings = AppearanceSettings {
            recents_layout: RecentsLayout::Editorial,
            theme: "Custom".into(),
            sidebar_visible: true,
            ..Default::default()
        };
        let saved = settings.clone();
        let profile = resolve(&settings);
        assert_eq!(profile.shell, Shell::ToolbarIslands);
        assert_eq!(profile.material, Material::Paper);
        assert_eq!(profile.recent_count, 6);
        assert!(profile.ambient_motion && !profile.native_translucency);
        assert_eq!(settings, saved);
        assert_eq!(resolve(&AppearanceSettings::default()).recent_count, 5);
    }

    #[test]
    fn ambient_motion_has_no_hover_or_focus_dependency() {
        assert!(EDITORIAL.ambient_enabled(true, true, 6, false));
        assert!(!EDITORIAL.ambient_enabled(true, true, 6, true));
        assert!(!EDITORIAL.ambient_enabled(false, true, 6, false));
        assert!(!EDITORIAL.ambient_enabled(true, false, 6, false));
        assert!(!EDITORIAL.ambient_enabled(true, true, 0, false));
        assert!(!CLASSIC.ambient_enabled(true, true, 5, false));
    }

    #[test]
    fn editorial_uses_classic_island_geometry_without_changing_its_palette() {
        assert_eq!(EDITORIAL.shell, CLASSIC.shell);
        let header = EDITORIAL.toolbar_material.unwrap();
        assert!(header.opacity > 0. && header.opacity < 1.);
        assert!(header.blur_radius > 0.);
        assert_eq!(header.radius, 12.);
        assert_eq!(EDITORIAL.material, Material::Paper);
        assert!(!EDITORIAL.native_translucency);
        assert!(EDITORIAL.sidebar_allowed(StartScreen::Recents, false));
        assert!(EDITORIAL.sidebar_allowed(StartScreen::Library, false));
        assert!(!EDITORIAL.sidebar_allowed(StartScreen::Recents, true));
    }
}
