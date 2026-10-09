use crate::{appearance, theme};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Colorize, Disableable, IconName, Selectable, Sizable, ThemeMode, ThemeSet,
    button::{Button, ButtonVariants},
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    input::{Input, InputEvent, InputState},
};
use std::path::PathBuf;

pub(crate) enum EditorEvent {
    Saved(String),
    Closed,
}

const COLOR_LABELS: [&str; 5] = ["Background", "Text", "Primary", "Surface", "Sidebar"];

pub(crate) struct Editor {
    draft: ThemeSet,
    original_name: String,
    mode: ThemeMode,
    name: Entity<InputState>,
    font: Entity<InputState>,
    colors: Vec<Entity<ColorPickerState>>,
    directory: PathBuf,
    error: Option<String>,
    saving: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<EditorEvent> for Editor {}

impl Editor {
    pub fn new(
        selected: &str,
        directory: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let catalog = cx.global::<theme::Catalog>();
        let draft = ThemeSet {
            name: selected.to_string().into(),
            themes: [ThemeMode::Light, ThemeMode::Dark]
                .into_iter()
                .map(|mode| catalog.configuration(selected, mode).as_ref().clone())
                .collect(),
            ..ThemeSet::default()
        };
        let mode = cx.theme().mode;
        let configuration = catalog.configuration(selected, mode);
        let mut name = selected.to_string();
        if !catalog.files.contains_key(&selected.to_ascii_lowercase()) {
            name = format!("{selected} Custom");
            let mut suffix = 2;
            while catalog.contains(&name) {
                name = format!("{selected} Custom {suffix}");
                suffix += 1;
            }
        }
        let name = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let font = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Native font")
                .default_value(configuration.font_family.clone().unwrap_or_default())
        });
        let colors = [
            cx.theme().background,
            cx.theme().foreground,
            cx.theme().primary,
            cx.theme().secondary,
            cx.theme().sidebar,
        ]
        .into_iter()
        .map(|color| cx.new(|cx| ColorPickerState::new(window, cx).default_value(color)))
        .collect::<Vec<_>>();
        let mut subscriptions = Vec::new();
        for (index, picker) in colors.iter().enumerate() {
            subscriptions.push(cx.subscribe_in(
                picker,
                window,
                move |editor, _, event, window, cx| {
                    if let ColorPickerEvent::Change(Some(color)) = event {
                        editor.set_color(index, color.alpha(1.), window, cx);
                    }
                },
            ));
        }
        subscriptions.push(
            cx.subscribe_in(&font, window, |editor, field, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = field.read(cx).value().trim().to_string();
                    if let Some(variant) = editor
                        .draft
                        .themes
                        .iter_mut()
                        .find(|variant| variant.mode == editor.mode)
                    {
                        variant.font_family = (!value.is_empty()).then(|| value.into());
                    }
                    editor.apply(window, cx);
                }
            }),
        );
        subscriptions.push(cx.subscribe_in(&name, window, |editor, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                editor.error = None;
                cx.notify();
            }
        }));
        Self {
            draft,
            original_name: selected.to_string(),
            mode,
            name,
            font,
            colors,
            directory,
            error: None,
            saving: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn preview_theme(&self) -> (ThemeSet, ThemeMode) {
        (self.draft.clone(), self.mode)
    }

    pub fn is_saving(&self) -> bool {
        self.saving
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        appearance::preview(&self.draft, self.mode, window, cx);
        cx.notify();
    }

    fn set_mode(&mut self, mode: ThemeMode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        self.apply(window, cx);
        let values = [
            cx.theme().background,
            cx.theme().foreground,
            cx.theme().primary,
            cx.theme().secondary,
            cx.theme().sidebar,
        ];
        for (picker, color) in self.colors.iter().zip(values) {
            picker.update(cx, |picker, cx| picker.set_value(color, window, cx));
        }
        let font = self
            .draft
            .themes
            .iter()
            .find(|variant| variant.mode == mode)
            .and_then(|variant| variant.font_family.clone())
            .unwrap_or_default();
        self.font
            .update(cx, |field, cx| field.set_value(font, window, cx));
    }

    fn set_color(
        &mut self,
        index: usize,
        color: Hsla,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.saving {
            return;
        }
        let Some(variant) = self
            .draft
            .themes
            .iter_mut()
            .find(|variant| variant.mode == self.mode)
        else {
            return;
        };
        let hex: SharedString = color.to_hex().into();
        let colors = &mut variant.colors;
        match index {
            0 => {
                colors.background = Some(hex.clone());
                colors.popover = Some(hex);
            }
            1 => {
                colors.foreground = Some(hex.clone());
                colors.popover_foreground = Some(hex.clone());
                colors.secondary_foreground = Some(hex.clone());
                colors.sidebar_foreground = Some(hex);
            }
            2 => {
                let black = Hsla::from(rgb(0x000000));
                let white = Hsla::from(rgb(0xffffff));
                let text = if theme::contrast(color, black) >= theme::contrast(color, white) {
                    black
                } else {
                    white
                };
                colors.primary = Some(hex.clone());
                colors.primary_hover = Some(hex.clone());
                colors.primary_active = Some(hex.clone());
                colors.ring = Some(hex.clone());
                colors.caret = Some(hex);
                colors.primary_foreground = Some(text.to_hex().into());
            }
            3 => {
                colors.secondary = Some(hex.clone());
                colors.secondary_hover = Some(hex.clone());
                colors.secondary_active = Some(hex.clone());
                colors.muted = Some(hex);
            }
            4 => colors.sidebar = Some(hex),
            _ => return,
        }
        self.apply(window, cx);
    }

    fn restore_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        restore_default_variants(&mut self.draft);
        self.error = None;
        // Refresh the live preview, pickers and font field in the current mode.
        // Keep the draft name and original save target; no file is written here.
        self.set_mode(self.mode, window, cx);
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        self.draft.name = self.name.read(cx).value().trim().to_string().into();
        if self.draft.themes.iter().any(|variant| {
            variant
                .font_family
                .as_ref()
                .is_some_and(|name| name.len() > 80 || name.chars().any(char::is_control))
        }) {
            self.error =
                Some("Font names must be at most 80 bytes without control characters".into());
            cx.notify();
            return;
        }
        let source = match theme::source(&self.draft)
            .and_then(|source| theme::parse(&source).map(|_| source))
        {
            Ok(source) => source,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let catalog = cx.global::<theme::Catalog>().clone();
        let directory = self.directory.clone();
        let original_name = self.original_name.clone();
        self.saving = true;
        self.error = None;
        let task = cx.background_executor().spawn(async move {
            catalog
                .save(&directory, &source, &original_name)
                .map(|name| (name, theme::Catalog::load(&directory)))
        });
        cx.spawn_in(window, async move |editor, cx| {
            let result = task.await;
            let _ = editor.update_in(cx, |editor, _, cx| {
                editor.saving = false;
                match result {
                    Ok((name, catalog)) => {
                        cx.set_global(catalog);
                        cx.emit(EditorEvent::Saved(name));
                    }
                    Err(error) => editor.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let variant = self
            .draft
            .themes
            .iter()
            .find(|variant| variant.mode == self.mode);
        let font_size = variant.and_then(|variant| variant.font_size).unwrap_or(14.);
        let radius = variant.and_then(|variant| variant.radius).unwrap_or(8);
        let contrast = theme::contrast(cx.theme().background, cx.theme().foreground);
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Theme editor"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Preview without saving. Built-in themes are copied, never overwritten.",
                    ),
            )
            .child(
                div().flex().child(
                    Input::new(&self.name)
                        .large()
                        .w_auto()
                        .flex_1()
                        .min_w_0()
                        .disabled(self.saving),
                ),
            )
            .child(div().flex().gap_2().children(
                [ThemeMode::Light, ThemeMode::Dark].into_iter().map(|mode| {
                    Button::new(mode.name())
                        .h(px(32.))
                        .px_3()
                        .label(mode.name())
                        .selected(self.mode == mode)
                        .border_0()
                        .disabled(self.saving)
                        .on_click(cx.listener(move |editor, _, window, cx| {
                            editor.set_mode(mode, window, cx)
                        }))
                }),
            ))
            .children(self.colors.iter().zip(COLOR_LABELS).map(|(picker, label)| {
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(div().text_sm().child(label))
                    .child(ColorPicker::new(picker).anchor(Corner::TopRight))
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(if contrast < 4.5 {
                        cx.theme().warning
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(format!(
                        "Text contrast {contrast:.1}:1{}",
                        if contrast < 4.5 {
                            " — consider 4.5:1 or higher"
                        } else {
                            ""
                        }
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_sm().child("Font family"))
                    .child(
                        div().flex().child(
                            Input::new(&self.font)
                                .large()
                                .w_auto()
                                .flex_1()
                                .min_w_0()
                                .disabled(self.saving),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .child(format!("Font size · {font_size:.0} px")),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .children([-1., 1.].into_iter().map(|step| {
                                Button::new(if step < 0. {
                                    "decrease-font"
                                } else {
                                    "increase-font"
                                })
                                .size(px(32.))
                                .icon(if step < 0. {
                                    IconName::Minus
                                } else {
                                    IconName::Plus
                                })
                                .disabled(self.saving)
                                .tooltip(if step < 0. {
                                    "Decrease font size"
                                } else {
                                    "Increase font size"
                                })
                                .on_click(cx.listener(
                                    move |editor, _, window, cx| {
                                        if let Some(variant) = editor
                                            .draft
                                            .themes
                                            .iter_mut()
                                            .find(|variant| variant.mode == editor.mode)
                                        {
                                            variant.font_size = Some(
                                                (variant.font_size.unwrap_or(14.) + step)
                                                    .clamp(10., 32.),
                                            );
                                        }
                                        editor.apply(window, cx);
                                    },
                                ))
                            })),
                    ),
            )
            .child(div().text_sm().child("Corner radius"))
            .child(
                div()
                    .flex()
                    .gap_1()
                    .children([0usize, 4, 8, 12, 16].into_iter().map(|value| {
                        Button::new(("theme-radius", value))
                            .h(px(32.))
                            .px_3()
                            .label(value.to_string())
                            .selected(radius == value)
                            .border_0()
                            .disabled(self.saving)
                            .on_click(cx.listener(move |editor, _, window, cx| {
                                if let Some(variant) = editor
                                    .draft
                                    .themes
                                    .iter_mut()
                                    .find(|variant| variant.mode == editor.mode)
                                {
                                    variant.radius = Some(value);
                                    variant.radius_lg = Some(value);
                                }
                                editor.apply(window, cx);
                            }))
                    })),
            )
            .when_some(self.error.clone(), |panel, error| {
                panel.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
            .child(
                Button::new("restore-theme-defaults")
                    .h(px(32.))
                    .ghost()
                    .label("Restore defaults")
                    .tooltip("Reset both light and dark variants to Startpoint defaults. Save theme to keep the changes.")
                    .disabled(self.saving)
                    .on_click(cx.listener(|editor, _, window, cx| editor.restore_defaults(window, cx))),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("save-theme")
                            .primary()
                            .h(px(32.))
                            .px_3()
                            .label(if self.saving {
                                "Saving…"
                            } else {
                                "Save theme"
                            })
                            .disabled(self.saving)
                            .on_click(cx.listener(|editor, _, window, cx| editor.save(window, cx))),
                    )
                    .child(
                        Button::new("cancel-theme")
                            .h(px(32.))
                            .px_3()
                            .label("Cancel")
                            .disabled(self.saving)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(EditorEvent::Closed))),
                    ),
            )
    }
}

fn restore_default_variants(draft: &mut ThemeSet) {
    let defaults = theme::Catalog::default();
    draft.themes = [ThemeMode::Light, ThemeMode::Dark]
        .into_iter()
        .map(|mode| defaults.configuration("Startpoint", mode).as_ref().clone())
        .collect();
}

#[cfg(test)]
#[::core::prelude::v1::test]
fn restoring_defaults_resets_both_variants_and_preserves_the_draft_name() {
    let catalog = theme::Catalog::default();
    let mut draft = catalog.themes[1].clone();
    draft.name = "My custom theme".into();
    for variant in &mut draft.themes {
        variant.font_family = Some("Custom font".into());
        variant.font_size = Some(24.);
        variant.radius = Some(0);
        variant.colors.background = Some("#123456".into());
    }
    let edited = theme::source(&draft).expect("edited draft");
    restore_default_variants(&mut draft);
    assert_eq!(draft.name, "My custom theme");
    assert_ne!(theme::source(&draft).expect("restored draft"), edited);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let restored = draft
            .themes
            .iter()
            .find(|variant| variant.mode == mode)
            .unwrap();
        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(catalog.configuration("Startpoint", mode).as_ref()).unwrap()
        );
    }
    let restored = theme::source(&draft).expect("restored draft");
    assert!(
        theme::parse(&restored).is_ok(),
        "restored theme can be saved"
    );
    restore_default_variants(&mut draft);
    assert_eq!(
        theme::source(&draft).unwrap(),
        restored,
        "reset is repeatable"
    );
    // Reset only touches the draft, never the loaded catalog or saved themes.
    assert_ne!(catalog.themes[1].name, draft.name);
}
