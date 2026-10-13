//! Focusable, keyboard-operable amount control with a pointer detent at the settled glass default.
use crate::components::button;
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable,
    button::ButtonVariants,
    slider::{Slider, SliderEvent, SliderState},
};

const DEFAULT_PERCENT: f32 = startpoint_sdk::DEFAULT_TRANSLUCENCY_AMOUNT * 100.;
const SNAP_DISTANCE: f32 = 2.;

pub(crate) enum Event {
    Changed(f32),
    Released,
}

pub(crate) struct AmountSlider {
    state: Entity<SliderState>,
    focus: FocusHandle,
    percent: f32,
    enabled: bool,
    _subscription: Subscription,
}

impl EventEmitter<Event> for AmountSlider {}

impl AmountSlider {
    pub fn new(amount: f32, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let percent = (amount * 100.).clamp(0., 100.).round();
        let state = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(percent)
        });
        let subscription = cx.subscribe_in(&state, window, |view, _, event, window, cx| {
            if !view.enabled {
                return;
            }
            match event {
                SliderEvent::Change(value) => view.change(pointer_percent(value.end()), window, cx),
                SliderEvent::Release(_) => cx.emit(Event::Released),
            }
        });
        Self {
            state,
            focus: cx.focus_handle(),
            percent,
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
        let percent = (amount * 100.).clamp(0., 100.).round();
        if self.percent != percent {
            self.percent = percent;
            self.state
                .update(cx, |state, cx| state.set_value(percent, window, cx));
            cx.notify();
        }
        if self.enabled != enabled {
            self.enabled = enabled;
            cx.notify();
        }
    }

    fn change(&mut self, percent: f32, window: &mut Window, cx: &mut Context<Self>) {
        // Correct the thumb too; SliderState::set_value deliberately doesn't emit
        // user events, so snapping cannot recursively trigger itself.
        self.state
            .update(cx, |state, cx| state.set_value(percent, window, cx));
        if self.percent != percent {
            self.percent = percent;
            cx.emit(Event::Changed(percent / 100.));
            cx.notify();
        }
    }
}

impl Render for AmountSlider {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.enabled;
        let is_default = self.percent == DEFAULT_PERCENT;
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
                    .child(
                        div()
                            .text_sm()
                            .child(format!("Translucency · {:.0}%", self.percent)),
                    )
                    .child(
                        button("default-translucency", cx)
                            .ghost()
                            .label("Default")
                            .tooltip("Restore the default translucency")
                            .disabled(!enabled || is_default)
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.change(DEFAULT_PERCENT, window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .id("translucency-amount")
                    .key_context("TranslucencyAmount")
                    .track_focus(&self.focus)
                    .tab_stop(enabled)
                    .role(Role::Slider)
                    .aria_label("Translucency amount")
                    .aria_numeric_value(self.percent as f64)
                    .aria_min_numeric_value(0.)
                    .aria_max_numeric_value(100.)
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
                        control.focus(|style| style.border_color(cx.theme().ring.alpha(0.5)))
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
                        if let Some(percent) =
                            keyboard_percent(view.percent, &event.keystroke.key, modifiers.shift)
                        {
                            view.change(percent, window, cx);
                            cx.stop_propagation();
                            window.prevent_default();
                        }
                    }))
                    .on_a11y_action(AccessibleAction::Increment, {
                        let owner = cx.weak_entity();
                        move |_, window, cx| {
                            let _ = owner.update(cx, |view, cx| {
                                if view.enabled {
                                    view.change((view.percent + 1.).min(100.), window, cx);
                                }
                            });
                        }
                    })
                    .on_a11y_action(AccessibleAction::Decrement, {
                        let owner = cx.weak_entity();
                        move |_, window, cx| {
                            let _ = owner.update(cx, |view, cx| {
                                if view.enabled {
                                    view.change((view.percent - 1.).max(0.), window, cx);
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
                                    .left(relative(DEFAULT_PERCENT / 100.))
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

fn pointer_percent(percent: f32) -> f32 {
    let percent = percent.clamp(0., 100.).round();
    if (percent - DEFAULT_PERCENT).abs() <= SNAP_DISTANCE {
        DEFAULT_PERCENT
    } else {
        percent
    }
}

fn keyboard_percent(percent: f32, key: &str, accelerated: bool) -> Option<f32> {
    let step = if accelerated { 5. } else { 1. };
    let next = match key {
        "left" | "down" => percent - step,
        "right" | "up" => percent + step,
        "home" => return Some(0.),
        "end" => return Some(100.),
        _ => return None,
    };
    // A large keyboard step stops at the notch once, but can always leave it.
    if (percent < DEFAULT_PERCENT && next > DEFAULT_PERCENT)
        || (percent > DEFAULT_PERCENT && next < DEFAULT_PERCENT)
    {
        Some(DEFAULT_PERCENT)
    } else {
        Some(next.clamp(0., 100.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
