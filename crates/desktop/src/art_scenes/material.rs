//! Cached paper fibers and brush coverage; live native vector marbling for Film Reel.
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

pub(crate) fn book_palette(accent: [f32; 3], _luminance: f32) -> BookPalette {
    let color: Hsla = Rgba {
        r: accent[0],
        g: accent[1],
        b: accent[2],
        a: 1.,
    }
    .into();
    let mut palette = BookPalette {
        paper: rgb(0xf4ead7).into(),
        ink: rgb(0x31271f).into(),
        accent: hsla(color.h, color.s.clamp(0.25, 0.55), 0.30, 1.),
    };
    // Yellow/green hues can be much brighter than blue at the same HSL
    // lightness. Keep artwork-derived actions readable for every hue.
    for _ in 0..40 {
        if crate::theme::contrast(palette.paper, palette.accent) >= 4.5 {
            break;
        }
        palette.accent.l = (palette.accent.l - 0.01).clamp(0., 1.);
    }
    palette
}

fn paper_noise(x: u32, y: u32) -> u32 {
    let mut value = x.wrapping_mul(1973) ^ y.wrapping_mul(9277) ^ 0x68bc21eb;
    value = (value ^ (value >> 16)).wrapping_mul(0x7feb352d);
    (value ^ (value >> 15)) & 255
}

fn paper_field(x: u32, y: u32, scale: u32) -> i64 {
    let sample = |dx, dy| i64::from(paper_noise(x / scale + dx, y / scale + dy));
    let fx = i64::from(x % scale);
    let fy = i64::from(y % scale);
    let scale = i64::from(scale);
    let top = sample(0, 0) * (scale - fx) + sample(1, 0) * fx;
    let bottom = sample(0, 1) * (scale - fx) + sample(1, 1) * fx;
    (top * (scale - fy) + bottom * fy) / (scale * scale) - 128
}

pub(crate) fn paper_pixel(x: u32, y: u32, layer: super::PaperLayer) -> [u8; 4] {
    let grain = i64::from(paper_noise(x, y)) - 128;
    let fiber = if paper_noise(x / 2, y / 9) > 246 {
        -7
    } else {
        0
    };
    let variation = grain / 16 + paper_field(x, y, 96) / 28 + fiber;
    let channel = |base: i64| u8::try_from((base + variation).clamp(0, 255)).unwrap_or(0);
    let alpha = match layer {
        super::PaperLayer::Sheet => 255,
        super::PaperLayer::Grain => u8::try_from(paper_noise(x, y) % 17).unwrap_or(0),
        super::PaperLayer::Edge => {
            let edge = 936 + paper_field(0, y, 192) / 3 + paper_field(0, y, 43) / 9;
            let distance = i64::from(x) - edge + paper_field(x, y, 8) / 5 + grain / 10;
            u8::try_from(((distance + 32) * 255 / 64).clamp(0, 255)).unwrap_or(0)
        }
    };
    [channel(244), channel(234), channel(215), alpha]
}

fn marble_point(radius: f32, angle: f32, seconds: f32) -> (f32, f32) {
    let phase = seconds * 0.16;
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

fn reel_lights(seconds: f32) -> [[f32; 3]; 2] {
    [
        [
            0.52 + 0.045 * (seconds * 0.13).sin(),
            0.25 + 0.025 * (seconds * 0.11).cos(),
            0.90 + 0.10 * (seconds * 0.19).sin(),
        ],
        [
            0.48 + 0.055 * (seconds * 0.09 + 1.4).sin(),
            0.76 + 0.025 * (seconds * 0.12).sin(),
            0.90 + 0.10 * (seconds * 0.16 + 2.).sin(),
        ],
    ]
}

pub(crate) fn reel_illumination(seconds: f32, accent: Hsla, width: f32, height: f32) -> Div {
    let mut light = div().absolute().inset_0().size_full();
    for ([x, y, intensity], strength) in reel_lights(seconds).into_iter().zip([0.35, 0.25]) {
        light = light.child(
            div()
                .absolute()
                .left(px(width * (x - 0.23)))
                .top(px(height * (y - 0.055)))
                .w(px(width * 0.46))
                .h(px(height * 0.11))
                .rounded_full()
                .shadow(vec![BoxShadow {
                    color: accent.alpha(strength * intensity),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(height * 0.15),
                    spread_radius: px(height * 0.025),
                    inset: false,
                }]),
        );
    }
    light
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn reel_light_spill_drifts_and_breathes_without_abrupt_changes() {
        assert_ne!(reel_lights(0.), reel_lights(10.));
        for seconds in [0., 1., 10., 60., 600.] {
            for (before, after) in reel_lights(seconds)
                .into_iter()
                .zip(reel_lights(seconds + 0.001))
            {
                assert!((0.42..0.58).contains(&before[0]));
                assert!((0.20..0.80).contains(&before[1]));
                assert!((0.79..1.01).contains(&before[2]));
                for (before, after) in before.into_iter().zip(after) {
                    assert!((before - after).abs() < 0.0001);
                }
            }
        }
    }
    #[::core::prelude::v1::test]
    fn marble_contours_tessellate_and_move_continuously() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(1296.), px(828.)));
        for seconds in [0., 1., 8., 60., 600.] {
            for i in 0..32 {
                let _ = marble_path(bounds, 0.025 + i as f32 * 0.047, seconds);
            }
        }
        assert_ne!(marble_point(0.5, 1., 0.), marble_point(0.5, 1., 10.));
        let before = marble_point(0.5, 1., 10.);
        let after = marble_point(0.5, 1., 10.001);
        assert!((before.0 - after.0).abs() < 0.001);
        assert!((before.1 - after.1).abs() < 0.001);
    }
    #[::core::prelude::v1::test]
    fn paper_is_warm_textured_repeatable_and_only_covers_the_artwork_at_its_edge() {
        use super::super::PaperLayer;
        for y in 0..1024 {
            assert_eq!(paper_pixel(0, y, PaperLayer::Edge)[3], 0);
            assert_eq!(paper_pixel(1100, y, PaperLayer::Edge)[3], 255);
        }
        let first = paper_pixel(100, 100, PaperLayer::Sheet);
        assert_eq!(first, paper_pixel(100, 100, PaperLayer::Sheet));
        assert!(first[0] > first[1] && first[1] > first[2]);
        assert!((101..200).any(|x| paper_pixel(x, 100, PaperLayer::Sheet) != first));
        assert!((870..1000).any(|x| (1..255).contains(&paper_pixel(x, 500, PaperLayer::Edge)[3])));
    }
    #[::core::prelude::v1::test]
    fn paper_stays_warm_and_readable_over_bright_and_dark_artwork() {
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
