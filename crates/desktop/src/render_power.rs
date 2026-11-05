//! One suspension policy shared by ambient art, automatic browsing and wgpu.
//! Native detection reads foreground/window geometry only: no process scans,
//! screen capture, global hooks, or high-frequency background polling.
use gpui::Window;
use std::time::{Duration, Instant};

pub(crate) const PROBE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Environment {
    pub minimized: bool,
    pub fullscreen_other_app: bool,
}

pub(crate) fn should_pause(
    pause_in_background: bool,
    active: bool,
    environment: Environment,
) -> bool {
    environment.minimized || environment.fullscreen_other_app || pause_in_background && !active
}

#[derive(Default)]
pub(crate) struct State {
    environment: Environment,
    probed: Option<Instant>,
    paused: bool,
}

impl State {
    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn refresh(
        &mut self,
        window: &Window,
        pause_in_background: bool,
        force: bool,
        render_offscreen: bool,
    ) -> bool {
        let now = Instant::now();
        if force
            || self
                .probed
                .is_none_or(|last| now.duration_since(last) >= PROBE_INTERVAL)
        {
            self.environment = native_environment(window);
            if render_offscreen {
                self.environment.minimized = false;
            }
            self.probed = Some(now);
        }
        let paused = should_pause(
            pause_in_background,
            window.is_window_active(),
            self.environment,
        );
        let changed = self.paused != paused;
        self.paused = paused;
        changed
    }
}

// Client bounds, not the decorated outer frame: an ordinary maximized window
// must not be mistaken for a borderless/exclusive-fullscreen game.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn covers_monitor(client: [i32; 4], monitor: [i32; 4]) -> bool {
    const TOLERANCE: i32 = 2;
    client[0] <= monitor[0] + TOLERANCE
        && client[1] <= monitor[1] + TOLERANCE
        && client[2] >= monitor[2] - TOLERANCE
        && client[3] >= monitor[3] - TOLERANCE
        && client[2] > client[0]
        && client[3] > client[1]
}

#[cfg(all(target_os = "windows", not(test)))]
fn native_environment(window: &Window) -> Environment {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::{
        Foundation::{POINT, RECT},
        Graphics::Gdi::{
            ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO,
            MonitorFromWindow,
        },
        UI::WindowsAndMessaging::{
            GetClassNameW, GetClientRect, GetForegroundWindow, GetShellWindow,
            GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        },
    };
    let mut environment = Environment::default();
    if let Ok(handle) = HasWindowHandle::window_handle(window)
        && let RawWindowHandle::Win32(handle) = handle.as_raw()
    {
        environment.minimized = unsafe {
            IsIconic(handle.hwnd.get() as _) != 0 || IsWindowVisible(handle.hwnd.get() as _) == 0
        };
    }
    // All handles are borrowed, and these calls only fill local stack records.
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_null()
            || foreground == GetShellWindow()
            || IsIconic(foreground) != 0
            || IsWindowVisible(foreground) == 0
        {
            return environment;
        }
        let mut class = [0u16; 64];
        let length = GetClassNameW(foreground, class.as_mut_ptr(), class.len() as i32);
        if length > 0
            && matches!(
                String::from_utf16_lossy(&class[..length as usize]).as_str(),
                "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
            )
        {
            return environment;
        }
        let mut process = 0;
        GetWindowThreadProcessId(foreground, &mut process);
        if process == std::process::id() {
            return environment;
        }
        let mut client: RECT = std::mem::zeroed();
        let mut origin = POINT { x: 0, y: 0 };
        let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetClientRect(foreground, &mut client) != 0
            && ClientToScreen(foreground, &mut origin) != 0
            && !monitor.is_null()
            && GetMonitorInfoW(monitor, &mut info) != 0
        {
            environment.fullscreen_other_app = covers_monitor(
                [
                    origin.x,
                    origin.y,
                    origin.x + client.right - client.left,
                    origin.y + client.bottom - client.top,
                ],
                [
                    info.rcMonitor.left,
                    info.rcMonitor.top,
                    info.rcMonitor.right,
                    info.rcMonitor.bottom,
                ],
            );
        }
    }
    environment
}

#[cfg(all(target_os = "macos", not(test)))]
fn native_environment(window: &Window) -> Environment {
    use core_foundation::{
        array::CFArray,
        base::{CFType, TCFType},
        dictionary::CFDictionary,
        number::CFNumber,
        string::CFString,
    };
    use core_graphics::{
        display::CGDisplay,
        window::{
            copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements,
            kCGWindowListOptionOnScreenOnly,
        },
    };
    let mut environment = Environment::default();
    if let Some(native) = crate::appearance::native_window(window) {
        environment.minimized = native.isMiniaturized() || !native.isVisible();
    }
    let Some(frontmost) = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication()
    else {
        return environment;
    };
    let process = frontmost.processIdentifier();
    if process as u32 == std::process::id() {
        return environment;
    }
    let Some(info) = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return environment;
    };
    // CGWindowListCopyWindowInfo documents an array of dictionaries. Only
    // public numeric geometry/owner fields are read, never titles or pixels.
    let windows: CFArray<CFDictionary<CFString, CFType>> =
        unsafe { TCFType::wrap_under_get_rule(info.as_concrete_TypeRef()) };
    let displays = CGDisplay::active_displays().unwrap_or_default();
    for record in windows.iter() {
        let number = |key: &str| {
            record
                .find(CFString::new(key))
                .and_then(|v| v.downcast::<CFNumber>())
                .and_then(|v| v.to_i64())
        };
        if number("kCGWindowOwnerPID") != Some(i64::from(process))
            || number("kCGWindowLayer") != Some(0)
        {
            continue;
        }
        let Some(bounds) = record
            .find(CFString::new("kCGWindowBounds"))
            .and_then(|v| v.downcast::<CFDictionary>())
        else {
            continue;
        };
        // Validate the dictionary type before using its documented string keys
        // and CFType values (typed dictionaries do not implement ConcreteCFType).
        let bounds: CFDictionary<CFString, CFType> =
            unsafe { TCFType::wrap_under_get_rule(bounds.as_concrete_TypeRef()) };
        let value = |key: &str| {
            bounds
                .find(CFString::new(key))
                .and_then(|v| v.downcast::<CFNumber>())
                .and_then(|v| v.to_f64())
        };
        let (Some(x), Some(y), Some(w), Some(h)) =
            (value("X"), value("Y"), value("Width"), value("Height"))
        else {
            continue;
        };
        if displays.iter().any(|display| {
            let b = CGDisplay::new(*display).bounds();
            covers_monitor(
                [
                    x.round() as i32,
                    y.round() as i32,
                    (x + w).round() as i32,
                    (y + h).round() as i32,
                ],
                [
                    b.origin.x.round() as i32,
                    b.origin.y.round() as i32,
                    (b.origin.x + b.size.width).round() as i32,
                    (b.origin.y + b.size.height).round() as i32,
                ],
            )
        }) {
            environment.fullscreen_other_app = true;
            break;
        }
    }
    environment
}

#[cfg(any(not(any(target_os = "windows", target_os = "macos")), test))]
fn native_environment(_window: &Window) -> Environment {
    Environment::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_background_setting_never_disables_fullscreen_or_minimized_protection() {
        for preference in [false, true] {
            for active in [false, true] {
                assert!(should_pause(
                    preference,
                    active,
                    Environment {
                        minimized: true,
                        ..Default::default()
                    }
                ));
                assert!(should_pause(
                    preference,
                    active,
                    Environment {
                        fullscreen_other_app: true,
                        ..Default::default()
                    }
                ));
            }
        }
        assert!(should_pause(true, false, Environment::default()));
        assert!(!should_pause(false, false, Environment::default()));
        assert!(!should_pause(true, true, Environment::default()));
    }
    #[test]
    fn fullscreen_detection_handles_secondary_monitors_but_not_a_maximized_client_area() {
        assert!(covers_monitor([1920, 0, 4480, 1440], [1920, 0, 4480, 1440]));
        assert!(covers_monitor([-1920, -300, 0, 780], [-1920, -300, 0, 780]));
        assert!(!covers_monitor([0, 32, 1920, 1040], [0, 0, 1920, 1080]));
        assert!(!covers_monitor([0, 0, 800, 600], [0, 0, 1920, 1080]));
    }
}
