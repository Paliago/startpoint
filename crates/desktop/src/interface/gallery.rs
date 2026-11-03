use super::*;
use crate::gallery::{self as exhibition, Layout, Placement};
use gpui_component::button::ButtonCustomVariant;

impl LauncherView {
    pub(super) fn gallery_active(&self) -> bool {
        self.design().recents == crate::design::RecentPresentation::Gallery
            && !self.settings_open
            && matches!(self.screen, StartScreen::Recents | StartScreen::Library)
    }

    pub(super) fn prepare_gallery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.gallery_active() || self.render_power.paused() {
            return;
        }
        let games = self.displayed_games();
        let count = games.len();
        let index = games
            .iter()
            .position(|game| self.selected.as_ref() == Some(&game.id))
            .unwrap_or(0);
        let now = Instant::now();
        self.gallery_motion
            .update(index, count, appearance::reduced_motion(), now);
        if self.gallery_motion.animating(now) {
            window.request_animation_frame();
        }
        if self.snapshot.ready && count > 0 {
            let position = self.gallery_motion.position(now);
            let covers = self
                .displayed_games()
                .into_iter()
                .enumerate()
                .filter_map(|(index, game)| {
                    let offset = exhibition::relative(index, position, count);
                    (offset.abs() < 3.).then(|| crate::gallery_gpu::scene::Cover {
                        id: game.id.clone(),
                        path: game
                            .artwork
                            .get(&ArtworkKind::Cover)
                            .map(|art| art.image_path.clone()),
                        offset,
                    })
                })
                .collect();
            let scene = crate::gallery_gpu::scene::Scene::new(
                self.main_content_width(window),
                f32::from(window.viewport_size().height),
                covers,
            );
            self.gallery_gpu
                .update(cx, |gpu, cx| gpu.request(scene, window, cx));
        }
    }

    pub(super) fn gallery(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let games = self.displayed_games();
        let Some(game) = games
            .iter()
            .find(|game| self.selected.as_ref() == Some(&game.id))
            .filter(|_| self.snapshot.ready)
        else {
            return self.gallery_empty(cx);
        };
        let selected_index = games
            .iter()
            .position(|candidate| candidate.id == game.id)
            .unwrap_or(0);
        let layout = Layout::new(
            self.main_content_width(window),
            f32::from(window.viewport_size().height),
        );
        let position = self.gallery_motion.position(Instant::now());
        let gpu = self.gallery_gpu.read(cx);
        let frame = gpu
            .frame()
            .filter(|frame| {
                frame
                    .scene
                    .covers
                    .iter()
                    .any(|cover| self.selected.as_ref() == Some(&cover.id))
                    && frame
                        .scene
                        .covers
                        .iter()
                        .all(|cover| games.iter().any(|game| game.id == cover.id))
            })
            .cloned();
        let mut placements = games
            .iter()
            .enumerate()
            .filter_map(|(index, game)| {
                let offset = exhibition::relative(index, position, games.len());
                (offset.abs() < 3.).then(|| (index, *game, offset, layout.place(offset)))
            })
            .collect::<Vec<_>>();
        // Farther artwork paints first, so interrupting a transition never puts
        // a receding neighbor in front of the new focal work.
        placements.sort_by(|a, b| b.2.abs().total_cmp(&a.2.abs()));
        let mut stage = div()
            .image_cache(self.full_resolution_images.clone())
            .id("gallery-stage")
            .size_full()
            .relative()
            .overflow_hidden();
        if let Some(frame) = &frame {
            stage = stage.child(crate::gallery_gpu::frame_image(frame));
            placements = frame
                .scene
                .placements()
                .into_iter()
                .filter_map(|(id, placement)| {
                    let index = games.iter().position(|game| game.id == id)?;
                    Some((
                        index,
                        games[index],
                        exhibition::relative(index, position, games.len()),
                        placement,
                    ))
                })
                .collect();
            placements.sort_by(|a, b| b.2.abs().total_cmp(&a.2.abs()));
        } else {
            for (_, game, _, placement) in &placements {
                if let Some(asset) = game.artwork.get(&ArtworkKind::Cover) {
                    let reflection = self.gallery_reflections.update(cx, |reflections, cx| {
                        reflections.image(asset.image_path.clone(), window, cx)
                    });
                    stage = stage.when_some(reflection, |stage, reflection| {
                        stage.child(
                            div()
                                .absolute()
                                .left(px(placement.x - placement.width * 0.08))
                                .top(px(placement.y + placement.height))
                                .w(px(placement.width * 1.16))
                                .h(px(placement.height * 0.32))
                                .opacity(placement.opacity * (1. - placement.shade))
                                .child(exhibition::reflection_image(reflection)),
                        )
                    });
                }
            }
        }
        for (index, game, _, placement) in placements {
            stage = stage.child(self.gallery_cover(game, index, placement, frame.is_some(), cx));
        }
        let title = game.title().to_string();
        let launch_id = game.id.clone();
        stage
            .child(
                div()
                    .absolute()
                    .top(px(layout.title_top))
                    .left(px(32.))
                    .right(px(32.))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .id("gallery-selected-title")
                            .max_w_full()
                            .font_family(self.design().display_font)
                            .text_size(px(layout.title_size))
                            .line_height(relative(1.2))
                            .font_weight(FontWeight::NORMAL)
                            .truncate()
                            .child(title.clone())
                            .tooltip(move |window, cx| {
                                Tooltip::new(title.clone()).build(window, cx)
                            }),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(layout.play_top))
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(with_accessible_label(
                        Button::new("gallery-play")
                            .custom(
                                ButtonCustomVariant::new(cx)
                                    .foreground(cx.theme().foreground)
                                    .hover(cx.theme().foreground.alpha(0.07))
                                    .active(cx.theme().foreground.alpha(0.12)),
                            )
                            .w(px(112.))
                            .h(px(34.))
                            .rounded_full()
                            .border_1()
                            .border_color(cx.theme().foreground.alpha(0.65))
                            .child(
                                div()
                                    .font_family(self.design().display_font)
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::NORMAL)
                                    .child("Play"),
                            )
                            .disabled(
                                !(game.available || game.has_launch_override())
                                    || self.collection_overlay_open(),
                            )
                            .on_click(cx.listener(move |view, _, window, cx| {
                                if !view.collection_overlay_open() {
                                    view.run_library_game_action(
                                        &launch_id,
                                        LibraryGameAction::Launch,
                                        window,
                                        cx,
                                    );
                                }
                            })),
                        format!("Play {}", game.title()),
                    )),
            )
            .when(
                games.len() > 1 && self.screen == StartScreen::Recents,
                |stage| {
                    stage.child(
                        div()
                            .absolute()
                            .top(px(layout.dots_top))
                            .w_full()
                            .flex()
                            .justify_center()
                            .gap(px(2.))
                            .children(games.iter().enumerate().map(|(index, game)| {
                                let id = game.id.clone();
                                with_accessible_label(
                                    Button::new(("gallery-position", index))
                                        .custom(gallery_variant(cx))
                                        .size(px(20.))
                                        .p_0()
                                        .rounded_full()
                                        .child(div().size(px(5.)).rounded_full().bg(
                                            if index == selected_index {
                                                cx.theme().foreground
                                            } else {
                                                cx.theme().muted_foreground.alpha(0.5)
                                            },
                                        ))
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            if !view.collection_overlay_open() {
                                                view.select_recent_game(id.clone(), window, cx);
                                            }
                                        })),
                                    format!("Select {}", game.title()),
                                )
                            })),
                    )
                },
            )
            .when(self.screen == StartScreen::Library, |stage| {
                stage.child(
                    div()
                        .id("gallery-library-position")
                        .absolute()
                        .top(px(layout.dots_top))
                        .w_full()
                        .flex()
                        .justify_center()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .role(Role::Label)
                        .aria_label(format!("Game {} of {}", selected_index + 1, games.len()))
                        .child(format!("{} / {}", selected_index + 1, games.len())),
                )
            })
            .on_scroll_wheel(cx.listener(|view, event: &ScrollWheelEvent, window, cx| {
                if view.collection_overlay_open() {
                    return;
                }
                let delta = event.delta.pixel_delta(px(40.));
                if let Some(forward) =
                    view.gallery_wheel
                        .push(f32::from(delta.x), f32::from(delta.y), Instant::now())
                {
                    view.move_selection(forward, window, cx);
                }
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    fn gallery_cover(
        &self,
        game: &Game,
        index: usize,
        placement: Placement,
        gpu_rendered: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = game.id.clone();
        let group = SharedString::from(format!("gallery-cover-{index}"));
        let art = match game.artwork.get(&ArtworkKind::Cover) {
            Some(asset) => img(asset.image_path.clone())
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div()
                .size_full()
                .bg(cx.theme().secondary)
                .flex()
                .items_center()
                .justify_center()
                .when(!gpu_rendered, |button| {
                    button.child(
                        Icon::new(IconName::GalleryVerticalEnd)
                            .size(px(32.))
                            .text_color(cx.theme().muted_foreground),
                    )
                })
                .into_any_element(),
        };
        let cover = with_accessible_label(
            Button::new(("gallery-cover", index))
                .custom(ButtonCustomVariant::new(cx))
                .size_full()
                .p_0()
                .rounded(px(1.))
                .border_0()
                .shadow_none()
                .group(group.clone())
                .child(
                    div()
                        .size_full()
                        .relative()
                        .overflow_hidden()
                        .rounded(px(1.))
                        .child(art)
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .bg(black().alpha(placement.shade))
                                .group_hover(group, |style| {
                                    style.bg(black().alpha((placement.shade - 0.05).max(0.)))
                                }),
                        )
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .border_1()
                                .border_color(white().alpha(0.055)),
                        ),
                )
                .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                    if !view.collection_overlay_open() {
                        if event.click_count() == 2 {
                            view.run_library_game_action(
                                &id,
                                LibraryGameAction::Launch,
                                window,
                                cx,
                            );
                        } else {
                            view.select_recent_game(id.clone(), window, cx);
                        }
                    }
                })),
            format!("Select {}. Right-click for game options.", game.title()),
        );
        div()
            .absolute()
            .left(px(placement.x))
            .top(px(placement.y))
            .w(px(placement.width))
            .h(px(placement.height))
            .opacity(placement.opacity)
            .flex()
            .child(self.with_library_context_menu(game, cover, cx))
            .into_any_element()
    }

    fn gallery_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .when(
                !self.snapshot.ready && self.snapshot.initialization_error.is_none(),
                |view| view.child(Spinner::new()),
            )
            .child(
                div()
                    .font_family(self.design().display_font)
                    .text_size(px(28.))
                    .child(
                        self.snapshot
                            .initialization_error
                            .clone()
                            .unwrap_or_else(|| {
                                if !self.snapshot.ready {
                                    "Opening your collection…".into()
                                } else {
                                    "Your collection starts here".into()
                                }
                            }),
                    ),
            )
            .when(self.snapshot.ready, |view| {
                view.child(hint(
                    "Refresh installed games, or add a game from the Library menu.",
                ))
                .child(
                    Button::new("gallery-empty-refresh")
                        .custom(gallery_variant(cx))
                        .label("Refresh library")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.refresh(&RefreshLibrary, window, cx)
                        })),
                )
            })
            .into_any_element()
    }
}

fn gallery_variant(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(cx.theme().foreground)
        .hover(cx.theme().foreground.alpha(0.04))
        .active(cx.theme().foreground.alpha(0.08))
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
    fn gallery_browses_all_sources_without_panels_and_preserves_launch_and_navigation_context() {
        let directory = tempfile::tempdir().unwrap();
        let api = Arc::new(TestApi {
            snapshot: Mutex::new(Snapshot {
                ready: true,
                games: Arc::new(
                    (0..22)
                        .map(|index| {
                            let mut game = sample_library_game();
                            game.id = GameId(format!("gallery:{index}"));
                            game.discovered_title = format!("Exhibit {index:02}");
                            game.provider_id = ["steam", "epic", "gog", "xbox"][index % 4].into();
                            game.last_launched = Some(100 - index as u64);
                            game.hidden = index == 20;
                            game.available = index != 21;
                            game
                        })
                        .collect(),
                ),
                activity: Default::default(),
                journal: Default::default(),
                jobs: vec![],
                settings: SettingsSnapshot {
                    appearance: AppearanceSettings {
                        recents_layout: RecentsLayout::Gallery,
                        sidebar_visible: true,
                        library_details: true,
                        theme: "Violet".into(),
                        mode: AppearanceMode::Light,
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
                KeyBinding::new("right", SelectNext, Some("Startpoint")),
                KeyBinding::new("left", SelectPrevious, Some("Startpoint")),
                KeyBinding::new("enter", LaunchSelected, Some("Startpoint")),
                KeyBinding::new("ctrl-f", FocusSearch, Some("Startpoint")),
                KeyBinding::new("escape", DismissPanel, Some("Startpoint")),
            ]);
        });
        let mut window = app.open_window(|window, cx| LauncherView::new(api.clone(), window, cx));
        window.draw();
        window.read(|view, cx| {
            assert!(view.gallery_active());
            assert_eq!(view.recent_games().len(), 5);
            assert_eq!(view.visible_sidebar_progress(), 0.);
            assert!(!view.shows_library_details());
            assert_eq!(cx.theme().background, Hsla::from(rgb(0x0c0a09)));
            assert_eq!(
                appearance::background(cx),
                WindowBackgroundAppearance::Opaque
            );
            assert!(!view.command_supported(&LauncherCommand::Sidebar));
            assert!(
                view.recent_games()
                    .iter()
                    .any(|game| game.provider_id == "xbox")
            );
        });
        let (first_dot, last_dot) = window.update(|_, window, _| {
            let layout = Layout::new(
                f32::from(window.viewport_size().width),
                f32::from(window.viewport_size().height),
            );
            (
                point(
                    window.viewport_size().width / 2. - px(44.),
                    px(layout.dots_top + 10.),
                ),
                point(
                    window.viewport_size().width / 2. + px(44.),
                    px(layout.dots_top + 10.),
                ),
            )
        });
        window.simulate_click(last_dot, MouseButton::Left);
        window.draw();
        window.read(|view, _| {
            assert_eq!(
                view.selected.as_ref().unwrap().0,
                "gallery:4",
                "the fifth fixed dot must always select the fifth recent game"
            )
        });
        window.simulate_click(first_dot, MouseButton::Left);
        window.draw();
        window.read(|view, _| assert_eq!(view.selected.as_ref().unwrap().0, "gallery:0"));
        // The same launch/selection regression runs with a displayed GPU frame
        // and invisible native hit targets, not only the loading fallback.
        window.update(|view, window, cx| {
            let covers = view
                .recent_games()
                .into_iter()
                .enumerate()
                .map(|(index, game)| crate::gallery_gpu::scene::Cover {
                    id: game.id.clone(),
                    path: None,
                    offset: exhibition::relative(index, 0., 5),
                })
                .collect();
            let scene = crate::gallery_gpu::scene::Scene::new(
                view.main_content_width(window),
                f32::from(window.viewport_size().height),
                covers,
            );
            view.gallery_gpu
                .update(cx, |gpu, _| gpu.install_test_frame(scene));
        });
        window.draw();
        let neighbor = window.update(|_, window, _| {
            let layout = Layout::new(
                f32::from(window.viewport_size().width),
                f32::from(window.viewport_size().height),
            );
            let card = layout.place(1.);
            point(px(card.x + card.width / 2.), px(card.y + card.height / 2.))
        });
        window.simulate_click(neighbor, MouseButton::Left);
        window.draw();
        window.read(|view, _| assert_eq!(view.selected.as_ref().unwrap().0, "gallery:1"));
        assert!(
            api.launched.lock().unwrap().is_empty(),
            "a single cover click selects but never launches"
        );
        let play = window.update(|_, window, _| {
            let layout = Layout::new(
                f32::from(window.viewport_size().width),
                f32::from(window.viewport_size().height),
            );
            point(window.viewport_size().width / 2., px(layout.play_top + 17.))
        });
        window.simulate_click(play, MouseButton::Left);
        assert_eq!(api.launched.lock().unwrap().last().unwrap().0, "gallery:1");
        window.simulate_keystroke("right");
        window.simulate_keystroke("enter");
        assert_eq!(api.launched.lock().unwrap().last().unwrap().0, "gallery:2");
        window.update(|view, window, cx| {
            view.select_recent_game(GameId("gallery:4".into()), window, cx);
            view.move_selection(true, window, cx);
            assert_eq!(view.selected.as_ref().unwrap().0, "gallery:0");
            assert!(
                !view.advance_carousel(window, cx),
                "Gallery must never auto-advance"
            );
        });
        window.draw();
        let library_tab = window.update(|_, window, _| {
            let inset = crate::window_chrome::native_left_inset(
                cfg!(target_os = "macos"),
                window.is_fullscreen(),
            );
            point(px(270. + inset), px(22.))
        });
        window.simulate_click(library_tab, MouseButton::Left);
        window.draw();
        window.read(|view, _| {
            assert!(
                view.screen == StartScreen::Library,
                "Gallery must use the shared screen-tab island"
            );
            assert!(!view.shows_library_details());
        });
        let customize = window.update(|_, window, _| {
            point(
                window.viewport_size().width
                    - px(128.
                        + if cfg!(target_os = "windows") {
                            102.
                        } else {
                            0.
                        }),
                px(22.),
            )
        });
        window.simulate_click(customize, MouseButton::Left);
        window.draw();
        window.read(|view, _| {
            assert!(
                view.customization_open,
                "Gallery must use the shared action island"
            )
        });
        let launches = api.launched.lock().unwrap().len();
        window.simulate_click(play, MouseButton::Left);
        assert_eq!(
            api.launched.lock().unwrap().len(),
            launches,
            "clicks through open customization must never launch a game"
        );
        window.simulate_keystroke("escape");
        window.draw();
        window.simulate_keystroke("ctrl-f");
        window.draw();
        window.read(|view, _| assert!(matches!(view.command_mode, Some(CommandMode::Games))));
        window.simulate_keystroke("escape");
        window.update(|view, window, cx| {
            view.run_command(
                LauncherCommand::SelectGame(GameId("gallery:12".into())),
                window,
                cx,
            );
            assert!(view.screen == StartScreen::Library);
            assert_eq!(view.selected.as_ref().unwrap().0, "gallery:12");
            assert!(view.gallery_active());
            assert!(!view.shows_library_details());
            view.open_settings(&OpenSettings, window, cx);
            assert!(view.selected.is_none());
            view.switch_screen(StartScreen::Recents, window, cx);
            for recents_layout in [
                RecentsLayout::Editorial,
                RecentsLayout::Carousel,
                RecentsLayout::Gallery,
            ] {
                view.set_appearance(
                    AppearanceSettings {
                        recents_layout,
                        ..view.appearance.clone()
                    },
                    window,
                    cx,
                );
                assert_eq!(view.appearance.theme, "Violet");
                assert!(view.appearance.sidebar_visible && view.appearance.library_details);
            }
            assert_eq!(view.recent_games().len(), 5);
        });
        window.draw();
        // Empty, one-game and large libraries all use the same presentation and
        // input paths; there is no modulo-zero or duplicated neighbor special UI.
        for count in [0, 1, 1000] {
            window.update(|view, window, cx| {
                view.snapshot.games = Arc::new(
                    (0..count)
                        .map(|index| {
                            let mut game = sample_library_game();
                            game.id = GameId(format!("sized:{index}"));
                            game.discovered_title = format!("Exhibit {index}");
                            game
                        })
                        .collect(),
                );
                view.selected = None;
                view.remembered_selection = None;
                view.gallery_motion = Default::default();
                view.filter(cx);
                view.ensure_selection(window, cx);
                assert_eq!(view.displayed_games().len(), count.min(5));
            });
            window.draw();
            window.simulate_keystroke("right");
            window.draw();
            window.read(|view, _| assert_eq!(view.selected.is_some(), count > 0));
        }
    }
}
