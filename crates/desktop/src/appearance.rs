use gpui::{App, Hsla, Window, WindowAppearance, WindowBackgroundAppearance};
use gpui_component::{Theme, ThemeConfig, ThemeMode, ThemeSet};
use startpoint_sdk::{AppearanceMode, AppearanceSettings};
use std::rc::Rc;

pub(crate) fn apply(settings: &AppearanceSettings, window: Option<&mut Window>, cx: &mut App) {
    cx.set_global(crate::glass::Effects {
        window_translucency: settings.window_translucency,
    });
    let mode = settings.mode;
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
    let themes = cx.global::<crate::theme::Catalog>();
    let light = themes.configuration(&settings.theme, ThemeMode::Light);
    let dark = themes.configuration(&settings.theme, ThemeMode::Dark);
    apply_configurations(light, dark, resolved, window, cx);
}

pub(crate) fn preview(theme: &ThemeSet, mode: ThemeMode, window: &mut Window, cx: &mut App) {
    sync_native_appearance(if mode.is_dark() {
        AppearanceMode::Dark
    } else {
        AppearanceMode::Light
    });
    let light = theme
        .themes
        .iter()
        .find(|variant| variant.mode == ThemeMode::Light);
    let dark = theme
        .themes
        .iter()
        .find(|variant| variant.mode == ThemeMode::Dark);
    if let (Some(light), Some(dark)) = (light, dark) {
        apply_configurations(
            Rc::new(light.clone()),
            Rc::new(dark.clone()),
            mode,
            Some(window),
            cx,
        );
    }
}

fn apply_configurations(
    light: Rc<ThemeConfig>,
    dark: Rc<ThemeConfig>,
    resolved: ThemeMode,
    mut window: Option<&mut Window>,
    cx: &mut App,
) {
    let native_font = if resolved.is_dark() {
        dark.font_family.is_none()
    } else {
        light.font_family.is_none()
    };
    let theme = Theme::global_mut(cx);
    theme.light_theme = light;
    theme.dark_theme = dark;
    Theme::change(resolved, window.as_deref_mut(), cx);
    if native_font {
        Theme::global_mut(cx).font_family = if cfg!(target_os = "windows") {
            "Bahnschrift"
        } else {
            "Avenir Next"
        }
        .into();
    }
    crate::components::configure_glass_theme(Theme::global_mut(cx));
    if let Some(window) = window {
        window.set_background_appearance(background(cx));
    }
    cx.refresh_windows();
}

pub(crate) fn background(cx: &App) -> WindowBackgroundAppearance {
    crate::glass::native_background(crate::glass::effects(cx))
}

pub(crate) fn surface(cx: &App) -> Hsla {
    crate::glass::base(cx)
}

#[cfg(target_os = "macos")]
fn native_window(window: &Window) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // GPUI owns the live AppKit view for the lifetime of this window handle.
    let object = unsafe { handle.ns_view.cast::<AnyObject>().as_ref() };
    object.downcast_ref::<NSView>()?.window()
}

pub(crate) fn configure_window(_window: &Window) {
    #[cfg(target_os = "macos")]
    {
        use objc2::{
            runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
            sel,
        };
        use objc2_app_kit::NSView;
        use objc2_foundation::NSRect;

        unsafe extern "C-unwind" fn titlebar_content(view: *mut NSView, _: Sel) -> NSRect {
            // Objective-C invokes this method only with a live instance of our NSView subclass.
            unsafe { (*view).bounds() }
        }

        let Some(content) = native_window(_window).and_then(|window| window.contentView()) else {
            return;
        };
        for view in content.subviews() {
            if view.class().name().to_bytes() != b"GPUIView" {
                continue;
            }
            let class = AnyClass::get(c"StartpointContentView").or_else(|| {
                let mut class = ClassBuilder::new(c"StartpointContentView", view.class())?;
                let implementation: unsafe extern "C-unwind" fn(*mut NSView, Sel) -> NSRect =
                    titlebar_content;
                // GPUI 0.2.2 otherwise leaves custom controls in AppKit's title-bar hit area.
                unsafe {
                    class.add_method(sel!(_opaqueRectForWindowMoveWhenInTitlebar), implementation)
                };
                Some(class.register())
            });
            if let Some(class) = class {
                // The subclass adds no storage and preserves every GPUIView implementation.
                unsafe { AnyObject::set_class(&view, class) };
            }
        }
    }
}

pub(crate) fn start_window_move(window: &Window) {
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSApplication;

        let Some(thread) = MainThreadMarker::new() else {
            return;
        };
        if let Some(native) = native_window(window)
            && let Some(event) = NSApplication::sharedApplication(thread).currentEvent()
        {
            native.performWindowDragWithEvent(&event);
        }
    }
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::{
            Input::KeyboardAndMouse::ReleaseCapture,
            WindowsAndMessaging::{HTCAPTION, PostMessageW, SC_MOVE, WM_SYSCOMMAND},
        };
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        // Post the native move request after GPUI finishes dispatching the mouse event.
        // Calling SendMessage here would re-enter GPUI with its input callback borrowed.
        unsafe {
            ReleaseCapture();
            PostMessageW(
                handle.hwnd.get() as _,
                WM_SYSCOMMAND,
                (SC_MOVE | HTCAPTION) as usize,
                0,
            );
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    window.start_window_move();
}

pub(crate) fn toggle_window_maximize(window: &Window) {
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IsZoomed, SW_MAXIMIZE, SW_RESTORE, ShowWindowAsync,
        };
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = handle.hwnd.get() as _;
        // GPUI 0.2.2's Windows zoom() only maximizes; restore must be explicit.
        unsafe {
            ShowWindowAsync(
                hwnd,
                if IsZoomed(hwnd) != 0 {
                    SW_RESTORE
                } else {
                    SW_MAXIMIZE
                },
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    window.zoom_window();
}

pub(crate) fn reduced_motion() -> bool {
    #[cfg(target_os = "macos")]
    return objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion();
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
        };
        let mut enabled = 1u32;
        // Windows writes one BOOL to the live local value; the call retains no pointer.
        return unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                std::ptr::from_mut(&mut enabled).cast(),
                0,
            ) != 0
                && enabled == 0
        };
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    false
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
