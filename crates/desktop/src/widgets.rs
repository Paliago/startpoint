use crate::{
    components::*,
    state::{LibraryFilter, Screen, State},
};
use num_traits::ToPrimitive;
use rune::{El, col, el, row};
use rune_kit::{ButtonVariant, Kit};

fn panel(title: &str, kit: &Kit) -> El {
    col()
        .grow(1.0)
        .min_w(0.0)
        .h(220.0)
        .padding(18.0)
        .gap(10.0)
        .radius(12.0)
        .fill(kit.c.page.alpha(0.78))
        .border(0.5, kit.c.text_base.alpha(0.12))
        .backdrop_blur(12.0)
        .child(eyebrow(title, kit))
}

pub fn view(state: &State, kit: &Kit) -> El {
    let mut widgets = row()
        .key("recents-widgets")
        .role("region")
        .label("Recents widgets")
        .w_full()
        .gap(14.0)
        .items_start();
    if state.appearance.recents_activity {
        let mut days: Vec<_> = state.snapshot.activity.iter().rev().take(7).collect();
        days.reverse();
        let launches: u64 = days.iter().map(|day| u64::from(day.launches)).sum();
        let maximum = days
            .iter()
            .map(|day| day.launches)
            .max()
            .unwrap_or(1)
            .max(1)
            .to_f32()
            .unwrap_or(1.0);
        let mut bars = row().w_full().h(56.0).gap(8.0).items_end();
        for day in &days {
            let date = day.date.clone();
            let height = 5.0 + day.launches.to_f32().unwrap_or(0.0) / maximum * 31.0;
            bars = bars.child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .h_full()
                    .justify_end()
                    .gap(6.0)
                    .role("button")
                    .label(format!("{}: {} launches", day.date, day.launches))
                    .focusable(true)
                    .cursor(rune::Cursor::Pointer)
                    .child(
                        el().w_full()
                            .h(height)
                            .radius(3.0)
                            .fill(kit.c.text_accent.alpha(0.7)),
                    )
                    .child(
                        label(
                            day.date.rsplit('-').next().unwrap_or(&day.date),
                            10.0,
                            450.0,
                            kit,
                        )
                        .color(kit.c.text_muted)
                        .self_center(),
                    )
                    .on_press(move |s: &mut State| s.open_activity_date(date.clone())),
            );
        }
        if days.is_empty() {
            bars = bars.child(hint("Your activity will appear here after you play.", kit));
        }
        widgets = widgets.child(
            panel("ACTIVITY", kit)
                .key("recents-activity-widget")
                .child(
                    row()
                        .center_y()
                        .gap(8.0)
                        .child(label(launches.to_string(), 28.0, 600.0, kit))
                        .child(label("launches", 11.0, 400.0, kit).color(kit.c.text_muted)),
                )
                .child(bars)
                .child(el().grow(1.0))
                .child(
                    button("View activity →")
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut State| s.navigate(Screen::Activity))
                        .build(kit)
                        .key("recents-activity-open")
                        .self_start(),
                ),
        );
    }
    if state.appearance.recents_progress {
        let games: Vec<_> = state
            .snapshot
            .games
            .iter()
            .filter(|game| game.available && !game.hidden)
            .collect();
        let completed = games.iter().filter(|game| game.completed).count();
        let fraction =
            completed.to_f32().unwrap_or(0.0) / games.len().max(1).to_f32().unwrap_or(1.0);
        widgets = widgets.child(
            panel("COLLECTION PROGRESS", kit)
                .key("recents-progress-widget")
                .child(
                    row()
                        .center_y()
                        .gap(6.0)
                        .child(label(completed.to_string(), 28.0, 600.0, kit))
                        .child(
                            label(format!("/ {}", games.len()), 16.0, 450.0, kit)
                                .color(kit.c.text_muted),
                        ),
                )
                .child(
                    el().w_full()
                        .h(5.0)
                        .radius(2.5)
                        .fill(kit.c.text_base.alpha(0.1))
                        .child(
                            el().w_frac(fraction)
                                .h_full()
                                .radius(2.5)
                                .fill(kit.c.text_accent),
                        ),
                )
                .child(hint("Games completed in your collection", kit))
                .child(el().grow(1.0))
                .child(
                    button("View completed →")
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut State| s.set_filter(LibraryFilter::Completed))
                        .build(kit)
                        .key("recents-progress-open")
                        .self_start(),
                ),
        );
    }
    if state.appearance.recents_journal {
        let latest = state
            .snapshot
            .journal
            .iter()
            .min_by_key(|entry| std::cmp::Reverse(entry.created_at));
        let game = latest
            .and_then(|entry| state.game(&entry.game_id))
            .or_else(|| state.selected_game());
        let game_id = game.map(|game| game.id.clone());
        let text = latest.map(|entry| entry.text.clone()).unwrap_or_default();
        widgets = widgets.child(
            panel("JOURNAL", kit)
                .key("recents-journal-widget")
                .child(
                    label(
                        game.map_or("Your next session", |game| game.title()),
                        14.0,
                        550.0,
                        kit,
                    )
                    .truncate(),
                )
                .child(
                    label(
                        if text.is_empty() {
                            "Keep a note, a discovery, or a reminder for next time.".into()
                        } else {
                            text.chars().take(140).collect::<String>()
                        },
                        12.0,
                        400.0,
                        kit,
                    )
                    .color(kit.c.text_muted)
                    .wrap()
                    .max_h(72.0)
                    .clip_rounded(0.0),
                )
                .child(el().grow(1.0))
                .child(
                    button(if latest.is_some() {
                        "Open journal →"
                    } else {
                        "Write a note →"
                    })
                    .variant(ButtonVariant::Ghost)
                    .disabled(game_id.is_none())
                    .on_press(move |s: &mut State| {
                        if let Some(id) = &game_id {
                            s.open_journal(id.clone());
                        }
                    })
                    .build(kit)
                    .key("recents-journal-open")
                    .self_start(),
                ),
        );
    }
    widgets
}
