use super::*;
use startpoint_sdk::{ApiError, ApplicationApi, ApplicationPaths, SettingsSnapshot};
use std::sync::Mutex;

#[::core::prelude::v1::test]
fn book_pages_follow_browsing_direction_and_reverse_without_jumping() {
    let now = Instant::now();
    let mut state = scene::State::default();
    let mut first = sample_library_game();
    first.id = GameId("first".into());
    let mut second = first.clone();
    second.id = GameId("second".into());
    state.select(&first, 0, 5, Mode::Book, false, now);
    state.select(&second, 1, 5, Mode::Book, false, now);
    assert_eq!(state.book_direction, 1.);
    let mid = now + Duration::from_millis(140);
    let before = scene::book_tear_front(1280., state.page.value(mid), state.book_direction);
    state.select(&first, 0, 5, Mode::Book, false, mid);
    let after = scene::book_tear_front(1280., state.page.value(mid), state.book_direction);
    assert!((before - after).abs() < 0.001);
    assert_eq!(scene::book_tear_front(800., 0., 1.), 800.);
    assert_eq!(scene::book_tear_front(800., 1., 1.), 0.);
    state.select(
        &second,
        4,
        5,
        Mode::Book,
        true,
        now + Duration::from_secs(2),
    );
    assert_eq!(state.book_direction, -1.);
    assert_eq!(state.page.value(now + Duration::from_secs(2)), 1.);
}

struct Api {
    snapshot: Snapshot,
    launched: Mutex<Vec<GameId>>,
}

#[::core::prelude::v1::test]
fn scene_collection_cache_tracks_snapshot_identity_scope_and_same_size_reordering() {
    let source = Arc::new(
        (0..8)
            .map(|i| {
                let mut game = sample_library_game();
                game.id = GameId(format!("cached:{i}"));
                game.last_launched = Some(i);
                game.available = i != 1;
                game.hidden = i == 2;
                game
            })
            .collect::<Vec<_>>(),
    );
    let mut state = scene::State::default();
    state.refresh_games(&source);
    assert_eq!(state.games().len(), 6);
    assert_eq!(state.games()[0].id.0, "cached:7");
    assert!(state.position(&GameId("cached:1".into())).is_none());
    assert!(state.position(&GameId("cached:2".into())).is_none());
    let id = GameId("cached:0".into());
    let game = state.games()[state.position(&id).unwrap()].clone();
    let now = Instant::now();
    state.select(&game, 5, 6, Mode::Reel, true, now);
    state.refresh_games(&source);
    assert_eq!(state.position(&id), Some(5));
    let mut refreshed = (*source).clone();
    refreshed[0].last_launched = Some(100);
    state.refresh_games(&Arc::new(refreshed));
    assert_eq!(state.position(&id), Some(0));
    state.select(&game, 0, 6, Mode::Reel, false, now);
    assert_eq!(state.index.value(now), 0.);
    assert_eq!(state.selected, Some(id));
}
impl ApplicationApi for Api {
    fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }
    fn subscribe(&self) -> async_channel::Receiver<AppEvent> {
        async_channel::unbounded().1
    }
    fn cancel(&self, _: JobId) {}
    fn submit(&self, command: Command) -> Result<JobId, ApiError> {
        if let Command::LaunchGame { game_id } = command {
            self.launched.lock().unwrap().push(game_id);
        }
        Ok(1)
    }
}
#[::core::prelude::v1::test]
fn three_scenes_keep_identity_reversible_previews_and_single_launch_dispatch() {
    let directory = tempfile::tempdir().unwrap();
    let api = Arc::new(Api {
        snapshot: Snapshot {
            ready: true,
            games: Arc::new(
                (0..24)
                    .map(|i| {
                        let mut game = sample_library_game();
                        game.id = GameId(format!("scene:{i}"));
                        game.discovered_title = format!("World {i:02}");
                        game.last_launched = Some(100 - i as u64);
                        game.available = true;
                        game.artwork.clear();
                        game.provider_id = ["steam", "epic", "gog", "xbox"][i % 4].into();
                        game
                    })
                    .collect(),
            ),
            activity: Default::default(),
            journal: Default::default(),
            jobs: vec![],
            settings: SettingsSnapshot {
                appearance: AppearanceSettings {
                    recents_layout: RecentsLayout::InfiniteWall,
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
        },
        launched: Default::default(),
    });
    let mut app = TestApp::new();
    app.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(theme::Catalog::default());
    });
    let mut window = app.open_window(|window, cx| LauncherView::new(api.clone(), window, cx));
    window.update(|_, window, _| window.activate_window());
    window.draw();
    let (origin, scroll) = window.update(|view, window, _| {
        assert!(view.art_scene_active());
        assert_eq!(view.recent_games().len(), 24);
        assert_eq!(view.visible_sidebar_progress(), 0.);
        let layout = WallLayout::new(
            view.main_content_width(window),
            f32::from(window.viewport_size().height),
        );
        (
            layout.rect(0, view.art_scenes.scroll.value(Instant::now())),
            view.art_scenes.scroll.target,
        )
    });
    window.simulate_click(
        point(px(origin.x + origin.w / 2.), px(origin.y + origin.h / 2.)),
        MouseButton::Left,
    );
    window.draw();
    window.update(|view, _, _| {
        let focus = view
            .art_scenes
            .focus
            .as_mut()
            .expect("cover click expands rather than launches");
        assert_eq!(focus.game.id.0, "scene:0");
        focus.amount.snap(1.);
    });
    window.update(|view, _, _| {
        let mut games = (*view.snapshot.games).clone();
        games[0].metadata.description = Some("Updated in the native editor".into());
        view.snapshot.games = Arc::new(games);
    });
    window.draw();
    window.read(|view, _| {
        assert_eq!(
            view.art_scenes
                .focus
                .as_ref()
                .unwrap()
                .game
                .metadata
                .description
                .as_deref(),
            Some("Updated in the native editor")
        )
    });
    assert!(api.launched.lock().unwrap().is_empty());
    window.update(|view, window, cx| {
        view.dismiss(&DismissPanel, window, cx);
        assert_eq!(view.art_scenes.focus.as_ref().unwrap().origin, origin);
        view.art_scenes.focus.as_mut().unwrap().amount.snap(0.);
    });
    window.draw();
    window.read(|view, _| {
        assert!(view.art_scenes.focus.is_none());
        assert_eq!(view.art_scenes.scroll.target, scroll);
    });
    window.simulate_mouse_move(point(
        px(origin.x + origin.w / 2.),
        px(origin.y + origin.h / 2.),
    ));
    window.update(|view, _, _| {
        view.art_scenes
            .hover(Some(GameId("scene:0".into())), true, Instant::now());
        view.art_scenes.hover.snap(1.);
    });
    window.draw();
    window.simulate_click(
        point(px(origin.x + origin.w / 2.), px(origin.y + origin.h - 20.)),
        MouseButton::Left,
    );
    assert_eq!(
        api.launched.lock().unwrap().len(),
        1,
        "small Play launches once, without bubbling into preview"
    );
    window.read(|view, _| assert!(view.art_scenes.focus.is_none()));
    window.update(|view, window, cx| {
        view.set_appearance(
            AppearanceSettings {
                recents_layout: RecentsLayout::FilmReel,
                ..view.appearance.clone()
            },
            window,
            cx,
        )
    });
    window.draw();
    window.update(|view, window, cx| {
        view.launch_art_scene(window, cx);
        view.art_scenes.focus.as_mut().unwrap().amount.snap(1.);
    });
    window.draw();
    window.draw();
    assert_eq!(
        api.launched.lock().unwrap().len(),
        2,
        "reel expansion dispatches exactly once"
    );
    window.update(|view, window, cx| {
        view.art_scenes.focus = None;
        view.launch_art_scene(window, cx);
        view.art_scenes.dismiss(true, Instant::now());
    });
    window.draw();
    assert_eq!(
        api.launched.lock().unwrap().len(),
        2,
        "Escape cancels a pending launch"
    );
    window.update(|view, window, cx| {
        view.set_appearance(
            AppearanceSettings {
                recents_layout: RecentsLayout::ArtBook,
                ..view.appearance.clone()
            },
            window,
            cx,
        )
    });
    window.draw();
    window.update(|view, _, _| view.art_scenes.overview.snap(1.));
    window.draw();
    let third = window.update(|view, window, _| {
        WallLayout::new(
            view.main_content_width(window),
            f32::from(window.viewport_size().height),
        )
        .rect(2, 0.)
    });
    window.simulate_click(
        point(px(third.x + third.w / 2.), px(third.y + third.h / 2.)),
        MouseButton::Left,
    );
    window.draw();
    window.read(|view, _| {
        assert_eq!(view.selected.as_ref().unwrap().0, "scene:2");
        assert_eq!(view.art_scenes.overview.target, 0.);
        assert_eq!(view.design().shell, crate::design::Shell::ToolbarIslands);
    });
    window.read(|_, cx| assert!(scene::material::chrome(cx).is_some()));
    window.update(|view, window, cx| view.open_settings(&OpenSettings, window, cx));
    window.draw();
    window.read(|_, cx| {
        assert!(
            scene::material::chrome(cx).is_none(),
            "artwork palette must not leak into Settings"
        )
    });
    window.update(|view, window, cx| view.dismiss(&DismissPanel, window, cx));
    window.draw();
    window.read(|_, cx| assert!(scene::material::chrome(cx).is_some()));
    assert_eq!(
        api.launched.lock().unwrap().len(),
        2,
        "overview selects a page without launching"
    );
    for layout in [RecentsLayout::Diagonal, RecentsLayout::Mosaic] {
        window.update(|view, window, cx| {
            view.set_appearance(
                AppearanceSettings {
                    recents_layout: layout,
                    ..view.appearance.clone()
                },
                window,
                cx,
            )
        });
        window.draw();
        let (position, expected) = window.update(|view, window, _| {
            let width = view.main_content_width(window);
            let height = f32::from(window.viewport_size().height);
            let games = view.recent_games();
            let current = games
                .iter()
                .position(|game| view.selected.as_ref() == Some(&game.id))
                .unwrap();
            if layout == RecentsLayout::Diagonal {
                (
                    point(px(width * 0.1), px(height * 0.55)),
                    games[(current + games.len() - 1) % games.len()].id.clone(),
                )
            } else {
                (
                    point(px(width * 0.8), px(height * 0.15)),
                    games[(current + 1) % games.len()].id.clone(),
                )
            }
        });
        let launches = api.launched.lock().unwrap().len();
        window.simulate_click(position, MouseButton::Left);
        window.draw();
        window.read(|view, _| assert_eq!(view.selected.as_ref(), Some(&expected)));
        assert_eq!(api.launched.lock().unwrap().len(), launches);
        window.simulate_event(MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Default::default(),
            click_count: 2,
            first_mouse: false,
        });
        window.simulate_event(MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Default::default(),
            click_count: 2,
        });
        window.draw();
        let launched = api.launched.lock().unwrap();
        assert_eq!(launched.len(), launches + 1);
        assert_eq!(
            launched.last(),
            Some(&expected),
            "rearranging artwork must not change the double-click target"
        );
    }
}
