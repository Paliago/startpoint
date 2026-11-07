//! Windows client-drawn caption, keeping the native resizable/system-menu window.
//! No framework fork, global HWND lookup, polling or synthetic process shutdown.
use crate::state::State;

pub fn is_maximized(state: &State) -> bool {
    #[cfg(target_os = "windows")]
    return state
        .native_window
        .as_ref()
        .is_some_and(native::is_maximized);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = state;
        false
    }
}

#[cfg(target_os = "windows")]
pub use native::install;

#[cfg(any(target_os = "windows", test))]
pub const CONTROL_WIDTH: f32 = 34.0;
#[cfg(any(target_os = "windows", test))]
pub const CONTROL_HEIGHT: f32 = 28.0;

/// Shared with WM_NCHITTEST: logical client coordinates, not screen pixels.
#[cfg(any(target_os = "windows", test))]
pub fn caption_rects(client_width: f32) -> [[f32; 4]; 3] {
    use crate::toolbar::{GAP, INSET, SIDE, TOP};
    let right = client_width - SIDE - INSET;
    std::array::from_fn(|index| {
        [
            right - CONTROL_WIDTH - (2 - index) as f32 * (CONTROL_WIDTH + GAP),
            TOP + INSET,
            CONTROL_WIDTH,
            CONTROL_HEIGHT,
        ]
    })
}

#[cfg(target_os = "windows")]
pub fn controls(state: &State, kit: &rune_kit::Kit) -> rune::El {
    use crate::icons;
    use rune::{Color, Cursor, Icon, el, icon, row};
    let maximized = is_maximized(state);
    let caption = |key: &str, name: &str, glyph: &'static Icon| {
        el().key(key)
            .size(CONTROL_WIDTH, CONTROL_HEIGHT)
            .center()
            .radius(7.0)
            .role("button")
            .label(name)
            .help(name)
            .focusable(true)
            .focus_ring(2.0, 1.0, kit.c.text_accent.alpha(0.7))
            .cursor(Cursor::Default)
            .hover(kit.c.text_base.alpha(0.08))
            .press(kit.c.text_base.alpha(0.14))
            .child(
                icon(glyph, 14.0)
                    .color(kit.c.icon_base)
                    .inert()
                    .a11y_hidden(),
            )
    };
    let close_red = rune::color::hex("#c42b1c").expect("caption hover color");
    let close = el()
        .key("window-close")
        .size(CONTROL_WIDTH, CONTROL_HEIGHT)
        .center()
        .radius(7.0)
        .role("button")
        .label("Close window")
        .help("Close window")
        .focusable(true)
        .focus_ring(2.0, 1.0, kit.c.text_accent.alpha(0.7))
        .cursor(Cursor::Default)
        .hover(close_red)
        .press(rune::color::hex("#a32418").expect("caption pressed color"))
        .on_hover_change(|s: &mut State, inside, tx| {
            tx.instant();
            s.window_close_hovered = inside;
        })
        .on_press_tx(|_: &mut State, tx| tx.close_window())
        .child(
            icon(&icons::CLOSE, 14.0)
                .color(if state.window_close_hovered {
                    Color::WHITE
                } else {
                    kit.c.icon_base
                })
                .inert()
                .a11y_hidden(),
        );
    row()
        .key("window-controls")
        .role("toolbar")
        .label("Window controls")
        .gap(crate::toolbar::GAP)
        .child(
            caption("window-minimize", "Minimize window", &icons::MINIMIZE)
                .on_press_tx(|_: &mut State, tx| tx.minimize()),
        )
        .child(
            caption(
                "window-maximize",
                if maximized {
                    "Restore window"
                } else {
                    "Maximize window"
                },
                if maximized {
                    &icons::RESTORE
                } else {
                    &icons::MAXIMIZE
                },
            )
            .on_press(|s: &mut State| {
                if let Some(window) = &s.native_window {
                    native::toggle_maximized(window);
                }
            }),
        )
        .child(close)
}

#[cfg(target_os = "windows")]
mod native {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::{
            Dwm::{DwmDefWindowProc, DwmExtendFrameIntoClientArea},
            Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
                ScreenToClient,
            },
        },
        System::SystemServices::MK_LBUTTON,
        UI::{
            Controls::MARGINS,
            HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    };

    const SUBCLASS_ID: usize = 0x53545054;

    fn with_hwnd<R>(window: &rune::WindowHandle, f: impl FnOnce(HWND) -> R) -> Option<R> {
        window
            .with_raw_handle(|handle| {
                let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                    return None;
                };
                Some(f(handle.hwnd.get() as HWND))
            })
            .ok()
            .flatten()
    }

    pub fn install(window: &rune::WindowHandle) -> Result<(), String> {
        with_hwnd(window, |hwnd| {
            // UI-thread callback, with static code and no owned pointer in subclass data.
            // Keep WS_THICKFRAME/SYSMENU/MINIMIZEBOX/MAXIMIZEBOX for native window behavior.
            if unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, 0) } == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let margins = MARGINS {
                cxLeftWidth: 1,
                cxRightWidth: 1,
                cyTopHeight: 1,
                cyBottomHeight: 1,
            };
            unsafe {
                let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }
            Ok(())
        })
        .unwrap_or_else(|| Err("native Windows handle unavailable".into()))
    }

    pub fn is_maximized(window: &rune::WindowHandle) -> bool {
        with_hwnd(window, |hwnd| unsafe { IsZoomed(hwnd) != 0 }).unwrap_or(false)
    }

    pub fn toggle_maximized(window: &rune::WindowHandle) {
        with_hwnd(window, |hwnd| unsafe {
            // Posted, not called reentrantly from Rune's input/state transaction.
            PostMessageW(
                hwnd,
                WM_SYSCOMMAND,
                if IsZoomed(hwnd) != 0 {
                    SC_RESTORE
                } else {
                    SC_MAXIMIZE
                } as usize,
                0,
            );
        });
    }

    fn client_point(hwnd: HWND, lparam: LPARAM) -> POINT {
        // Signed coordinates matter on monitors above/left of the primary display.
        let mut point = POINT {
            x: lparam as i16 as i32,
            y: (lparam >> 16) as i16 as i32,
        };
        unsafe {
            ScreenToClient(hwnd, &mut point);
        }
        point
    }

    fn client_lparam(hwnd: HWND, lparam: LPARAM) -> LPARAM {
        let point = client_point(hwnd, lparam);
        ((point.x as u16 as u32) | ((point.y as u16 as u32) << 16)) as LPARAM
    }

    unsafe extern "system" fn subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _: usize,
        _: usize,
    ) -> LRESULT {
        match message {
            WM_NCCALCSIZE if wparam != 0 => {
                // Remove only the non-client caption, not the resizable window styles.
                let params = unsafe { &mut *(lparam as *mut NCCALCSIZE_PARAMS) };
                if unsafe { IsZoomed(hwnd) } != 0 {
                    let monitor =
                        unsafe { MonitorFromRect(&params.rgrc[0], MONITOR_DEFAULTTONEAREST) };
                    let mut info = MONITORINFO {
                        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
                        params.rgrc[0] = info.rcWork;
                    }
                }
                return 0;
            }
            WM_NCHITTEST => {
                let mut rect = RECT::default();
                if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
                    return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
                }
                let dpi = match unsafe { GetDpiForWindow(hwnd) } {
                    0 => 96,
                    dpi => dpi,
                };
                let border = unsafe {
                    GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi)
                        + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
                };
                let x = lparam as i16 as i32 - rect.left;
                let y = (lparam >> 16) as i16 as i32 - rect.top;
                if x < 0 || y < 0 || x >= rect.right - rect.left || y >= rect.bottom - rect.top {
                    return HTNOWHERE as LRESULT;
                }
                if unsafe { IsZoomed(hwnd) } == 0 {
                    if let Some(hit) =
                        resize_hit(x, y, rect.right - rect.left, rect.bottom - rect.top, border)
                    {
                        return hit as LRESULT;
                    }
                }
                let point = client_point(hwnd, lparam);
                let mut client = RECT::default();
                unsafe {
                    GetClientRect(hwnd, &mut client);
                }
                let scale = dpi as f32 / 96.0;
                let [left, top, width, height] =
                    super::caption_rects((client.right - client.left) as f32 / scale)[1];
                let (x, y) = (point.x as f32 / scale, point.y as f32 / scale);
                if x >= left && x < left + width && y >= top && y < top + height {
                    // Windows 11 can show Snap Layouts over our maximize button.
                    return HTMAXBUTTON as LRESULT;
                }
                return HTCLIENT as LRESULT;
            }
            WM_NCMOUSEMOVE if wparam == HTMAXBUTTON as usize => {
                // Rune paints the button's hover; DWM keeps its native Snap Layout affordance.
                unsafe {
                    DefSubclassProc(hwnd, WM_MOUSEMOVE, 0, client_lparam(hwnd, lparam));
                }
                let mut result = 0;
                if unsafe { DwmDefWindowProc(hwnd, message, wparam, lparam, &mut result) } != 0 {
                    return result;
                }
            }
            WM_NCLBUTTONDOWN | WM_NCLBUTTONUP | WM_NCLBUTTONDBLCLK
                if wparam == HTMAXBUTTON as usize =>
            {
                // One action path: feed Rune the press/release, not DefWindowProc's second toggle.
                let down = message != WM_NCLBUTTONUP;
                return unsafe {
                    DefSubclassProc(
                        hwnd,
                        if down { WM_LBUTTONDOWN } else { WM_LBUTTONUP },
                        if down { MK_LBUTTON as usize } else { 0 },
                        client_lparam(hwnd, lparam),
                    )
                };
            }
            WM_NCDESTROY => unsafe {
                RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID);
            },
            _ => {}
        }
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    fn resize_hit(x: i32, y: i32, width: i32, height: i32, border: i32) -> Option<u32> {
        if x < 0 || y < 0 || x >= width || y >= height {
            return None;
        }
        let (left, right, top, bottom) = (
            x < border,
            x >= width - border,
            y < border,
            y >= height - border,
        );
        match (left, right, top, bottom) {
            (true, _, true, _) => Some(HTTOPLEFT),
            (_, true, true, _) => Some(HTTOPRIGHT),
            (true, _, _, true) => Some(HTBOTTOMLEFT),
            (_, true, _, true) => Some(HTBOTTOMRIGHT),
            (true, _, _, _) => Some(HTLEFT),
            (_, true, _, _) => Some(HTRIGHT),
            (_, _, true, _) => Some(HTTOP),
            (_, _, _, true) => Some(HTBOTTOM),
            _ => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn resizing_preserves_all_eight_native_edges_without_claiming_the_toolbar() {
            for border in [8, 12, 16] {
                for (x, y, hit) in [
                    (0, 0, HTTOPLEFT),
                    (1279, 0, HTTOPRIGHT),
                    (0, 839, HTBOTTOMLEFT),
                    (1279, 839, HTBOTTOMRIGHT),
                    (0, 400, HTLEFT),
                    (1279, 400, HTRIGHT),
                    (600, 0, HTTOP),
                    (600, 839, HTBOTTOM),
                ] {
                    assert_eq!(resize_hit(x, y, 1280, 840, border), Some(hit));
                }
                assert_eq!(resize_hit(100, 24, 1280, 840, border), None);
                assert_eq!(resize_hit(-1, 24, 1280, 840, border), None);
            }
        }
    }
}
