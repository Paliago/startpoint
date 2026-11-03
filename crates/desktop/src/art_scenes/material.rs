//! Live, bounded native vector materials. No per-frame image generation/readback.
use gpui::{prelude::*, *};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BookPalette {
    pub paper: Hsla,
    pub ink: Hsla,
    pub accent: Hsla,
}

#[derive(Default)]
pub(crate) struct Chrome(pub Option<BookPalette>);
impl Global for Chrome {}

pub(crate) fn chrome(cx: &App) -> Option<BookPalette> {
    cx.try_global::<Chrome>().and_then(|chrome| chrome.0)
}

pub(crate) fn book_palette(accent: [f32; 3], luminance: f32) -> BookPalette {
    let color: Hsla = Rgba {
        r: accent[0],
        g: accent[1],
        b: accent[2],
        a: 1.,
    }
    .into();
    let dark = luminance < 0.22;
    let mut palette = BookPalette {
        paper: hsla(
            color.h,
            color.s.clamp(0.12, 0.28),
            if dark { 0.115 } else { 0.93 },
            1.,
        ),
        ink: hsla(color.h, 0.14, if dark { 0.91 } else { 0.13 }, 1.),
        accent: hsla(
            color.h,
            color.s.clamp(0.25, 0.55),
            if dark { 0.73 } else { 0.30 },
            1.,
        ),
    };
    // Yellow/green hues can be much brighter than blue at the same HSL
    // lightness. Keep artwork-derived actions readable for every hue.
    for _ in 0..40 {
        if crate::theme::contrast(palette.paper, palette.accent) >= 4.5 {
            break;
        }
        palette.accent.l = (palette.accent.l + if dark { 0.01 } else { -0.01 }).clamp(0., 1.);
    }
    palette
}

/// The seam changes shape locally along its length, not just position. The
/// image/crop and paper grain remain stationary beneath this feathered mask.
pub(crate) fn tear_x(y: f32, seconds: f32) -> f32 {
    let phase = seconds * 0.19;
    0.615
        + (y * 9. + phase).sin() * 0.024
        + (y * 29. - phase * 0.73).sin() * 0.009
        + (y * 113. + phase * 1.31).sin() * 0.0025
}

fn tear_path(bounds: Bounds<Pixels>, seconds: f32, offset: f32) -> Path<Pixels> {
    let mut path = PathBuilder::fill();
    for i in 0..=80 {
        let y = i as f32 / 80.;
        let p = point(
            bounds.left() + bounds.size.width * (tear_x(y, seconds) + offset),
            bounds.top() + bounds.size.height * y,
        );
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    path.line_to(bounds.bottom_right());
    path.line_to(bounds.top_right());
    path.close();
    path.build().expect("monotonic torn edge")
}

pub(crate) fn book_tear(seconds: f32, paper: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let mut previous = 0.;
            // Incremental coverage reproduces a smoothstep feather instead of
            // stacking the same semi-transparent silhouette into a hard band.
            for i in 1..=20 {
                let t = i as f32 / 20.;
                let coverage = t * t * (3. - 2. * t);
                let alpha = (coverage - previous) / (1. - previous).max(0.00001);
                window.paint_path(
                    tear_path(bounds, seconds, (t - 0.5) * 0.075),
                    paper.alpha(alpha),
                );
                previous = coverage;
            }
        },
    )
    .absolute()
    .inset_0()
}

fn marble_point(radius: f32, angle: f32, seconds: f32) -> (f32, f32) {
    let phase = seconds * 0.045;
    let warp = 1.
        + 0.17 * (angle * 3. + phase + radius * 3.).sin()
        + 0.075 * (angle * 5. - phase * 0.7 + radius * 7.).cos();
    let twist = angle + (1.4 - radius).powi(2) * 2.3 + phase * 0.23;
    (
        0.5 + twist.cos() * radius * warp,
        0.5 + twist.sin() * radius * warp,
    )
}

fn marble_path(bounds: Bounds<Pixels>, radius: f32, seconds: f32) -> Path<Pixels> {
    let mut path = PathBuilder::fill();
    for i in 0..=96 {
        let angle = i as f32 / 96. * std::f32::consts::TAU;
        let (x, y) = marble_point(radius, angle, seconds);
        let p = point(
            bounds.left() + bounds.size.width * x,
            bounds.top() + bounds.size.height * y,
        );
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    path.close();
    path.build().expect("radial marble contour")
}

/// Balatro-inspired liquid contour bands, deliberately slow and subdued.
/// Thirty-two small native paths use native GPU color painting;
/// there is no new rendering worker or image upload every atmospheric tick.
pub(crate) fn reel_marble(seconds: f32, accent: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            for i in (0..32).rev() {
                let r = 0.025 + i as f32 * 0.047;
                let hue = if i % 5 < 3 {
                    accent.h
                } else {
                    (accent.h + 0.48) % 1.
                };
                let color = hsla(
                    hue,
                    0.34 + (i % 3) as f32 * 0.10,
                    0.10 + (i % 5) as f32 * 0.035,
                    1.,
                );
                // Solid fills avoid backend-specific per-triangle gradient
                // seams. Depth comes from the nested liquid contours.
                window.paint_path(marble_path(bounds, r, seconds), color);
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
    fn living_materials_tessellate_are_continuous_and_stay_in_the_art_region() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(1296.), px(828.)));
        for seconds in [0., 1., 8., 60., 600.] {
            for i in 0..=100 {
                let y = i as f32 / 100.;
                let x = tear_x(y, seconds);
                assert!((0.57..0.66).contains(&x));
                assert!((x - tear_x(y, seconds + 0.001)).abs() < 0.0001);
            }
            let _ = tear_path(bounds, seconds, 0.);
            for i in 0..32 {
                let _ = marble_path(bounds, 0.025 + i as f32 * 0.047, seconds);
            }
        }
        assert_ne!(tear_x(0.3, 0.), tear_x(0.3, 10.));
        assert_ne!(marble_point(0.5, 1., 0.), marble_point(0.5, 1., 10.));
    }
    #[::core::prelude::v1::test]
    fn image_palettes_change_hue_and_lightness_without_losing_text_contrast() {
        for color in [[0.9, 0.3, 0.2], [0.2, 0.6, 0.9], [0.7, 0.8, 0.3]] {
            for luminance in [0.08, 0.35, 0.75] {
                let p = book_palette(color, luminance);
                assert!(crate::theme::contrast(p.paper, p.ink) >= 7.);
                assert!(crate::theme::contrast(p.paper, p.accent) >= 4.5);
            }
        }
        assert_ne!(
            book_palette([0.9, 0.3, 0.2], 0.3),
            book_palette([0.2, 0.6, 0.9], 0.3)
        );
    }
}
