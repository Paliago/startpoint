use super::Rect;
use image::{DynamicImage, RgbaImage};

pub(crate) fn diagonal_edges(y: f32) -> [f32; 2] {
    [0.41 - 0.35 * y, 0.95 - 0.37 * y]
}

pub(crate) fn diagonal_slot(x: f32, y: f32) -> usize {
    let [left, right] = diagonal_edges(y.clamp(0., 1.));
    if x < left {
        0
    } else if x < right {
        1
    } else {
        2
    }
}

pub(crate) fn diagonal_bounds(slot: usize, emphasis: f32) -> Rect {
    let scale = 1. + emphasis * 0.20;
    let pivot = [0.17, 0.50, 0.83][slot];
    Rect {
        x: pivot * (1. - scale),
        y: 0.5 * (1. - scale),
        w: scale,
        h: scale,
    }
}

pub(crate) fn diagonal_hit(x: f32, y: f32, emphasis: [f32; 3]) -> usize {
    let mut order = [0, 1, 2];
    order.sort_by(|a, b| emphasis[*b].total_cmp(&emphasis[*a]));
    for slot in order {
        let rect = diagonal_bounds(slot, emphasis[slot]);
        let local_x = (x - rect.x) / rect.w;
        let local_y = (y - rect.y) / rect.h;
        if (0. ..=1.).contains(&local_x)
            && (0. ..=1.).contains(&local_y)
            && diagonal_slot(local_x, local_y) == slot
        {
            return slot;
        }
    }
    diagonal_slot(x, y)
}

pub(crate) fn diagonal_pixels(
    source: DynamicImage,
    width: u16,
    height: u16,
    slot: u8,
) -> RgbaImage {
    let width = width.max(1);
    let height = height.max(1);
    let (left, right) = match slot {
        0 => (0, u32::from(width) * 41 / 100),
        1 => (u32::from(width) * 6 / 100, u32::from(width) * 95 / 100),
        _ => (u32::from(width) * 58 / 100, u32::from(width)),
    };
    let cropped = source
        .resize_to_fill(
            (right - left).max(1),
            u32::from(height),
            image::imageops::FilterType::Triangle,
        )
        .to_rgba8();
    RgbaImage::from_fn(u32::from(width), u32::from(height), |x, y| {
        let normalized_y = f32::from(u16::try_from(y).unwrap_or(height)) / f32::from(height);
        let normalized_x = f32::from(u16::try_from(x).unwrap_or(width)) / f32::from(width);
        let [edge_left, edge_right] = diagonal_edges(normalized_y);
        let distance = match slot {
            0 => edge_left - normalized_x,
            1 => (normalized_x - edge_left).min(edge_right - normalized_x),
            _ => normalized_x - edge_right,
        } * f32::from(width);
        if distance < -0.5 || x < left || x >= right {
            return image::Rgba([0, 0, 0, 0]);
        }
        let mut pixel = *cropped.get_pixel((x - left).min(cropped.width() - 1), y);
        // One pixel of coverage smooths the diagonal without blurring the image.
        let alpha = (distance + 0.5).clamp(0., 1.);
        pixel[3] = (f32::from(pixel[3]) * alpha).round() as u8;
        pixel
    })
}

pub(crate) fn mosaic_rects(width: f32, height: f32, count: usize) -> Vec<Rect> {
    let cells: &[[f32; 4]] = match count {
        0 => &[],
        1 => &[[0., 0., 1., 1.]],
        2 => &[[0., 0., 0.566, 1.], [0.566, 0., 0.434, 1.]],
        3 => &[
            [0., 0., 0.566, 1.],
            [0.566, 0., 0.434, 0.5],
            [0.566, 0.5, 0.434, 0.5],
        ],
        4 => &[
            [0., 0., 0.566, 0.77],
            [0.566, 0., 0.434, 0.5],
            [0., 0.77, 0.566, 0.23],
            [0.566, 0.5, 0.434, 0.5],
        ],
        5 => &[
            [0., 0., 0.566, 0.77],
            [0.566, 0., 0.434, 0.31],
            [0.566, 0.31, 0.434, 0.38],
            [0., 0.77, 0.566, 0.23],
            [0.566, 0.69, 0.434, 0.31],
        ],
        _ => &[
            [0., 0., 0.566, 0.77],
            [0.566, 0., 0.434, 0.31],
            [0.566, 0.31, 0.207, 0.38],
            [0.773, 0.31, 0.227, 0.38],
            [0., 0.77, 0.566, 0.23],
            [0.566, 0.69, 0.434, 0.31],
        ],
    };
    cells
        .iter()
        .map(|[x, y, w, h]| {
            let left = if *x > 0. { 1. } else { 0. };
            let top = if *y > 0. { 1. } else { 0. };
            Rect {
                x: x * width + left,
                y: y * height + top,
                w: (w * width - left - if x + w < 0.999 { 1. } else { 0. }).max(1.),
                h: (h * height - top - if y + h < 0.999 { 1. } else { 0. }).max(1.),
            }
        })
        .collect()
}

pub(crate) fn mosaic_working_pages(requested: usize, displayed: usize, count: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    let pages = count.div_ceil(6);
    let requested = (requested / 6).min(pages - 1);
    let mut working = Vec::new();
    for page in [
        requested,
        (displayed / 6).min(pages - 1),
        (requested + 1) % pages,
        (requested + pages - 1) % pages,
    ] {
        if !working.contains(&page) {
            working.push(page);
        }
    }
    working
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagonal_pixels_and_pointer_targets_follow_the_same_seams() {
        let source = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            600,
            400,
            image::Rgba([160, 80, 40, 255]),
        ));
        for slot in 0..3 {
            let pixels = diagonal_pixels(source.clone(), 320, 200, slot);
            for y in 0..200_u16 {
                for x in 0..320_u16 {
                    let hit = diagonal_slot(f32::from(x) / 320., f32::from(y) / 200.);
                    let alpha = pixels.get_pixel(u32::from(x), u32::from(y))[3];
                    if alpha > 200 {
                        assert_eq!(hit, usize::from(slot));
                    }
                }
            }
        }
        assert_eq!(diagonal_slot(0.3, 0.), 0);
        assert_eq!(diagonal_slot(0.3, 1.), 1);
        assert_eq!(diagonal_slot(0.8, 1.), 2);
        assert_eq!(diagonal_hit(0.5, 0.5, [0.; 3]), 1);
        for slot in 0..3 {
            let resting = diagonal_bounds(slot, 0.);
            let expanded = diagonal_bounds(slot, 1.);
            assert!(expanded.w > resting.w && expanded.h > resting.h);
            assert!(expanded.x <= resting.x && expanded.y <= resting.y);
        }
        let mut expansion = [0.; 3];
        expansion[0] = 1.;
        assert_eq!(diagonal_slot(0.245, 0.5), 1);
        assert_eq!(diagonal_hit(0.245, 0.5, expansion), 0);
    }

    #[test]
    fn mosaic_fills_the_viewport_without_repeating_games_or_overlapping_tiles() {
        for count in 0..12 {
            let rects = mosaic_rects(1586., 992., count);
            assert_eq!(rects.len(), count.min(6));
            for (index, rect) in rects.iter().enumerate() {
                assert!(rect.w > 0. && rect.h > 0.);
                assert!(rect.x >= 0. && rect.y >= 0.);
                assert!(rect.x + rect.w <= 1586.1 && rect.y + rect.h <= 992.1);
                for other in &rects[index + 1..] {
                    assert!(
                        rect.x + rect.w <= other.x
                            || other.x + other.w <= rect.x
                            || rect.y + rect.h <= other.y
                            || other.y + other.h <= rect.y
                    );
                }
            }
            if count > 0 {
                let area: f32 = rects.iter().map(|rect| rect.w * rect.h).sum();
                assert!(area > 1586. * 992. * 0.98);
            }
        }
        for count in 1..50 {
            for requested in 0..count {
                let pages = mosaic_working_pages(requested, 0, count);
                assert_eq!(pages[0], requested / 6);
                assert!(pages.contains(&0));
                assert!(pages.len() <= 4);
                assert!(pages.contains(&((requested / 6 + 1) % count.div_ceil(6))));
                assert!(pages.iter().all(|page| page * 6 < count));
            }
        }
    }
}
