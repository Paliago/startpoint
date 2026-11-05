use super::*;
use scene::composition;

impl LauncherView {
    fn diagonal_keys(
        &self,
        index: usize,
        width: f32,
        height: f32,
    ) -> Vec<crate::cinematic::ArtworkKey> {
        let games = self.art_scenes.games();
        let scale = (1600. / width.max(1.)).min(1000. / height.max(1.)).min(1.);
        let width = ((width * scale).clamp(1., 1600.) as u16).div_ceil(16) * 16;
        let height = ((height * scale).clamp(1., 1000.) as u16).div_ceil(16) * 16;
        [
            (index + games.len() - 1) % games.len(),
            index,
            (index + 1) % games.len(),
        ]
        .into_iter()
        .zip(0_u8..3)
        .map(|(index, slot)| crate::cinematic::ArtworkKey::Diagonal {
            path: games[index]
                .artwork
                .get(&ArtworkKind::Background)
                .or_else(|| games[index].artwork.get(&ArtworkKind::Cover))
                .map(|art| art.image_path.clone())
                .unwrap_or_default(),
            width,
            height,
            slot,
        })
        .collect()
    }

    fn diagonal_artwork(
        &self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Option<crate::cinematic::Artwork>> {
        let width = self.main_content_width(window);
        let height = f32::from(window.viewport_size().height);
        let keys = self.diagonal_keys(index, width, height);
        let mut pinned = keys.clone();
        if let Some(current) = self
            .art_scenes
            .selected
            .as_ref()
            .and_then(|id| self.art_scenes.position(id))
            && current != index
        {
            pinned.extend(self.diagonal_keys(current, width, height));
        }
        if let Some(previous) = &self.art_scenes.previous
            && self.art_scenes.page.active(Instant::now())
            && let Some(index) = self.art_scenes.position(&previous.id)
        {
            pinned.extend(self.diagonal_keys(index, width, height));
        }
        self.scene_artwork.update(cx, |cache, cx| {
            cache.set_capacity(9);
            keys.iter()
                .map(|key| cache.load(key, &pinned, window, cx))
                .collect()
        })
    }

    pub(super) fn diagonal_ready(
        &self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.art_scenes.games().len() < 3
            || self
                .diagonal_artwork(index, window, cx)
                .iter()
                .all(Option::is_some)
    }

    pub(super) fn diagonal_scene(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let games = self.art_scenes.games();
        if games.len() < 3 {
            return self.mosaic_scene(window, cx);
        }
        let width = self.main_content_width(window);
        let height = f32::from(window.viewport_size().height);
        let index = self
            .art_scenes
            .selected
            .as_ref()
            .and_then(|id| self.art_scenes.position(id))
            .unwrap_or(0);
        let indices = [
            (index + games.len() - 1) % games.len(),
            index,
            (index + 1) % games.len(),
        ];
        let artworks = self.diagonal_artwork(index, window, cx);
        let now = Instant::now();
        let progress = self.art_scenes.page.value(now);
        let emphasis = indices.map(|index| self.art_scenes.emphasis(&games[index].id, now));
        let mut stage = div()
            .id("diagonal-composition")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(rgb(0x090a0c))
            .role(Role::Group)
            .aria_label(format!(
                "Diagonal library. Selected: {}. Use Left and Right to browse.",
                games[index].title()
            ));
        if progress < 0.999
            && let Some(previous) = &self.art_scenes.previous
            && let Some(previous) = self.art_scenes.position(&previous.id)
        {
            for art in self.diagonal_artwork(previous, window, cx) {
                if let Some(image) = art.and_then(|art| art.image) {
                    stage = stage.child(
                        div()
                            .absolute()
                            .inset_0()
                            .size_full()
                            .opacity(1. - progress)
                            .child(img(image).size_full().object_fit(ObjectFit::Fill)),
                    );
                }
            }
        }
        let mut slices = indices.iter().zip(artworks).enumerate().collect::<Vec<_>>();
        slices.sort_by(|a, b| emphasis[a.0].total_cmp(&emphasis[b.0]));
        for (slot, (index, art)) in slices {
            if let Some(image) = art.and_then(|art| art.image) {
                let rect = composition::diagonal_bounds(slot, emphasis[slot]);
                let arrival = ((progress * 1.2 - slot as f32 * 0.10) / 0.95).clamp(0., 1.);
                let travel = (1. - arrival) * self.art_scenes.book_direction;
                stage = stage.child(
                    positioned(Rect {
                        x: (rect.x + travel * 0.10) * width,
                        y: (rect.y - travel * 0.14) * height,
                        w: rect.w * width,
                        h: rect.h * height,
                    })
                    .id(("diagonal-artwork", *index))
                    .opacity(arrival)
                    .role(Role::Image)
                    .aria_label(games[*index].title().to_string())
                    .child(img(image).size_full().object_fit(ObjectFit::Fill)),
                );
            }
        }
        let game = &games[index];
        let title = if game.title().chars().count() <= 24 {
            crate::artwork_motion::tracked(game.title())
        } else {
            game.title().to_string()
        };
        stage = stage
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .w_full()
                    .h(px(height * 0.35))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(black().alpha(0.), 0.),
                        linear_color_stop(black().alpha(0.55), 1.),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(height * 0.065))
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_5()
                    .child(
                        div()
                            .font_family(self.design().display_font)
                            .text_size(px((height * 0.04).clamp(28., 48.)))
                            .text_color(white())
                            .text_center()
                            .line_height(relative(1.1))
                            .max_w(px(width * 0.6))
                            .child(title),
                    )
                    .child(self.composition_play(game, true, cx)),
            );
        let ids = indices.map(|index| games[index].id.clone());
        let menu_games = indices.map(|index| games[index].clone());
        let hover_ids = ids.clone();
        let owner = cx.weak_entity();
        stage
            .cursor_pointer()
            .on_mouse_move(cx.listener(move |view, event: &MouseMoveEvent, _, cx| {
                if view.collection_overlay_open() || view.art_scenes.page.active(Instant::now()) {
                    return;
                }
                let slot = composition::diagonal_hit(
                    f32::from(event.position.x) / width,
                    f32::from(event.position.y) / height,
                    emphasis,
                );
                let id = hover_ids[slot].clone();
                if view.art_scenes.hovered.as_ref() != Some(&id) {
                    let now = Instant::now();
                    view.art_scenes
                        .hover(Some(id), appearance::reduced_motion(), now);
                    view.art_scenes.hover.set(
                        1.,
                        Duration::from_millis(160),
                        appearance::reduced_motion(),
                        now,
                    );
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|view, hovered: &bool, _, cx| {
                if !*hovered {
                    view.art_scenes
                        .hover(None, appearance::reduced_motion(), Instant::now());
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                if view.collection_overlay_open() {
                    return;
                }
                let position = event.position();
                let slot = composition::diagonal_hit(
                    f32::from(position.x) / width,
                    f32::from(position.y) / height,
                    emphasis,
                );
                view.activate_composition(&ids[slot], event.click_count() == 2, window, cx);
            }))
            .context_menu(move |menu, window, cx| {
                let position = window.mouse_position();
                let slot = composition::diagonal_hit(
                    f32::from(position.x) / width,
                    f32::from(position.y) / height,
                    emphasis,
                );
                library_game_menu(menu, &menu_games[slot], owner.clone(), cx)
            })
            .into_any_element()
    }

    pub(super) fn mosaic_scene(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let games = self.art_scenes.games();
        let width = self.main_content_width(window);
        let height = f32::from(window.viewport_size().height);
        let selected = self
            .selected
            .as_ref()
            .and_then(|id| self.art_scenes.position(id))
            .unwrap_or(0);
        let page_start = selected / 6 * 6;
        let page_count = games.len().div_ceil(6);
        let page_size = (games.len() - page_start).min(6);
        let mut stage = div()
            .id("mosaic-composition")
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(black());
        for (slot, rect) in composition::mosaic_rects(width, height, page_size)
            .into_iter()
            .enumerate()
        {
            let index = page_start + slot;
            let game = &games[index];
            let id = game.id.clone();
            let art = self
                .scene_art(game, Mode::Mosaic, window, cx)
                .and_then(|art| art.image);
            let cover = with_accessible_label(
                Button::new(("mosaic-tile", slot))
                    .custom(ButtonCustomVariant::new(cx))
                    .size_full()
                    .p_0()
                    .border_0()
                    .rounded(px(0.))
                    .child(scene_image(art, game, false, cx))
                    .child(div().absolute().inset_0().size_full().bg(linear_gradient(
                        180.,
                        linear_color_stop(black().alpha(0.), 0.5),
                        linear_color_stop(black().alpha(0.50), 1.),
                    )))
                    .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                        if view.collection_overlay_open() {
                            return;
                        }
                        view.activate_composition(&id, event.click_count() == 2, window, cx);
                    })),
                format!("Select {}. Double-click to play.", game.title()),
            );
            let inset = (rect.w * 0.04).clamp(16., 36.);
            let logo_width = (rect.w * 0.64).min(if slot == 0 { 310. } else { 250. });
            let logo_height = (rect.h * 0.18).clamp(30., if slot == 0 { 80. } else { 56. });
            stage = stage.child(
                positioned(rect)
                    .flex()
                    .child(self.with_library_context_menu(game, cover, cx))
                    .child(
                        div()
                            .absolute()
                            .left(px(inset))
                            .bottom(px(inset))
                            .right(px(inset))
                            .flex()
                            .items_center()
                            .gap_5()
                            .child(self.composition_heading(game, logo_width, logo_height, cx))
                            .when(index == selected, |caption| {
                                caption.child(self.composition_play(game, false, cx))
                            }),
                    ),
            );
        }
        if page_count > 1 {
            stage = stage.child(
                div().absolute().top(px(58.)).right(px(20.)).child(
                    toolbar_island("mosaic-pagination", toolbar_rounding(cx))
                        .child(
                            icon_button(
                                "mosaic-previous-screen",
                                IconName::ChevronLeft,
                                "Previous screen",
                                cx,
                            )
                            .on_click(cx.listener(
                                |view, _, window, cx| {
                                    view.move_mosaic_page(false, window, cx);
                                },
                            )),
                        )
                        .child(div().px_3().text_sm().text_color(white()).child(format!(
                            "{} / {}",
                            selected / 6 + 1,
                            page_count
                        )))
                        .child(
                            icon_button(
                                "mosaic-next-screen",
                                IconName::ChevronRight,
                                "Next screen",
                                cx,
                            )
                            .on_click(cx.listener(
                                |view, _, window, cx| {
                                    view.move_mosaic_page(true, window, cx);
                                },
                            )),
                        ),
                ),
            );
        }
        stage.into_any_element()
    }

    pub(super) fn move_mosaic_page(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.collection_overlay_open() {
            return;
        }
        let games = self.art_scenes.games();
        if games.is_empty() {
            return;
        }
        let index = self
            .selected
            .as_ref()
            .and_then(|id| self.art_scenes.position(id))
            .unwrap_or(0);
        let pages = games.len().div_ceil(6);
        let page = index / 6;
        let next = if forward {
            (page + 1) % pages
        } else {
            (page + pages - 1) % pages
        };
        let id = games[next * 6].id.clone();
        self.art_scenes.composition_pointer = None;
        self.select_recent_game(id, window, cx);
    }

    fn activate_composition(
        &mut self,
        id: &GameId,
        double: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if double {
            if let Some(id) = self.art_scenes.composition_pointer.take() {
                self.run_library_game_action(&id, LibraryGameAction::Launch, window, cx);
            }
        } else {
            self.art_scenes.composition_pointer = Some(id.clone());
            self.select_recent_game(id.clone(), window, cx);
        }
    }

    fn composition_heading(
        &self,
        game: &Game,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if game.artwork.contains_key(&ArtworkKind::Background)
            && let Some(asset) = game.artwork.get(&ArtworkKind::Logo)
            && let crate::logo_cache::State::Ready(logo) =
                self.logos.update(cx, |cache, cx| cache.load(asset, cx))
        {
            let scale = (width / logo.width.max(1) as f32).min(height / logo.height.max(1) as f32);
            return img(logo.path)
                .w(px(logo.width as f32 * scale))
                .h(px(logo.height as f32 * scale))
                .object_fit(ObjectFit::Contain)
                .into_any_element();
        }
        div()
            .max_w(px(width))
            .text_color(white())
            .font_family(self.design().display_font)
            .text_size(px((height * 0.75).clamp(18., 36.)))
            .truncate()
            .child(game.title().to_string())
            .into_any_element()
    }

    fn composition_play(
        &self,
        game: &Game,
        filled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = game.id.clone();
        with_accessible_label(
            Button::new("composition-play")
                .custom(if filled {
                    ButtonCustomVariant::new(cx)
                        .color(white())
                        .foreground(black())
                        .hover(white().alpha(0.88))
                        .active(white().alpha(0.75))
                } else {
                    action_variant(white(), cx)
                })
                .icon(Icon::default().path("icons/cinematic-play.svg"))
                .rounded_full()
                .border_0()
                .h(px(42.))
                .w(px(if filled { 144. } else { 42. }))
                .when(filled, |button| button.label("Play"))
                .disabled(
                    self.collection_overlay_open() || self.selected.as_ref() != Some(&game.id),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _, window, cx| {
                    cx.stop_propagation();
                    view.art_scenes.composition_pointer = None;
                    if !view.collection_overlay_open() {
                        view.run_library_game_action(&id, LibraryGameAction::Launch, window, cx);
                    }
                })),
            format!("Play {}", game.title()),
        )
    }
}
