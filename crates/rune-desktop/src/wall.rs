use crate::{
    components::*,
    library,
    state::{Overlay, State},
};
use num_traits::ToPrimitive;
use rune::{El, GrowthSpec, ScrollSpec, col, el, row};
use rune_kit::Kit;
use startpoint_sdk::ArtworkKind;
use std::sync::atomic::Ordering;

pub fn view(state: &State, kit: &Kit, width: f32) -> El {
    let available = width
        - if state.sidebar_open() {
            state.appearance.sidebar_width.clamp(140.0, 260.0) + 24.0
        } else {
            0.0
        };
    let mut columns = 2;
    let mut column_count = 2.0;
    while (column_count + 1.0) * 170.0 <= available - 10.0 {
        columns += 1;
        column_count += 1.0;
    }
    let cover_width = ((available - 10.0 * (column_count + 1.0)) / column_count).max(1.0);
    let height = cover_width * 1.5;
    let count = state.visible_games().len().div_ceil(columns);
    let palette = kit.clone();
    let start = state.wall_start.clone();
    col()
        .key("infinite-wall")
        .w_full()
        .h_full()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .on_scroll_offset(move |position| {
            let row = (position.offset[1].max(0.0) / (height + 10.0))
                .floor()
                .to_usize()
                .unwrap_or(0);
            start.store(row.saturating_mul(columns), Ordering::Relaxed);
        })
        .child(
            col()
                .key("wall-rows")
                .w_full()
                .growth(GrowthSpec::default())
                .virtual_rows(
                    count,
                    height + 10.0,
                    |_: &State, index| format!("wall-row-{index}").into(),
                    move |state: &State, index| {
                        let games = state.visible_games();
                        let mut line = row().w_full().gap(10.0).px(10.0).pb(10.0);
                        for game in games.iter().skip(index * columns).take(columns) {
                            let selected = game.id.clone();
                            let launch = game.id.clone();
                            let hover_id = game.id.clone();
                            let hovered = state.wall_hover.as_ref() == Some(&game.id);
                            let emphasized = hovered && !state.reduced_motion;
                            let mut cover = el()
                                .key(format!("wall-tile-{}", game.id.0))
                                .size(cover_width, height)
                                .radius(6.0)
                                .clip_rounded(6.0)
                                .hover_group()
                                .offset(0.0, if emphasized { -6.0 } else { 0.0 })
                                .scale(if emphasized { 1.06 } else { 1.0 })
                                .hover_delays(
                                    std::time::Duration::from_millis(100),
                                    std::time::Duration::ZERO,
                                )
                                .on_hover_change(move |state: &mut State, inside, tx| {
                                    crate::motion::fold(tx);
                                    if inside {
                                        state.wall_hover = Some(hover_id.clone());
                                    } else if state.wall_hover.as_ref() == Some(&hover_id) {
                                        state.wall_hover = None;
                                    }
                                })
                                .cursor(rune::Cursor::Pointer)
                                .hover(palette.c.hover)
                                .child(
                                    library::artwork(
                                        state,
                                        game,
                                        ArtworkKind::Cover,
                                        false,
                                        &palette,
                                    )
                                    .w_full()
                                    .h_full()
                                    .inert(),
                                )
                                .child(
                                    el().key(format!("wall-cover-{}", game.id.0))
                                        .absolute(0.0, if index == 0 { 64.0 } else { 0.0 })
                                        .w_full()
                                        .h(height - if index == 0 { 64.0 } else { 0.0 })
                                        .role("button")
                                        .label(game.title())
                                        .focusable(true)
                                        .on_press(move |state: &mut State| {
                                            state.wall_focus = Some(selected.clone());
                                            state.show_overlay(Overlay::WallFocus);
                                        }),
                                )
                                .child(
                                    el().absolute(0.0, 0.0)
                                        .w_full()
                                        .h_full()
                                        .fill(rune::Color::BLACK)
                                        .opacity(if state.wall_hover.is_some() && !hovered {
                                            0.2
                                        } else {
                                            0.0
                                        })
                                        .inert(),
                                )
                                .child(
                                    button("Play")
                                        .on_press(move |state: &mut State| {
                                            state.launch(launch.clone())
                                        })
                                        .build(&palette)
                                        .absolute(10.0, height - 44.0)
                                        .group_opacity(0.0, 1.0),
                                );
                            if hovered {
                                cover = cover.raise();
                            }
                            line = line.child(cover);
                        }
                        line
                    },
                ),
        )
}

pub fn focus(state: &State, kit: &Kit) -> El {
    let game = state
        .wall_focus
        .as_ref()
        .and_then(|id| state.snapshot.games.iter().find(|game| &game.id == id));
    if let Some(game) = game {
        let launch = game.id.clone();
        col()
            .gap(16.0)
            .center()
            .child(
                library::artwork(state, game, ArtworkKind::Cover, true, kit)
                    .size(290.0, 435.0)
                    .clip_rounded(10.0),
            )
            .child(heading(game.title(), 24.0, kit).wrap())
            .child(
                primary("Play")
                    .on_press(move |state: &mut State| state.launch(launch.clone()))
                    .build(kit),
            )
    } else {
        hint("Artwork is unavailable.", kit)
    }
}
