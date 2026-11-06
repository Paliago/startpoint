use crate::{
    components::*,
    icons,
    state::{Overlay, Screen, State},
};
use rune::{Color, Cx, El, el, row};
use rune_kit::{ButtonVariant, Kit};

pub fn surface(state: &State, kit: &Kit) -> El {
    let mut surface = row()
        .center_y()
        .padding(4.0)
        .gap(2.0)
        .radius(12.0)
        .continuous_corners()
        .fill(kit.c.page.alpha(if state.appearance.window_translucency {
            0.82
        } else {
            1.0
        }))
        .border(0.5, kit.c.text_base.alpha(0.12))
        .shadow(3.0, 12.0, Color::BLACK.alpha(0.09));
    if state.appearance.window_translucency {
        surface = surface.backdrop_blur(16.0);
    }
    surface
}

pub fn view(state: &State, cx: &mut Cx<State>, kit: &Kit, width: f32) -> El {
    let reserve = (cx.titlebar_inset() - 10.0).max(0.0);
    let controls = surface(state, kit)
        .key("window-toolbar")
        .role("toolbar")
        .label("Window and navigation controls")
        .window_drag()
        .child(el().key("native-window-controls").w(reserve).h(28.0))
        .child(
            toolbar_icon(&icons::SIDEBAR, "Toggle sidebar", kit)
                .size(28.0, 28.0)
                .disabled(state.screen == Screen::ComponentEffects)
                .on_press_tx(|s: &mut State, tx| {
                    crate::motion::fold(tx);
                    s.appearance.sidebar_visible = !s.appearance.sidebar_visible;
                    s.save_appearance();
                })
                .key("sidebar-toggle"),
        )
        .child(
            toolbar_icon(&icons::BACK, "Back", kit)
                .size(28.0, 28.0)
                .disabled(!state.can_go_back())
                .opacity(if state.can_go_back() { 1.0 } else { 0.3 })
                .on_press_tx(|s: &mut State, tx| s.go_back(tx))
                .key("navigate-back"),
        )
        .child(
            toolbar_icon(&icons::FORWARD, "Forward", kit)
                .size(28.0, 28.0)
                .disabled(!state.can_go_forward())
                .opacity(if state.can_go_forward() { 1.0 } else { 0.3 })
                .on_press_tx(|s: &mut State, tx| s.go_forward(tx))
                .key("navigate-forward"),
        );
    let mut navigation = row().w_full().gap(2.0).role("tablist").focus_group();
    for (key, title, screen) in [
        ("recents", "Recents", Screen::Recents),
        ("library", "Library", Screen::Library),
        ("activity", "Activity", Screen::Activity),
    ] {
        let selected = state.screen == screen;
        navigation = navigation.child(
            button(title)
                .variant(ButtonVariant::Ghost)
                .on_press_tx(move |s: &mut State, tx| {
                    tx.instant();
                    s.navigate(screen);
                })
                .build(kit)
                .grow(1.0)
                .min_w(0.0)
                .h(28.0)
                .px(10.0)
                .fill(if selected {
                    kit.c.text_base.alpha(0.08)
                } else {
                    Color::TRANSPARENT
                })
                .key(key)
                .role("tab")
                .focus_selected(selected),
        );
    }
    let navigation_width = if width < 1000.0 { 222.0 } else { 246.0 };
    let navigation = surface(state, kit)
        .key("screen-toolbar")
        .w(navigation_width)
        .window_drag()
        .child(navigation);
    let search_width = if width < 1100.0 { 150.0 } else { 210.0 };
    let search = if state.overlay == Overlay::Commands {
        el().w(search_width).h(28.0)
    } else {
        button(if width < 1100.0 {
            "Search…"
        } else {
            "Search or run a command…"
        })
        .variant(ButtonVariant::Ghost)
        .icon(kit.glyph.search)
        .on_press_tx(|s: &mut State, tx| {
            tx.animated();
            s.open_commands(String::new(), true);
        })
        .build(kit)
        .w(search_width)
        .h(28.0)
        .key("search")
        .matched("command-surface")
        .motion(rune::Spring::new(0.24, 0.0))
    };
    let actions = surface(state, kit)
        .key("action-toolbar")
        .role("toolbar")
        .label("Application actions")
        .child(search)
        .child(el().w(1.0).h(14.0).fill(kit.c.text_base.alpha(0.1)))
        .child(
            toolbar_icon(&icons::CUSTOMIZE, "Customize", kit)
                .size(28.0, 28.0)
                .on_press(|s: &mut State| s.show_overlay(Overlay::Customize))
                .key("customize"),
        )
        .child(
            toolbar_icon(&icons::NOTIFICATIONS, "Activity & Notifications", kit)
                .size(28.0, 28.0)
                .on_press(|s: &mut State| s.show_overlay(Overlay::Notifications))
                .key("notifications"),
        )
        .child(
            toolbar_icon(&icons::SETTINGS, "Settings", kit)
                .size(28.0, 28.0)
                .on_press_tx(|s: &mut State, tx| {
                    tx.instant();
                    s.navigate(Screen::Settings);
                })
                .key("settings"),
        );
    el().key("toolbar")
        .w(width - 24.0)
        .h(36.0)
        .window_drag()
        .child(controls.absolute(0.0, 0.0))
        .child(navigation.absolute((width - 24.0 - navigation_width) * 0.5, 0.0))
        .child(actions.pin(None, Some(0.0), Some(0.0), None))
}
