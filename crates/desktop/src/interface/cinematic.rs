use super::*;
use crate::cinematic::{self as cinema, Layout, Placement};
use gpui_component::button::ButtonCustomVariant;

impl LauncherView {
    pub(super) fn cinematic_active(&self) -> bool {
        self.design().recents == crate::design::RecentPresentation::Cinematic
            && self.screen == StartScreen::Recents
            && !self.settings_open
    }

    pub(super) fn prepare_cinematic(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.cinematic_active() || self.render_power.paused() || !self.snapshot.ready {
            return;
        }
        let games = self.recent_games();
        let Some(index) = games
            .iter()
            .position(|game| self.selected.as_ref() == Some(&game.id))
        else {
            return;
        };
        let count = games.len();
        let key = cinema::artwork_key(games[index]);
        let neighbors: Vec<_> = [(index + 1) % count, (index + count - 1) % count]
            .into_iter()
            .filter(|neighbor| *neighbor != index)
            .filter_map(|neighbor| cinema::artwork_key(games[neighbor]))
            .collect();
        let now = Instant::now();
        let reduced = appearance::reduced_motion();
        self.cinematic_motion.update(index, count, reduced, now);
        let mut pinned = self.cinematic_scene.keys();
        if let Some(key) = &key {
            pinned.push(key.clone());
        }
        let artwork = match &key {
            Some(key) => self.cinematic_artwork.update(cx, |cache, cx| {
                cache.set_capacity(6);
                cache.load(key, &pinned, window, cx)
            }),
            None => Some(cinema::Artwork::default()),
        };
        // Keep the previous scenery until the destination has decoded. Loading
        // completion notifies GPUI; no frame loop or blank flash while waiting.
        if let Some(artwork) = artwork {
            self.cinematic_scene.update(key, artwork, reduced, now);
            for key in neighbors {
                self.cinematic_artwork
                    .update(cx, |cache, cx| cache.load(&key, &pinned, window, cx));
            }
        }
        if self.cinematic_scene.animating(now) || self.cinematic_motion.animating(now) {
            window.request_animation_frame();
        }
    }

    fn cinematic_accent(&self) -> Hsla {
        let [r, g, b] = self.cinematic_scene.accent(Instant::now());
        Rgba { r, g, b, a: 1. }.into()
    }

    pub(super) fn cinematic(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let games = self.recent_games();
        let Some(game) = games
            .iter()
            .find(|game| self.selected.as_ref() == Some(&game.id))
            .filter(|_| self.snapshot.ready)
        else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .bg(rgb(0x0b0d10))
                .text_color(rgb(0xeee6d6))
                .when(!self.snapshot.ready, |stage| stage.child(Spinner::new()))
                .child(if self.snapshot.initialization_error.is_some() {
                    "Your library could not be loaded. Use Refresh in the top bar to retry."
                } else if self.snapshot.ready {
                    "Your next world is waiting."
                } else {
                    "Bringing your libraries together…"
                })
                .when(self.snapshot.ready, |stage| {
                    stage.child(
                        div()
                            .text_sm()
                            .opacity(0.6)
                            .child("Refresh your libraries or add a game from the top bar."),
                    )
                })
                .into_any_element();
        };
        let now = Instant::now();
        let layout = Layout::new(
            self.main_content_width(window),
            f32::from(window.viewport_size().height),
        );
        let accent = self.cinematic_accent();
        let foreground = rgb(0xf4eddd);
        let mut stage = div()
            .id("cinematic-stage")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(rgb(0x0b0d10))
            .text_color(foreground);
        // Convert normalized mixture weights to source-over opacities. This
        // preserves the displayed blend on interrupted transitions without
        // darkening the art or allocating a flattened screenshot each frame.
        let mut accumulated = 0.;
        for layer in self.cinematic_scene.layers(now) {
            accumulated += layer.weight;
            let opacity = (layer.weight / accumulated).clamp(0., 1.);
            let background = match layer.artwork.image {
                Some(image) => img(image)
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element(),
                None => div().size_full().bg(rgb(0x101418)).into_any_element(),
            };
            stage = stage.child(
                div()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .opacity(opacity)
                    .child(background),
            );
        }
        stage = stage
            .child(div().absolute().inset_0().size_full().bg(linear_gradient(
                180.,
                linear_color_stop(black().alpha(0.), 0.43),
                linear_color_stop(black().alpha(0.88), 1.),
            )))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(110.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(black().alpha(0.22), 0.),
                        linear_color_stop(black().alpha(0.), 1.),
                    )),
            );
        let position = self.cinematic_motion.position(now);
        let mut covers = games
            .iter()
            .enumerate()
            .filter_map(|(index, game)| {
                let offset = crate::gallery::relative(index, position, games.len());
                (offset.abs() < 3.).then(|| (index, *game, offset, layout.place(offset)))
            })
            .collect::<Vec<_>>();
        covers.sort_by(|a, b| b.2.abs().total_cmp(&a.2.abs()));
        for (index, game, _, placement) in covers {
            stage = stage.child(self.cinematic_cover(game, index, placement, accent, cx));
        }
        let title = game.title().to_string();
        let launch_id = game.id.clone();
        let action = if game.last_launched.is_some() {
            "Continue"
        } else {
            "Play"
        };
        stage
            .child(
                div()
                    .absolute()
                    .left(px(layout.left))
                    .right(px(layout.left))
                    .top(px(layout.title_top))
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .id("cinematic-selected-title")
                            .max_w(px(layout.width * 0.82))
                            .font_family(self.design().display_font)
                            .font_weight(FontWeight::NORMAL)
                            .text_size(px(layout.title_size))
                            .line_height(relative(1.25))
                            .truncate()
                            .text_color(foreground)
                            .role(Role::Label)
                            .aria_label(title.clone())
                            .child(crate::artwork_motion::tracked(&title.to_uppercase()))
                            .tooltip(move |window, cx| {
                                Tooltip::new(title.clone()).build(window, cx)
                            }),
                    )
                    .child(with_accessible_label(
                        Button::new("cinematic-play")
                            .custom(cinematic_variant(accent, cx))
                            .h(px(30.))
                            .px_0()
                            .border_0()
                            .rounded(px(3.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        Icon::default()
                                            .path("icons/cinematic-play.svg")
                                            .size(px(15.)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(crate::artwork_motion::tracked(
                                                &action.to_uppercase(),
                                            )),
                                    ),
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
                        format!("{action} {}", game.title()),
                    )),
            )
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

    fn cinematic_cover(
        &self,
        game: &Game,
        index: usize,
        placement: Placement,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = game.id.clone();
        let group = SharedString::from(format!("cinematic-cover-{index}"));
        let art = match game.artwork.get(&ArtworkKind::Cover) {
            Some(asset) => img(if asset.thumbnail_path.as_os_str().is_empty() {
                asset.image_path.clone()
            } else {
                asset.thumbnail_path.clone()
            })
            .size_full()
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
            None => div()
                .size_full()
                .bg(rgb(0x20282b))
                .flex()
                .items_end()
                .p_2()
                .text_sm()
                .text_color(rgb(0xeee6d6))
                .child(game.title().to_string())
                .into_any_element(),
        };
        let cover = with_accessible_label(
            Button::new(("cinematic-cover", index))
                .custom(ButtonCustomVariant::new(cx))
                .size_full()
                .p_0()
                .rounded(px(4.))
                .border_0()
                .shadow_none()
                .group(group.clone())
                .child(
                    div()
                        .relative()
                        .size_full()
                        .rounded(px(4.))
                        .overflow_hidden()
                        .child(art)
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .size_full()
                                .bg(black().alpha((1. - placement.focus) * 0.20))
                                .group_hover(group, |style| style.bg(black().alpha(0.02))),
                        ),
                )
                .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                    if view.collection_overlay_open() {
                        return;
                    }
                    if event.click_count() == 2 {
                        view.run_library_game_action(&id, LibraryGameAction::Launch, window, cx);
                    } else {
                        view.select_recent_game(id.clone(), window, cx);
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
            .flex()
            .opacity(placement.opacity)
            .rounded(px(4.))
            .shadow(vec![BoxShadow {
                color: accent.alpha(0.40 * placement.focus),
                offset: point(px(0.), px(2.)),
                blur_radius: px(20.),
                spread_radius: px(2.),
                inset: false,
            }])
            .child(self.with_library_context_menu(game, cover, cx))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(accent.alpha(placement.focus * 0.95)),
            )
            .into_any_element()
    }
}

fn cinematic_variant(accent: Hsla, cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(accent)
        .hover(white().alpha(0.07))
        .active(white().alpha(0.12))
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
                Command::LaunchGame { game_id } => self.launched.lock().unwrap().push(game_id),
                Command::SetAppearance { settings } => {
                    self.snapshot.lock().unwrap().settings.appearance = settings
                }
                _ => {}
            }
            Ok(1)
        }
    }

    #[::core::prelude::v1::test]
    fn cinematic_unifies_sources_and_keeps_pointer_search_launch_and_settings_native() {
        let directory = tempfile::tempdir().unwrap();
        let hero = directory.path().join("landscape.png");
        image::RgbaImage::from_pixel(128, 72, image::Rgba([180, 55, 20, 255]))
            .save(&hero)
            .unwrap();
        let api = Arc::new(TestApi {
            snapshot: Mutex::new(Snapshot {
                ready: true,
                games: Arc::new(
                    (0..32)
                        .map(|index| {
                            let mut game = sample_library_game();
                            game.id = GameId(format!("cinema:{index}"));
                            game.discovered_title = format!("World {index:02}");
                            game.provider_id =
                                ["steam", "epic", "gog", "xbox", "manual"][index % 5].into();
                            game.last_launched = Some(100 - index as u64);
                            game.artwork.clear();
                            if index == 0 {
                                game.artwork.insert(
                                    ArtworkKind::Background,
                                    ArtworkAsset {
                                        id: "cinema-test-hero".into(),
                                        kind: ArtworkKind::Background,
                                        image_path: hero.clone(),
                                        thumbnail_path: hero.clone(),
                                        provider_id: "test".into(),
                                        source_url: None,
                                        author: "test".into(),
                                    },
                                );
                            }
                            game.available = index != 31;
                            game.hidden = index == 30;
                            game
                        })
                        .collect(),
                ),
                activity: Default::default(),
                journal: Default::default(),
                jobs: vec![],
                settings: SettingsSnapshot {
                    appearance: AppearanceSettings {
                        recents_layout: RecentsLayout::Cinematic,
                        sidebar_visible: true,
                        library_details: true,
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
        window.update(|_, window, _| window.activate_window());
        window.draw();
        window.update(|_, _, _| {});
        window.draw();
        window.read(|view, _| {
            assert!(view.cinematic_active());
            assert!(
                view.cinematic_scene
                    .layers(Instant::now())
                    .iter()
                    .any(|layer| layer.artwork.image.is_some()),
                "cold artwork must become available without navigating to another game"
            );
            assert_eq!(view.recent_games().len(), 30);
            assert_eq!(view.visible_sidebar_progress(), 0.);
            assert!(!view.shows_library_details());
            assert!(
                view.recent_games()
                    .iter()
                    .any(|game| game.provider_id == "xbox")
            );
            assert!(
                view.recent_games()
                    .iter()
                    .any(|game| game.provider_id == "manual")
            );
        });
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
        window.read(|view, _| assert_eq!(view.selected.as_ref().unwrap().0, "cinema:1"));
        assert!(api.launched.lock().unwrap().is_empty());
        let play = window.update(|_, window, _| {
            let layout = Layout::new(
                f32::from(window.viewport_size().width),
                f32::from(window.viewport_size().height),
            );
            point(
                px(layout.left + 40.),
                px(layout.title_top + layout.title_size * 1.25 + 27.),
            )
        });
        window.simulate_click(play, MouseButton::Left);
        assert_eq!(api.launched.lock().unwrap().last().unwrap().0, "cinema:1");
        window.simulate_keystroke("right");
        window.simulate_keystroke("enter");
        assert_eq!(api.launched.lock().unwrap().last().unwrap().0, "cinema:2");
        window.update(|view, window, cx| {
            view.select_recent_game(GameId("cinema:29".into()), window, cx);
            view.move_selection(true, window, cx);
            assert_eq!(view.selected.as_ref().unwrap().0, "cinema:0");
            assert!(!view.advance_carousel(window, cx));
        });
        window.simulate_keystroke("ctrl-f");
        window.draw();
        window.read(|view, _| assert!(matches!(view.command_mode, Some(CommandMode::Games))));
        let launches = api.launched.lock().unwrap().len();
        window.simulate_click(play, MouseButton::Left);
        assert_eq!(
            api.launched.lock().unwrap().len(),
            launches,
            "search must block click-through launches"
        );
        window.update(|view, window, cx| {
            view.run_command(
                LauncherCommand::SelectGame(GameId("cinema:12".into())),
                window,
                cx,
            );
            assert!(
                view.cinematic_active(),
                "installed search results return to the cinematic experience"
            );
            assert_eq!(view.selected.as_ref().unwrap().0, "cinema:12");
            view.open_settings(&OpenSettings, window, cx);
            assert!(!view.cinematic_active());
            assert!(view.selected.is_none());
            view.switch_screen(StartScreen::Recents, window, cx);
        });
        window.draw();
        let inset = window.update(|_, window, _| {
            crate::window_chrome::native_left_inset(
                cfg!(target_os = "macos"),
                window.is_fullscreen(),
            )
        });
        window.simulate_click(point(px(128. + inset), px(22.)), MouseButton::Left);
        window.draw();
        window.read(|view, _| {
            assert!(
                view.settings_open,
                "Cinematic must use Settings in the same left toolbar island as every other design"
            )
        });
        window.update(|view, window, cx| view.switch_screen(StartScreen::Recents, window, cx));
        window.draw();
        let library = point(px(270. + inset), px(22.));
        window.simulate_click(library, MouseButton::Left);
        window.draw();
        window.read(|view, _| {
            assert!(view.screen == StartScreen::Library);
            assert!(!view.cinematic_active());
            assert!(!view.shows_library_details());
        });
        window.update(|view, window, cx| {
            for recents_layout in [
                RecentsLayout::Carousel,
                RecentsLayout::Editorial,
                RecentsLayout::Gallery,
                RecentsLayout::Cinematic,
            ] {
                view.set_appearance(
                    AppearanceSettings {
                        recents_layout,
                        ..view.appearance.clone()
                    },
                    window,
                    cx,
                );
                assert_eq!(view.appearance.recents_layout, recents_layout);
            }
        });
    }
}
