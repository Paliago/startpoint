use super::*;
use crate::art_scenes::{self as scene, Mode, Rect, WallLayout};
use gpui_component::button::ButtonCustomVariant;
mod compositions;
#[cfg(test)]
mod tests;

impl LauncherView {
    pub(super) fn art_scene_mode(&self) -> Option<Mode> {
        match self.design().recents {
            crate::design::RecentPresentation::InfiniteWall => Some(Mode::Wall),
            crate::design::RecentPresentation::FilmReel => Some(Mode::Reel),
            crate::design::RecentPresentation::ArtBook => Some(Mode::Book),
            crate::design::RecentPresentation::Diagonal => Some(Mode::Diagonal),
            crate::design::RecentPresentation::Mosaic => Some(Mode::Mosaic),
            _ => None,
        }
    }
    pub(super) fn art_scene_active(&self) -> bool {
        self.art_scene_mode().is_some()
            && self.screen == StartScreen::Recents
            && !self.settings_open
    }
    pub(super) fn art_scene_layer_open(&self) -> bool {
        self.art_scene_active()
            && (self.art_scenes.focus.is_some() || self.art_scenes.overview.target > 0.)
    }
    pub(super) fn tick_art_scenes(&mut self) -> bool {
        let running = self.art_scene_active()
            && matches!(self.art_scene_mode(), Some(Mode::Reel | Mode::Book))
            && self.snapshot.ready
            && !self.art_scenes.games().is_empty()
            && !self.render_power.paused()
            && !appearance::reduced_motion()
            && !self.collection_overlay_open()
            && self.art_scenes.overview.target == 0.
            && self.art_scenes.focus.is_none();
        self.art_scenes.drift.update(running);
        running
    }
    fn scene_key(game: &Game, mode: Mode) -> Option<crate::cinematic::ArtworkKey> {
        use crate::cinematic::ArtworkKey;
        match mode {
            Mode::Book => {
                let background = game
                    .artwork
                    .get(&ArtworkKind::Background)
                    .map(|a| a.image_path.clone());
                let cover = game
                    .artwork
                    .get(&ArtworkKind::Cover)
                    .map(|a| a.image_path.clone());
                (background.is_some() || cover.is_some())
                    .then_some(ArtworkKey::Book { background, cover })
            }
            Mode::Wall => game
                .artwork
                .get(&ArtworkKind::Cover)
                .map(|a| ArtworkKey::Portrait(a.image_path.clone())),
            Mode::Reel | Mode::Diagonal => crate::cinematic::artwork_key(game),
            Mode::Mosaic => game
                .artwork
                .get(&ArtworkKind::Background)
                .map(|art| ArtworkKey::Landscape(art.image_path.clone()))
                .or_else(|| {
                    game.artwork
                        .get(&ArtworkKind::Cover)
                        .map(|art| ArtworkKey::Portrait(art.image_path.clone()))
                }),
        }
    }
    fn scene_art(
        &self,
        game: &Game,
        mode: Mode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<crate::cinematic::Artwork> {
        let Some(key) = Self::scene_key(game, mode) else {
            return Some(Default::default());
        };
        let mut pinned = self.art_scenes.ambient.keys();
        for game in self.art_scenes.current.iter().chain(
            self.art_scenes
                .previous
                .iter()
                .filter(|_| self.art_scenes.page.active(Instant::now())),
        ) {
            if let Some(key) = Self::scene_key(game, mode) {
                pinned.push(key);
            }
        }
        if let Some(focus) = &self.art_scenes.focus
            && let Some(key) = Self::scene_key(&focus.game, mode)
        {
            pinned.push(key);
        }
        self.scene_artwork.update(cx, |cache, cx| {
            cache.set_capacity(6);
            cache.load(&key, &pinned, window, cx)
        })
    }
    pub(super) fn prepare_art_scenes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.art_scene_active() || self.render_power.paused() {
            return;
        }
        let mode = self.art_scene_mode().unwrap();
        self.tick_art_scenes();
        let now = Instant::now();
        let reduced = appearance::reduced_motion();
        self.art_scenes.refresh_games(&self.snapshot.games);
        let games = self.art_scenes.games();
        let Some(index) = self
            .selected
            .as_ref()
            .and_then(|id| self.art_scenes.position(id))
        else {
            self.art_scenes.focus = None;
            return;
        };
        let count = games.len();
        let game = games[index].clone();
        // The native artwork/metadata editor can update an open detail view.
        // Keep its game identity, but do not retain a stale cloned hero path.
        if let Some(focus) = &mut self.art_scenes.focus
            && focus.game.id == game.id
        {
            if Self::scene_key(&focus.game, Mode::Reel) != Self::scene_key(&game, Mode::Reel) {
                focus.hero.snap(0.);
            }
            focus.game = game.clone();
        }
        let changed = self.art_scenes.selected.as_ref() != Some(&game.id);
        // Hold the displayed page/reel until a cold destination is decoded.
        // The selection itself remains the real game ID, but do not wipe the
        // previous image into an empty paper/background while waiting on IO.
        let ready = match mode {
            Mode::Wall | Mode::Mosaic => true,
            Mode::Diagonal => self.diagonal_ready(index, window, cx),
            _ => self.scene_art(&game, mode, window, cx).is_some(),
        };
        if ready || self.art_scenes.selected.is_none() {
            self.art_scenes
                .select(&game, index, count, mode, reduced, now);
        }
        let layout = WallLayout::new(
            self.main_content_width(window),
            f32::from(window.viewport_size().height),
        );
        let viewport = [
            self.main_content_width(window),
            f32::from(window.viewport_size().height),
        ];
        let resized = self.art_scenes.viewport != viewport;
        self.art_scenes.viewport = viewport;
        let max = layout.max_scroll(count);
        if self.art_scenes.scroll.target > max {
            self.art_scenes
                .scroll
                .set(max, Duration::from_millis(160), reduced, now);
        }
        if (mode == Mode::Wall || self.art_scenes.overview.target > 0.)
            && changed
            && self.art_scenes.focus.is_none()
        {
            let rect = layout.rect(index, self.art_scenes.scroll.target);
            let target = if rect.y < 54. {
                self.art_scenes.scroll.target + rect.y - 54.
            } else if rect.y + rect.h > layout.height - 10. {
                self.art_scenes.scroll.target + rect.y + rect.h - layout.height + 10.
            } else {
                self.art_scenes.scroll.target
            };
            self.art_scenes.scroll.set(
                target.clamp(0., max),
                Duration::from_millis(180),
                reduced,
                now,
            );
        }
        if mode == Mode::Reel {
            if let Some(art) = self.scene_art(&game, mode, window, cx) {
                self.art_scenes.ambient.set_duration(Duration::from_millis(
                    if mode == Mode::Book { 650 } else { 800 },
                ));
                self.art_scenes
                    .ambient
                    .update(Self::scene_key(&game, mode), art, reduced, now);
            }
            let scene = crate::gallery_gpu::scene::Scene::reel(
                viewport[0],
                viewport[1],
                if reduced {
                    0.
                } else {
                    self.art_scenes.drift.seconds()
                },
                self.art_scenes.ambient.accent(now),
            );
            self.gallery_gpu
                .update(cx, |gpu, cx| gpu.request(scene, window, cx));
        }
        if mode == Mode::Wall
            && let Some(game) = self.art_scenes.focus.as_ref().map(|f| f.game.clone())
            && self
                .scene_art(&game, Mode::Reel, window, cx)
                .is_some_and(|art| art.image.is_some())
            && let Some(focus) = &mut self.art_scenes.focus
            && focus.hero.target == 0.
        {
            focus.hero.set(1., Duration::from_millis(240), reduced, now);
        }
        let mut launch = None;
        let mut remove = false;
        let focus_valid = self
            .art_scenes
            .focus
            .as_ref()
            .is_some_and(|focus| self.art_scenes.position(&focus.game.id).is_some());
        if let Some(focus) = &mut self.art_scenes.focus {
            if focus_valid {
                let p = focus.amount.value(now);
                if focus.amount.target == 0. && p < 0.0001 && !focus.amount.active(now) {
                    remove = true;
                }
                if focus.launch
                    && !focus.dispatched
                    && focus.amount.target == 1.
                    && !focus.amount.active(now)
                    && p >= 0.999
                {
                    focus.dispatched = true;
                    launch = Some(focus.game.id.clone());
                }
            } else {
                remove = true;
            }
        }
        if mode == Mode::Wall
            && resized
            && let Some(id) = self.art_scenes.focus.as_ref().map(|f| f.game.id.clone())
            && let Some(index) = self.art_scenes.position(&id)
        {
            let rect = layout.rect(index, self.art_scenes.scroll.value(now));
            self.art_scenes.focus.as_mut().unwrap().origin = rect;
        }
        if remove {
            self.art_scenes.focus = None;
            self.art_scenes.hover(None, reduced, now);
        }
        if let Some(game_id) = launch
            && !self.collection_overlay_open()
        {
            self.submit(Command::LaunchGame { game_id }, cx);
        }
        if self.art_scenes.active(now) {
            window.request_animation_frame();
        }
    }
    pub(super) fn launch_art_scene(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.art_scenes.overview.target > 0. {
            self.art_scenes.overview.set(
                0.,
                Duration::from_millis(400),
                appearance::reduced_motion(),
                Instant::now(),
            );
            cx.notify();
            return;
        }
        if self.collection_overlay_open() || self.art_scenes.overview.target > 0. {
            return;
        }
        let Some(game) = self.selected_game() else {
            return;
        };
        if !(game.available || game.has_launch_override()) {
            return;
        }
        let mode = self.art_scene_mode().unwrap();
        let now = Instant::now();
        if matches!(
            mode,
            Mode::Wall | Mode::Book | Mode::Diagonal | Mode::Mosaic
        ) {
            // Wall's small Play and Art Book's quiet action launch directly;
            // only Film Reel specifies a launch expansion.
            self.submit(
                Command::LaunchGame {
                    game_id: game.id.clone(),
                },
                cx,
            );
            return;
        }
        if self
            .art_scenes
            .focus
            .as_ref()
            .is_some_and(|focus| focus.launch || focus.dispatched)
        {
            return;
        }
        let width = self.main_content_width(window);
        let height = f32::from(window.viewport_size().height);
        let games = self.art_scenes.games();
        let index = self.art_scenes.position(&game.id).unwrap_or(0);
        let offset = crate::gallery::relative(index, self.art_scenes.index.value(now), games.len());
        let rect = scene::reel_rect(width, height, offset);
        self.art_scenes
            .open(game, rect, true, appearance::reduced_motion(), now);
        cx.notify();
    }
    pub(super) fn art_scene(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mode = self.art_scene_mode().unwrap();
        let games = self.art_scenes.games();
        if games.is_empty() || !self.snapshot.ready {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .child(if self.snapshot.ready {
                    "Your installed games will appear here."
                } else {
                    "Bringing your libraries together…"
                })
                .child(
                    div()
                        .text_sm()
                        .opacity(0.6)
                        .child("Use Refresh or Add Game in the shared top bar."),
                )
                .into_any_element();
        }
        let mut stage = match mode {
            Mode::Wall => self.wall_scene(false, window, cx),
            Mode::Reel => self.reel_scene(window, cx),
            Mode::Book => self.book_scene(window, cx),
            Mode::Diagonal => self.diagonal_scene(window, cx),
            Mode::Mosaic => self.mosaic_scene(window, cx),
        };
        if let Some(focus) = &self.art_scenes.focus {
            stage = div()
                .size_full()
                .relative()
                .child(stage)
                .child(self.expanded_scene(focus, window, cx))
                .into_any_element();
        }
        div()
            .id("art-scene-root")
            .size_full()
            .relative()
            .overflow_hidden()
            .child(stage)
            .on_scroll_wheel(cx.listener(|view, event: &ScrollWheelEvent, window, cx| {
                if view.collection_overlay_open() || view.art_scenes.focus.is_some() {
                    return;
                }
                let delta = event.delta.pixel_delta(px(40.));
                let now = Instant::now();
                if view.art_scene_mode() == Some(Mode::Wall) || view.art_scenes.overview.target > 0.
                {
                    view.art_scenes
                        .hover(None, appearance::reduced_motion(), now);
                    let layout = WallLayout::new(
                        view.main_content_width(window),
                        f32::from(window.viewport_size().height),
                    );
                    let amount = if delta.y.abs() > delta.x.abs() {
                        f32::from(delta.y)
                    } else {
                        f32::from(delta.x)
                    };
                    let target = (view.art_scenes.scroll.target - amount)
                        .clamp(0., layout.max_scroll(view.art_scenes.games().len()));
                    view.art_scenes.scroll.set(
                        target,
                        Duration::from_millis(160),
                        appearance::reduced_motion(),
                        now,
                    );
                    cx.notify();
                } else if let Some(forward) =
                    view.gallery_wheel
                        .push(f32::from(delta.x), f32::from(delta.y), now)
                {
                    if view.art_scene_mode() == Some(Mode::Mosaic) {
                        view.move_mosaic_page(forward, window, cx);
                    } else {
                        view.move_selection(forward, window, cx);
                    }
                }
                cx.stop_propagation();
            }))
            .into_any_element()
    }
    fn wall_scene(
        &self,
        overview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = WallLayout::new(
            self.main_content_width(window),
            f32::from(window.viewport_size().height),
        );
        let now = Instant::now();
        let scroll = self.art_scenes.scroll.value(now);
        let games = self.art_scenes.games();
        let mut stage = div()
            .id("infinite-wall")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(rgb(0x101214));
        let mut cards = layout
            .visible(scroll, games.len())
            .map(|i| (i, &games[i], self.art_scenes.emphasis(&games[i].id, now)))
            .collect::<Vec<_>>();
        cards.sort_by(|a, b| a.2.total_cmp(&b.2));
        for (index, game, emphasis) in cards {
            let rect = layout.rect(index, scroll);
            let lift = emphasis * 6.;
            let scale = 1. + 0.06 * emphasis;
            let displayed = Rect {
                x: rect.x - rect.w * (scale - 1.) / 2.,
                y: rect.y - rect.h * (scale - 1.) / 2. - lift,
                w: rect.w * scale,
                h: rect.h * scale,
            };
            let id = game.id.clone();
            let enter_id = id.clone();
            let play_id = id.clone();
            let game_for_click = game.clone();
            let label = game.title().to_string();
            let figure = div()
                .id(("wall-card", index))
                .size_full()
                .relative()
                .rounded(px(5.))
                .overflow_hidden()
                .role(Role::Button)
                .aria_label(format!("Open artwork for {label}"))
                .child(cover_art(game, cx))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .size_full()
                        .bg(black().alpha(self.art_scenes.surrounding_shade(&game.id, now))),
                )
                .on_hover(cx.listener(move |view, hovered, _, cx| {
                    if view.art_scenes.focus.is_some() || view.collection_overlay_open() {
                        return;
                    }
                    if *hovered {
                        view.art_scenes.hover(
                            Some(enter_id.clone()),
                            appearance::reduced_motion(),
                            Instant::now(),
                        );
                    } else if view.art_scenes.hovered.as_ref() == Some(&enter_id) {
                        view.art_scenes
                            .hover(None, appearance::reduced_motion(), Instant::now());
                    }
                    cx.notify();
                }))
                .on_click(cx.listener(move |view, _, window, cx| {
                    if view.collection_overlay_open() {
                        return;
                    }
                    if overview {
                        view.select_recent_game(id.clone(), window, cx);
                        view.art_scenes.overview.set(
                            0.,
                            Duration::from_millis(400),
                            appearance::reduced_motion(),
                            Instant::now(),
                        );
                    } else {
                        view.select_recent_game(id.clone(), window, cx);
                        view.art_scenes.open(
                            game_for_click.clone(),
                            displayed,
                            false,
                            appearance::reduced_motion(),
                            Instant::now(),
                        );
                    }
                    cx.notify();
                }))
                .child(
                    div()
                        .absolute()
                        .bottom(px(12.))
                        .w_full()
                        .flex()
                        .justify_center()
                        .opacity(emphasis)
                        .child(with_accessible_label(
                            Button::new(("wall-play", index))
                                .custom(action_variant(cx.theme().primary, cx))
                                .rounded_full()
                                .h(px(28.))
                                .label("Play")
                                .disabled(emphasis < 0.01 || self.collection_overlay_open())
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    cx.stop_propagation();
                                    if !view.collection_overlay_open() {
                                        view.submit(
                                            Command::LaunchGame {
                                                game_id: play_id.clone(),
                                            },
                                            cx,
                                        );
                                    }
                                })),
                            format!("Play {label}"),
                        )),
                );
            stage = stage.child(
                positioned(displayed)
                    .flex()
                    .child(self.with_library_context_menu(game, figure, cx)),
            );
        }
        if overview {
            stage = stage.child(
                div().absolute().top(px(52.)).right(px(20.)).child(
                    button("close-book-overview", cx)
                        .label("Back to page")
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.art_scenes.overview.set(
                                0.,
                                Duration::from_millis(400),
                                appearance::reduced_motion(),
                                Instant::now(),
                            );
                            cx.notify();
                        })),
                ),
            );
        }
        stage.into_any_element()
    }
    fn reel_scene(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let width = self.main_content_width(window);
        let height = f32::from(window.viewport_size().height);
        let now = Instant::now();
        let games = self.art_scenes.games();
        let position = self.art_scenes.index.value(now);
        let [r, g, b] = self.art_scenes.ambient.accent(now);
        let accent: Hsla = Rgba { r, g, b, a: 1. }.into();
        let mut stage = div()
            .id("film-reel")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(linear_gradient(
                180.,
                linear_color_stop(rgb(0x100b09), 0.),
                linear_color_stop(hsla(accent.h, accent.s * 0.65, 0.12, 1.), 1.),
            ));
        if let Some(frame) = self
            .gallery_gpu
            .read(cx)
            .frame()
            .filter(|frame| frame.scene.liquid.is_some())
        {
            stage = stage.child(crate::gallery_gpu::frame_image(frame));
        }
        let mut drawn = Vec::with_capacity(5);
        let base = position.round() as i64;
        for delta in -2..=2 {
            let index = (base + delta).rem_euclid(games.len() as i64) as usize;
            if drawn.contains(&index) {
                continue;
            }
            drawn.push(index);
            let game = &games[index];
            let offset = crate::gallery::relative(index, position, games.len());
            if offset.abs() > 1.7 {
                continue;
            }
            let rect = scene::reel_rect(width, height, offset);
            let id = game.id.clone();
            let artwork = self.scene_art(game, Mode::Reel, window, cx);
            let figure = with_accessible_label(
                Button::new(("reel-scene", index))
                    .custom(ButtonCustomVariant::new(cx))
                    .size_full()
                    .p_0()
                    .border_0()
                    .rounded(px(2.))
                    .child(scene_image(
                        artwork.and_then(|art| art.image),
                        game,
                        false,
                        cx,
                    ))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        if !view.collection_overlay_open() {
                            view.select_recent_game(id.clone(), window, cx);
                        }
                    })),
                format!("Select {}", game.title()),
            );
            stage = stage.child(
                positioned(rect)
                    .flex()
                    .opacity(0.94 - offset.abs().min(1.) * 0.20)
                    .child(self.with_library_context_menu(game, figure, cx)),
            );
        }
        let center = scene::reel_rect(width, height, 0.);
        let top = (center.y + center.h + height * 0.07).min(height - 104.);
        let (old, new) = scene::text_phase(self.art_scenes.page.value(now));
        for (game, opacity, drift) in [
            (self.art_scenes.previous.as_ref(), old, -4.),
            (self.art_scenes.current.as_ref(), new, 6. * (1. - new)),
        ] {
            if let Some(game) = game
                && opacity > 0.001
            {
                stage = stage.child(
                    div()
                        .absolute()
                        .top(px(top + drift))
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_5()
                        .opacity(opacity)
                        .child(
                            div()
                                .font_family(self.design().display_font)
                                .text_size(px(22.))
                                .text_color(white())
                                .child(crate::artwork_motion::tracked(
                                    &game.title().to_uppercase(),
                                )),
                        )
                        .child(self.scene_play(game, Mode::Reel, white(), opacity < 0.99, cx)),
                );
            }
        }
        stage.into_any_element()
    }
    fn book_scene(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let full_w = self.main_content_width(window);
        let full_h = f32::from(window.viewport_size().height);
        let now = Instant::now();
        let p = self.art_scenes.page.value(now);
        let overview = self.art_scenes.overview.value(now);
        let scale = 1. - overview * 0.065;
        let w = full_w * scale;
        let h = full_h * scale;
        let palette = self.book_palette();
        let mut page = div()
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(palette.paper);
        let ink = palette.ink;
        if let Some(game) = &self.art_scenes.current {
            page = page.child(self.book_figure(game, w, h, window, cx));
        }
        for (index, (game, opacity)) in [
            (
                self.art_scenes.previous.as_ref().filter(|_| p < 0.999),
                1. - p,
            ),
            (self.art_scenes.current.as_ref(), p),
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(game) = game {
                page = page.child(
                    positioned(Rect { x: 0., y: 0., w, h })
                        .id(("book-page", index))
                        .opacity(opacity)
                        .overflow_hidden()
                        .child(
                            positioned(Rect {
                                x: w * 0.69,
                                y: h * 0.37,
                                w: w * 0.28,
                                h: h * 0.23,
                            })
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap_5()
                            .text_color(ink)
                            .child(
                                div()
                                    .font_family(self.design().display_font)
                                    .italic()
                                    .text_size(px((w * 0.034).clamp(28., 54.)))
                                    .line_height(relative(1.15))
                                    .text_center()
                                    .child(game.title().to_string()),
                            )
                            .child(self.scene_play(game, Mode::Book, ink.into(), p < 0.999, cx))
                            .when(
                                !game.artwork.contains_key(&ArtworkKind::Background),
                                |panel| {
                                    panel.child(
                                        button("book-choose-background", cx)
                                            .label("Choose background…")
                                            .disabled(p < 0.999)
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.cancel_artwork();
                                                view.artwork_kind = ArtworkKind::Background;
                                                view.reset_artwork_pages();
                                                view.open_details(DetailTab::Artwork, window, cx);
                                            })),
                                    )
                                },
                            ),
                        ),
                );
            }
        }
        let games = self.art_scenes.games();
        let index = self
            .selected
            .as_ref()
            .and_then(|id| self.art_scenes.position(id))
            .unwrap_or(0);
        page = page.child(
            div()
                .absolute()
                .bottom(px(32.))
                .right(px(36.))
                .child(with_accessible_label(
                    Button::new("book-collection-index")
                        .custom(action_variant(ink.into(), cx))
                        .label(format!("{:02} / {:02}", index + 1, games.len()))
                        .h(px(32.))
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.art_scenes.overview.set(
                                1.,
                                Duration::from_millis(400),
                                appearance::reduced_motion(),
                                Instant::now(),
                            );
                            cx.notify();
                        })),
                    "Open collection overview",
                )),
        );
        let mut stage = div()
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(palette.paper)
            .child(
                positioned(Rect {
                    x: (full_w - w) / 2.,
                    y: (full_h - h) / 2.,
                    w,
                    h,
                })
                .opacity(1. - overview * 0.65)
                .child(page),
            );
        if overview > 0.001 {
            stage = stage.child(
                div()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .opacity(overview)
                    .occlude()
                    .child(self.wall_scene(true, window, cx)),
            );
        }
        stage.into_any_element()
    }
    pub(super) fn book_palette(&self) -> scene::material::BookPalette {
        let now = Instant::now();
        scene::material::book_palette(
            self.art_scenes.ambient.accent(now),
            self.art_scenes.ambient.luminance(now),
        )
    }

    fn book_figure(
        &self,
        game: &Game,
        _w: f32,
        _h: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let art = self
            .scene_art(game, Mode::Book, window, cx)
            .and_then(|a| a.image);
        let previous = self
            .art_scenes
            .previous
            .as_ref()
            .filter(|_| self.art_scenes.page.active(Instant::now()))
            .and_then(|game| self.scene_art(game, Mode::Book, window, cx))
            .and_then(|art| art.image);
        let paper = self.book_palette().paper;
        let progress = self.art_scenes.page.value(Instant::now());
        let direction = self.art_scenes.book_direction;
        let seconds = if appearance::reduced_motion() {
            0.
        } else {
            self.art_scenes.drift.seconds()
        };
        let sheet = window.use_asset::<scene::Paper>(&scene::PaperLayer::Sheet, cx);
        let grain = window.use_asset::<scene::Paper>(&scene::PaperLayer::Grain, cx);
        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(paper)
            .when_some(sheet, |figure, sheet| {
                figure.child(
                    img(sheet)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Fill),
                )
            })
            .child(scene::material::book_artwork(
                art, previous, progress, direction, seconds, paper,
            ))
            .when_some(grain, |figure, grain| {
                figure.child(
                    img(grain)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Fill),
                )
            })
            .into_any_element()
    }
    fn scene_play(
        &self,
        game: &Game,
        mode: Mode,
        accent: Hsla,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = game.id.clone();
        let book = mode == Mode::Book;
        let underline = self.art_scenes.underline.value(Instant::now());
        with_accessible_label(
            Button::new(if book { "book-play" } else { "reel-play" })
                .custom(action_variant(accent, cx))
                .border_0()
                .p_0()
                .h(px(36.))
                .disabled(
                    disabled
                        || self.selected.as_ref() != Some(&game.id)
                        || self.collection_overlay_open()
                        || self.art_scenes.overview.value(Instant::now()) > 0.001,
                )
                .child(
                    div()
                        .relative()
                        .px_3()
                        .pb_2()
                        .font_family(self.design().display_font)
                        .when(book, |label| label.italic())
                        .text_size(px(if book { 20. } else { 14. }))
                        .child(if book {
                            "Play".into()
                        } else {
                            crate::artwork_motion::tracked("PLAY")
                        })
                        .when(book, |label| {
                            label.child(
                                div()
                                    .absolute()
                                    .bottom_0()
                                    .left(px(12.))
                                    .w(px(36. + 12. * underline))
                                    .h(px(1.))
                                    .bg(accent),
                            )
                        }),
                )
                .on_hover(cx.listener(move |view, hovered, _, cx| {
                    if book {
                        view.art_scenes.underline.set(
                            if *hovered { 1. } else { 0. },
                            Duration::from_millis(280),
                            appearance::reduced_motion(),
                            Instant::now(),
                        );
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |view, _, window, cx| {
                    if !view.collection_overlay_open() && view.selected.as_ref() == Some(&id) {
                        view.launch_art_scene(window, cx);
                    }
                })),
            format!("Play {}", game.title()),
        )
        .into_any_element()
    }
    fn expanded_scene(
        &self,
        focus: &scene::Focus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let w = self.main_content_width(window);
        let h = f32::from(window.viewport_size().height);
        let p = focus.amount.value(Instant::now());
        let mode = self.art_scene_mode().unwrap();
        let rect = focus.origin.mix(Rect { x: 0., y: 0., w, h }, p);
        let art = self
            .scene_art(&focus.game, mode, window, cx)
            .and_then(|a| a.image);
        let hero = if mode == Mode::Wall {
            self.scene_art(&focus.game, Mode::Reel, window, cx)
                .and_then(|a| a.image)
        } else {
            None
        };
        let hero_mix = if hero.is_some() {
            scene::hero_mix(p, focus.hero.value(Instant::now()))
        } else {
            0.
        };
        let game_id = focus.game.id.clone();
        let title = focus.game.title().to_string();
        let metadata = focus.game.effective_metadata();
        let description = metadata.description.as_deref().unwrap_or("");
        let description = if description.chars().count() > 420 {
            format!("{}…", description.chars().take(420).collect::<String>())
        } else {
            description.to_string()
        };
        let info = [
            source_name(&focus.game.provider_id).to_string(),
            metadata.developer.unwrap_or_default(),
            metadata.genres.unwrap_or_default(),
            last_played(&focus.game),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        div()
            .absolute()
            .inset_0()
            .size_full()
            .occlude()
            .bg(black().alpha(p * 0.88))
            .child(
                positioned(rect)
                    .overflow_hidden()
                    .opacity(1. - hero_mix)
                    .child(scene_image(art, &focus.game, false, cx)),
            )
            .when(hero_mix > 0., |layer| {
                layer.child(
                    positioned(rect)
                        .overflow_hidden()
                        .opacity(hero_mix)
                        .child(scene_image(hero, &focus.game, false, cx)),
                )
            })
            .when(!focus.launch, |layer| {
                layer.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .w_full()
                        .h(px(h * 0.65))
                        .opacity(p)
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(black().alpha(0.), 0.),
                            linear_color_stop(black().alpha(0.74), 1.),
                        )),
                )
            })
            .when(!focus.launch, |layer| {
                layer.child(
                    div()
                        .absolute()
                        .bottom(px(32.))
                        .left(px(40.))
                        .w(px((w * 0.62).min(760.)))
                        .opacity(p)
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap_3()
                        .text_color(white())
                        .child(
                            div()
                                .w_full()
                                .text_size(px((w * 0.036).clamp(30., 54.)))
                                .font_family(self.design().display_font)
                                .child(title.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .text_sm()
                                .text_color(white().alpha(0.78))
                                .child(info),
                        )
                        .when(!description.is_empty(), |caption| {
                            caption.child(
                                div()
                                    .w_full()
                                    .text_size(px(15.))
                                    .line_height(relative(1.45))
                                    .text_color(white().alpha(0.9))
                                    .child(description),
                            )
                        })
                        .child(with_accessible_label(
                            Button::new("wall-expanded-play")
                                .custom(action_variant(white(), cx))
                                .label("Play")
                                .disabled(p < 0.99 || self.collection_overlay_open())
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    if !view.collection_overlay_open() {
                                        view.submit(
                                            Command::LaunchGame {
                                                game_id: game_id.clone(),
                                            },
                                            cx,
                                        );
                                    }
                                })),
                            format!("Play {title}"),
                        ))
                        .child(
                            Button::new("wall-detail-artwork")
                                .custom(action_variant(white(), cx))
                                .label("Choose background…")
                                .disabled(p < 0.99 || self.collection_overlay_open())
                                .on_click(cx.listener(|view, _, window, cx| {
                                    if !view.collection_overlay_open() {
                                        view.cancel_artwork();
                                        view.artwork_kind = ArtworkKind::Background;
                                        view.reset_artwork_pages();
                                        view.open_details(DetailTab::Artwork, window, cx);
                                    }
                                })),
                        ),
                )
            })
            .child(
                div().absolute().top(px(54.)).right(px(18.)).child(
                    button("close-expanded-scene", cx)
                        .label("Close · Esc")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.art_scenes
                                .dismiss(appearance::reduced_motion(), Instant::now());
                            view.focus.focus(window, cx);
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }
}

fn positioned(rect: Rect) -> Div {
    div()
        .absolute()
        .left(px(rect.x))
        .top(px(rect.y))
        .w(px(rect.w))
        .h(px(rect.h))
}
fn action_variant(accent: Hsla, cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(accent)
        .hover(accent.alpha(0.08))
        .active(accent.alpha(0.14))
}
fn cover_art(game: &Game, cx: &App) -> AnyElement {
    match game.artwork.get(&ArtworkKind::Cover) {
        Some(art) => img(if art.thumbnail_path.as_os_str().is_empty() {
            art.image_path.clone()
        } else {
            art.thumbnail_path.clone()
        })
        .size_full()
        .object_fit(ObjectFit::Cover)
        .rounded(px(5.))
        .into_any_element(),
        None => div()
            .size_full()
            .bg(cx.theme().secondary)
            .flex()
            .items_end()
            .p_3()
            .text_sm()
            .child(game.title().to_string())
            .into_any_element(),
    }
}
fn scene_image(art: Option<Arc<RenderImage>>, game: &Game, book: bool, cx: &App) -> AnyElement {
    if let Some(art) = art {
        return img(art)
            .size_full()
            .object_fit(ObjectFit::Cover)
            .into_any_element();
    }
    if book {
        div().size_full().into_any_element()
    } else {
        cover_art(game, cx)
    }
}
