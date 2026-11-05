//! Cached paper fibers and brush coverage for Art Book.
use gpui::*;

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

#[cfg(test)]
mod tests {
    use super::*;
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
