//! Cached paper fibers and brush coverage for Art Book.
use gpui::{prelude::*, *};
use std::sync::Arc;

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
    };
    [channel(244), channel(234), channel(215), alpha]
}

fn tear_edge(y: f32, seconds: f32) -> f32 {
    0.012 * (y * 9. + seconds * 0.28).sin()
        + 0.006 * (y * 23. - seconds * 0.19).sin()
        + 0.002 * (y * 173.).sin()
}

fn fitted_artwork(bounds: Bounds<Pixels>, image: &RenderImage) -> Bounds<Pixels> {
    let source = image.size(0);
    let scale = (f32::from(bounds.size.width) / source.width.0 as f32)
        .min(f32::from(bounds.size.height) / source.height.0 as f32);
    let width = source.width.0 as f32 * scale;
    let height = source.height.0 as f32 * scale;
    Bounds::new(
        point(
            bounds.left() + (bounds.size.width - px(width)) / 2.,
            bounds.top() + (bounds.size.height - px(height)) / 2.,
        ),
        size(px(width), px(height)),
    )
}

pub(crate) fn book_artwork(
    current: Option<Arc<RenderImage>>,
    previous: Option<Arc<RenderImage>>,
    progress: f32,
    direction: f32,
    seconds: f32,
    paper: Hsla,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            let artwork = Bounds::new(
                point(
                    bounds.left() + px(width * 0.02),
                    bounds.top() + px(height * 0.03),
                ),
                size(px(width * 0.59), px(height * 0.94)),
            );
            if let Some(image) = &current {
                let fitted = fitted_artwork(artwork, image);
                let _ = window.paint_image_fitted(
                    fitted,
                    fitted,
                    Corners::all(px(0.)),
                    image.clone(),
                    0,
                    false,
                );
            }
            if let Some(image) = &previous {
                let fitted = fitted_artwork(artwork, image);
                for row in 0..160 {
                    let y = row as f32 / 160.;
                    let front = (super::book_tear_front(width * 0.65, progress, direction)
                        + width * tear_edge(y, seconds) * (std::f32::consts::PI * progress).sin())
                    .clamp(0., width * 0.65);
                    let (x, remaining) = if direction >= 0. {
                        (0., front)
                    } else {
                        (front, width * 0.65 - front)
                    };
                    if remaining > 0.5 {
                        let strip = Bounds::new(
                            point(bounds.left() + px(x), bounds.top() + px(y * height)),
                            size(px(remaining), px(height / 160. + 0.5)),
                        );
                        window.paint_quad(fill(strip, paper));
                        let left = strip.left().max(fitted.left());
                        let top = strip.top().max(fitted.top());
                        let right = strip.right().min(fitted.right());
                        let bottom = strip.bottom().min(fitted.bottom());
                        if right > left && bottom > top {
                            let visible =
                                Bounds::new(point(left, top), size(right - left, bottom - top));
                            let _ = window.paint_image_fitted(
                                visible,
                                fitted,
                                Corners::all(px(0.)),
                                image.clone(),
                                0,
                                false,
                            );
                        }
                    }
                    if progress > 0.001 && progress < 0.999 {
                        let curl = (std::f32::consts::PI * progress).sin();
                        let shadow = Bounds::new(
                            point(
                                bounds.left() + px(front - 5.),
                                bounds.top() + px(y * height),
                            ),
                            size(px(10. + curl * 9.), px(height / 160. + 0.5)),
                        );
                        window.paint_quad(fill(shadow, black().alpha(0.16 * curl)));
                        let fiber = Bounds::new(
                            point(
                                bounds.left() + px(front - 2.),
                                bounds.top() + px(y * height),
                            ),
                            size(px(3. + curl * 3.), px(height / 160. + 0.5)),
                        );
                        window.paint_quad(fill(fiber, paper.alpha(0.9)));
                    }
                }
            }
            let mut edge = PathBuilder::fill();
            edge.move_to(point(bounds.right(), bounds.top()));
            for row in 0..=160 {
                let y = row as f32 / 160.;
                edge.line_to(point(
                    bounds.left() + px(width * (0.63 + tear_edge(y, seconds))),
                    bounds.top() + px(height * y),
                ));
            }
            edge.line_to(point(bounds.right(), bounds.bottom()));
            edge.close();
            if let Ok(edge) = edge.build() {
                window.paint_path(edge, paper);
            }
        },
    )
    .absolute()
    .inset_0()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn paper_is_warm_textured_repeatable_and_only_covers_the_artwork_at_its_edge() {
        use super::super::PaperLayer;
        let first = paper_pixel(100, 100, PaperLayer::Sheet);
        assert_eq!(first, paper_pixel(100, 100, PaperLayer::Sheet));
        assert!(first[0] > first[1] && first[1] > first[2]);
        assert!((101..200).any(|x| paper_pixel(x, 100, PaperLayer::Sheet) != first));
        assert_ne!(tear_edge(0.4, 0.), tear_edge(0.4, 8.));
        for y in 0..100 {
            let y = y as f32 / 100.;
            assert!(tear_edge(y, 8.).abs() < 0.025);
            assert!((tear_edge(y, 8.) - tear_edge(y, 8.001)).abs() < 0.0001);
        }
        let area = Bounds::new(point(px(20.), px(30.)), size(px(590.), px(940.)));
        for (width, height) in [(96, 31), (20, 30), (30, 20)] {
            let image = RenderImage::new([image::Frame::new(image::RgbaImage::new(width, height))]);
            let fitted = fitted_artwork(area, &image);
            assert!(fitted.left() >= area.left() && fitted.right() <= area.right());
            assert!(fitted.top() >= area.top() && fitted.bottom() <= area.bottom());
            let ratio = f32::from(fitted.size.width) / f32::from(fitted.size.height);
            assert!((ratio - width as f32 / height as f32).abs() < 0.0001);
            assert!((f32::from(fitted.left() + fitted.size.width / 2.) - 315.).abs() < 0.0001);
        }
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
