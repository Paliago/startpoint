use super::*;
use gpui::ImageCache;
use scene::composition;

impl LauncherView {
    pub(super) fn prepare_mosaic_assets(
        &self,
        requested: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (bool, Vec<usize>) {
        let games = self.art_scenes.games();
        let displayed = self
            .art_scenes
            .selected
            .as_ref()
            .and_then(|id| self.art_scenes.position(id))
            .unwrap_or(requested);
        let pages = composition::mosaic_working_pages(requested, displayed, games.len());
        let pinned = pages
            .iter()
            .flat_map(|page| page * 6..((page + 1) * 6).min(games.len()))
            .filter_map(|index| Self::scene_key(&games[index], Mode::Mosaic))
            .collect::<Vec<_>>();
        let mut ready_pages = Vec::new();
        for page in pages {
            let mut ready = true;
            for index in page * 6..((page + 1) * 6).min(games.len()) {
                let game = &games[index];
                if let Some(key) = Self::scene_key(game, Mode::Mosaic) {
                    ready &= self.scene_artwork.update(cx, |cache, cx| {
                        cache.set_capacity(24);
                        cache.load(&key, &pinned, window, cx).is_some()
                    });
                }
                if game.artwork.contains_key(&ArtworkKind::Background)
                    && let Some(asset) = game.artwork.get(&ArtworkKind::Logo)
                {
                    match self.logos.update(cx, |cache, cx| cache.load(asset, cx)) {
                        crate::logo_cache::State::Pending => ready = false,
                        crate::logo_cache::State::Ready(logo) => {
                            ready &= self.thumbnails.update(cx, |cache, cx| {
                                cache.load(&Resource::from(logo.path), window, cx).is_some()
                            });
                        }
                        crate::logo_cache::State::Failed => {}
                    }
                }
            }
            if ready {
                ready_pages.push(page);
            }
        }
        ready_pages.sort_unstable();
        (ready_pages.contains(&(requested / 6)), ready_pages)
    }

    pub(super) fn mosaic_scene(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let games = self.art_scenes.games();
        let width = self.main_content_width(window);
        let height = f32::from(window.viewport_size().height);
        let Some(displayed) = self
            .art_scenes
            .selected
            .as_ref()
            .and_then(|id| self.art_scenes.position(id))
        else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(Spinner::new())
                .into_any_element();
        };
        let page_start = displayed / 6 * 6;
        let page_count = games.len().div_ceil(6);
        let page_size = (games.len() - page_start).min(6);
        let mut stage = div()
            .image_cache(self.thumbnails.clone())
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
                    .child(scene_image(art, game, cx))
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
                            .child(self.composition_heading(
                                game,
                                logo_width,
                                logo_height,
                                window,
                                cx,
                            ))
                            .when(index == displayed, |caption| {
                                caption.child(self.composition_play(game, cx))
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
                            displayed / 6 + 1,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if game.artwork.contains_key(&ArtworkKind::Background)
            && let Some(asset) = game.artwork.get(&ArtworkKind::Logo)
            && let crate::logo_cache::State::Ready(logo) =
                self.logos.update(cx, |cache, cx| cache.load(asset, cx))
            && self
                .thumbnails
                .update(cx, |cache, cx| {
                    cache.load(&Resource::from(logo.path.clone()), window, cx)
                })
                .is_some_and(|image| image.is_ok())
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

    fn composition_play(&self, game: &Game, cx: &mut Context<Self>) -> impl IntoElement {
        let id = game.id.clone();
        with_accessible_label(
            Button::new("composition-play")
                .custom(action_variant(white(), cx))
                .icon(Icon::default().path("icons/cinematic-play.svg"))
                .rounded_full()
                .border_0()
                .h(px(42.))
                .w(px(42.))
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
