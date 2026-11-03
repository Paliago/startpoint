use super::*;
use crate::editorial_collection::{self as editorial, CAPTION_HEIGHT, INTRO_HEIGHT, Placement};
use gpui_component::button::ButtonCustomVariant;

impl LauncherView {
    pub(super) fn design(&self) -> &'static crate::design::Profile {
        crate::design::resolve(&self.appearance)
    }

    pub(super) fn editorial_active(&self) -> bool {
        self.screen == StartScreen::Recents
            && !self.settings_open
            && self.design().recents == crate::design::RecentPresentation::Organic
    }

    /// Drive ambient artwork from the existing UI timer, not hover notifications
    /// or a renderer's current-view cache. It also runs behind open UI panels.
    pub(super) fn tick_ambient_artwork(&mut self, cx: &mut Context<Self>) -> bool {
        let running = !self.render_power.paused()
            && self.design().ambient_enabled(
                self.editorial_active(),
                self.snapshot.ready,
                self.recent_games().len(),
                appearance::reduced_motion(),
            );
        self.collection_flow.update(running);
        if running {
            cx.notify();
        }
        running
    }

    pub(super) fn collection_overlay_open(&self) -> bool {
        self.settings_open
            || self.command_mode.is_some()
            || self.customization_open
            || self.theme_editor.is_some()
            || self.jobs_open
            || self.game_menu_open
            || self.editor_is_modal()
    }

    pub(super) fn recents_layout_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.weak_entity();
        choice_dropdown(
            "recents-layout",
            self.appearance.recents_layout,
            [
                RecentsLayout::Carousel,
                RecentsLayout::Editorial,
                RecentsLayout::Gallery,
            ]
            .into_iter()
            .map(|layout| {
                let mut settings = self.appearance.clone();
                settings.recents_layout = layout;
                (layout, crate::design::resolve(&settings).name.into())
            })
            .collect(),
            self.snapshot.ready,
            move |recents_layout, window, cx| {
                let _ = owner.update(cx, |view, cx| {
                    view.set_appearance(
                        AppearanceSettings {
                            recents_layout,
                            ..view.appearance.clone()
                        },
                        window,
                        cx,
                    );
                    view.focus.focus(window, cx);
                });
            },
            cx,
        )
    }

    pub(super) fn prepare_collection(&mut self, window: &Window) {
        if !self.editorial_active() {
            return;
        }
        let count = self.recent_games().len();
        let width = self.main_content_width(window).round() as u32;
        let key = (count, width);
        if self.collection_list_key != Some(key) {
            let rows = count.div_ceil(2).max(1) + 1;
            let previous = self
                .collection_list_key
                .map(|_| self.collection_list.logical_scroll_top());
            self.collection_list.reset(rows);
            if let Some(previous) = previous {
                self.collection_list.scroll_to(previous);
            }
            self.collection_list_key = Some(key);
        }
    }

    pub(super) fn editorial_collection(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .size_full()
            .image_cache(self.full_resolution_images.clone())
            .child(
                list(
                    self.collection_list.clone(),
                    cx.processor(|view, index: usize, window, cx| {
                        if index == 0 {
                            return view.collection_heading(window, cx);
                        }
                        let games = view.recent_games();
                        if !view.snapshot.ready || games.is_empty() {
                            return view.collection_empty(cx);
                        }
                        let width = view.main_content_width(window);
                        let margin = editorial::margin(width);
                        let content_width = (width - margin * 2.).min(1840.);
                        let start = (index - 1) * 2;
                        let geometry =
                            editorial::row(content_width, index - 1, start + 1 >= games.len());
                        let row = div()
                            .w_full()
                            .h(px(geometry.height))
                            .flex()
                            .justify_center()
                            .child({
                                let mut pair = div()
                                    .w(px(content_width))
                                    .h_full()
                                    .flex()
                                    .items_start()
                                    .gap(px(geometry.gap));
                                for column in 0..2 {
                                    if let Some(game) = games.get(start + column) {
                                        pair = pair.child(view.collection_card(
                                            game,
                                            start + column,
                                            geometry.cards[column],
                                            cx,
                                        ));
                                    }
                                }
                                pair
                            });
                        row.into_any_element()
                    }),
                )
                .size_full()
                .bg(cx.theme().background.alpha(1.))
                .text_color(cx.theme().foreground),
            )
            .into_any_element()
    }

    fn collection_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .h(px(360.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_5()
            .text_color(cx.theme().foreground)
            .when(
                !self.snapshot.ready && self.snapshot.initialization_error.is_none(),
                |view| view.child(Spinner::new()),
            )
            .child(
                div()
                    .font_family(editorial::serif())
                    .italic()
                    .text_2xl()
                    .child(if !self.snapshot.ready {
                        self.snapshot
                            .initialization_error
                            .clone()
                            .unwrap_or_else(|| "Opening Recents…".into())
                    } else {
                        "No recent games".into()
                    }),
            )
            .when(self.snapshot.ready, |view| {
                view.child(div().text_sm().child("No visible installed games yet."))
                    .child(
                        button("collection-empty-add", cx)
                            .label("Add a game")
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.pick(PickerPurpose::AddGame, window, cx)
                            })),
                    )
            })
            .into_any_element()
    }

    fn collection_card(
        &self,
        game: &Game,
        index: usize,
        placement: Placement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = game.id.clone();
        let hover_id = id.clone();
        let launch_id = id.clone();
        let play_id = id.clone();
        let selected = self.selected.as_ref() == Some(&id);
        let motion = self
            .collection_motion
            .value(&id, appearance::reduced_motion());
        let artwork = game
            .artwork
            .get(&ArtworkKind::Background)
            .or_else(|| game.artwork.get(&ArtworkKind::Cover))
            .or_else(|| game.artwork.get(&ArtworkKind::Icon));
        let tooltip: SharedString =
            format!("{} · {}", game.title(), source_name(&game.provider_id)).into();
        let owner = cx.weak_entity();
        let menu_game = game.clone();
        let title = game.title().to_owned();
        let foreground = cx.theme().foreground;
        let card = div()
            .id(SharedString::from(format!("collection-card-{}", id.0)))
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .cursor_pointer()
            .aria_label(title.clone())
            .on_hover(cx.listener(move |view, hovered, _, cx| {
                if *hovered && view.collection_overlay_open() {
                    return;
                }
                view.collection_motion.set(&hover_id, *hovered);
                cx.notify();
            }))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(placement.height))
                    .flex_shrink_0()
                    .child(match artwork {
                        Some(asset) => img(asset.image_path.clone())
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .with_fallback(move || {
                                collection_placeholder(index, foreground).into_any_element()
                            })
                            .into_any_element(),
                        None => collection_placeholder(index, foreground).into_any_element(),
                    })
                    .child(editorial::cutout(
                        index,
                        (!appearance::reduced_motion()).then(|| self.collection_flow.seconds()),
                        motion,
                        cx.theme().background,
                    ))
                    .child(
                        div()
                            .absolute()
                            .right(px(placement.width * 0.055))
                            .bottom(px(placement.height * 0.075))
                            .child(with_accessible_label(
                                Button::new(SharedString::from(format!(
                                    "collection-play-{}",
                                    id.0
                                )))
                                .custom(
                                    ButtonCustomVariant::new(cx)
                                        .color(cx.theme().foreground)
                                        .foreground(cx.theme().background)
                                        .hover(rgb(0x151410).into())
                                        .active(black()),
                                )
                                .w(px(46.))
                                .h(px(46.))
                                .p_0()
                                .rounded_full()
                                .opacity(if selected { 1. } else { motion })
                                .map(keyboard_reveal)
                                .icon(Icon::default().path("icons/play.svg").size(px(17.)))
                                .tooltip(format!("Play {}", game.title()))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(
                                    move |view, _, window, cx| {
                                        cx.stop_propagation();
                                        if view.collection_overlay_open() {
                                            return;
                                        }
                                        view.run_library_game_action(
                                            &play_id,
                                            LibraryGameAction::Launch,
                                            window,
                                            cx,
                                        );
                                    },
                                )),
                                format!("Play {}", game.title()),
                            )),
                    )
                    .id(SharedString::from(format!(
                        "collection-artwork-{}",
                        game.id.0
                    )))
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx)),
            )
            .child(
                div()
                    .w_full()
                    .h(px(CAPTION_HEIGHT))
                    .px(px(14.))
                    .pt(px(6.))
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .font_family(editorial::serif())
                            .italic()
                            .text_color(cx.theme().foreground.alpha(1.))
                            .text_ellipsis()
                            .child(editorial::tracked(&title)),
                    )
                    .child(
                        div().child(
                            with_accessible_label(
                                Button::new(SharedString::from(format!(
                                    "collection-manage-{}",
                                    id.0
                                )))
                                .custom(quiet_variant(cx))
                                .size(px(24.))
                                .p_0()
                                .opacity(if selected { 0.8 } else { motion })
                                .map(keyboard_reveal)
                                .icon(Icon::new(IconName::Ellipsis).small())
                                .tooltip("Game options"),
                                format!("Manage {}", game.title()),
                            )
                            .dropdown_menu(move |menu, _, cx| {
                                library_game_menu(menu, &menu_game, owner.clone(), cx)
                            }),
                        ),
                    ),
            )
            .on_click(cx.listener(move |view, _, window, cx| {
                if !view.collection_overlay_open() {
                    view.select_recent_game(id.clone(), window, cx);
                }
            }))
            .on_double_click(cx.listener(move |view, _, window, cx| {
                if !view.collection_overlay_open() {
                    view.run_library_game_action(&launch_id, LibraryGameAction::Launch, window, cx);
                }
            }));
        div()
            .w(px(placement.width))
            .pt(px(placement.top))
            .flex_shrink_0()
            .child(self.with_library_context_menu(game, card, cx))
            .into_any_element()
    }

    fn collection_heading(&self, window: &Window, cx: &App) -> AnyElement {
        let width = self.main_content_width(window);
        let content_width = (width - editorial::margin(width) * 2.).min(1840.);
        div()
            .id("editorial-heading")
            .w_full()
            .h(px(INTRO_HEIGHT))
            .flex()
            .justify_center()
            .child(
                div()
                    .w(px(content_width))
                    .pt(px(HEADER_HEIGHT + 14.))
                    .px(px(14.))
                    .font_family(self.design().display_font)
                    .italic()
                    .text_size(px(24.))
                    .text_color(cx.theme().foreground)
                    .child(editorial::tracked("Startpoint")),
            )
            .into_any_element()
    }
}

fn quiet_variant(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(cx.theme().foreground)
        .hover(cx.theme().secondary_hover)
        .active(cx.theme().secondary_active)
}

fn collection_placeholder(index: usize, foreground: Hsla) -> Div {
    div()
        .size_full()
        .bg(rgb(match index % 4 {
            0 => 0xd3c8b3,
            1 => 0xb9c2ad,
            2 => 0xb8c1c3,
            _ => 0xc7b5ab,
        }))
        .flex()
        .items_center()
        .justify_center()
        .child(
            Icon::new(IconName::GalleryVerticalEnd)
                .size(px(32.))
                .text_color(foreground.alpha(0.25)),
        )
}

struct KeyboardReveal(Button);
impl InteractiveElement for KeyboardReveal {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.0.interactivity()
    }
}
impl StatefulInteractiveElement for KeyboardReveal {}

fn keyboard_reveal(button: Button) -> Button {
    KeyboardReveal(button)
        .focus_visible(|style| style.opacity(1.))
        .0
}

#[cfg(test)]
mod tests {
    use super::*;
    use startpoint_sdk::{ApiError, ApplicationApi, ApplicationPaths, SettingsSnapshot};
    use std::sync::Mutex;

    struct TestApi {
        snapshot: Mutex<Snapshot>,
        launched: Mutex<Vec<GameId>>,
    }

    impl ApplicationApi for TestApi {
        fn snapshot(&self) -> Snapshot {
            self.snapshot.lock().unwrap().clone()
        }
        fn subscribe(&self) -> async_channel::Receiver<AppEvent> {
            async_channel::unbounded().1
        }
        fn cancel(&self, _: JobId) {}
        fn submit(&self, command: Command) -> Result<JobId, ApiError> {
            match command {
                Command::SetAppearance { settings } => {
                    self.snapshot.lock().unwrap().settings.appearance = settings
                }
                Command::LaunchGame { game_id } => self.launched.lock().unwrap().push(game_id),
                _ => {}
            }
            Ok(1)
        }
    }

    #[::core::prelude::v1::test]
    fn editorial_pointer_keyboard_and_layout_switching_keep_the_correct_game_context() {
        let directory = tempfile::tempdir().unwrap();
        let api = Arc::new(TestApi {
            snapshot: Mutex::new(Snapshot {
                ready: true,
                games: Arc::new(
                    (0..15)
                        .map(|index| {
                            let mut game = sample_library_game();
                            game.id = GameId(format!("collection:{index}"));
                            game.discovered_title = format!("World {index}");
                            game.last_launched = Some(100 - index);
                            game
                        })
                        .collect(),
                ),
                activity: Default::default(),
                journal: Default::default(),
                jobs: vec![],
                settings: SettingsSnapshot {
                    appearance: AppearanceSettings {
                        recents_layout: RecentsLayout::Editorial,
                        theme: "Violet".into(),
                        mode: AppearanceMode::Dark,
                        sidebar_visible: false,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                paths: ApplicationPaths {
                    data_directory: directory.path().join("data"),
                    cache_directory: directory.path().join("cache"),
                },
                initialization_error: None,
                providers: vec![],
            }),
            launched: Default::default(),
        });
        let mut app = TestApp::new();
        app.update(|cx| {
            gpui_component::init(cx);
            cx.set_global(theme::Catalog::default());
            cx.bind_keys([
                KeyBinding::new("ctrl-b", ToggleSidebar, Some("Startpoint")),
                KeyBinding::new("cmd-b", ToggleSidebar, Some("Startpoint")),
            ]);
        });
        let mut window = app.open_window(|window, cx| LauncherView::new(api.clone(), window, cx));
        window.update(|_, window, _| window.activate_window());
        window.draw();
        window.read(|view, cx| {
            assert!(view.editorial_active());
            assert_eq!(view.recent_games().len(), 6);
            assert_eq!(view.selected.as_ref().unwrap().0, "collection:0");
            assert_eq!(cx.theme().background, Hsla::from(rgb(0xf9efdf)));
            assert_eq!(cx.theme().foreground, Hsla::from(rgb(0x302c27)));
            assert_eq!(crate::glass::base(cx).a, 1.);
            assert_eq!(
                crate::design::current(cx).shell,
                crate::design::Shell::ToolbarIslands
            );
            assert_eq!(crate::glass::Palette::of(cx).surface.a, 1.);
            assert!(crate::glass::Palette::of(cx).shadows().is_empty());
        });
        window.update(|view, _, cx| {
            // No pointer has entered any artwork. The UI tick still advances
            // ambient time, including behind an open panel.
            let reduced = appearance::reduced_motion();
            assert_eq!(view.tick_ambient_artwork(cx), !reduced);
            let before = view.collection_flow.seconds();
            view.customization_open = true;
            assert_eq!(view.tick_ambient_artwork(cx), !reduced);
            if !reduced {
                assert!(view.collection_flow.seconds() > before);
            }
            view.customization_open = false;
        });
        let mut other = app.open_window(|window, cx| LauncherView::new(api.clone(), window, cx));
        other.update(|_, window, _| window.activate_window());
        window.update(|view, window, cx| {
            view.refresh_render_power(window, true, cx);
            assert!(view.render_power.paused());
            assert!(!view.tick_ambient_artwork(cx));
            let frozen = view.collection_flow.seconds();
            assert!(!view.tick_ambient_artwork(cx));
            assert_eq!(view.collection_flow.seconds(), frozen);
            // Turning the preference off restores ambient effects while
            // unfocused, without changing the user's layout or selection.
            view.appearance.pause_in_background = false;
            view.refresh_render_power(window, true, cx);
            assert!(!view.render_power.paused());
            assert_eq!(view.tick_ambient_artwork(cx), !appearance::reduced_motion());
            view.appearance.pause_in_background = true;
            window.activate_window();
        });
        window.draw();
        // The main Recents screen supports the same sidebar toggle as Classic.
        // Its artwork must reflow inside the remaining width, not overflow it.
        window.simulate_keystroke("ctrl-b");
        window.update(|view, window, _| {
            view.sidebar_transition = None;
            assert!(view.appearance.sidebar_visible);
            assert_eq!(view.visible_sidebar_progress(), 1.);
            assert_eq!(
                view.main_content_width(window),
                f32::from(window.viewport_size().width) - view.appearance.sidebar_width
            );
        });
        window.draw();
        window.update(|view, window, _| {
            assert_eq!(
                view.collection_list_key.unwrap().1,
                view.main_content_width(window).round() as u32
            );
        });
        // Source rows are navigation: clicking one from Recents must actually
        // enter Library, not silently change an invisible Library-only filter.
        window.simulate_click(point(px(80.), px(210.)), MouseButton::Left);
        window.draw();
        window.read(|view, _| {
            assert!(view.screen == StartScreen::Library);
            assert_eq!(view.source_filter.as_deref(), Some("steam"));
            assert_eq!(view.visible.len(), 15);
            assert_eq!(view.recent_games().len(), 6);
        });
        window.update(|view, window, cx| view.switch_screen(StartScreen::Recents, window, cx));
        window.simulate_keystroke("ctrl-b");
        window.update(|view, window, _| {
            view.sidebar_transition = None;
            assert!(!view.appearance.sidebar_visible);
            assert_eq!(
                view.main_content_width(window),
                f32::from(window.viewport_size().width)
            );
        });
        window.draw();
        // A real pointer hit on the smaller, offset second artwork.
        let inside_second = window.update(|_, window, _| {
            let width = f32::from(window.viewport_size().width);
            let margin = editorial::margin(width);
            let geometry = editorial::row((width - margin * 2.).min(1840.), 0, false);
            point(
                px(margin + geometry.cards[0].width + geometry.gap + geometry.cards[1].width * 0.5),
                px(INTRO_HEIGHT + geometry.cards[1].top + geometry.cards[1].height * 0.5),
            )
        });
        window.simulate_click(inside_second, MouseButton::Left);
        window.read(|view, _| assert_eq!(view.selected.as_ref().unwrap().0, "collection:1"));
        let second_play = window.update(|view, window, _| {
            // Settle hover geometry so the real button's center is deterministic.
            view.collection_motion = Default::default();
            let width = f32::from(window.viewport_size().width);
            let margin = editorial::margin(width);
            let geometry = editorial::row((width - margin * 2.).min(1840.), 0, false);
            let card = geometry.cards[1];
            point(
                px(margin + geometry.cards[0].width + geometry.gap + card.width * 0.945 - 23.),
                px(INTRO_HEIGHT + card.top + card.height * 0.925 - 23.),
            )
        });
        window.draw();
        window.simulate_click(second_play, MouseButton::Left);
        assert_eq!(
            api.launched.lock().unwrap().as_slice(),
            &[GameId("collection:1".into())]
        );
        // The caption menu must receive the trigger's mouse-down. Stopping it
        // on the nested Button prevents the native popover from opening.
        let options = window.update(|_, window, _| {
            let width = f32::from(window.viewport_size().width);
            let margin = editorial::margin(width);
            let geometry = editorial::row((width - margin * 2.).min(1840.), 0, false);
            let card = geometry.cards[1];
            point(
                px(margin + geometry.cards[0].width + geometry.gap + card.width - 26.),
                px(INTRO_HEIGHT + card.top + card.height + 18.),
            )
        });
        window.simulate_click(options, MouseButton::Left);
        window.draw();
        window.read(|view, _| {
            assert!(
                view.game_menu_open,
                "caption options must open a real popup"
            )
        });
        window.simulate_keystrokes("down down enter");
        window.draw();
        window.read(|view, _| {
            assert!(view.editor_is_modal());
            assert!(
                view.detail_tab == DetailTab::Metadata,
                "menu selection must open the editor without a card click closing it"
            );
            assert_eq!(view.selected.as_ref().unwrap().0, "collection:1");
        });
        let (launch_tab, close_editor) = window.update(|_, window, _| {
            let viewport = window.viewport_size();
            let width = px(860.).min(viewport.width - px(64.));
            let height = (viewport.height - px(64.)).min(px(720.));
            let left = (viewport.width - width) / 2.;
            let top = (viewport.height - height) / 2.;
            (
                point(left + px(84.), top + px(188.)),
                point(left + width - px(40.), top + px(36.)),
            )
        });
        window.simulate_click(launch_tab, MouseButton::Left);
        window.read(|view, _| {
            assert!(
                view.detail_tab == DetailTab::Launch,
                "editor tabs must remain clickable above artwork"
            )
        });
        window.simulate_click(close_editor, MouseButton::Left);
        window.read(|view, _| assert!(view.detail_tab == DetailTab::Overview));
        // Ctrl+P must work even after a popup/editor has owned keyboard focus.
        window.simulate_keystroke("ctrl-p");
        window.draw();
        window.read(|view, _| assert!(view.command_mode == Some(CommandMode::Actions)));
        window.simulate_keystroke("escape");
        window.read(|view, _| assert!(view.command_mode.is_none()));
        let customize = window.update(|_, window, _| {
            let caption_and_gap = if cfg!(target_os = "windows") {
                102.
            } else {
                0.
            };
            point(
                window.viewport_size().width - px(128. + caption_and_gap),
                px(22.),
            )
        });
        window.simulate_click(customize, MouseButton::Left);
        window.draw();
        window.read(|view, _| {
            assert!(
                view.customization_open,
                "the dedicated Customize button must open Recents controls"
            )
        });
        let close_customize =
            window.update(|_, window, _| point(window.viewport_size().width - px(44.), px(84.)));
        window.simulate_click(close_customize, MouseButton::Left);
        window.read(|view, _| assert!(!view.customization_open));
        let library_tab = window.update(|_, window, _| {
            let native_inset = crate::window_chrome::native_left_inset(
                cfg!(target_os = "macos"),
                window.is_fullscreen(),
            );
            point(px(270. + native_inset), px(22.))
        });
        window.simulate_click(library_tab, MouseButton::Left);
        window.draw();
        window.read(|view, cx| {
            assert!(view.screen == StartScreen::Library);
            assert_eq!(view.design().shell, crate::design::Shell::ToolbarIslands);
            assert_eq!(crate::glass::base(cx), Hsla::from(rgb(0xf9efdf)));
        });
        window.update(|view, window, cx| view.switch_screen(StartScreen::Activity, window, cx));
        window.draw();
        window.read(|view, cx| {
            assert_eq!(view.design().material, crate::design::Material::Paper);
            assert_eq!(cx.theme().background, Hsla::from(rgb(0xf9efdf)));
        });
        window.update(|view, window, cx| view.open_settings(&OpenSettings, window, cx));
        window.draw();
        window.read(|view, cx| {
            assert!(view.settings_open);
            assert_eq!(view.design().shell, crate::design::Shell::ToolbarIslands);
            assert_eq!(crate::glass::base(cx), Hsla::from(rgb(0xf9efdf)));
        });
        window.update(|view, window, cx| view.switch_screen(StartScreen::Recents, window, cx));
        window.draw();
        window.simulate_keystroke("cmd-p");
        window.read(|view, _| assert!(view.command_mode == Some(CommandMode::Actions)));
        window.simulate_keystroke("escape");
        window.update(|view, window, cx| {
            assert!(!view.advance_carousel(window, cx));
            for _ in 0..3 {
                view.move_selection(true, window, cx);
            }
            assert_eq!(view.selected.as_ref().unwrap().0, "collection:4");
            view.launch_selected(&LaunchSelected, window, cx);
        });
        window.draw();
        assert_eq!(
            api.launched.lock().unwrap().last().unwrap().0,
            "collection:4"
        );
        window.read(|view, _| assert!(view.collection_list.logical_scroll_top().item_ix > 0));
        let header_gap = window.update(|view, window, _| {
            view.collection_list.scroll_to(ListOffset {
                item_ix: 1,
                offset_in_item: px(140.),
            });
            point(window.viewport_size().width / 2., px(22.))
        });
        window.draw();
        window.simulate_click(header_gap, MouseButton::Left);
        window.read(|view, _| {
            assert_eq!(
                view.selected.as_ref().unwrap().0,
                "collection:4",
                "scrolling artwork behind the translucent header must not receive its clicks"
            )
        });
        assert_eq!(api.launched.lock().unwrap().len(), 2);
        window.update(|view, window, cx| {
            view.set_appearance(
                AppearanceSettings {
                    recents_layout: RecentsLayout::Carousel,
                    ..view.appearance.clone()
                },
                window,
                cx,
            );
            assert_eq!(view.recent_games().len(), 5);
            assert_eq!(view.selected.as_ref().unwrap().0, "collection:4");
            assert!(
                !view.appearance.sidebar_visible,
                "editorial mode must not overwrite sidebar preferences"
            );
            assert_eq!(view.appearance.theme, "Violet");
            assert_eq!(view.appearance.mode, AppearanceMode::Dark);
            assert_eq!(
                crate::design::current(cx).material,
                crate::design::Material::SmokedGlass
            );
            assert!(
                cx.theme().background.l < 0.5,
                "switching back restores the authored dark colour theme"
            );
            view.set_appearance(
                AppearanceSettings {
                    recents_layout: RecentsLayout::Editorial,
                    ..view.appearance.clone()
                },
                window,
                cx,
            );
            view.snapshot.games = Arc::new(vec![]);
            view.ensure_selection(window, cx);
        });
        window.draw();
        window.read(|view, _| assert!(view.selected.is_none()));
    }
}
