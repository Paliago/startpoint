use gpui::{App, Hsla, Window, WindowAppearance, WindowBackgroundAppearance, px, rgb};
use gpui_component::{ActiveTheme, Theme, ThemeMode};
use startpoint_sdk::AppearanceMode;

pub(crate) fn apply(mode: AppearanceMode, mut window: Option<&mut Window>, cx: &mut App) {
    sync_native_appearance(mode);
    let system = window
        .as_ref()
        .map(|window| window.appearance())
        .unwrap_or_else(|| cx.window_appearance());
    let resolved = match mode {
        AppearanceMode::Light => ThemeMode::Light,
        AppearanceMode::Dark => ThemeMode::Dark,
        AppearanceMode::System => match system {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
            _ => ThemeMode::Light,
        },
    };
    Theme::change(resolved, window.as_deref_mut(), cx);
    let theme = Theme::global_mut(cx);
    let dark = theme.is_dark();
    theme.font_family = if cfg!(target_os = "windows") {
        "Bahnschrift"
    } else {
        "Avenir Next"
    }
    .into();
    theme.font_size = px(14.);
    theme.radius = px(8.);
    theme.radius_lg = px(12.);
    theme.shadow = false;
    theme.background = rgb(if dark { 0x1b1e22 } else { 0xf6f5f1 }).into();
    theme.foreground = rgb(if dark { 0xf1f0ec } else { 0x17202b }).into();
    theme.border = rgb(if dark { 0x393c40 } else { 0xd8d7d2 }).into();
    theme.input = theme.border;
    theme.muted = rgb(if dark { 0x282c30 } else { 0xecebe6 }).into();
    theme.muted_foreground = rgb(if dark { 0xa4a7aa } else { 0x74777b }).into();
    theme.secondary = rgb(if dark { 0x303438 } else { 0xfdfcf9 }).into();
    theme.secondary_foreground = theme.foreground;
    theme.secondary_hover = theme.muted;
    theme.secondary_active = theme.border;
    theme.primary = rgb(0xff650d).into();
    theme.primary_hover = rgb(0xf57824).into();
    theme.primary_active = rgb(0xe85400).into();
    theme.primary_foreground = rgb(0xffffff).into();
    theme.ring = theme.primary;
    theme.caret = theme.primary;
    theme.accent = rgb(if dark { 0x493123 } else { 0xffeadd }).into();
    theme.accent_foreground = rgb(if dark { 0xffa268 } else { 0xa33a00 }).into();
    theme.selection = theme.accent;
    theme.switch = theme.border;
    theme.switch_thumb = rgb(0xffffff).into();
    theme.title_bar = gpui::transparent_black();
    theme.title_bar_border = gpui::transparent_black();
    theme.popover = theme.secondary;
    theme.popover_foreground = theme.foreground;
    if let Some(window) = window {
        window.set_background_appearance(background());
    }
    cx.refresh_windows();
}

pub(crate) fn background() -> WindowBackgroundAppearance {
    WindowBackgroundAppearance::Opaque
}

pub(crate) fn surface(cx: &App) -> Hsla {
    cx.theme().background
}

#[cfg(target_os = "macos")]
fn sync_native_appearance(mode: AppearanceMode) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    };
    let Some(marker) = MainThreadMarker::new() else {
        return;
    };
    // AppKit exports immutable appearance-name constants; access stays on the UI thread.
    let appearance = match mode {
        AppearanceMode::System => None,
        AppearanceMode::Light => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }),
        AppearanceMode::Dark => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }),
    };
    let application = NSApplication::sharedApplication(marker);
    if application.appearance() != appearance {
        application.setAppearance(appearance.as_deref());
    }
}

#[cfg(not(target_os = "macos"))]
fn sync_native_appearance(_: AppearanceMode) {}
