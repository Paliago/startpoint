//! Visual identity only: development builds keep the existing library paths.
pub(crate) const IS_DEV: bool = crate::build_branding::is_development(
    cfg!(debug_assertions),
    cfg!(feature = "dev-icon"),
    cfg!(feature = "production-branding"),
);
pub(crate) const NAME: &str = if IS_DEV {
    "Startpoint Rune Dev"
} else {
    "Startpoint"
};
pub(crate) const APP_ID: &str = if IS_DEV {
    "startpoint.dev"
} else {
    "startpoint"
};

pub(crate) const ICON_PNG: &[u8] = if IS_DEV {
    include_bytes!("../assets/icons/startpoint-rune-dev.png")
} else {
    include_bytes!("../assets/icons/startpoint-rune.png")
};

#[cfg(target_os = "windows")]
pub(crate) const WINDOWS_APP_ID: &str = if IS_DEV {
    "Paliago.Startpoint.Rune.Dev"
} else {
    // Retain the installed app/update identity for production.
    "Paliago.Startpoint.App"
};

pub(crate) fn configure_native_identity() {
    #[cfg(target_os = "windows")]
    {
        let id = WINDOWS_APP_ID
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // Called on the UI thread before any window, giving development its own taskbar group.
        let result = unsafe {
            windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(id.as_ptr())
        };
        if result < 0 {
            log::warn!("Could not set the Windows application identity: {result:#x}");
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn configure_window_icon(window: &rune::WindowHandle) {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::{
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
            WindowsAndMessaging::*,
        },
    };
    let _ = window.with_raw_handle(|handle| {
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        // The build script embeds the chosen Rune ICO at resource 1. LR_SHARED icons belong
        // to the module, not this window; recreation cannot leak or double-free an HICON.
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            let dpi = match GetDpiForWindow(hwnd) {
                0 => 96,
                dpi => dpi,
            };
            for (kind, cx, cy) in [
                (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
                (ICON_BIG, SM_CXICON, SM_CYICON),
            ] {
                let icon = LoadImageW(
                    module,
                    1usize as *const u16,
                    IMAGE_ICON,
                    GetSystemMetricsForDpi(cx, dpi),
                    GetSystemMetricsForDpi(cy, dpi),
                    LR_SHARED,
                );
                if !icon.is_null() {
                    SendMessageW(hwnd, WM_SETICON, kind as usize, icon as isize);
                }
            }
        }
    });
}

pub(crate) fn window_title(example_library: bool) -> &'static str {
    match (IS_DEV, example_library) {
        (true, true) => "Startpoint Rune Dev — Example library",
        (false, true) => "Startpoint — Example library",
        (_, false) => NAME,
    }
}
