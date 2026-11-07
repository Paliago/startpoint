//! Real Rune layout/input tests, with isolated example data and no native window.
use super::*;
#[cfg(target_os = "windows")]
use rune::Shell;
use rune::{Env, Runtime, TitlebarAction};

fn view(state: &State, cx: &mut Cx<State>) -> El {
    let width = cx
        .layout_size("toolbar-test-root")
        .unwrap_or([1280.0, 200.0])[0];
    el().key("toolbar-test-root").w_full().h_full().child(
        super::view(state, cx, &state.kit(cx.system_dark()), width).pin(
            Some(SIDE),
            Some(TOP),
            Some(SIDE),
            None,
        ),
    )
}

fn rect(rt: &Runtime<State>, key: &str) -> [f32; 4] {
    rt.elements()
        .into_iter()
        .find(|element| {
            element
                .key
                .as_deref()
                .is_some_and(|value| value == key || value.ends_with(&format!("/{key}")))
        })
        .unwrap_or_else(|| panic!("missing toolbar element {key}"))
        .rect
}

fn click(rt: &mut Runtime<State>, key: &str) {
    let [x, y, w, h] = rect(rt, key);
    rt.pointer_down(x + w * 0.5, y + h * 0.5);
    rt.pointer_up(x + w * 0.5, y + h * 0.5);
    rt.frame(1.0 / 60.0);
    assert!(
        rt.take_titlebar_action().is_none(),
        "{key} must not drag the window"
    );
}

#[test]
fn toolbar_groups_fit_and_caption_hitboxes_match_at_all_supported_widths_and_scales() {
    let mut rt = Runtime::new(
        State::for_test(),
        view,
        (1280.0, 200.0),
        Env::new(true, 1.0),
    );
    for width in [900.0, 1000.0, 1280.0, 1600.0] {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            rt.set_viewport((width, 200.0), scale);
            rt.frame(0.0);
            let left = rect(&rt, "window-toolbar");
            let tabs = rect(&rt, "screen-toolbar");
            let right = rect(&rt, "action-toolbar");
            let drag = rect(&rt, "titlebar-drag");
            for group in [left, tabs, right] {
                assert!((group[1] - TOP).abs() <= 1.0, "{group:?}");
                assert!((group[3] - HEIGHT).abs() <= 1.0, "{group:?}");
                assert!(
                    group[0] >= SIDE - 1.0 && group[0] + group[2] <= width - SIDE + 1.0,
                    "{group:?} at {width}"
                );
            }
            assert!(left[0] + left[2] < tabs[0]);
            assert!(tabs[0] + tabs[2] < right[0]);
            assert!(drag[2] >= 24.0, "leave a usable native drag region");
            let search = rect(&rt, "search");
            let settings = rect(&rt, "settings");
            assert!(search[0] >= left[0] && settings[0] + settings[2] <= left[0] + left[2]);
            #[cfg(target_os = "windows")]
            for (key, expected) in ["window-minimize", "window-maximize", "window-close"]
                .into_iter()
                .zip(crate::window_chrome::caption_rects(width))
            {
                let actual = rect(&rt, key);
                for (actual, expected) in actual.into_iter().zip(expected) {
                    assert!(
                        (actual - expected).abs() <= 1.0,
                        "{key}: {actual} != {expected} at {width}/{scale}"
                    );
                }
            }
        }
    }
}

#[test]
fn navigation_search_and_actions_remain_clickable_inside_the_titlebar() {
    let mut rt = Runtime::new(State::for_test(), view, (900.0, 200.0), Env::new(true, 1.0));
    rt.frame(0.0);
    click(&mut rt, "library");
    assert!(rt.state.screen == Screen::Library);
    click(&mut rt, "activity");
    assert!(rt.state.screen == Screen::Activity);
    click(&mut rt, "navigate-back");
    assert!(rt.state.screen == Screen::Library);
    click(&mut rt, "navigate-forward");
    assert!(rt.state.screen == Screen::Activity);
    click(&mut rt, "settings");
    assert!(rt.state.screen == Screen::Settings);
    click(&mut rt, "search");
    assert!(rt.state.overlay == Overlay::Commands);
    rt.update(|state, tx| {
        state.close_overlay();
        tx.instant();
    });
    rt.frame(0.0);
    click(&mut rt, "customize");
    assert!(rt.state.overlay == Overlay::Customize);
    rt.update(|state, tx| {
        state.close_overlay();
        tx.instant();
    });
    rt.frame(0.0);
    click(&mut rt, "add-game");
    assert!(rt.state.overlay == Overlay::AddGame);
    rt.update(|state, tx| {
        state.close_overlay();
        tx.instant();
    });
    rt.frame(0.0);
    click(&mut rt, "notifications");
    assert!(rt.state.overlay == Overlay::Notifications);
}

#[test]
fn empty_toolbar_space_drags_and_double_clicks_without_affecting_controls() {
    let mut rt = Runtime::new(
        State::for_test(),
        view,
        (1280.0, 200.0),
        Env::new(true, 1.0),
    );
    rt.frame(0.0);
    let [x, y, w, h] = rect(&rt, "titlebar-drag");
    let (x, y) = (x + w * 0.5, y + h * 0.5);
    rt.pointer_down(x, y);
    assert_eq!(rt.take_titlebar_action(), Some(TitlebarAction::Drag));
    rt.pointer_up(x, y);
    rt.frame(0.1);
    rt.pointer_down(x, y);
    assert_eq!(rt.take_titlebar_action(), Some(TitlebarAction::DoubleClick));
    rt.pointer_up(x, y);
}

#[cfg(target_os = "windows")]
#[test]
fn close_control_uses_the_orderly_window_close_path_not_process_exit() {
    let mut rt = Runtime::new(State::for_test(), view, (900.0, 200.0), Env::new(true, 1.0));
    rt.frame(0.0);
    let [x, y, w, h] = rect(&rt, "window-close");
    rt.pointer_move(x + w * 0.5, y + h * 0.5);
    rt.frame(0.0);
    assert!(rt.state.window_close_hovered);
    click(&mut rt, "window-close");
    assert_eq!(rt.shell_request.take(), Some(Shell::Close));
}

#[test]
fn macos_traffic_light_reservation_does_not_overlap_the_leading_controls() {
    let mut rt = Runtime::new(State::for_test(), view, (900.0, 200.0), Env::new(true, 2.0));
    rt.set_titlebar_inset(78.0);
    rt.frame(0.0);
    let toggle = rect(&rt, "sidebar-toggle");
    assert!(
        toggle[0] >= 86.0,
        "leave space for native macOS traffic lights: {toggle:?}"
    );
    let tabs = rect(&rt, "screen-toolbar");
    let right = rect(&rt, "action-toolbar");
    assert!(tabs[0] + tabs[2] < right[0]);
}
