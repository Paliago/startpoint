//! Shared presentation primitives. Keep screen-specific state and actions in views.
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, ThemeColor,
    button::{Button, ButtonVariants},
    input::{Input, InputState},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::{path::PathBuf, rc::Rc};

pub(crate) const BUTTON_TEXT_REM: f32 = 0.875;
pub(crate) const BUTTON_RADIUS: f32 = 9.;

/// Flat, borderless controls with consistent type, spacing and corners.
/// Native GPUI variants retain their hover, pressed, disabled and focus behavior.
pub(crate) fn button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .small()
        .rounded(px(BUTTON_RADIUS))
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
        .ghost()
        .size(px(32.))
        .p_0()
        .icon(icon)
        .tooltip(tooltip)
}

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
        .rounded(px(7.))
        .label(label)
        .selected(selected)
        .when(selected, |button| {
            button
                .bg(cx.theme().background.alpha(0.96))
                .text_color(cx.theme().foreground)
        })
}

pub(crate) fn segmented_control(cx: &App) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(2.))
        .p(px(3.))
        .rounded(px(10.))
        .bg(cx.theme().secondary.alpha(0.65))
        .map(|mut control| {
            control.style().align_self = Some(AlignSelf::FlexStart);
            control
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

pub(crate) fn snug_sidebar_button(mut button: Button, icon: IconName) -> Button {
    button.style().justify_content = Some(JustifyContent::FlexStart);
    button
        .small()
        .px_2()
        .rounded(px(BUTTON_RADIUS))
        .icon(Icon::new(icon).small())
}

pub(crate) fn field(label: &str, state: &Entity<InputState>, cx: &App) -> Div {
    editable_field(label, state, false, cx)
}

pub(crate) fn editable_field(
    label: &str,
    state: &Entity<InputState>,
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
        .child(
            div().flex().child(
                Input::new(state)
                    .large()
                    .w_auto()
                    .flex_1()
                    .min_w_0()
                    .disabled(disabled),
            ),
        )
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
        .rounded(cx.theme().radius_lg)
        .bg(cx.theme().background.alpha(0.96))
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
        .rounded(px(12.))
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border.alpha(0.5))
        .shadow_md()
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
    fn shared_buttons_are_flat_without_forcing_label_padding_onto_icon_buttons() {
        let mut button = button("test");
        let style = button.style();
        assert_eq!(style.border_widths.top, Some(px(0.).into()));
        assert!(style.box_shadow.as_ref().is_some_and(Vec::is_empty));
        assert!(style.padding.left.is_none());
        assert!(style.size.height.is_none());
    }
}
