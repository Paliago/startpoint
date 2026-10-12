//! Shared presentation primitives. Keep screen-specific state and actions in views.
//! UI vocabulary: action button, icon button, choice group, choice dropdown,
//! navigation tab, toolbar island, path row, glass panel and customize panel.
use crate::glass::{self, Palette};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, ThemeColor,
    button::{Button, ButtonRounded, ButtonVariants},
    input::{Input, InputState, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::{path::PathBuf, rc::Rc};

pub(crate) const BUTTON_TEXT_REM: f32 = 0.875;

/// Resolved render tokens only: never alter the authored theme/configuration.
/// Native variants still own primary/danger, hover, pressed and disabled behavior.
pub(crate) fn configure_glass_theme(theme: &mut gpui_component::Theme) {
    let palette = Palette::from_colors(
        theme.background,
        theme.secondary,
        theme.foreground,
        theme.border,
    );
    theme.tokens.button = palette.button.into();
    theme.tokens.button_secondary = theme.tokens.button;
    // Derive interaction emphasis from the same resting fill. Dark controls
    // brighten when engaged; light controls darken, rather than swapping to a
    // darker input well that makes selected triggers look inactive.
    let emphasis = if theme.background.l < 0.5 {
        white()
    } else {
        black()
    };
    let hover = palette.button.blend(emphasis.alpha(0.06)).alpha(1.);
    let active = palette.button.blend(emphasis.alpha(0.12)).alpha(1.);
    theme.tokens.button_hover = hover.into();
    theme.tokens.button_secondary_hover = theme.tokens.button_hover;
    theme.tokens.button_active = active.into();
    theme.tokens.button_secondary_active = theme.tokens.button_active;
    theme.colors.button_foreground = palette.foreground;
    theme.colors.button_secondary_foreground = palette.foreground;
    for token in [
        &mut theme.tokens.button_primary,
        &mut theme.tokens.button_primary_hover,
        &mut theme.tokens.button_primary_active,
        &mut theme.tokens.button_danger,
        &mut theme.tokens.button_danger_hover,
        &mut theme.tokens.button_danger_active,
        &mut theme.tokens.button_warning,
        &mut theme.tokens.button_warning_hover,
        &mut theme.tokens.button_warning_active,
        &mut theme.tokens.button_success,
        &mut theme.tokens.button_success_hover,
        &mut theme.tokens.button_success_active,
        &mut theme.tokens.button_info,
        &mut theme.tokens.button_info_hover,
        &mut theme.tokens.button_info_active,
    ] {
        token.color.a = 1.;
        token.background = token.color.into();
    }
    theme.tokens.popover = theme.popover.alpha(1.).into();
    theme.tokens.accent = theme.accent.alpha(1.).into();
    theme.colors.popover_foreground.a = 1.;
    theme.colors.foreground.a = 1.;
}

pub(crate) fn glass_panel<T: Styled + IntoElement>(panel: T, cx: &App) -> glass::Frosted {
    glass_panel_with_blur(panel, glass::BLUR_RADIUS, cx)
}

pub(crate) fn glass_panel_with_blur<T: Styled + IntoElement>(
    panel: T,
    blur: f32,
    cx: &App,
) -> glass::Frosted {
    let radius = f32::from(cx.theme().radius_lg);
    glass::frosted(radius, blur, glass::surface(panel, radius, cx))
}

pub(crate) fn style_input<T: Styled>(input: T, cx: &App) -> T {
    let palette = Palette::of(cx);
    input
        .rounded(cx.theme().radius)
        .bg(palette.input)
        .text_color(palette.foreground)
        .border_1()
        .border_color(cx.theme().border)
        .shadow(vec![BoxShadow {
            color: palette.shadow,
            offset: point(px(0.), px(1.)),
            blur_radius: px(3.),
            spread_radius: px(0.),
            inset: true,
        }])
}

pub(crate) fn input(state: &Entity<InputState>, cx: &App) -> Input {
    style_input(Input::new(state).focus_bordered(false), cx)
}

/// Flat, borderless controls with compact, theme-scaled corners.
/// Native GPUI variants retain their hover, pressed, disabled and focus behavior.
pub(crate) fn button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .small()
        .h(px(32.))
        .px(px(12.))
        .rounded(ButtonRounded::Small)
        .font_weight(FontWeight::MEDIUM)
        .border_0()
        .shadow_none()
        .cursor_pointer()
}

pub(crate) fn icon_button(
    id: impl Into<ElementId>,
    icon: impl Into<Icon>,
    tooltip: impl Into<SharedString>,
) -> Button {
    button(id)
        .secondary()
        .w(px(32.))
        .h(px(32.))
        .p_0()
        .icon(icon)
        .tooltip(tooltip)
}

/// Top chrome belongs to one shared glass island, not separate button plates.
/// Ghost controls keep native hover/pressed/disabled and keyboard-focus behavior.
/// Ordinary actions, fields and opened dropdown menus remain opaque.
pub(crate) fn toolbar_control(id: impl Into<ElementId>) -> Button {
    button(id).ghost()
}

pub(crate) fn navigation_tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Button {
    toolbar_control(id)
        .h(px(32.))
        .px_3()
        .label(label)
        .selected(selected)
        .toggled(selected)
        .when(selected, |button| button.primary())
        .when(!selected, |button| {
            button.text_color(Palette::of(cx).foreground)
        })
}

/// Opaque, accent-selected choices. Unselected items share the choice group's fill.
pub(crate) fn choice_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Button {
    button(id)
        .ghost()
        .h(px(30.))
        .px_3()
        .label(label)
        .selected(selected)
        .toggled(selected)
        .when(selected, |button| button.primary())
        .when(!selected, |button| {
            button.text_color(Palette::of(cx).foreground)
        })
}

pub(crate) fn choice_group(cx: &App) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(2.))
        .p(px(2.))
        .rounded(cx.theme().radius)
        .bg(Palette::of(cx).input)
        .map(|mut control| {
            control.style().align_self = Some(AlignSelf::FlexStart);
            control
        })
}

/// Path row: label and complete, wrapping path at left, directory actions at right.
pub(crate) fn path_row(
    label: impl Into<SharedString>,
    path: impl Into<SharedString>,
    actions: Div,
    cx: &App,
) -> Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.into()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(Palette::of(cx).muted)
                        .child(path.into()),
                ),
        )
        .child(actions.flex_shrink_0().flex().items_center().gap_2())
}

/// Keyboard-selected and hovered command rows use the same text and plate as tabs.
pub(crate) fn navigation_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    let palette = Palette::of(cx);
    div()
        .id(id)
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .text_color(palette.foreground)
        .when(selected, |row| row.bg(palette.input))
        .hover(|row| {
            row.bg(cx.theme().secondary_hover.alpha(1.))
                .text_color(palette.foreground)
        })
}

pub(crate) fn choice_dropdown<T: Clone + PartialEq + 'static>(
    id: impl Into<ElementId>,
    selected: T,
    choices: Vec<(T, SharedString)>,
    enabled: bool,
    on_select: impl Fn(T, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = choices
        .iter()
        .find(|(value, _)| *value == selected)
        .map(|(_, label)| label.clone())
        .unwrap_or_default();
    let on_select = Rc::new(on_select);
    button(id)
        .w_full()
        .min_w_0()
        .h(px(34.))
        .px_3()
        .dropdown_caret(true)
        .disabled(!enabled)
        .tooltip(label.clone())
        .child(div().flex_1().min_w_0().truncate().child(label))
        .dropdown_menu(move |mut menu, _, _| {
            for (value, label) in &choices {
                let callback = on_select.clone();
                let value = value.clone();
                let checked = value == selected;
                menu = menu.item(
                    PopupMenuItem::new(label.clone())
                        .checked(checked)
                        .on_click(move |_, window, cx| callback(value.clone(), window, cx)),
                );
            }
            menu.min_w(px(200.)).scrollable(true)
        })
}

pub(crate) fn snug_sidebar_button(mut button: Button, icon: IconName, cx: &App) -> Button {
    button.style().justify_content = Some(JustifyContent::FlexStart);
    button
        .small()
        .ghost()
        .px_2()
        .font_weight(FontWeight::NORMAL)
        .icon(Icon::new(icon).small())
        .hover(|button| button.text_color(Palette::of(cx).foreground))
}

pub(crate) fn sidebar_label(label: impl Into<SharedString>) -> Div {
    // Expand inside Button's centered content row to anchor its icon and label.
    div()
        .flex_1()
        .min_w_0()
        .text_left()
        .truncate()
        .child(label.into())
}

pub(crate) trait EditableFieldState: Sized + 'static {
    fn control(state: &Entity<Self>, disabled: bool, cx: &App) -> AnyElement;
    fn replace(state: &Entity<Self>, value: &str, window: &mut Window, cx: &mut App);
}

impl EditableFieldState for InputState {
    fn control(state: &Entity<Self>, disabled: bool, cx: &App) -> AnyElement {
        input(state, cx)
            .large()
            .w_auto()
            .flex_1()
            .min_w_0()
            .disabled(disabled)
            .into_any_element()
    }

    fn replace(state: &Entity<Self>, value: &str, window: &mut Window, cx: &mut App) {
        state.update(cx, |state, cx| {
            state.set_value(value.to_string(), window, cx)
        });
    }
}

impl EditableFieldState for TextareaState {
    fn control(state: &Entity<Self>, disabled: bool, cx: &App) -> AnyElement {
        style_input(Textarea::new(state).bordered(false), cx)
            .w_auto()
            .flex_1()
            .min_w_0()
            .disabled(disabled)
            .into_any_element()
    }

    fn replace(state: &Entity<Self>, value: &str, window: &mut Window, cx: &mut App) {
        state.update(cx, |state, cx| {
            state.set_value(value.to_string(), window, cx)
        });
    }
}

pub(crate) fn field<S: EditableFieldState>(label: &str, state: &Entity<S>, cx: &App) -> Div {
    editable_field(label, state, false, cx)
}

pub(crate) fn editable_field<S: EditableFieldState>(
    label: &str,
    state: &Entity<S>,
    disabled: bool,
    cx: &App,
) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .font_weight(FontWeight::MEDIUM)
                .child(label.to_string()),
        )
        .child(div().flex().child(S::control(state, disabled, cx)))
}

pub(crate) fn eyebrow(label: &str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .opacity(0.65)
        .child(label.to_string())
}

pub(crate) fn hint(text: &str) -> Div {
    div().text_xs().opacity(0.7).child(text.to_string())
}

pub(crate) fn widget_surface(cx: &App) -> Div {
    div()
        .flex_1()
        .min_w(px(280.))
        .p_4()
        .rounded(px(12.))
        .bg(Palette::of(cx).surface)
}

/// Place after the app toolbar so both the dimming and hit testing cover it.
pub(crate) fn modal_backdrop(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .inset_0()
        .occlude()
        .bg(gpui::black().alpha(0.45))
        .flex()
        .items_center()
        .justify_center()
}

/// A common floating customization surface, with geometry supplied by its screen.
pub(crate) fn floating_panel(
    id: impl Into<ElementId>,
    top: f32,
    width: f32,
    window: &Window,
    _cx: &App,
) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .top(px(top))
        .right(px(12.))
        .w(px(width))
        .max_h(window.viewport_size().height - px(top + 16.))
        .overflow_y_scroll()
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .rounded(px(12.))
        .p_4()
}

pub(crate) fn image(path: PathBuf, fit: ObjectFit, cx: &App) -> Img {
    let colors = cx.theme().colors;
    img(path)
        .size_full()
        .object_fit(fit)
        .with_fallback(move || placeholder("Image unavailable", colors).into_any_element())
}

pub(crate) fn placeholder(title: &str, colors: ThemeColor) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_5()
        .px_4()
        .bg(colors.muted)
        .child(div().w_6().h_1().bg(colors.primary))
        .child(
            div()
                .text_sm()
                .text_color(colors.muted_foreground)
                .text_center()
                .child(title.to_string()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn modal_backdrop_covers_every_edge_including_the_toolbar() {
        let mut backdrop = modal_backdrop("test-backdrop");
        let style = backdrop.style();
        assert_eq!(style.inset.top, Some(px(0.).into()));
        assert_eq!(style.inset.bottom, Some(px(0.).into()));
        assert_eq!(style.inset.left, Some(px(0.).into()));
        assert_eq!(style.inset.right, Some(px(0.).into()));
    }

    #[::core::prelude::v1::test]
    fn shared_buttons_are_flat_without_forcing_label_padding_onto_icon_buttons() {
        let mut button = button("test");
        let style = button.style();
        assert_eq!(style.border_widths.top, Some(px(0.).into()));
        assert!(style.box_shadow.as_ref().is_some_and(Vec::is_empty));
        assert_eq!(style.padding.left, Some(px(12.).into()));
        assert_eq!(style.padding.right, Some(px(12.).into()));
        assert_eq!(style.size.height, Some(px(32.).into()));
        assert!(
            style.corner_radii.top_left.is_none(),
            "use the live theme radius"
        );
    }

    #[::core::prelude::v1::test]
    fn icon_buttons_have_equal_hit_targets_without_label_padding() {
        let mut control = icon_button("bell", IconName::Bell, "Notifications");
        let style = control.style();
        assert_eq!(style.size.width, Some(px(32.).into()));
        assert_eq!(style.size.height, Some(px(32.).into()));
        assert_eq!(style.padding.left, Some(px(0.).into()));
        assert_eq!(style.padding.right, Some(px(0.).into()));
    }

    #[::core::prelude::v1::test]
    fn top_chrome_controls_have_no_individual_plate_or_shadow() {
        let mut control = toolbar_control("top-chrome");
        let style = control.style();
        assert!(
            style.background.is_none(),
            "the shared island owns the fill"
        );
        assert_eq!(style.border_widths.top, Some(px(0.).into()));
        assert!(style.box_shadow.as_ref().is_some_and(Vec::is_empty));
        assert!(style.opacity.is_none(), "do not fade control text or icons");
    }

    #[::core::prelude::v1::test]
    fn smoke_tokens_preserve_authored_theme_and_accent_roles() {
        let mut theme = gpui_component::Theme::default();
        let authored = serde_json::to_value(&theme.dark_theme).unwrap();
        let original_background = theme.background;
        let original_primary = theme.primary;
        let original_primary_text = theme.button_primary_foreground;
        let original_ring = theme.ring;
        configure_glass_theme(&mut theme);
        assert_eq!(theme.background, original_background);
        assert_eq!(theme.primary, original_primary);
        assert_eq!(theme.button_primary_foreground, original_primary_text);
        assert_eq!(theme.ring, original_ring);
        assert_eq!(serde_json::to_value(&theme.dark_theme).unwrap(), authored);
        assert_eq!(theme.tokens.button.color.a, 1.);
        assert_eq!(theme.button_foreground.a, 1.);
        assert_eq!(
            theme.tokens.button.background,
            theme.tokens.button.color.into()
        );
        assert!(
            theme.focus_ring,
            "button keyboard-focus indicators remain available"
        );
    }

    #[::core::prelude::v1::test]
    fn control_and_menu_fills_stay_flat_opaque_and_readable_in_every_builtin_theme() {
        let catalog = crate::theme::Catalog::default();
        let mut theme = gpui_component::Theme::default();
        for set in &catalog.themes {
            for mode in [
                gpui_component::ThemeMode::Light,
                gpui_component::ThemeMode::Dark,
            ] {
                theme.apply_config(&catalog.configuration(&set.name, mode));
                configure_glass_theme(&mut theme);
                let normal = theme.tokens.button.color;
                let hover = theme.tokens.button_hover.color;
                let active = theme.tokens.button_active.color;
                if theme.background.l < 0.5 {
                    assert!(
                        normal.l < hover.l && hover.l < active.l,
                        "dark controls brighten when engaged: {}",
                        set.name
                    );
                } else {
                    assert!(
                        normal.l > hover.l && hover.l > active.l,
                        "light controls darken when engaged: {}",
                        set.name
                    );
                }
                for token in [
                    theme.tokens.button,
                    theme.tokens.button_hover,
                    theme.tokens.button_active,
                    theme.tokens.button_secondary,
                    theme.tokens.button_primary,
                    theme.tokens.button_primary_hover,
                    theme.tokens.button_primary_active,
                    theme.tokens.button_secondary_hover,
                    theme.tokens.button_secondary_active,
                    theme.tokens.button_danger,
                    theme.tokens.button_danger_hover,
                    theme.tokens.button_danger_active,
                    theme.tokens.button_warning,
                    theme.tokens.button_warning_hover,
                    theme.tokens.button_warning_active,
                    theme.tokens.button_success,
                    theme.tokens.button_success_hover,
                    theme.tokens.button_success_active,
                    theme.tokens.button_info,
                    theme.tokens.button_info_hover,
                    theme.tokens.button_info_active,
                    theme.tokens.popover,
                ] {
                    assert_eq!(token.color.a, 1., "{} {}", set.name, mode.name());
                    assert_eq!(
                        token.background,
                        token.color.into(),
                        "no gradients on {} {}",
                        set.name,
                        mode.name()
                    );
                }
                for (background, foreground) in [
                    (theme.tokens.button.color, theme.button_foreground),
                    (theme.tokens.button_hover.color, theme.button_foreground),
                    (theme.tokens.button_active.color, theme.button_foreground),
                    (
                        theme.tokens.button_primary.color,
                        theme.button_primary_foreground,
                    ),
                    (theme.tokens.popover.color, theme.popover_foreground),
                ] {
                    assert!(
                        crate::theme::contrast(background, foreground) >= 4.5,
                        "{} {}",
                        set.name,
                        mode.name()
                    );
                }
            }
        }
    }
}
