use crate::{
    components::*,
    editor::EditorSection,
    effects, icons,
    state::{LibraryFilter, Screen, SortOrder, State},
};
use num_traits::ToPrimitive;
use rune::{Color, Cx, El, GrowthSpec, ScrollSpec, SurfaceFit, col, el, row};
use rune_kit::{Cell, Kit, Sidebar, SidebarRow, SidebarSection, SortDir, Table, TableColumn};
use startpoint_sdk::*;
use std::{borrow::Cow, collections::BTreeSet, path::Path};

pub fn image(state: &State, path: &Path, kit: &Kit) -> El {
    if let Some(image) = state
        .images
        .get(path)
        .filter(|image| image.error().is_none())
    {
        el().pixels_fit(image.pixels(), SurfaceFit::Fill)
    } else {
        el().fill(kit.c.layer_02)
    }
}

pub fn artwork(state: &State, game: &Game, kind: ArtworkKind, full: bool, kit: &Kit) -> El {
    if let Some(asset) = game
        .artwork
        .get(&kind)
        .or_else(|| game.artwork.get(&ArtworkKind::Cover))
    {
        let path = if full
            && state
                .images
                .get(&asset.image_path)
                .is_some_and(|image| image.is_ready() && image.error().is_none())
        {
            &asset.image_path
        } else {
            &asset.thumbnail_path
        };
        if state
            .images
            .get(path)
            .is_some_and(|image| image.error().is_none())
        {
            let recent = state.screen == Screen::Recents;
            let enabled = if recent {
                state.appearance.dither_recents
            } else {
                state.appearance.dither_artwork
            };
            let amount = if recent {
                state.appearance.dither_recents_strength
            } else {
                state.appearance.dither_artwork_strength
            };
            let pixel = if recent {
                state.appearance.dither_recents_size
            } else {
                state.appearance.dither_artwork_size
            };
            let image = image(state, path, kit);
            return if full && enabled && kind != ArtworkKind::Logo {
                effects::artwork_surface(image, amount * amount, pixel).clip_rounded(kit.radius.lg)
            } else {
                image
            };
        }
    }
    let initials = game
        .title()
        .split_whitespace()
        .filter_map(|word| word.chars().find(|character| character.is_alphanumeric()))
        .take(2)
        .collect::<String>();
    col()
        .center()
        .padding(16.0)
        .fill(kit.c.layer_02)
        .radial_gradient(
            [0.25, 0.1],
            360.0,
            [
                (0.0, kit.c.text_accent.alpha(0.12)),
                (1.0, Color::TRANSPARENT),
            ],
        )
        .child(label(initials, 34.0, 500.0, kit).opacity(0.18))
}

pub fn view(state: &State, cx: &mut Cx<State>, kit: &Kit, layers: &mut Vec<El>) -> El {
    let size = cx.layout_size("startpoint").unwrap_or([1280.0, 840.0]);
    let width = size[0];
    if state.screen == Screen::Recents {
        return match state.appearance.recents_layout {
            RecentsLayout::Carousel => classic(state, kit, size),
            RecentsLayout::InfiniteWall => crate::wall::view(state, kit, width),
            RecentsLayout::Gallery
            | RecentsLayout::Cinematic
            | RecentsLayout::FilmReel
            | RecentsLayout::Mosaic => classic(state, kit, size),
        };
    }
    grid_view(state, kit, layers, width)
}

pub fn immersive_recents(state: &State) -> bool {
    state.screen == Screen::Recents
        && matches!(
            state.appearance.recents_layout,
            RecentsLayout::Carousel | RecentsLayout::FilmReel | RecentsLayout::Mosaic
        )
}

pub fn sidebar(state: &State, kit: &Kit) -> El {
    let mut sidebar_kit = kit.clone();
    sidebar_kit.c.pressed = kit.c.text_accent.alpha(0.12);
    sidebar_kit.ty.text_xsmall_medium = rune::TextStyle::new(9.0, 600.0).tracking(1.2);
    let kit = &sidebar_kit;
    let mut library = SidebarSection::new("COLLECTION");
    for (title, filter) in [
        ("All installed", LibraryFilter::All),
        ("Favourites", LibraryFilter::Favourites),
        ("Manually added", LibraryFilter::Manual),
        ("Completed", LibraryFilter::Completed),
        ("Unfinished", LibraryFilter::Unfinished),
        ("Hidden games", LibraryFilter::Hidden),
    ] {
        library = library.row(
            SidebarRow::new(title)
                .height(34.0)
                .icon(match filter {
                    LibraryFilter::Favourites => &rune_kit::glyphs::STAR,
                    LibraryFilter::Completed => kit.glyph.check,
                    LibraryFilter::Hidden => kit.glyph.eye_off,
                    LibraryFilter::Manual => &icons::ADD,
                    LibraryFilter::Unfinished => &icons::PLAY,
                    LibraryFilter::All => &icons::SIDEBAR,
                })
                .selected(
                    state.screen == Screen::Library
                        && state.filter == filter
                        && state.source.is_none()
                        && state.collection.is_none(),
                )
                .on_select(move |s: &mut State, tx| {
                    tx.instant();
                    s.set_filter(filter);
                })
                .build(kit)
                .key(match filter {
                    LibraryFilter::All => "filter-all",
                    LibraryFilter::Favourites => "filter-favourites",
                    LibraryFilter::Manual => "filter-manual",
                    LibraryFilter::Completed => "filter-completed",
                    LibraryFilter::Unfinished => "filter-unfinished",
                    LibraryFilter::Hidden => "filter-hidden",
                }),
        );
    }
    let mut sources = SidebarSection::new("SOURCES");
    for provider in &state.snapshot.providers {
        if !state
            .snapshot
            .games
            .iter()
            .any(|game| game.provider_id == provider.id)
        {
            continue;
        }
        let id = provider.id.clone();
        sources = sources.row(
            SidebarRow::new(&provider.name)
                .height(34.0)
                .badge(
                    state
                        .snapshot
                        .games
                        .iter()
                        .filter(|game| game.provider_id == provider.id && game.available)
                        .count()
                        .to_string(),
                )
                .selected(state.source.as_ref() == Some(&provider.id))
                .on_select(move |s: &mut State, tx| {
                    tx.instant();
                    s.set_source(id.clone());
                })
                .build(kit)
                .key(format!("source-{}", provider.id)),
        );
    }
    let collections: BTreeSet<_> = state
        .snapshot
        .games
        .iter()
        .filter(|game| !game.hidden)
        .flat_map(|game| game.collections.iter())
        .collect();
    let mut groups = SidebarSection::new("YOUR COLLECTIONS");
    let has_collections = !collections.is_empty();
    for collection in collections {
        let name = collection.clone();
        groups = groups.row(
            SidebarRow::new(collection)
                .selected(state.collection.as_ref() == Some(collection))
                .on_select(move |s: &mut State, tx| {
                    tx.instant();
                    s.set_collection(name.clone());
                })
                .build(kit)
                .key(format!("collection-{collection}")),
        );
    }
    let mut sidebar = Sidebar::new()
        .label("Library navigation")
        .inset(0.0)
        .row_gap(6.0)
        .section(library)
        .section(sources);
    if has_collections {
        sidebar = sidebar.section(groups);
    }
    sidebar
        .build(kit)
        .w(state.appearance.sidebar_width.clamp(140.0, 260.0))
        .h_full()
}

pub fn grid_geometry(state: &State, window_width: f32) -> (usize, f32, f32) {
    let detail_visible =
        state.appearance.library_details && !state.detail_closed && state.selected_game().is_some();
    let detail_width = if window_width >= 1150.0 { 288.0 } else { 240.0 };
    let available = window_width
        - 48.0
        - if state.appearance.sidebar_visible {
            state.appearance.sidebar_width.clamp(140.0, 260.0) + 24.0
        } else {
            0.0
        }
        - if detail_visible {
            detail_width + 24.0
        } else {
            0.0
        };
    let target = match state.appearance.library_density {
        LibraryDensity::Small => 128.0,
        LibraryDensity::Medium => 160.0,
        LibraryDensity::Large => 196.0,
    };
    let target = if state.appearance.library_layout == LibraryLayout::Tiles {
        target * 1.5
    } else {
        target
    };
    let columns = ((available + 18.0) / (target + 18.0)).round().max(1.0);
    let card_width = ((available - (columns - 1.0) * 18.0) / columns).max(80.0);
    (
        columns.to_usize().unwrap_or(1).max(1),
        card_width,
        detail_width,
    )
}

fn grid_view(state: &State, kit: &Kit, layers: &mut Vec<El>, window_width: f32) -> El {
    let games = state.visible_games();
    let title = state.collection.as_deref().unwrap_or(match state.filter {
        LibraryFilter::All => "Library",
        LibraryFilter::Favourites => "Favourites",
        LibraryFilter::Manual => "Manually added",
        LibraryFilter::Hidden => "Hidden games",
        LibraryFilter::Completed => "Completed",
        LibraryFilter::Unfinished => "Unfinished",
    });
    let mut layout_controls = crate::toolbar::surface(state, kit).key("library-view-controls");
    for (key, icon, title, layout) in [
        (
            "layout-covers",
            &icons::GRID,
            "Cover grid",
            LibraryLayout::Covers,
        ),
        (
            "layout-tiles",
            &icons::TILES,
            "Landscape tiles",
            LibraryLayout::Tiles,
        ),
        (
            "layout-compact",
            &icons::LIST,
            "Compact list",
            LibraryLayout::Compact,
        ),
    ] {
        layout_controls = layout_controls.child(
            toolbar_icon(icon, title, kit)
                .size(28.0, 26.0)
                .fill(if state.appearance.library_layout == layout {
                    kit.c.text_base.alpha(0.08)
                } else {
                    Color::TRANSPARENT
                })
                .focus_selected(state.appearance.library_layout == layout)
                .on_press(move |s: &mut State| {
                    s.appearance.library_layout = layout;
                    s.page = 0;
                    s.refresh_library_order();
                    s.save_appearance();
                })
                .key(key),
        );
    }
    let overview = state.appearance.library_details && !state.detail_closed;
    layout_controls = layout_controls
        .child(el().w(1.0).h(14.0).fill(kit.c.text_base.alpha(0.1)))
        .child(
            toolbar_icon(
                &icons::SIDEBAR,
                if overview {
                    "Hide overview"
                } else {
                    "Show overview"
                },
                kit,
            )
            .size(28.0, 26.0)
            .fill(if overview {
                kit.c.text_base.alpha(0.08)
            } else {
                Color::TRANSPARENT
            })
            .on_press_tx(|s: &mut State, tx| {
                crate::motion::fold(tx);
                if s.appearance.library_details && !s.detail_closed {
                    s.detail_closed = true;
                } else {
                    s.appearance.library_details = true;
                    s.detail_closed = false;
                    s.save_appearance();
                }
            })
            .key("overview-toggle"),
        );
    let header = row()
        .w_full()
        .justify_between()
        .gap(16.0)
        .center_y()
        .child(
            col().grow(1.0).min_w(0.0).gap(6.0).child(
                heading(
                    if state.screen == Screen::Recents {
                        "Infinite Wall"
                    } else {
                        title
                    },
                    30.0,
                    kit,
                )
                .w_full()
                .truncate()
                .help(title),
            ),
        )
        .child(
            col()
                .w(190.0)
                .gap(8.0)
                .items_end()
                .child(layout_controls)
                .child(
                    row()
                        .w_full()
                        .gap(6.0)
                        .center_y()
                        .child(
                            choice(
                                state,
                                ("library-sort", "Sort"),
                                &["Title", "Recently played", "Source"],
                                match state.sort {
                                    SortOrder::Title => "Title",
                                    SortOrder::Recent => "Recently played",
                                    SortOrder::Source => "Source",
                                },
                                |s, value, _| {
                                    s.sort = match value {
                                        "Recently played" => SortOrder::Recent,
                                        "Source" => SortOrder::Source,
                                        _ => SortOrder::Title,
                                    };
                                    s.sort_reverse = false;
                                    s.page = 0;
                                    s.refresh_library_order();
                                },
                                kit,
                                layers,
                            )
                            .grow(1.0)
                            .min_w(0.0),
                        )
                        .child(
                            toolbar_icon(
                                &icons::SORT,
                                if state.sort_reverse {
                                    "Use ascending order"
                                } else {
                                    "Reverse sort order"
                                },
                                kit,
                            )
                            .size(28.0, 28.0)
                            .fill(if state.sort_reverse {
                                kit.c.text_accent.alpha(0.12)
                            } else {
                                Color::TRANSPARENT
                            })
                            .on_press(|s: &mut State| {
                                s.sort_reverse = !s.sort_reverse;
                                s.page = 0;
                                s.refresh_library_order();
                            })
                            .key("sort-direction"),
                        ),
                ),
        );
    let compact = state.appearance.library_layout == LibraryLayout::Compact
        && state.screen == Screen::Library;
    let (column_count, card_width, detail_width) = grid_geometry(state, window_width);
    let content = if compact {
        compact_table(state, kit)
    } else {
        let palette = kit.clone();
        let mut cards = col()
            .key("game-grid")
            .w_full()
            .growth(GrowthSpec::default())
            .instant_changes();
        if games.is_empty() {
            cards = cards.child(
                col()
                    .key("empty-library")
                    .padding(24.0)
                    .gap(12.0)
                    .child(label(
                        if state.snapshot.ready {
                            "No games match this view"
                        } else {
                            "Opening library…"
                        },
                        22.0,
                        500.0,
                        kit,
                    ))
                    .child(hint(
                        "Change your search or filters, refresh discovery, or add a game.",
                        kit,
                    )),
            );
        } else {
            let order: std::sync::Arc<Vec<_>> =
                std::sync::Arc::new(games.iter().map(|game| game.id.clone()).collect());
            let keys = order.clone();
            cards = cards.virtual_rows(
                order.len().div_ceil(column_count),
                card_width * 1.5 + 70.0,
                move |_: &State, index| {
                    keys.get(index * column_count).map_or_else(
                        || Cow::Owned(format!("library-row-empty-{index}")),
                        |id| Cow::Owned(format!("library-row-{}", id.0)),
                    )
                },
                move |s: &State, index| {
                    row().w_full().gap(18.0).pb(18.0).items_start().children(
                        order
                            .iter()
                            .skip(index * column_count)
                            .take(column_count)
                            .filter_map(|id| s.game(id))
                            .map(|game| sized_card(s, game, &palette, card_width)),
                    )
                },
            );
        }
        let start = state.library_start.clone();
        let stride = if state.appearance.library_layout == LibraryLayout::Tiles {
            card_width * 0.6 + 70.0
        } else {
            card_width * 1.5 + 70.0
        };
        col()
            .key("library-scroll")
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .scroll_edge_fade(24.0)
            .on_scroll_offset(move |position| {
                let first = (position.offset[1].max(0.0) / stride)
                    .floor()
                    .to_usize()
                    .unwrap_or(0);
                start.store(
                    first.saturating_mul(column_count),
                    std::sync::atomic::Ordering::Relaxed,
                );
            })
            .child(cards)
    };
    let mut collection = col()
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .gap(20.0)
        .child(header);
    if missing_artwork(state) {
        collection = collection.child(artwork_prompt(state, kit));
    }
    collection = collection.child(content.grow(1.0).min_h(0.0));
    let mut body = row().w_full().min_h(0.0).gap(0.0).child(collection);
    if let Some(game) = state.selected_game() {
        body = body.child(
            el().key("overview-fold")
                .w((detail_width + 24.0) * state.overview_fold.progress)
                .instant_changes()
                .h_full()
                .clip_rounded(0.0)
                .child(
                    details(state, game, kit)
                        .w(detail_width)
                        .absolute(24.0, 0.0)
                        .h_full()
                        .instant_changes(),
                ),
        );
    }
    body.instant_changes().transition(rune::Transition::none())
}

fn compact_table(state: &State, kit: &Kit) -> El {
    let mut table = Table::new(
        |s: &State| &s.table,
        |s: &mut State| &mut s.table,
        |s: &State| s.library_order.len(),
        |s: &State, index| {
            s.library_order
                .get(index)
                .map_or(Cow::Borrowed(""), |id| Cow::Borrowed(id.0.as_str()))
        },
    )
    .column(
        TableColumn::new("title", "Game")
            .flex(2.0)
            .min(160.0)
            .sortable(),
    )
    .column(
        TableColumn::new("developer", "Developer")
            .flex(1.0)
            .min(100.0),
    )
    .column(TableColumn::new("progress", "Progress").width(90.0))
    .cell(|s: &State, index, column| {
        let Some(game) = s.library_order.get(index).and_then(|id| s.game(id)) else {
            return Cell::text("");
        };
        Cell::text(match column {
            "title" => game.title().into(),
            "developer" => game.effective_metadata().developer.unwrap_or_default(),
            "source" => game.provider_id.clone(),
            "date" => last_played(game.last_launched),
            "progress" => {
                if game.completed {
                    "Completed".into()
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        })
    })
    .on_select(|s: &mut State, id, _| s.select(GameId(id.into())))
    .on_open(|s: &mut State, id, _| s.launch(GameId(id.into())))
    .on_sort(|s: &mut State, column, direction, _| {
        s.sort = match column {
            "source" => SortOrder::Source,
            "date" => SortOrder::Recent,
            _ => SortOrder::Title,
        };
        s.sort_reverse = if s.sort == SortOrder::Recent {
            direction == SortDir::Ascending
        } else {
            direction == SortDir::Descending
        };
        s.refresh_library_order();
    });
    if state.appearance.library_sources {
        table = table.column(TableColumn::new("source", "Source").width(100.0).sortable());
    }
    if state.appearance.library_last_played {
        table = table.column(
            TableColumn::new("date", "Last played")
                .width(110.0)
                .sortable(),
        );
    }
    table.build(state, kit).grow(1.0).min_h(0.0)
}

fn sized_card(state: &State, game: &Game, kit: &Kit, width: f32) -> El {
    let selected = state.selected.as_ref() == Some(&game.id);
    let tile = state.appearance.library_layout == LibraryLayout::Tiles;
    let pill = state.appearance.library_layout == LibraryLayout::Pills;
    let id = game.id.clone();
    let launch_id = game.id.clone();
    let context_id = game.id.clone();
    let mut card = col()
        .key(format!("game-{}", game.id.0))
        .role("button")
        .label(game.title())
        .focusable(true)
        .focus_selected(selected)
        .cursor(rune::Cursor::Pointer)
        .w(width)
        .padding(0.0)
        .pb(10.0)
        .gap(8.0)
        .radius(if pill { 24.0 } else { kit.radius.lg })
        .fill(Color::TRANSPARENT)
        .hover(kit.c.hover)
        .press(kit.c.pressed)
        .focus_ring(2.0, 2.0, kit.c.border_focus)
        .on_press(move |s: &mut State| s.select(id.clone()))
        .on_double_press(move |s: &mut State| s.launch(launch_id.clone()))
        .on_secondary_press(move |s: &mut State, point, _| {
            s.context = Some((context_id.clone(), point))
        });
    card = card.child(
        artwork(
            state,
            game,
            if tile {
                ArtworkKind::Background
            } else {
                ArtworkKind::Cover
            },
            false,
            kit,
        )
        .w_full()
        .h(if pill {
            76.0
        } else if tile {
            130.0
        } else {
            width * 1.5
        })
        .clip_rounded(if pill { 20.0 } else { 8.0 })
        .border(
            if selected { 2.0 } else { 0.5 },
            if selected {
                kit.c.text_accent
            } else {
                kit.c.text_base.alpha(0.1)
            },
        )
        .radius(if pill { 20.0 } else { 8.0 }),
    );
    card = card.child(
        label(game.title(), 12.0, 550.0, kit)
            .w_full()
            .truncate()
            .help(game.title()),
    );
    if state.appearance.library_favourite_badges && game.favourite {
        card = card.child(label("★ Favourite", 10.0, 500.0, kit).color(kit.c.text_accent));
    }
    if state.appearance.library_sources {
        card = card.child(label(&game.provider_id, 10.0, 400.0, kit).color(kit.c.text_muted));
    }
    if state.appearance.library_last_played && game.last_launched.is_some() {
        card = card.child(
            label(last_played(game.last_launched), 10.0, 400.0, kit).color(kit.c.text_muted),
        );
    }
    card
}

pub fn details(state: &State, game: &Game, kit: &Kit) -> El {
    let presented = state
        .presented_detail
        .as_ref()
        .and_then(|id| state.game(id))
        .unwrap_or(game);
    let metadata = game.effective_metadata();
    let id = game.id.clone();
    let progress_id = game.id.clone();
    let journal_id = game.id.clone();
    let mut artwork_kit = kit.clone();
    artwork_kit.c.text_base = Color::WHITE;
    artwork_kit.c.icon_base = Color::WHITE;
    artwork_kit.c.icon_muted = Color::WHITE;
    let mut title = label(presented.title(), 28.0, 650.0, kit)
        .color(Color::WHITE)
        .wrap()
        .w_full();
    if let Some(logo) = presented
        .artwork
        .get(&ArtworkKind::Logo)
        .and_then(|asset| state.images.get(&asset.image_path))
        .filter(|image| image.is_ready() && image.error().is_none())
    {
        title = el()
            .w_full()
            .h(88.0)
            .pixels_fit(logo.pixels(), SurfaceFit::Fit)
            .label(presented.title());
    }
    col()
        .key("game-details")
        .w(280.0)
        .min_h(0.0)
        .h_full()
        .gap(0.0)
        .radius(14.0)
        .clip_rounded(14.0)
        .fill(kit.c.layer_01)
        .border(1.0, kit.c.text_base.alpha(0.07))
        .child(
            el().w_full()
                .h(350.0)
                .child(
                    presented
                        .artwork
                        .get(&ArtworkKind::Background)
                        .map_or_else(
                            || artwork(state, presented, ArtworkKind::Cover, false, kit),
                            |asset| image(state, &asset.image_path, kit),
                        )
                        .w_full()
                        .h_full(),
                )
                .child(
                    col()
                        .absolute(0.0, 0.0)
                        .w_full()
                        .h_full()
                        .padding(22.0)
                        .justify_end()
                        .gap(12.0)
                        .gradient(Color::BLACK.alpha(0.0), Color::BLACK.alpha(0.9))
                        .child(title),
                )
                .child(
                    toolbar_icon(kit.glyph.close, "Close overview", &artwork_kit)
                        .on_press_tx(|s: &mut State, tx| {
                            crate::motion::fold(tx);
                            s.detail_closed = true;
                        })
                        .absolute(0.0, 0.0)
                        .pin(None, Some(10.0), Some(10.0), None)
                        .fill(Color::BLACK.alpha(0.4))
                        .color(Color::WHITE),
                ),
        )
        .child(
            col()
                .w_full()
                .padding(20.0)
                .gap(12.0)
                .child(
                    label(last_played(game.last_launched), 10.0, 400.0, kit)
                        .color(kit.c.text_muted),
                )
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .child(
                            primary("Play now")
                                .icon(&icons::PLAY)
                                .disabled(game.hidden || !game.available)
                                .on_press(|s: &mut State| s.launch_selected())
                                .build(kit)
                                .grow(1.0)
                                .h(38.0)
                                .key("play-selected"),
                        )
                        .child(
                            rune_kit::IconButton::new(if game.favourite {
                                &rune_kit::glyphs::STAR_FILLED
                            } else {
                                &rune_kit::glyphs::STAR
                            })
                            .label(if game.favourite {
                                "Unfavourite"
                            } else {
                                "Favourite"
                            })
                            .on_press(|s: &mut State| s.toggle_favourite())
                            .build(kit)
                            .size(38.0, 38.0)
                            .key("favourite-selected"),
                        ),
                ),
        )
        .child(
            row()
                .w_full()
                .px(20.0)
                .gap(8.0)
                .center_y()
                .child(
                    button("Journal")
                        .icon(&icons::PEN)
                        .on_press(move |s: &mut State| s.open_journal(journal_id.clone()))
                        .build(kit)
                        .h(32.0)
                        .grow(1.0)
                        .min_w(0.0)
                        .key("journal-selected"),
                )
                .child(
                    toolbar_icon(&icons::SETTINGS, "Manage game", kit)
                        .on_press(move |s: &mut State| {
                            s.open_editor(id.clone(), EditorSection::Details)
                        })
                        .size(32.0, 32.0)
                        .fill(kit.c.bg_button_neutral)
                        .key("edit-selected"),
                )
                .child(
                    toolbar_icon(
                        kit.glyph.check,
                        if game.completed {
                            "Mark unfinished"
                        } else {
                            "Mark completed"
                        },
                        kit,
                    )
                    .size(32.0, 32.0)
                    .fill(if game.completed {
                        kit.c.text_accent.alpha(0.12)
                    } else {
                        kit.c.bg_button_neutral
                    })
                    .on_press(move |s: &mut State| {
                        if let Some(game) = s.game(&progress_id) {
                            s.submit(Command::SetGameProgress {
                                game_id: progress_id.clone(),
                                collections: game.collections.clone(),
                                completed: !game.completed,
                            });
                        }
                    })
                    .key("completed-selected"),
                ),
        )
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .padding(20.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    col()
                        .w_full()
                        .gap(16.0)
                        .child(el().w_full().h(1.0).fill(kit.c.text_base.alpha(0.08)))
                        .child(hint(
                            metadata
                                .description
                                .unwrap_or_else(|| "No description available.".into()),
                            kit,
                        ))
                        .child(
                            label(metadata.developer.unwrap_or_default(), 12.0, 550.0, kit).wrap(),
                        )
                        .child(
                            label(metadata.genres.unwrap_or_default(), 11.0, 400.0, kit)
                                .color(kit.c.text_muted)
                                .wrap(),
                        ),
                ),
        )
}

fn classic(state: &State, kit: &Kit, size: [f32; 2]) -> El {
    let immersive = immersive_recents(state);
    let enabled_widgets = state.appearance.recents_activity
        || state.appearance.recents_progress
        || state.appearance.recents_journal;
    let games = state.visible_games();
    let available = size[0]
        - 48.0
        - if state.sidebar_open() {
            state.appearance.sidebar_width.clamp(140.0, 260.0) + 24.0
        } else {
            0.0
        };
    let count = games.len().to_f32().unwrap_or(1.0).max(1.0);
    let width = ((available - (count - 1.0) * 14.0) / count).clamp(112.0, 260.0);
    let rotation_height = (width - 10.0) * 0.6 + 40.0 + 34.0 + 32.0;
    let prompt_height = if missing_artwork(state) { 76.0 } else { 0.0 };
    let hero_height = (size[1] - 96.0 - rotation_height - prompt_height).max(210.0);
    let mut artwork_kit = kit.clone();
    artwork_kit.c.text_base = Color::WHITE;
    artwork_kit.c.icon_base = Color::WHITE;
    let mut content = col().key("classic-content").w_full().gap(16.0);
    if !immersive {
        content = content.child(
            row()
                .w_full()
                .justify_between()
                .center_y()
                .child(
                    col()
                        .gap(6.0)
                        .child(eyebrow("RECENTLY PLAYED", kit))
                        .child(heading("Pick up where you left off.", 32.0, kit)),
                )
                .child(
                    label(
                        format!(
                            "{} games",
                            state
                                .snapshot
                                .games
                                .iter()
                                .filter(|game| game.available && !game.hidden)
                                .count()
                        ),
                        11.0,
                        400.0,
                        kit,
                    )
                    .opacity(0.5),
                ),
        );
    }
    if let Some(game) = state.selected_game() {
        let mut hero = el()
            .key("recents-hero")
            .w_full()
            .h(hero_height)
            .min_h(hero_height)
            .radius(14.0)
            .clip_rounded(14.0);
        if !immersive {
            hero = hero.child(
                artwork(state, game, ArtworkKind::Background, true, kit)
                    .w_full()
                    .h_full()
                    .radius(kit.radius.lg),
            );
        }
        let mut narrative = col()
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .padding(if immersive { 8.0 } else { 32.0 })
            .pb(if immersive { 16.0 } else { 32.0 })
            .justify_end()
            .gap(12.0)
            .child(hero_title(state, game, kit));
        if let Some(description) = game.effective_metadata().description {
            narrative = narrative.child(
                label(
                    description.chars().take(200).collect::<String>(),
                    12.0,
                    400.0,
                    kit,
                )
                .color(Color::WHITE.alpha(0.72))
                .max_w(490.0)
                .wrap(),
            );
        }
        if !immersive {
            narrative = narrative.gradient(Color::BLACK.alpha(0.0), Color::BLACK.alpha(0.88));
        }
        narrative = narrative.child(
            row()
                .gap(8.0)
                .child(
                    primary("Play now")
                        .icon(&icons::PLAY)
                        .on_press(|s: &mut State| s.launch_selected())
                        .build(kit)
                        .h(38.0)
                        .px(22.0)
                        .key("play-selected"),
                )
                .child(
                    toolbar_icon(
                        if game.favourite {
                            &rune_kit::glyphs::STAR_FILLED
                        } else {
                            &rune_kit::glyphs::STAR
                        },
                        if game.favourite {
                            "Unfavourite"
                        } else {
                            "Favourite"
                        },
                        &artwork_kit,
                    )
                    .size(38.0, 38.0)
                    .fill(Color::BLACK.alpha(0.35))
                    .on_press(|s: &mut State| s.toggle_favourite())
                    .key("favourite-selected"),
                ),
        );
        content = content.child(hero.child(narrative));
    }
    let mut strip = row().gap(14.0);
    for game in games {
        strip = strip.child(recent_card(state, game, kit, width));
    }
    if missing_artwork(state) {
        content = content.child(artwork_prompt(state, kit));
    }
    content = content
        .child(
            row()
                .w_full()
                .justify_between()
                .center_y()
                .child(eyebrow("ON YOUR ROTATION", kit))
                .child(
                    row()
                        .gap(8.0)
                        .center_y()
                        .child(
                            button("Widgets")
                                .variant(rune_kit::ButtonVariant::Ghost)
                                .icon(&icons::CUSTOMIZE)
                                .on_press_tx(|s: &mut State, tx| {
                                    s.show_overlay(crate::state::Overlay::Customize);
                                    tx.scroll_into_view("customization-recents-widgets");
                                })
                                .build(kit)
                                .key("recents-widgets-settings"),
                        )
                        .child(
                            button("View library →")
                                .variant(rune_kit::ButtonVariant::Ghost)
                                .on_press(|s: &mut State| s.navigate(Screen::Library))
                                .build(kit)
                                .key("view-library"),
                        ),
                ),
        )
        .child(
            col()
                .key("recents-rotation")
                .w_full()
                .scroll_x()
                .child(strip),
        );
    if enabled_widgets {
        content = col()
            .w_full()
            .gap(24.0)
            .child(content.min_h((size[1] - 96.0).max(0.0)))
            .child(widgets(state, kit));
    }
    let mut viewport = col().key("classic-recents").w_full().h_full().min_h(0.0);
    if enabled_widgets {
        viewport = viewport.scroll(ScrollSpec {
            stick_bottom: false,
        });
    }
    viewport.child(content)
}

fn recent_card(state: &State, game: &Game, kit: &Kit, width: f32) -> El {
    let selected = state.selected.as_ref() == Some(&game.id);
    let id = game.id.clone();
    let launch = game.id.clone();
    let context = game.id.clone();
    col()
        .key(format!("game-{}", game.id.0))
        .w(width)
        .padding(5.0)
        .gap(7.0)
        .pb(8.0)
        .role("button")
        .label(game.title())
        .focusable(true)
        .focus_selected(selected)
        .cursor(rune::Cursor::Pointer)
        .radius(8.0)
        .fill(if selected {
            kit.c.text_base.alpha(0.08)
        } else {
            Color::TRANSPARENT
        })
        .hover(kit.c.hover)
        .focus_ring(2.0, 2.0, kit.c.border_focus)
        .on_press(move |s: &mut State| s.select(id.clone()))
        .on_double_press(move |s: &mut State| s.launch(launch.clone()))
        .on_secondary_press(move |s: &mut State, point, _| {
            s.context = Some((context.clone(), point))
        })
        .child(
            artwork(state, game, ArtworkKind::Background, false, kit)
                .w_full()
                .h((width - 10.0) * 0.6)
                .radius(8.0)
                .clip_rounded(8.0)
                .border(
                    if selected { 2.0 } else { 0.5 },
                    if selected {
                        kit.c.text_accent
                    } else {
                        kit.c.text_base.alpha(0.1)
                    },
                ),
        )
        .child(label(game.title(), 12.0, 550.0, kit).w_full().truncate())
}

fn hero_title(state: &State, game: &Game, kit: &Kit) -> El {
    if let Some(logo) = game
        .artwork
        .get(&ArtworkKind::Logo)
        .and_then(|asset| state.images.get(&asset.image_path))
        .filter(|image| image.is_ready() && image.error().is_none())
    {
        return el()
            .w(360.0)
            .max_w(500.0)
            .h(88.0)
            .pixels_fit(logo.pixels(), SurfaceFit::Fit)
            .surface_anchor(rune::Align::Start, rune::Align::Center)
            .label(game.title());
    }
    label(game.title(), 38.0, 600.0, kit)
        .color(Color::WHITE)
        .wrap()
        .max_w(700.0)
}

fn missing_artwork(state: &State) -> bool {
    state.snapshot.games.iter().any(|game| {
        game.available && !game.hidden && !game.artwork.contains_key(&ArtworkKind::Cover)
    })
}

pub fn artwork_prompt(state: &State, kit: &Kit) -> El {
    let missing = state
        .snapshot
        .games
        .iter()
        .filter(|game| {
            game.available && !game.hidden && !game.artwork.contains_key(&ArtworkKind::Cover)
        })
        .count();
    if missing == 0 {
        return el();
    }
    let status = state.snapshot.settings.credential_status.get("steamgriddb");
    let connected = state.artwork_connected();
    let title = if connected {
        format!("{missing} games are missing artwork")
    } else {
        "SteamGridDB artwork".into()
    };
    let action = if connected {
        "Download artwork"
    } else if matches!(
        status,
        Some(CredentialStatus::Stored | CredentialStatus::Locked)
    ) {
        "Verify / unlock SteamGridDB"
    } else {
        "Connect SteamGridDB"
    };
    row()
        .key("artwork-connection")
        .w_full()
        .gap(18.0)
        .center_y()
        .justify_between()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(label(title, 13.0, 500.0, kit))
                .child(
                    hint(
                        if connected {
                            "Covers, backgrounds and logos from SteamGridDB"
                        } else {
                            "Add artwork to your library"
                        },
                        kit,
                    )
                    .opacity(0.65),
                ),
        )
        .child(
            button(action)
                .disabled(connected && state.busy())
                .on_press(|s: &mut State| s.fetch_artwork())
                .build(kit)
                .key("artwork-setup"),
        )
}

pub fn widgets(state: &State, kit: &Kit) -> El {
    crate::widgets::view(state, kit)
}

pub fn activity(state: &State, kit: &Kit) -> El {
    let mut content = col()
        .w_full()
        .gap(24.0)
        .child(eyebrow("LAUNCH HISTORY", kit));
    if state.activity_date.is_some() {
        content = content.child(
            button("Show all activity")
                .on_press(|s: &mut State| s.activity_date = None)
                .build(kit),
        );
    }
    for day in state.snapshot.activity.iter().rev().filter(|day| {
        state
            .activity_date
            .as_ref()
            .is_none_or(|date| date == &day.date)
    }) {
        let mut entries = section(
            &format!(
                "{} · {} {}",
                day.date,
                day.launches,
                if day.launches == 1 {
                    "launch"
                } else {
                    "launches"
                }
            ),
            kit,
        );
        for (id, count) in &day.games {
            let game = state.game(id);
            let title = game.map(Game::title).unwrap_or("Removed game");
            let id = id.clone();
            let journal = id.clone();
            entries = entries.child(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(
                        button(title)
                            .disabled(game.is_none())
                            .on_press(move |s: &mut State| {
                                s.navigate(Screen::Library);
                                s.select(id.clone());
                            })
                            .build(kit),
                    )
                    .child(hint(count.to_string(), kit))
                    .child(
                        button("Journal…")
                            .disabled(game.is_none())
                            .on_press(move |s: &mut State| {
                                s.open_editor(journal.clone(), EditorSection::Journal)
                            })
                            .build(kit),
                    ),
            );
        }
        content = content.child(entries);
    }
    if state.snapshot.activity.is_empty() {
        content = content.child(hint("No recorded launches yet.", kit));
    } else if state
        .activity_date
        .as_ref()
        .is_some_and(|date| !state.snapshot.activity.iter().any(|day| &day.date == date))
    {
        content = content.child(hint("No launches recorded on this day.", kit));
    }
    content
}

fn last_played(timestamp: Option<u64>) -> String {
    let Some(timestamp) = timestamp.filter(|timestamp| *timestamp > 0) else {
        return String::new();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_secs());
    let days = now.saturating_sub(timestamp) / 86_400;
    match days {
        0 => "Played today".into(),
        1 => "Played yesterday".into(),
        2..=30 => format!("Played {days} days ago"),
        _ => "Played earlier".into(),
    }
}
