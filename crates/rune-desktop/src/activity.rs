use crate::{
    components::*,
    state::{Screen, State},
};
use chrono::{Datelike, Duration, Utc};
use num_traits::ToPrimitive;
use rune::{El, col, el, row};
use rune_kit::Kit;
use std::collections::BTreeMap;

pub fn view(state: &State, kit: &Kit) -> El {
    let mut totals = BTreeMap::new();
    let mut dates = BTreeMap::new();
    let mut launches = 0_u64;
    for day in state.snapshot.activity.iter() {
        launches += u64::from(day.launches);
        dates.insert(day.date.clone(), day.launches);
        for (id, count) in &day.games {
            *totals.entry(id.clone()).or_insert(0_u64) += u64::from(*count);
        }
    }
    let mut summary = row().w_full().gap(32.0);
    for (title, value) in [
        ("RECORDED LAUNCHES", launches),
        ("GAMES PLAYED", totals.len().to_u64().unwrap_or(0)),
        ("ACTIVE DAYS", dates.len().to_u64().unwrap_or(0)),
    ] {
        summary = summary.child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(8.0)
                .child(eyebrow(title, kit))
                .child(label(value.to_string(), 34.0, 600.0, kit)),
        );
    }
    let today = Utc::now().date_naive();
    let end = today + Duration::days(6 - i64::from(today.weekday().num_days_from_monday()));
    let start = end - Duration::days(181);
    let maximum = dates
        .values()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1)
        .to_f32()
        .unwrap_or(1.0);
    let mut weeks = row().gap(4.0);
    for week in 0..26 {
        let mut days = col().gap(4.0);
        for offset in 0..7 {
            let date = start + Duration::days(i64::from(week * 7 + offset));
            let key = date.format("%Y-%m-%d").to_string();
            let count = dates.get(&key).copied().unwrap_or(0);
            let future = date > today;
            let fraction = count.to_f32().unwrap_or(0.0) / maximum;
            let selected = state.activity_date.as_deref() == Some(key.as_str());
            let mut cell = el()
                .key(format!("activity-date-{key}"))
                .size(14.0, 14.0)
                .radius(3.0)
                .fill(if count == 0 {
                    kit.c.text_base.alpha(if future { 0.025 } else { 0.08 })
                } else {
                    kit.c.text_accent.alpha(0.25 + fraction.sqrt() * 0.75)
                })
                .border(if selected { 1.5 } else { 0.0 }, kit.c.border_focus);
            if !future {
                cell = cell
                    .role("button")
                    .label(format!("{key}: {count} launches"))
                    .help(format!("{key} · {count} launches"))
                    .focusable(true)
                    .cursor(rune::Cursor::Pointer)
                    .on_press(move |s: &mut State| s.open_activity_date(key.clone()));
            }
            days = days.child(cell);
        }
        weeks = weeks.child(days);
    }
    let mut ranked: Vec<_> = totals.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let maximum = ranked
        .first()
        .map_or(1, |(_, count)| *count)
        .max(1)
        .to_f32()
        .unwrap_or(1.0);
    let mut games = col()
        .w_full()
        .gap(16.0)
        .child(eyebrow("MOST LAUNCHED", kit));
    for (id, count) in ranked.iter().take(5) {
        let selected = id.clone();
        let title = state.game(id).map_or("Removed game", |game| game.title());
        let fraction = count.to_f32().unwrap_or(0.0) / maximum;
        games = games.child(
            col()
                .key(format!("activity-game-{}", id.0))
                .w_full()
                .gap(8.0)
                .role("button")
                .label(title)
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .child(
                    row()
                        .w_full()
                        .justify_between()
                        .gap(16.0)
                        .child(label(title, 13.0, 500.0, kit).truncate().min_w(0.0))
                        .child(
                            label(format!("{count} launches"), 11.0, 400.0, kit)
                                .color(kit.c.text_muted),
                        ),
                )
                .child(
                    el().w_full()
                        .h(6.0)
                        .radius(3.0)
                        .fill(kit.c.text_base.alpha(0.06))
                        .child(
                            el().w_frac(fraction)
                                .h_full()
                                .radius(3.0)
                                .fill(kit.c.text_accent.alpha(0.75)),
                        ),
                )
                .on_press_tx(move |s: &mut State, tx| {
                    if s.game(&selected).is_some() {
                        tx.instant();
                        s.navigate(Screen::Library);
                        s.select(selected.clone());
                        s.reveal_game(&selected, tx);
                    }
                }),
        );
    }
    if ranked.is_empty() {
        games = games.child(hint("Launch a game from Startpoint to begin building your activity history. Existing last-played dates are not counted as recorded sessions.", kit));
    }
    col()
        .key("activity-dashboard")
        .w_full()
        .gap(30.0)
        .child(heading("Activity", 32.0, kit))
        .child(summary)
        .child(
            col()
                .w_full()
                .gap(14.0)
                .child(
                    row()
                        .w_full()
                        .justify_between()
                        .child(eyebrow("LAUNCH CALENDAR", kit))
                        .child(
                            label("Last 26 weeks · UTC", 11.0, 400.0, kit).color(kit.c.text_muted),
                        ),
                )
                .child(col().w_full().scroll_x().child(weeks))
                .child(hint(
                    "Each square is one day. Select a day to inspect its launches below.",
                    kit,
                )),
        )
        .child(games)
        .child(crate::library::activity(state, kit))
}
