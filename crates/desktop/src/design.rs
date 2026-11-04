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
    Gallery,
    Cinematic,
    InfiniteWall,
    FilmReel,
    ArtBook,
    Diagonal,
    Mosaic,
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
    pub header_height: f32,
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
    header_height: crate::window_chrome::HEADER_HEIGHT,
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
    header_height: crate::window_chrome::HEADER_HEIGHT,
};

pub(crate) const GALLERY: Profile = Profile {
    name: "Gallery",
    shell: Shell::ToolbarIslands,
    material: Material::Paper,
    recents: RecentPresentation::Gallery,
    // Recents is a fixed shortlist; Library remains the complete collection.
    recent_count: 5,
    palette: PaletteSource::Builtin {
        source: include_str!("../assets/themes/gallery.json"),
        mode: ThemeMode::Dark,
    },
    body_font: EDITORIAL_BODY,
    display_font: EDITORIAL_DISPLAY,
    ambient_motion: false,
    motion_interval: Duration::from_millis(16),
    native_translucency: false,
    sidebar_pages: &[],
    sidebar_in_settings: false,
    toolbar_material: Some(ToolbarMaterial {
        opacity: 0.82,
        blur_radius: 12.,
        radius: 12.,
    }),
    header_height: crate::window_chrome::HEADER_HEIGHT,
};

pub(crate) const CINEMATIC: Profile = Profile {
    name: "Cinematic",
    recents: RecentPresentation::Cinematic,
    // The filmstrip is a viewport into every installed game, not a shortlist.
    recent_count: usize::MAX,
    ..GALLERY
};

pub(crate) const INFINITE_WALL: Profile = Profile {
    name: "Infinite Wall",
    recents: RecentPresentation::InfiniteWall,
    ..CINEMATIC
};
pub(crate) const FILM_REEL: Profile = Profile {
    name: "Film Reel",
    recents: RecentPresentation::FilmReel,
    ..CINEMATIC
};
pub(crate) const ART_BOOK: Profile = Profile {
    name: "Art Book",
    recents: RecentPresentation::ArtBook,
    recent_count: usize::MAX,
    sidebar_pages: &[],
    ambient_motion: false,
    ..EDITORIAL
};
pub(crate) const DIAGONAL: Profile = Profile {
    name: "Diagonal",
    recents: RecentPresentation::Diagonal,
    ..CINEMATIC
};
pub(crate) const MOSAIC: Profile = Profile {
    name: "Mosaic",
    recents: RecentPresentation::Mosaic,
    ..CINEMATIC
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
        RecentsLayout::Gallery => &GALLERY,
        RecentsLayout::Cinematic => &CINEMATIC,
        RecentsLayout::InfiniteWall => &INFINITE_WALL,
        RecentsLayout::FilmReel => &FILM_REEL,
        RecentsLayout::ArtBook => &ART_BOOK,
        RecentsLayout::Diagonal => &DIAGONAL,
        RecentsLayout::Mosaic => &MOSAIC,
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

    #[test]
    fn gallery_is_an_opaque_dark_exhibition_without_sidebars_or_automatic_advancement() {
        let saved = AppearanceSettings {
            recents_layout: RecentsLayout::Gallery,
            sidebar_visible: true,
            library_details: true,
            theme: "Custom".into(),
            ..Default::default()
        };
        let before = saved.clone();
        let profile = resolve(&saved);
        assert_eq!(profile.shell, CLASSIC.shell);
        assert_eq!(profile.header_height, CLASSIC.header_height);
        assert_eq!(
            profile.toolbar_material.unwrap().radius,
            EDITORIAL.toolbar_material.unwrap().radius
        );
        assert_eq!(profile.recents, RecentPresentation::Gallery);
        assert_eq!(profile.recent_count, 5);
        assert!(!profile.native_translucency && !profile.ambient_motion);
        for screen in [
            StartScreen::Recents,
            StartScreen::Library,
            StartScreen::Activity,
        ] {
            assert!(!profile.sidebar_allowed(screen, false));
            assert!(!profile.sidebar_allowed(screen, true));
        }
        assert!(profile.fixed_palette().unwrap().1.is_dark());
        assert_eq!(saved, before);
    }

    #[test]
    fn cinematic_unifies_all_installed_games_without_mutating_other_design_preferences() {
        let saved = AppearanceSettings {
            recents_layout: RecentsLayout::Cinematic,
            sidebar_visible: true,
            theme: "Custom".into(),
            ..Default::default()
        };
        let before = saved.clone();
        let profile = resolve(&saved);
        assert_eq!(profile.name, "Cinematic");
        assert_eq!(profile.recent_count, usize::MAX);
        assert_eq!(profile.recents, RecentPresentation::Cinematic);
        assert!(!profile.ambient_motion && !profile.native_translucency);
        assert!(!profile.sidebar_allowed(StartScreen::Recents, false));
        assert!(profile.fixed_palette().unwrap().1.is_dark());
        assert_eq!(saved, before);
    }

    #[test]
    fn artwork_scene_designs_keep_shared_chrome_and_provider_independent_scope() {
        for (layout, kind) in [
            (
                RecentsLayout::InfiniteWall,
                RecentPresentation::InfiniteWall,
            ),
            (RecentsLayout::FilmReel, RecentPresentation::FilmReel),
            (RecentsLayout::ArtBook, RecentPresentation::ArtBook),
            (RecentsLayout::Diagonal, RecentPresentation::Diagonal),
            (RecentsLayout::Mosaic, RecentPresentation::Mosaic),
        ] {
            let settings = AppearanceSettings {
                recents_layout: layout,
                sidebar_visible: true,
                ..Default::default()
            };
            let profile = resolve(&settings);
            assert_eq!(profile.shell, Shell::ToolbarIslands);
            assert_eq!(profile.header_height, CLASSIC.header_height);
            assert_eq!(profile.recents, kind);
            assert_eq!(profile.recent_count, usize::MAX);
            assert!(!profile.sidebar_allowed(StartScreen::Recents, false));
            assert!(!profile.ambient_motion);
        }
    }
}
