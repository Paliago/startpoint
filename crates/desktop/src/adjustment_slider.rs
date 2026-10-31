//! Keyboard-operable appearance adjustments with a default detent.
use crate::components::button;
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable,
    button::ButtonVariants,
    slider::{Slider, SliderEvent, SliderState},
};

const DEFAULT_PERCENT: f32 = startpoint_sdk::DEFAULT_TRANSLUCENCY_AMOUNT * 100.;
const SNAP_DISTANCE: f32 = 2.;

#[derive(Clone, Copy)]
pub(crate) enum Adjustment {
    Translucency,
    Radius,
    Padding,
    OutlineStrength,
}

impl Adjustment {
    fn range(self) -> (f32, f32, f32) {
        let defaults = startpoint_sdk::ControlAppearance::default();
        match self {
            Self::Translucency => (0., 100., DEFAULT_PERCENT),
            Self::Radius => (0., 12., defaults.radius),
            Self::Padding => (8., 20., defaults.horizontal_padding),
            Self::OutlineStrength => (25., 100., defaults.outline_strength * 100.),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Translucency => "Translucency",
            Self::Radius => "Control rounding",
            Self::Padding => "Button padding",
            Self::OutlineStrength => "Outline strength",
        }
    }

    fn display_value(self, value: f32) -> f32 {
        match self {
            Self::Translucency | Self::OutlineStrength => value * 100.,
            _ => value,
        }
    }

    fn stored_value(self, value: f32) -> f32 {
        match self {
            Self::Translucency | Self::OutlineStrength => value / 100.,
            _ => value,
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Self::Translucency | Self::OutlineStrength => "%",
            _ => "px",
        }
    }
}

pub(crate) enum Event {
    Changed(f32),
    Released,
}

pub(crate) struct AdjustmentSlider {
    adjustment: Adjustment,
    state: Entity<SliderState>,
    focus: FocusHandle,
    value: f32,
    enabled: bool,
    _subscription: Subscription,
}

impl EventEmitter<Event> for AdjustmentSlider {}

impl AdjustmentSlider {
    pub fn new(
        adjustment: Adjustment,
        amount: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (min, max, default) = adjustment.range();
        let value = adjustment.display_value(amount);
        let value = if value.is_finite() {
            value.clamp(min, max).round()
        } else {
            default
        };
        let state = cx.new(|_| {
            SliderState::new()
                .min(min)
                .max(max)
                .step(1.)
                .default_value(value)
        });
        let subscription = cx.subscribe_in(&state, window, |view, _, event, window, cx| {
            if !view.enabled {
                return;
            }
            match event {
                SliderEvent::Change(value) => {
                    let value = pointer_value(value.end(), view.adjustment.range());
                    view.change(value, window, cx);
                }
                SliderEvent::Release(_) => cx.emit(Event::Released),
            }
        });
        Self {
            adjustment,
            state,
            focus: cx.focus_handle(),
            value,
            enabled: true,
            _subscription: subscription,
        }
    }

    pub fn sync(
        &mut self,
        amount: f32,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (min, max, default) = self.adjustment.range();
        let value = self.adjustment.display_value(amount);
        let value = if value.is_finite() {
            value.clamp(min, max).round()
        } else {
            default
        };
        if self.value != value {
            self.value = value;
            self.state
                .update(cx, |state, cx| state.set_value(value, window, cx));
            cx.notify();
        }
        if self.enabled != enabled {
            self.enabled = enabled;
            cx.notify();
        }
    }

    fn change(&mut self, value: f32, window: &mut Window, cx: &mut Context<Self>) {
        // Correct the thumb too; SliderState::set_value deliberately doesn't emit
        // user events, so snapping cannot recursively trigger itself.
        self.state
            .update(cx, |state, cx| state.set_value(value, window, cx));
        if self.value != value {
            self.value = value;
            cx.emit(Event::Changed(self.adjustment.stored_value(value)));
            cx.notify();
        }
    }
}

impl Render for AdjustmentSlider {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.enabled;
        let (min, max, default) = self.adjustment.range();
        let is_default = self.value == default;
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().child(format!(
                        "{} · {:.0}{}",
                        self.adjustment.label(),
                        self.value,
                        self.adjustment.suffix()
                    )))
                    .child(
                        button("default-adjustment", cx)
                            .ghost()
                            .border_0()
                            .label("Default")
                            .tooltip("Restore the default value")
                            .disabled(!enabled || is_default)
                            .on_click(cx.listener(|view, _, window, cx| {
                                let default = view.adjustment.range().2;
                                view.change(default, window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .id("appearance-adjustment")
                    .key_context("AppearanceAdjustment")
                    .track_focus(&self.focus)
                    .tab_stop(enabled)
                    .role(Role::Slider)
                    .aria_label(self.adjustment.label())
                    .aria_numeric_value(f64::from(self.value))
                    .aria_min_numeric_value(f64::from(min))
                    .aria_max_numeric_value(f64::from(max))
                    .aria_numeric_value_step(1.)
                    .relative()
                    .w_full()
                    .h(px(36.))
                    .px_2()
                    .pt_1()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(transparent_black())
                    .when(enabled, |control| {
                        control
                            .focus_visible(|style| style.border_color(cx.theme().ring.alpha(0.7)))
                    })
                    .capture_any_mouse_down(cx.listener(
                        |view, event: &MouseDownEvent, window, cx| {
                            if view.enabled && event.button == MouseButton::Left {
                                view.focus.focus(window, cx);
                            }
                        },
                    ))
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                        let modifiers = event.keystroke.modifiers;
                        if !view.enabled || modifiers.control || modifiers.platform || modifiers.alt
                        {
                            return;
                        }
                        if let Some(value) = keyboard_value(
                            view.value,
                            &event.keystroke.key,
                            modifiers.shift,
                            view.adjustment.range(),
                        ) {
                            view.change(value, window, cx);
                            cx.stop_propagation();
                            window.prevent_default();
                        }
                    }))
                    .on_a11y_action(AccessibleAction::Increment, {
                        let owner = cx.weak_entity();
                        move |_, window, cx| {
                            let _ = owner.update(cx, |view, cx| {
                                if view.enabled {
                                    let max = view.adjustment.range().1;
                                    view.change((view.value + 1.).min(max), window, cx);
                                }
                            });
                        }
                    })
                    .on_a11y_action(AccessibleAction::Decrement, {
                        let owner = cx.weak_entity();
                        move |_, window, cx| {
                            let _ = owner.update(cx, |view, cx| {
                                if view.enabled {
                                    let min = view.adjustment.range().0;
                                    view.change((view.value - 1.).max(min), window, cx);
                                }
                            });
                        }
                    })
                    .child(Slider::new(&self.state).disabled(!enabled).h(px(24.)))
                    .child(
                        div()
                            .absolute()
                            .left(px(8.))
                            .right(px(8.))
                            .top(px(4.))
                            .h(px(30.))
                            .child(
                                div()
                                    .absolute()
                                    .left(relative((default - min) / (max - min)))
                                    .ml(px(-1.))
                                    .top(px(20.))
                                    .w(px(2.))
                                    .h(px(6.))
                                    .bg(cx.theme().foreground)
                                    .opacity(if enabled { 0.7 } else { 0.3 }),
                            ),
                    ),
            )
    }
}

fn pointer_value(value: f32, (min, max, default): (f32, f32, f32)) -> f32 {
    let value = if value.is_finite() {
        value.clamp(min, max).round()
    } else {
        default
    };
    let snap_distance = if max - min > 20. { SNAP_DISTANCE } else { 0. };
    if (value - default).abs() <= snap_distance {
        default
    } else {
        value
    }
}

fn keyboard_value(
    value: f32,
    key: &str,
    accelerated: bool,
    (min, max, default): (f32, f32, f32),
) -> Option<f32> {
    let step = if accelerated { 5. } else { 1. };
    let next = match key {
        "left" | "down" => value - step,
        "right" | "up" => value + step,
        "home" => return Some(min),
        "end" => return Some(max),
        _ => return None,
    };
    // A large keyboard step stops at the notch once, but can always leave it.
    if (value < default && next > default) || (value > default && next < default) {
        Some(default)
    } else {
        Some(next.clamp(min, max))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer_percent(value: f32) -> f32 {
        pointer_value(value, Adjustment::Translucency.range())
    }

    fn keyboard_percent(value: f32, key: &str, accelerated: bool) -> Option<f32> {
        keyboard_value(value, key, accelerated, Adjustment::Translucency.range())
    }

    #[::core::prelude::v1::test]
    fn pointer_snaps_at_the_current_default_and_can_leave_the_detent() {
        for value in [18., 19., 20., 21., 22.] {
            assert_eq!(pointer_percent(value), DEFAULT_PERCENT);
        }
        assert_eq!(pointer_percent(17.), 17.);
        assert_eq!(pointer_percent(23.), 23.);
        assert_eq!(pointer_percent(-5.), 0.);
        assert_eq!(pointer_percent(105.), 100.);
    }

    #[::core::prelude::v1::test]
    fn arrow_keys_reach_and_leave_the_default_without_getting_trapped() {
        assert_eq!(keyboard_percent(19., "right", false), Some(20.));
        assert_eq!(keyboard_percent(20., "right", false), Some(21.));
        assert_eq!(keyboard_percent(20., "left", false), Some(19.));
        assert_eq!(keyboard_percent(18., "up", true), Some(20.));
        assert_eq!(keyboard_percent(20., "up", true), Some(25.));
        assert_eq!(keyboard_percent(22., "down", true), Some(20.));
        assert_eq!(keyboard_percent(20., "down", true), Some(15.));
        assert_eq!(keyboard_percent(0., "left", false), Some(0.));
        assert_eq!(keyboard_percent(100., "right", false), Some(100.));
        assert_eq!(keyboard_percent(20., "home", false), Some(0.));
        assert_eq!(keyboard_percent(20., "end", false), Some(100.));
        assert_eq!(keyboard_percent(20., "tab", false), None);
        for adjustment in [
            Adjustment::Translucency,
            Adjustment::Radius,
            Adjustment::Padding,
            Adjustment::OutlineStrength,
        ] {
            let range = adjustment.range();
            let (min, max, default) = range;
            assert_eq!(keyboard_value(default, "home", false, range), Some(min));
            assert_eq!(keyboard_value(default, "end", false, range), Some(max));
            assert_eq!(keyboard_value(min, "left", false, range), Some(min));
            assert_eq!(keyboard_value(max, "right", false, range), Some(max));
            assert_eq!(pointer_value(f32::NAN, range), default);
            assert_eq!(pointer_value(min - 10., range), min);
            assert_eq!(pointer_value(max + 10., range), max);
            assert_eq!(
                adjustment.display_value(adjustment.stored_value(default)),
                default
            );
        }
    }
}
