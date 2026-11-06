use crate::state::State;
use rune::{El, Icon, ScrollSpec, TextEdit, TextStyle, col, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, Field, Kit, Select, SelectAppearance, TextArea, TextInput,
};

pub fn label(value: impl Into<String>, size: f32, weight: f32, kit: &Kit) -> El {
    text(value)
        .font(TextStyle::new(size, weight))
        .color(kit.c.text_base)
}

pub fn hint(value: impl Into<String>, kit: &Kit) -> El {
    text(value).font(kit.ty.ui).color(kit.c.text_muted).wrap()
}

pub fn error_message(value: impl Into<String>, kit: &Kit) -> El {
    hint(value, kit)
        .color(kit.c.danger.fg)
        .role("alert")
        .live_region(rune::LiveRegion::Assertive)
}

pub fn eyebrow(value: impl Into<String>, kit: &Kit) -> El {
    text(value)
        .font(TextStyle::new(9.0, 600.0).tracking(1.2))
        .color(kit.c.text_muted)
}

pub fn heading(value: impl Into<String>, size: f32, kit: &Kit) -> El {
    text(value)
        .font(TextStyle::new(size, 650.0).tracking(-0.7))
        .color(kit.c.text_base)
}

pub struct ActionButton(Button);

impl ActionButton {
    pub fn icon(self, icon: &'static Icon) -> Self {
        Self(self.0.icon(icon))
    }
    pub fn variant(self, variant: ButtonVariant) -> Self {
        Self(self.0.variant(variant))
    }
    pub fn disabled(self, disabled: bool) -> Self {
        Self(self.0.disabled(disabled))
    }
    pub fn on_press(self, action: impl Fn(&mut State) + 'static) -> Self {
        Self(self.0.on_press(action))
    }
    pub fn on_press_tx(self, action: impl Fn(&mut State, &mut rune::Tx) + 'static) -> Self {
        Self(self.0.on_press_tx(action))
    }
    pub fn build(self, kit: &Kit) -> El {
        self.0.build(kit).h(36.0).px(14.0).radius(8.0)
    }
}

pub fn button(title: &str) -> ActionButton {
    ActionButton(Button::new(title).control_size(ControlSize::Large))
}

pub fn primary(title: &str) -> ActionButton {
    button(title).variant(ButtonVariant::Submit)
}

pub fn toolbar_icon(icon: &'static Icon, label: &str, kit: &Kit) -> El {
    rune_kit::IconButton::new(icon)
        .label(label)
        .help(label)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .build(kit)
        .size(32.0, 32.0)
        .radius(7.0)
}

pub fn field(
    title: &str,
    key: &str,
    edit: &TextEdit,
    access: impl for<'a> Fn(&'a mut State) -> &'a mut TextEdit + 'static,
    kit: &Kit,
) -> El {
    Field::new()
        .label(title)
        .control(
            TextInput::new(edit)
                .label(title)
                .width(None)
                .height(36.0)
                .on_edit(access)
                .build(kit)
                .key(key),
        )
        .build(kit)
}

pub fn area(
    title: &str,
    key: &str,
    edit: &TextEdit,
    height: f32,
    access: impl for<'a> Fn(&'a mut State) -> &'a mut TextEdit + 'static,
    kit: &Kit,
) -> El {
    Field::new()
        .label(title)
        .control(
            TextArea::new(edit)
                .label(title)
                .width(None)
                .rows(4)
                .max_height(height)
                .on_edit(access)
                .build(kit)
                .key(key),
        )
        .build(kit)
}

pub fn section(title: &str, kit: &Kit) -> El {
    col()
        .w_full()
        .gap(16.0)
        .child(divider(kit).mb(4.0))
        .child(label(title, 16.0, 600.0, kit))
}

pub fn divider(kit: &Kit) -> El {
    rune_kit::Divider::horizontal().build(kit)
}

pub fn form_actions() -> El {
    row().w_full().gap(8.0).center_y().justify_end().pt(8.0)
}

pub fn setting_row(title: &str, description: impl Into<String>, actions: El, kit: &Kit) -> El {
    row()
        .w_full()
        .gap(16.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(6.0)
                .child(label(title, 13.0, 500.0, kit))
                .child(hint(description, kit)),
        )
        .child(actions)
}

pub fn modal_body(content: El, maximum_height: f32) -> El {
    col()
        .w_full()
        .max_h(maximum_height)
        .min_h(0.0)
        .padding(16.0)
        .pt(8.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content)
}

pub fn choice(
    state: &State,
    identity: (&str, &str),
    options: &[&str],
    current: &str,
    choose: impl Fn(&mut State, &str, &mut rune::Tx) + Clone + 'static,
    kit: &Kit,
    layers: &mut Vec<El>,
) -> El {
    let (key, title) = identity;
    let toggle_key = key.to_owned();
    row()
        .w_full()
        .justify_between()
        .center_y()
        .gap(16.0)
        .child(label(title, 13.0, 500.0, kit).grow(1.0).min_w(0.0).wrap())
        .child(
            Select::new(key, options)
                .appearance(if key == "library-sort" {
                    SelectAppearance::Inline
                } else {
                    SelectAppearance::Field
                })
                .width(220.0)
                .label(title)
                .current(Some(current))
                .open(state.open_select.as_deref() == Some(key))
                .on_toggle(move |state: &mut State| {
                    state.open_select = if state.open_select.as_ref() == Some(&toggle_key) {
                        None
                    } else {
                        Some(toggle_key.clone())
                    };
                })
                .on_close(|state: &mut State| state.open_select = None)
                .on_choose(move |state: &mut State, value, tx| {
                    state.open_select = None;
                    choose(state, value, tx);
                })
                .build(kit, layers)
                .h(if key == "library-sort" { 28.0 } else { 36.0 })
                .max_w(if key == "library-sort" { 164.0 } else { 220.0 })
                .radius(8.0)
                .border(
                    if key == "library-sort" { 0.0 } else { 0.5 },
                    kit.c.border_base,
                ),
        )
}
