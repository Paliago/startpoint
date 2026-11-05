use super::Rect;

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
