//! Shared presentation primitives. Keep screen-specific state and actions in views.
//! UI vocabulary: action button, icon button, choice group, choice dropdown,
//! navigation tab, toolbar island, path row, glass panel and customize panel.
use crate::{
    glass::{self, Palette},
    rounding::Rounding,
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, FocusableExt, Icon, IconName, Selectable, Sizable, ThemeColor,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    input::{Input, InputState, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
};
use startpoint_sdk::ControlAppearance;
use std::{path::PathBuf, rc::Rc};

pub(crate) struct Controls(pub ControlAppearance);
impl Global for Controls {}

pub(crate) fn control_appearance(cx: &App) -> ControlAppearance {
    cx.try_global::<Controls>()
        .map(|controls| controls.0)
        .unwrap_or_default()
}

pub(crate) fn control_border(cx: &App) -> Hsla {
    let settings = control_appearance(cx);
    let theme = cx.theme();
    let emphasis = if theme.background.l < 0.5 {
        white()
    } else {
        black()
    };
    theme
        .border
        .alpha(1.)
        .blend(emphasis.alpha(settings.outline_strength * 0.16))
}

pub(crate) const BUTTON_TEXT_REM: f32 = 0.875;

struct ButtonAccessibility(Button);

impl InteractiveElement for ButtonAccessibility {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.0.interactivity()
    }
}

impl StatefulInteractiveElement for ButtonAccessibility {}

pub(crate) fn with_accessible_label(button: Button, label: impl Into<SharedString>) -> Button {
    // The styled button exposes interactivity but omits the stateful ARIA builders.
    ButtonAccessibility(button).aria_label(label).0
}

/// Resolved render tokens only: never alter the authored theme/configuration.
/// Native variants still own primary/danger, hover, pressed and disabled behavior.
pub(crate) fn configure_glass_theme(
    theme: &mut gpui_component::Theme,
    controls: ControlAppearance,
) {
    let palette = Palette::from_colors(
        theme.background,
        theme.secondary,
        theme.foreground,
        theme.border,
    );
    let fill = if controls.filled_secondary {
        palette.button
    } else {
        theme.background.alpha(1.)
    };
    theme.tokens.button = fill.into();
    theme.tokens.button_secondary = theme.tokens.button;
    // Derive interaction emphasis from the same resting fill. Dark controls
    // brighten when engaged; light controls darken, rather than swapping to a
    // darker input well that makes selected triggers look inactive.
    let emphasis = if theme.background.l < 0.5 {
        white()
    } else {
        black()
    };
    let mut emphasis_amount: f32 = 0.12;
    while emphasis_amount > 0.
        && crate::theme::contrast(
            fill.blend(emphasis.alpha(emphasis_amount)),
            palette.foreground,
        ) < 4.5
    {
        emphasis_amount = (emphasis_amount - 0.01).max(0.);
    }
    let hover = fill.blend(emphasis.alpha(emphasis_amount * 0.5)).alpha(1.);
    let active = fill.blend(emphasis.alpha(emphasis_amount)).alpha(1.);
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
    let radius = f32::from(Rounding::of(cx).panel);
    glass::frosted(radius, blur, glass::surface(panel, radius, cx))
}

pub(crate) fn style_input<T: Styled>(input: T, cx: &App) -> T {
    let palette = Palette::of(cx);
    input
        .rounded(Rounding::of(cx).control)
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

/// Precision controls keep neutral outlines separate from their containing glass.
/// Native GPUI variants retain their hover, pressed, disabled and focus behavior.
pub(crate) fn button(id: impl Into<ElementId>, cx: &App) -> Button {
    let controls = control_appearance(cx);
    keyboard_focus(button_base(id, Rounding::of(cx).control), cx)
        .px(px(controls.horizontal_padding))
        .when(controls.outlines, |button| {
            button.border_1().border_color(control_border(cx))
        })
}

fn keyboard_focus(button: Button, cx: &App) -> Button {
    button
        .focus_ring(false)
        .focus_visible(|style| style.border_1().border_color(cx.theme().ring))
}

fn button_base(id: impl Into<ElementId>, radius: Pixels) -> Button {
    Styled::rounded(
        Button::new(id)
            .small()
            .h(px(32.))
            .px(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .border_0()
            .shadow_none()
            .cursor_pointer(),
        radius,
    )
}

pub(crate) fn icon_button(
    id: impl Into<ElementId>,
    icon: impl Into<Icon>,
    tooltip: impl Into<SharedString>,
    cx: &App,
) -> Button {
    icon_button_base(button(id, cx), icon, tooltip)
}

fn icon_button_base(
    button: Button,
    icon: impl Into<Icon>,
    tooltip: impl Into<SharedString>,
) -> Button {
    let tooltip = tooltip.into();
    with_accessible_label(button, tooltip.clone())
        .secondary()
        .w(px(32.))
        .h(px(32.))
        .p_0()
        .icon(icon)
        .tooltip(tooltip)
}

fn toolbar_control_base(id: impl Into<ElementId>, radius: Pixels) -> Button {
    button_base(id, radius)
        .ghost()
        .border_1()
        .border_color(transparent_black())
}

/// All toolbar islands use the same neutral hover, pressed and selected fills.
/// No resting plate; native button behavior still owns focus and disabled states.
pub(crate) fn toolbar_control(id: impl Into<ElementId>, cx: &App) -> Button {
    keyboard_focus(toolbar_control_base(id, Rounding::of(cx).control), cx).custom(
        ButtonCustomVariant::new(cx)
            .color(transparent_black())
            .foreground(cx.theme().foreground.alpha(1.))
            .hover(cx.theme().tokens.button_hover.color)
            .active(cx.theme().tokens.button_active.color),
    )
}

pub(crate) fn toolbar_icon_button(
    id: impl Into<ElementId>,
    icon: impl Into<Icon>,
    tooltip: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let tooltip = tooltip.into();
    with_accessible_label(toolbar_control(id, cx), tooltip.clone())
        .w(px(32.))
        .p_0()
        .icon(icon)
        .tooltip(tooltip)
}

/// Geometry and drag shielding shared by left, screen and right toolbar islands.
/// Apply the glass surface separately so the left island can fade with the sidebar.
pub(crate) fn toolbar_island(id: impl Into<ElementId>, radius: Pixels) -> Stateful<Div> {
    div()
        .id(id)
        .occlude()
        .relative()
        .h(px(36.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(2.))
        .p(px(2.))
        .rounded(radius)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
}

pub(crate) fn navigation_tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Button {
    toolbar_control(id, cx)
        .label(label)
        .selected(selected)
        .toggled(selected)
}

/// Neutral selected choices. Unselected items share the choice group's fill.
pub(crate) fn choice_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Button {
    button(id, cx)
        .ghost()
        .h(px(30.))
        .px_3()
        .label(label)
        .selected(selected)
        .toggled(selected)
        .border_1()
        .border_color(transparent_black())
        .when(selected, |button| {
            button
                .secondary()
                .bg(cx.theme().secondary_hover.alpha(1.))
                .text_color(Palette::of(cx).foreground)
        })
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
        .rounded(Rounding::of(cx).choice_group())
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
        .rounded(Rounding::of(cx).control)
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
    cx: &App,
) -> impl IntoElement {
    let label = choices
        .iter()
        .find(|(value, _)| *value == selected)
        .map(|(_, label)| label.clone())
        .unwrap_or_default();
    let on_select = Rc::new(on_select);
    with_accessible_label(button(id, cx), label.clone())
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
        .border_0()
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
        .min_w(px(240.))
        .px_2()
        .py_4()
        .border_t_1()
        .border_color(cx.theme().border.alpha(0.5))
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
    cx: &App,
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
        .rounded(Rounding::of(cx).panel)
        .p_4()
}

pub(crate) fn image(path: PathBuf, fit: ObjectFit, cx: &App) -> Img {
    image_with_radius(path, fit, px(0.), cx.theme().colors)
}

/// GPUI overflow masks are rectangular. Round the image itself, not just its
/// parent, and give the loading/error placeholder the exact same contour.
pub(crate) fn rounded_image(path: PathBuf, fit: ObjectFit, radius: Pixels, cx: &App) -> Img {
    image_with_radius(path, fit, radius, cx.theme().colors)
}

fn image_with_radius(path: PathBuf, fit: ObjectFit, radius: Pixels, colors: ThemeColor) -> Img {
    img(path)
        .size_full()
        .object_fit(fit)
        .rounded(radius)
        .with_fallback(move || {
            rounded_placeholder("Image unavailable", colors, radius).into_any_element()
        })
}

pub(crate) fn rounded_placeholder(title: &str, colors: ThemeColor, radius: Pixels) -> Div {
    placeholder(title, colors).rounded(radius)
}

/// Card geometry budgets padding, not an invisible border. A transparent 1px
/// border would steal space from the bottom caption and distort cover ratios.
pub(crate) fn artwork_card_frame(radius: Pixels) -> Div {
    div()
        .border_0()
        .p(px(crate::rounding::CARD_INSET))
        .rounded(radius)
}

/// Captions sit just inside the artwork edges, with extra bottom breathing room.
/// Fixed-height library captions must include this bottom padding in their height.
pub(crate) fn artwork_card_caption() -> Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .gap(px(4.))
        .px(px(crate::rounding::CARD_CAPTION_SIDE_INSET))
        .pb(px(crate::rounding::CARD_CAPTION_INSET))
}

/// Selected hover subtly lifts the opaque selection instead of replacing it
/// with the pressed fill. Keep rest, hover and press distinct without a new hue.
pub(crate) fn artwork_card_states(
    card: Stateful<Div>,
    selected: bool,
    colors: ThemeColor,
) -> Stateful<Div> {
    let selected_hover = colors
        .secondary
        .alpha(1.)
        .blend(colors.secondary_hover.alpha(0.5));
    card.when(selected, |card| card.bg(colors.secondary.alpha(1.)))
        .hover(move |card| {
            card.bg(if selected {
                selected_hover
            } else {
                colors.secondary.alpha(0.45)
            })
        })
        .active(move |card| {
            card.bg(if selected {
                colors.secondary_active.alpha(1.)
            } else {
                colors.secondary_hover
            })
        })
}

#[cfg(test)]
#[path = "card_tests.rs"]
mod card_tests;

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
    fn artwork_hover_frames_have_equal_padding_and_no_hidden_border() {
        let mut frame = artwork_card_frame(px(12.));
        let style = frame.style();
        for padding in [
            style.padding.top,
            style.padding.bottom,
            style.padding.left,
            style.padding.right,
        ] {
            assert_eq!(padding, Some(px(crate::rounding::CARD_INSET).into()));
        }
        for border in [
            style.border_widths.top,
            style.border_widths.bottom,
            style.border_widths.left,
            style.border_widths.right,
        ] {
            assert_eq!(border, Some(px(0.).into()));
        }
    }

    #[::core::prelude::v1::test]
    fn artwork_images_and_placeholders_round_their_own_painted_corners() {
        let colors = gpui_component::Theme::default().colors;
        for radius in [0., 4., 8., 16.] {
            let mut image =
                image_with_radius("cover.png".into(), ObjectFit::Cover, px(radius), colors);
            let mut placeholder = rounded_placeholder("", colors, px(radius));
            assert_eq!(image.style().corner_radii, placeholder.style().corner_radii);
            assert_eq!(image.style().corner_radii.top_left, Some(px(radius).into()));
            assert_eq!(image.style().size.width, Some(relative(1.).into()));
            assert_eq!(image.style().size.height, Some(relative(1.).into()));
        }
    }

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
        let mut button = button_base("test", px(12.));
        let style = button.style();
        assert_eq!(style.border_widths.top, Some(px(0.).into()));
        assert!(style.box_shadow.as_ref().is_some_and(Vec::is_empty));
        assert_eq!(style.padding.left, Some(px(12.).into()));
        assert_eq!(style.padding.right, Some(px(12.).into()));
        assert_eq!(style.size.height, Some(px(32.).into()));
        assert_eq!(style.corner_radii.top_left, Some(px(12.).into()));
    }

    #[::core::prelude::v1::test]
    fn icon_buttons_have_equal_hit_targets_without_label_padding() {
        let mut control = icon_button_base(
            button_base("bell", px(12.)),
            IconName::Bell,
            "Notifications",
        );
        let style = control.style();
        assert_eq!(style.size.width, Some(px(32.).into()));
        assert_eq!(style.size.height, Some(px(32.).into()));
        assert_eq!(style.padding.left, Some(px(0.).into()));
        assert_eq!(style.padding.right, Some(px(0.).into()));
    }

    #[::core::prelude::v1::test]
    fn top_chrome_controls_have_no_individual_plate_or_shadow() {
        let mut control = toolbar_control_base("top-chrome", px(12.));
        let style = control.style();
        assert!(
            style.background.is_none(),
            "the shared island owns the fill"
        );
        assert_eq!(style.border_widths.top, Some(px(1.).into()));
        assert_eq!(style.border_color, Some(transparent_black()));
        assert!(style.box_shadow.as_ref().is_some_and(Vec::is_empty));
        assert!(style.opacity.is_none(), "do not fade control text or icons");
    }

    #[::core::prelude::v1::test]
    fn every_toolbar_island_uses_the_same_compact_geometry() {
        for id in [
            "window-control-island",
            "navigation-controls",
            "action-toolbar-island",
        ] {
            let mut island = toolbar_island(id, px(12.));
            let style = island.style();
            assert_eq!(style.size.height, Some(px(36.).into()));
            assert_eq!(style.padding.left, Some(px(2.).into()));
            assert_eq!(style.padding.right, Some(px(2.).into()));
            assert_eq!(style.padding.top, Some(px(2.).into()));
            assert_eq!(style.padding.bottom, Some(px(2.).into()));
            assert_eq!(style.corner_radii.top_left, Some(px(12.).into()));
            assert!(
                style.background.is_none(),
                "shared glass owns the background"
            );
        }
    }

    #[::core::prelude::v1::test]
    fn button_and_toolbar_bases_honor_their_supplied_radius() {
        for radius in [0., 4., 12., 20.] {
            let mut control = button_base("button-radius", px(radius));
            let mut island = toolbar_island("navbar-radius", px(radius));
            assert_eq!(control.style().corner_radii, island.style().corner_radii);
            assert_eq!(
                control.style().corner_radii.top_left,
                Some(px(radius).into())
            );
        }
    }

    #[::core::prelude::v1::test]
    fn smoke_tokens_preserve_authored_theme_and_accent_roles() {
        let mut theme = gpui_component::Theme::default();
        let authored = serde_json::to_value(&theme.dark_theme).unwrap();
        let original_background = theme.background;
        let original_primary = theme.primary;
        let original_primary_text = theme.button_primary_foreground;
        let original_ring = theme.ring;
        configure_glass_theme(&mut theme, ControlAppearance::default());
        assert_eq!(theme.tokens.button.color, theme.background.alpha(1.));
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
                configure_glass_theme(&mut theme, ControlAppearance::default());
                assert_eq!(theme.tokens.button.color, theme.background.alpha(1.));
                for fill in [
                    theme.tokens.button.color,
                    theme.tokens.button_hover.color,
                    theme.tokens.button_active.color,
                ] {
                    assert!(
                        crate::theme::contrast(fill, theme.button_foreground) >= 4.5,
                        "quiet controls retain readable labels: {} {}",
                        set.name,
                        mode.name()
                    );
                }
                configure_glass_theme(
                    &mut theme,
                    ControlAppearance {
                        filled_secondary: true,
                        ..Default::default()
                    },
                );
                let normal = theme.tokens.button.color;
                let hover = theme.tokens.button_hover.color;
                let active = theme.tokens.button_active.color;
                let panel = Palette::from_colors(
                    theme.background,
                    theme.secondary,
                    theme.foreground,
                    theme.border,
                )
                .surface
                .alpha(1.);
                for surface in [theme.background.alpha(1.), panel] {
                    assert!(
                        [normal, hover, active]
                            .into_iter()
                            .all(|fill| crate::theme::contrast(fill, surface)
                                >= glass::BUTTON_SURFACE_CONTRAST),
                        "resting button disappears into its surface: {} {}",
                        set.name,
                        mode.name()
                    );
                }
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
