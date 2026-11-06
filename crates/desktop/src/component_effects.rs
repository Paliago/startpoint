use crate::{
    components::*,
    icons,
    state::{Overlay, State},
};
use rune::{Color, El, ScrollSpec, TextEdit, col, effect, el, row};
use rune_kit::{
    Badge, BadgeVariant, ButtonVariant, Card, Cell, Checkbox, Collapsible, Dialog, Field, Kit,
    Menu, MenuItem, Progress, RadioItem, SegmentedControl, Select, Switch, Table, TableColumn,
    TableState, TextArea, TextInput,
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum ComponentEffect {
    None,
    #[default]
    Aura,
    Metal,
    Charged,
    Fire,
    Glass,
    Mesh,
    Plastic,
}

impl ComponentEffect {
    pub const ALL: [Self; 8] = [
        Self::None,
        Self::Aura,
        Self::Metal,
        Self::Charged,
        Self::Fire,
        Self::Glass,
        Self::Mesh,
        Self::Plastic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Neutral",
            Self::Aura => "Aura glow",
            Self::Metal => "Liquid metal",
            Self::Charged => "Charged edge",
            Self::Fire => "Warm embers",
            Self::Glass => "Fluted glass",
            Self::Mesh => "Mesh gradient",
            Self::Plastic => "Moulded plastic",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::None => "Unmodified Rune Kit controls. The reference point for every experiment.",
            Self::Aura => {
                "A flowing spectrum on the edge, with soft light spilling onto the surface."
            }
            Self::Metal => {
                "Sculpted chrome with moving reflections. A material, not an image of a button."
            }
            Self::Charged => "A fine, travelling current with a cool illuminated edge.",
            Self::Fire => "Small tongues of heat and rising embers around the control.",
            Self::Glass => {
                "Reeded glass that refracts the colour beneath it. Text stays outside the shader."
            }
            Self::Mesh => "A slowly shifting mesh of colour underneath native component content.",
            Self::Plastic => {
                "A tactile keycap surface with a rolled rim and a soft studio highlight."
            }
        }
    }

    pub fn mark(self) -> u32 {
        match self {
            Self::None => 100,
            Self::Aura => 101,
            Self::Metal => 102,
            Self::Charged => 103,
            Self::Fire => 104,
            Self::Glass => 105,
            Self::Mesh => 106,
            Self::Plastic => 107,
        }
    }

    pub fn from_mark(mark: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|style| style.mark() == mark)
    }

    fn material(self) -> bool {
        matches!(self, Self::Metal | Self::Glass | Self::Mesh | Self::Plastic)
    }
}

pub struct EffectsPreview {
    pub style: ComponentEffect,
    pub intensity: usize,
    pub speed: usize,
    pub rounding: usize,
    pub paused: bool,
    pub reduced_motion: bool,
    pub disabled: bool,
    pub contrast: bool,
    pub enabled: bool,
    pub checked: bool,
    pub selection: usize,
    pub category: usize,
    pub density: usize,
    pub expanded: bool,
    pub progress: f32,
    pub table: TableState,
    pub text: TextEdit,
    pub notes: TextEdit,
    pub presses: u32,
    pub message: String,
    pub menu_open: bool,
    pub dialog_open: bool,
}

impl Default for EffectsPreview {
    fn default() -> Self {
        Self {
            style: ComponentEffect::Aura,
            intensity: 1,
            speed: 1,
            rounding: 1,
            paused: false,
            reduced_motion: false,
            disabled: false,
            contrast: true,
            enabled: true,
            checked: true,
            selection: 0,
            category: 0,
            density: 0,
            expanded: false,
            progress: 0.35,
            table: TableState::new().key("effect-data-table"),
            text: TextEdit::new(""),
            notes: TextEdit::new(""),
            presses: 0,
            message: "Ready for interaction".into(),
            menu_open: false,
            dialog_open: false,
        }
    }
}

impl EffectsPreview {
    pub fn strength(&self) -> f32 {
        [0.4, 0.8, 1.3].get(self.intensity).copied().unwrap_or(0.8)
    }

    pub fn rate(&self) -> f32 {
        [0.35, 1.0, 1.8].get(self.speed).copied().unwrap_or(1.0)
    }

    fn radius(&self) -> f32 {
        [6.0, 12.0, 22.0]
            .get(self.rounding)
            .copied()
            .unwrap_or(12.0)
    }

    pub fn running(&self) -> bool {
        !self.paused && !self.reduced_motion && !self.menu_open && !self.dialog_open
    }
}

fn surface(style: ComponentEffect, content: El, preview: &EffectsPreview) -> El {
    if style == ComponentEffect::None {
        return content;
    }
    let radius = preview.radius();
    let content = if style.material() {
        content.fill(if preview.contrast {
            match style {
                ComponentEffect::Metal => Color::WHITE.alpha(0.22),
                ComponentEffect::Mesh => Color::BLACK.alpha(0.72),
                ComponentEffect::Glass => Color::BLACK.alpha(0.25),
                _ => Color::TRANSPARENT,
            }
        } else {
            Color::TRANSPARENT
        })
    } else {
        content
    };
    let mut root = el().w_full().radius(radius);
    if style == ComponentEffect::Glass {
        root = root.child(
            el().absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .radius(radius)
                .gradient(
                    rune::color::oklch(0.48, 0.2, 285.0),
                    rune::color::oklch(0.58, 0.15, 35.0),
                )
                .inert(),
        );
    }
    root.child(
        effect(style.mark())
            .key("material")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .radius(radius)
            .effect_backdrop(style == ComponentEffect::Glass)
            .effect_data([
                preview.strength() * if preview.disabled { 0.35 } else { 1.0 },
                0.0,
                0.0,
                0.0,
            ])
            .inert(),
    )
    .child(content.radius(radius))
}

fn specimen(title: &str, subtitle: &str, content: El, kit: &Kit) -> El {
    col()
        .w_full()
        .gap(16.0)
        .child(
            row()
                .w_full()
                .justify_between()
                .center_y()
                .child(label(title, 15.0, 600.0, kit))
                .child(label(subtitle, 10.0, 500.0, kit).color(kit.c.text_muted)),
        )
        .child(content)
}

fn preview_kit(kit: &Kit, style: ComponentEffect) -> Kit {
    let mut palette = kit.clone();
    if matches!(style, ComponentEffect::Metal | ComponentEffect::Plastic) {
        palette.c.text_base = Color::BLACK;
        palette.c.text_muted = Color::BLACK.alpha(0.65);
        palette.c.text_faint = Color::BLACK.alpha(0.5);
        palette.c.icon_base = Color::BLACK;
        palette.c.bg_button_neutral = Color::WHITE.alpha(0.65);
        palette.c.std_text = Color::BLACK;
        palette.c.std_icon = Color::BLACK;
        palette.c.text_weak = Color::BLACK.alpha(0.65);
    } else if matches!(style, ComponentEffect::Glass | ComponentEffect::Mesh) {
        palette.c.text_base = Color::WHITE;
        palette.c.text_muted = Color::WHITE.alpha(0.8);
        palette.c.text_faint = Color::WHITE.alpha(0.7);
        palette.c.icon_base = Color::WHITE;
        palette.c.std_text = Color::WHITE;
        palette.c.std_icon = Color::WHITE;
        palette.c.text_weak = Color::WHITE.alpha(0.8);
    }
    palette
}

fn inspector(state: &State, kit: &Kit) -> El {
    let preview = &state.effects_preview;
    let mut styles = col()
        .w_full()
        .gap(4.0)
        .role("radiogroup")
        .label("Effect material");
    for style in ComponentEffect::ALL {
        styles = styles.child(
            RadioItem::new(style.label(), preview.style == style)
                .on_select(move |s: &mut State| s.effects_preview.style = style)
                .build(kit)
                .key(format!("effect-style-{}", style.mark())),
        );
    }
    col().key("effects-inspector").w(236.0).h_full().min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false }).gap(20.0)
        .child(eyebrow("MATERIAL", kit))
        .child(styles)
        .child(divider(kit))
        .child(eyebrow("INTENSITY", kit))
        .child(SegmentedControl::new(&["Low", "Medium", "High"], Some(preview.intensity))
            .width(None).on_pick(|s: &mut State, index, _| s.effects_preview.intensity = index)
            .build(kit).key("effect-intensity"))
        .child(eyebrow("SPEED", kit))
        .child(SegmentedControl::new(&["Slow", "Normal", "Fast"], Some(preview.speed))
            .width(None).on_pick(|s: &mut State, index, _| s.effects_preview.speed = index)
            .build(kit).key("effect-speed"))
        .child(eyebrow("CORNERS", kit))
        .child(SegmentedControl::new(&["6", "12", "22"], Some(preview.rounding))
            .width(None).on_pick(|s: &mut State, index, _| s.effects_preview.rounding = index)
            .build(kit).key("effect-rounding"))
        .child(divider(kit))
        .child(Switch::new(preview.disabled).label("Disabled controls")
            .on_toggle(|s: &mut State| s.effects_preview.disabled = !s.effects_preview.disabled)
            .build(kit).key("effect-disabled"))
        .child(Switch::new(preview.reduced_motion).label("Reduced motion")
            .on_toggle(|s: &mut State| s.effects_preview.reduced_motion = !s.effects_preview.reduced_motion)
            .build(kit).key("effect-reduced-motion"))
        .child(Switch::new(preview.contrast).label("Protect text contrast")
            .on_toggle(|s: &mut State| s.effects_preview.contrast = !s.effects_preview.contrast)
            .build(kit).key("effect-contrast"))
        .child(button("Copy configuration").icon(kit.glyph.copy)
            .on_press_tx(|s: &mut State, tx| {
                let preview = &s.effects_preview;
                tx.copy(format!("Component effects\nMaterial: {}\nIntensity: {:.2}\nSpeed: {:.2}\nCorner radius: {:.0} pt\nReduced motion: {}\nDisabled: {}",
                    preview.style.label(), preview.strength(), preview.rate(), preview.radius(), preview.reduced_motion, preview.disabled));
                s.effects_preview.message = "Configuration copied".into();
            }).build(kit).key("effect-copy"))
        .child(hint("Local preview only. No library, theme or preference changes are saved.", kit))
}

fn controls(state: &State, kit: &Kit, layers: &mut Vec<El>) -> El {
    let preview = &state.effects_preview;
    let style = preview.style;
    let palette = preview_kit(kit, style);
    let control_state = if preview.disabled {
        rune_kit::State::Disabled
    } else {
        rune_kit::State::Rest
    };
    let action = button("Try this button")
        .icon(&icons::PLAY)
        .disabled(preview.disabled)
        .on_press(|s: &mut State| {
            s.effects_preview.presses = s.effects_preview.presses.saturating_add(1);
            s.effects_preview.message =
                format!("Button activated {} times", s.effects_preview.presses);
        })
        .build(&palette)
        .w_full()
        .h(56.0)
        .key("effect-button");
    let input = TextInput::new(&preview.text)
        .label("Sample input")
        .placeholder("Type something…")
        .width(None)
        .height(44.0)
        .disabled(preview.disabled)
        .on_edit(|s: &mut State| &mut s.effects_preview.text)
        .on_submit(|s: &mut State, _| s.effects_preview.message = "Input submitted locally".into())
        .build(&palette)
        .key("effect-input");
    let notes = TextArea::new(&preview.notes)
        .label("Sample text area")
        .placeholder("A larger surface for light and material…")
        .width(None)
        .rows(3)
        .disabled(preview.disabled)
        .on_edit(|s: &mut State| &mut s.effects_preview.notes)
        .build(&palette)
        .key("effect-notes")
        .h(92.0);
    let selections = col()
        .w_full()
        .gap(18.0)
        .child(
            Switch::new(preview.enabled)
                .label("Background processing")
                .disabled(preview.disabled)
                .on_toggle(|s: &mut State| s.effects_preview.enabled = !s.effects_preview.enabled)
                .build(kit)
                .key("effect-switch"),
        )
        .child(
            Checkbox::new(preview.checked)
                .label("Include extra details")
                .disabled(preview.disabled)
                .on_toggle(|s: &mut State| s.effects_preview.checked = !s.effects_preview.checked)
                .build(kit)
                .key("effect-checkbox"),
        )
        .child(surface(
            style,
            SegmentedControl::new(&["Overview", "Details", "History"], Some(preview.selection))
                .width(None)
                .label("Sample view")
                .segment_state(0, control_state)
                .segment_state(1, control_state)
                .segment_state(2, control_state)
                .on_pick(|s: &mut State, index, _| s.effects_preview.selection = index)
                .build(&palette)
                .key("effect-segments"),
            preview,
        ));
    let menu = Menu::new("effect-menu")
        .trigger(surface(
            style,
            button("Open menu")
                .disabled(preview.disabled)
                .on_press(|s: &mut State| {
                    s.effects_preview.menu_open = !s.effects_preview.menu_open
                })
                .build(&palette)
                .w_full()
                .key("effect-menu-button"),
            preview,
        ))
        .open(preview.menu_open)
        .on_close(|s: &mut State| s.effects_preview.menu_open = false)
        .item(
            MenuItem::new("Duplicate sample")
                .icon(kit.glyph.copy)
                .on_select(|s: &mut State| {
                    s.effects_preview.menu_open = false;
                    s.effects_preview.message = "Sample duplicated locally".into();
                }),
        )
        .item(
            MenuItem::new("Inspect material")
                .icon(&icons::CUSTOMIZE)
                .on_select(|s: &mut State| {
                    s.effects_preview.menu_open = false;
                    s.effects_preview.dialog_open = true;
                }),
        )
        .separator()
        .item(
            MenuItem::new("Reset sample fields").on_select(|s: &mut State| {
                s.effects_preview.menu_open = false;
                s.effects_preview.text = TextEdit::new("");
                s.effects_preview.notes = TextEdit::new("");
                s.effects_preview.message = "Sample fields reset".into();
            }),
        )
        .build(kit, layers);
    let overlays = col()
        .w_full()
        .gap(20.0)
        .child(menu)
        .child(surface(
            style,
            button("Open dialog")
                .disabled(preview.disabled)
                .on_press(|s: &mut State| s.effects_preview.dialog_open = true)
                .build(&palette)
                .w_full()
                .key("effect-dialog-button"),
            preview,
        ))
        .child(
            row()
                .gap(10.0)
                .center_y()
                .child(
                    toolbar_icon(&icons::FAVOURITE, "Sample icon button", kit)
                        .disabled(preview.disabled)
                        .on_press(|s: &mut State| {
                            s.effects_preview.message = "Icon button activated".into();
                        })
                        .key("effect-icon-button"),
                )
                .child(hint("Hover for a tooltip", kit)),
        );
    let density = Select::new("effect-choice", &["Default", "Compact", "Spacious"])
        .label("Sample density")
        .width(220.0)
        .current(Some(
            ["Default", "Compact", "Spacious"]
                .get(preview.density)
                .copied()
                .unwrap_or("Default"),
        ))
        .disabled(preview.disabled)
        .open(state.open_select.as_deref() == Some("effect-choice"))
        .on_toggle(|s: &mut State| {
            s.open_select = if s.open_select.as_deref() == Some("effect-choice") {
                None
            } else {
                Some("effect-choice".into())
            };
        })
        .on_close(|s: &mut State| s.open_select = None)
        .on_choose(|s: &mut State, value, _| {
            s.open_select = None;
            s.effects_preview.density = match value {
                "Compact" => 1,
                "Spacious" => 2,
                _ => 0,
            };
        })
        .build(kit, layers);
    col()
        .w_full()
        .gap(28.0)
        .child(specimen(
            "Buttons",
            "NATIVE PRESS / FOCUS",
            surface(style, action, preview),
            kit,
        ))
        .child(divider(kit))
        .child(specimen(
            "Inputs",
            "EDITABLE / SELECTABLE",
            col()
                .w_full()
                .gap(20.0)
                .child(
                    Field::new()
                        .label("Single line")
                        .control(surface(style, input, preview))
                        .build(kit),
                )
                .child(
                    Field::new()
                        .label("Text area")
                        .control(surface(style, notes, preview))
                        .build(kit),
                ),
            kit,
        ))
        .child(divider(kit))
        .child(
            row()
                .w_full()
                .gap(32.0)
                .items_start()
                .child(
                    specimen("Selection", "STATE", selections, kit)
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(
                    specimen("Overlays", "FOCUS / DISMISSAL", overlays, kit)
                        .grow(1.0)
                        .min_w(0.0),
                ),
        )
        .child(divider(kit))
        .child(
            Field::new()
                .label("Sample density")
                .control(density)
                .build(kit),
        )
}

fn surfaces(state: &State, kit: &Kit) -> El {
    let preview = &state.effects_preview;
    let palette = preview_kit(kit, preview.style);
    let card = Card::new()
        .title("A material surface", Some(&icons::GRID))
        .description("Native text and actions stay sharp above the GPU material.")
        .action(
            button("Inspect")
                .disabled(preview.disabled)
                .on_press(|s: &mut State| s.effects_preview.dialog_open = true)
                .build(&palette)
                .key("effect-card-inspect"),
        )
        .build(&palette)
        .padding(16.0);
    let disclosure = Collapsible::new("Expandable details", preview.expanded).disabled(preview.disabled)
        .on_toggle(|s: &mut State| s.effects_preview.expanded = !s.effects_preview.expanded)
        .child(hint("The content is part of the layout. Disclosure state and keyboard focus remain owned by Rune.", &palette).pt(12.0))
        .build(&palette).padding(16.0).key("effect-disclosure");
    let badge = Badge::new("Experimental")
        .variant(BadgeVariant::Accent)
        .standard()
        .build(kit);
    col()
        .w_full()
        .gap(32.0)
        .child(specimen(
            "Cards",
            "CONTENT / ACTIONS",
            surface(preview.style, card, preview),
            kit,
        ))
        .child(divider(kit))
        .child(specimen(
            "Disclosure",
            "EXPANDED / COLLAPSED",
            surface(preview.style, disclosure, preview),
            kit,
        ))
        .child(divider(kit))
        .child(specimen(
            "Badges and icons",
            "COMPACT GEOMETRY",
            row()
                .gap(20.0)
                .center_y()
                .child(surface(preview.style, badge, preview).w(140.0))
                .child(
                    surface(
                        preview.style,
                        toolbar_icon(&icons::FAVOURITE, "Toggle sample favourite", &palette)
                            .disabled(preview.disabled)
                            .on_press(|s: &mut State| {
                                s.effects_preview.message = "Sample favourite toggled".into();
                            })
                            .size(40.0, 40.0)
                            .key("effect-surface-icon"),
                        preview,
                    )
                    .w(40.0),
                ),
            kit,
        ))
        .child(divider(kit))
        .child(specimen(
            "Neutral reference",
            "UNCHANGED RUNE KIT",
            button("Baseline button")
                .disabled(preview.disabled)
                .on_press(|s: &mut State| {
                    s.effects_preview.message = "Baseline button activated".into()
                })
                .build(kit)
                .key("effect-baseline"),
            kit,
        ))
}

fn data(state: &State, kit: &Kit) -> El {
    let preview = &state.effects_preview;
    let palette = preview_kit(kit, preview.style);
    let names = ["Primary button", "Text input", "Selection", "Overlay"];
    let table = Table::new(
        |s: &State| &s.effects_preview.table,
        |s: &mut State| &mut s.effects_preview.table,
        |_: &State| 4,
        |_: &State, index| format!("sample-{index}").into(),
    )
    .column(
        TableColumn::new("component", "Component")
            .flex(2.0)
            .min(100.0),
    )
    .column(TableColumn::new("status", "State").flex(1.0).min(70.0))
    .cell(move |_: &State, index, column| {
        if column == "component" {
            Cell::text(names.get(index).copied().unwrap_or("Component"))
        } else {
            Cell::text("Ready").muted()
        }
    })
    .on_select(|s: &mut State, id, _| s.effects_preview.message = format!("Selected {id}"))
    .build(state, &palette)
    .h(210.0)
    .key("effect-table")
    .disabled(preview.disabled);
    let table = if preview.disabled {
        table.inert().opacity(0.45)
    } else {
        table
    };
    col()
        .w_full()
        .gap(32.0)
        .child(specimen(
            "Progress",
            "DETERMINATE",
            col()
                .w_full()
                .gap(24.0)
                .child(surface(
                    preview.style,
                    Progress::new(Some(preview.progress))
                        .label("Sample operation")
                        .show_value()
                        .build(&palette)
                        .padding(16.0),
                    preview,
                ))
                .child(
                    row().child(
                        button("Advance progress")
                            .disabled(preview.disabled)
                            .on_press(|s: &mut State| {
                                s.effects_preview.progress = if s.effects_preview.progress >= 1.0 {
                                    0.0
                                } else {
                                    (s.effects_preview.progress + 0.15).min(1.0)
                                };
                            })
                            .build(kit)
                            .key("effect-progress"),
                    ),
                ),
            kit,
        ))
        .child(divider(kit))
        .child(specimen(
            "Table",
            "SELECT / RESIZE / KEYBOARD",
            surface(preview.style, table, preview),
            kit,
        ))
        .child(hint(
            "Effect values and table selections are demonstration data, not library records.",
            kit,
        ))
}

pub fn view(state: &State, kit: &Kit, layers: &mut Vec<El>) -> El {
    let preview = &state.effects_preview;
    let samples = match preview.category {
        1 => surfaces(state, kit),
        2 => data(state, kit),
        _ => controls(state, kit, layers),
    };
    let stage = col()
        .key("effects-stage")
        .w_full()
        .h_full()
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .padding(32.0)
        .gap(28.0)
        .radius(20.0)
        .fill(kit.c.layer_02)
        .border(0.5, kit.c.border_base)
        .child(
            col()
                .w_full()
                .gap(8.0)
                .child(eyebrow("LIVE COMPONENT SURFACE", kit))
                .child(heading(preview.style.label(), 28.0, kit))
                .child(hint(preview.style.description(), kit)),
        )
        .child(
            SegmentedControl::new(&["Controls", "Surfaces", "Data"], Some(preview.category))
                .label("Component category")
                .width(None)
                .on_pick(|s: &mut State, index, _| s.effects_preview.category = index)
                .build(kit)
                .key("effect-category"),
        )
        .child(
            label(&preview.message, 11.0, 500.0, kit)
                .live_region(rune::LiveRegion::Polite)
                .key("effect-feedback"),
        )
        .child(samples);
    if preview.dialog_open && state.overlay == Overlay::None {
        let body = col().w_full().gap(24.0)
            .child(hint("This is a real Rune Kit dialog. Try Tab, Escape and clicking outside. No application data is modified.", kit))
            .child(surface(preview.style, button("Confirm sample").build(&preview_kit(kit, preview.style))
                .w_full().h(48.0).on_press(|s: &mut State| {
                    s.effects_preview.dialog_open = false;
                    s.effects_preview.message = "Sample dialog confirmed".into();
                }).key("effect-dialog-confirm"), preview));
        Dialog::new("effect-sample", "Material inspection")
            .description(preview.style.label())
            .open(true)
            .fit()
            .width(480.0)
            .click_outside(true)
            .on_close(|s: &mut State| s.effects_preview.dialog_open = false)
            .child(modal_body(body, 300.0))
            .build(kit, layers);
    }
    col().key("component-effects-page").w_full().h_full().gap(24.0)
        .child(row().w_full().gap(16.0).center_y().justify_between()
            .child(col().grow(1.0).min_w(0.0).gap(8.0)
                .child(eyebrow("DEVELOPER TOOLS / RUNE FX", kit))
                .child(heading("Component effects", 32.0, kit)))
            .child(row().gap(8.0)
                .child(button(if preview.paused { "Resume" } else { "Pause" })
                    .on_press(|s: &mut State| s.effects_preview.paused = !s.effects_preview.paused)
                    .build(kit).key("effect-pause"))
                .child(button("Reset").variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut State| s.effects_preview = EffectsPreview::default())
                    .build(kit).key("effect-reset"))))
        .child(hint("A working bench for materials, light and component states. Use Neutral to compare with the unchanged controls.", kit))
        .child(row().w_full().grow(1.0).min_h(0.0).gap(32.0).items_start()
            .child(inspector(state, kit))
            .child(stage.grow(1.0).min_w(0.0)))
}
