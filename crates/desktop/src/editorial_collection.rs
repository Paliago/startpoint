//! Geometry and native vector cut-outs for the editorial Recents collection.
//! The original artwork is never modified; curves are painted at device resolution.
use gpui::{prelude::*, *};
use startpoint_sdk::GameId;
use std::time::{Duration, Instant};

pub(crate) const CAPTION_HEIGHT: f32 = 32.;
pub(crate) const INTRO_HEIGHT: f32 = 116.;
const MOTION: Duration = Duration::from_millis(240);

pub(crate) fn serif() -> &'static str {
    crate::design::EDITORIAL.display_font
}

pub(crate) fn tracked(text: &str) -> String {
    // GPUI has no tracking property. Use ordinary spaces supported by both
    // system serifs; thin-space glyphs are absent in some Windows font runs.
    // Preserve non-ASCII shaping, combining marks and emoji sequences intact.
    if !text.is_ascii() {
        return text.to_owned();
    }
    text.chars()
        .map(|ch| ch.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    pub width: f32,
    pub height: f32,
    pub top: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Row {
    pub cards: [Placement; 2],
    pub height: f32,
    pub gap: f32,
}

pub(crate) fn margin(width: f32) -> f32 {
    (width * 0.022).clamp(20., 44.)
}

pub(crate) fn row(width: f32, index: usize, single: bool) -> Row {
    let gap = (width * 0.022).clamp(22., 40.);
    let available = (width - gap).max(1.);
    let (ratio, left_aspect, right_aspect, left_offset, right_offset) = match index % 4 {
        0 => (0.665, 1.98, 1.45, 0., 0.073),
        1 => (0.445, 1.86, 2.28, 0.008, 0.),
        2 => (0.565, 2.10, 1.66, 0., 0.047),
        _ => (0.365, 1.56, 2.16, 0.055, 0.),
    };
    let left_width = if single {
        width * 0.76
    } else {
        available * ratio
    };
    let cards = [
        Placement {
            width: left_width,
            height: left_width / left_aspect,
            top: width * left_offset,
        },
        Placement {
            width: available * (1. - ratio),
            height: available * (1. - ratio) / right_aspect,
            top: width * right_offset,
        },
    ];
    let bottom = cards[0].top + cards[0].height;
    let bottom = if single {
        bottom
    } else {
        bottom.max(cards[1].top + cards[1].height)
    };
    Row {
        cards,
        height: bottom + CAPTION_HEIGHT + gap * 0.5,
        gap,
    }
}

/// Interruption-safe hover easing. Only currently moving cards are retained.
#[derive(Default)]
pub(crate) struct HoverMotion {
    target: Option<GameId>,
    from: Vec<(GameId, f32)>,
    started: Option<Instant>,
}

/// Local animation time, frozen only when Recents is not visible or motion is disabled.
/// Resuming never jumps to a new wall-clock phase.
#[derive(Default)]
pub(crate) struct FlowClock {
    elapsed: Duration,
    tick: Option<Instant>,
}

impl FlowClock {
    pub fn update(&mut self, running: bool) {
        self.update_at(running, Instant::now());
    }

    fn update_at(&mut self, running: bool, now: Instant) {
        if let Some(tick) = self.tick {
            self.elapsed += now
                .saturating_duration_since(tick)
                .min(Duration::from_millis(100));
        }
        self.tick = running.then_some(now);
    }

    pub fn seconds(&self) -> f32 {
        self.elapsed.as_secs_f32()
    }
}

impl HoverMotion {
    fn progress(&self) -> f32 {
        let t = self.started.map_or(1., |at| {
            (at.elapsed().as_secs_f32() / MOTION.as_secs_f32()).min(1.)
        });
        1. - (1. - t).powi(3)
    }

    pub fn value(&self, id: &GameId, reduced: bool) -> f32 {
        let to = if self.target.as_ref() == Some(id) {
            1.
        } else {
            0.
        };
        if reduced {
            return to;
        }
        let from = self
            .from
            .iter()
            .find(|(key, _)| key == id)
            .map_or(0., |(_, value)| *value);
        from + (to - from) * self.progress()
    }

    pub fn set(&mut self, id: &GameId, hovered: bool) {
        if !hovered && self.target.as_ref() != Some(id) {
            return;
        }
        let target = hovered.then(|| id.clone());
        if target == self.target {
            return;
        }
        let mut values = self
            .from
            .iter()
            .map(|(id, _)| (id.clone(), self.value(id, false)))
            .filter(|(_, value)| *value > 0.001)
            .collect::<Vec<_>>();
        for id in self.target.iter().chain(target.iter()) {
            if !values.iter().any(|(key, _)| key == id) {
                values.push((id.clone(), self.value(id, false)));
            }
        }
        self.from = values;
        self.target = target;
        self.started = Some(Instant::now());
    }

    pub fn animating(&self) -> bool {
        self.started.is_some_and(|at| at.elapsed() < MOTION)
    }
}

fn flowing_point(x: f32, y: f32, variant: usize, seconds: Option<f32>, hover: f32) -> (f32, f32) {
    let Some(seconds) = seconds else {
        return (x, y);
    };
    let phase = variant as f32 * 0.93;
    let strength = 1. + hover.clamp(0., 1.) * 0.15;
    let dx = ((seconds * 0.47 + phase + y * 2.8).sin() * 0.010
        + (seconds * 0.29 + phase * 1.7 + x * 3.1).sin() * 0.005)
        * strength;
    let dy = ((seconds * 0.41 + phase + x * 3.5).cos() * 0.018
        + (seconds * 0.27 + phase * 0.7 + y * 2.4).sin() * 0.006)
        * strength;
    ((x + dx).clamp(0.002, 0.998), (y + dy).clamp(0.002, 0.998))
}

fn outline(
    builder: &mut PathBuilder,
    bounds: Bounds<Pixels>,
    variant: usize,
    seconds: Option<f32>,
    hover: f32,
) {
    // Each window has its own gently lopsided contour, not a rounded rectangle.
    let p = |x: f32, y: f32| {
        let (x, y) = flowing_point(x, y, variant, seconds, hover);
        let (x, y) = match variant % 4 {
            1 => (1. - x, y),
            2 => (x, 1. - y),
            3 => (1. - x, 1. - y),
            _ => (x, y),
        };
        point(
            bounds.left() + bounds.size.width * x,
            bounds.top() + bounds.size.height * y,
        )
    };
    builder.move_to(p(0.008, 0.61));
    builder.cubic_bezier_to(p(0.25, 0.075), p(0.018, 0.26), p(0.075, 0.13));
    builder.cubic_bezier_to(p(0.92, 0.025), p(0.47, 0.005), p(0.82, -0.005));
    builder.cubic_bezier_to(p(0.994, 0.33), p(0.986, 0.05), p(0.997, 0.11));
    builder.cubic_bezier_to(p(0.955, 0.91), p(0.996, 0.55), p(0.984, 0.82));
    builder.cubic_bezier_to(p(0.76, 0.99), p(0.934, 0.986), p(0.90, 0.995));
    builder.cubic_bezier_to(p(0.095, 0.955), p(0.54, 0.982), p(0.20, 0.995));
    builder.cubic_bezier_to(p(0.008, 0.61), p(0.016, 0.94), p(0.0, 0.80));
    builder.close();
}

pub(crate) fn cutout(
    variant: usize,
    seconds: Option<f32>,
    hover: f32,
    canvas_color: Hsla,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let mut mask = PathBuilder::fill().with_style(PathStyle::Fill(
                FillOptions::default().with_fill_rule(FillRule::EvenOdd),
            ));
            mask.add_polygon(
                &[
                    bounds.origin,
                    bounds.top_right(),
                    bounds.bottom_right(),
                    bounds.bottom_left(),
                ],
                true,
            );
            outline(&mut mask, bounds, variant, seconds, hover);
            if let Ok(path) = mask.build() {
                window.paint_path(path, canvas_color.alpha(1.));
            }
        },
    )
    .absolute()
    .inset_0()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn editorial_tracking_uses_supported_spaces_without_breaking_complex_titles() {
        assert_eq!(tracked("Hades II"), "H a d e s   I I");
        assert_eq!(tracked("Café — 世界"), "Café — 世界");
        assert_eq!(tracked("e\u{301}"), "e\u{301}");
    }

    #[::core::prelude::v1::test]
    fn rows_fit_and_leave_room_for_captions_at_all_desktop_sizes() {
        for width in [560., 760., 860., 960., 1280., 1586., 1920., 2560.] {
            let width = width - margin(width) * 2.;
            for index in 0..8 {
                let row = row(width, index, false);
                assert!((row.cards[0].width + row.gap + row.cards[1].width - width).abs() < 0.01);
                for card in row.cards {
                    assert!(card.width > 0. && card.height > 0.);
                    assert!(card.top + card.height + CAPTION_HEIGHT < row.height);
                }
            }
            let last = row(width, 2, true);
            assert!(last.cards[0].width < width);
            assert!(last.cards[0].height + CAPTION_HEIGHT < last.height);
        }
    }

    #[::core::prelude::v1::test]
    fn vector_masks_tessellate_in_every_orientation() {
        for variant in 0..4 {
            let bounds = Bounds::new(point(px(0.), px(0.)), size(px(800.), px(400.)));
            let mut path = PathBuilder::fill();
            outline(&mut path, bounds, variant, None, 0.);
            assert!(path.build().is_ok());
        }
    }

    #[::core::prelude::v1::test]
    fn stale_hover_leave_does_not_clear_the_new_hover() {
        let mut motion = HoverMotion::default();
        let a = GameId("a".into());
        let b = GameId("b".into());
        motion.set(&a, true);
        motion.set(&b, true);
        motion.set(&a, false);
        assert_eq!(motion.value(&b, true), 1.);
        assert_eq!(motion.value(&a, true), 0.);
        motion.set(&b, false);
        assert_eq!(motion.value(&b, true), 0.);
    }

    #[::core::prelude::v1::test]
    fn flow_freezes_and_resumes_without_skipping_to_a_wall_clock_phase() {
        let mut clock = FlowClock::default();
        let start = Instant::now();
        clock.update_at(true, start);
        clock.update_at(false, start + Duration::from_millis(50));
        let paused = clock.seconds();
        assert!(paused > 0.);
        clock.update_at(false, start + Duration::from_secs(20));
        clock.update_at(true, start + Duration::from_secs(30));
        assert_eq!(clock.seconds(), paused);
        clock.update_at(true, start + Duration::from_millis(30_050));
        assert!((clock.seconds() - paused - 0.05).abs() < 0.0001);
    }

    #[::core::prelude::v1::test]
    fn living_curves_are_continuous_bounded_and_have_a_static_reduced_motion_path() {
        for variant in 0..5 {
            for seconds in [0., 3., 8., 16., 28.] {
                let bounds = Bounds::new(point(px(0.), px(0.)), size(px(800.), px(400.)));
                let mut path = PathBuilder::fill();
                outline(&mut path, bounds, variant, Some(seconds), 1.);
                assert!(path.build().is_ok());
                let a = flowing_point(0.25, 0.075, variant, Some(seconds), 0.);
                let b = flowing_point(0.25, 0.075, variant, Some(seconds + 0.001), 0.);
                assert!((a.0 - b.0).abs() < 0.0001 && (a.1 - b.1).abs() < 0.0001);
                assert!((0.002..=0.998).contains(&a.0) && (0.002..=0.998).contains(&a.1));
            }
        }
        assert_ne!(
            flowing_point(0.25, 0.075, 0, Some(0.), 0.),
            flowing_point(0.25, 0.075, 0, Some(8.), 0.)
        );
        assert_eq!(flowing_point(0.25, 0.075, 0, None, 1.), (0.25, 0.075));
    }
}
