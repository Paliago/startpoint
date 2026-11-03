//! Image-dependent ordered color dithering, independent of shell translucency.
use image::RgbaImage;

// Zeron's wallpaper treatment uses this Bayer order with a two-pixel cell.
// Brightness selects a hue-preserving bright or dark color, not overlay grain.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

pub(crate) fn cell_size(value: f32) -> u32 {
    if value.is_finite() {
        value.round().clamp(1., startpoint_sdk::MAX_DITHER_SIZE) as u32
    } else {
        startpoint_sdk::DEFAULT_DITHER_SIZE as u32
    }
}

fn quantize([r, g, b, a]: [u8; 4], threshold: u8) -> [u8; 4] {
    let peak = f32::from(r.max(g).max(b));
    let bright = peak / 255. > (f32::from(threshold) + 0.5) / 16.;
    let gain = if bright { 255. / peak.max(1.) } else { 0.08 };
    [
        (f32::from(r) * gain).round() as u8,
        (f32::from(g) * gain).round() as u8,
        (f32::from(b) * gain).round() as u8,
        a,
    ]
}

/// Called by the background image worker. Sample/quantize once per cell and
/// blend RGB only; preserve per-pixel source alpha even at transparent edges.
pub(crate) fn apply(source: &RgbaImage, size: u32, intensity: u8) -> RgbaImage {
    let mut output = source.clone();
    if intensity == 0 {
        return output;
    }
    let cell = size.clamp(1, startpoint_sdk::MAX_DITHER_SIZE as u32);
    let amount = f32::from(intensity.min(100)) / 100.;
    for y in (0..source.height()).step_by(cell as usize) {
        for x in (0..source.width()).step_by(cell as usize) {
            let sample_x = (x + cell / 2).min(source.width() - 1);
            let sample_y = (y + cell / 2).min(source.height() - 1);
            let color = quantize(
                source.get_pixel(sample_x, sample_y).0,
                BAYER[(y / cell % 4) as usize][(x / cell % 4) as usize],
            );
            for dy in 0..cell.min(source.height() - y) {
                for dx in 0..cell.min(source.width() - x) {
                    let pixel = output.get_pixel_mut(x + dx, y + dy);
                    for channel in 0..3 {
                        pixel[channel] = (f32::from(pixel[channel]) * (1. - amount)
                            + f32::from(color[channel]) * amount)
                            .round() as u8;
                    }
                }
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_controls_dot_density_and_quantization_preserves_hue_and_alpha() {
        for color in [[32, 16, 8, 173], [128, 64, 32, 173], [224, 112, 56, 173]] {
            let mut bright = 0;
            for threshold in 0..16 {
                let result = quantize(color, threshold);
                assert_eq!(result[3], color[3]);
                if result[0] == 255 {
                    bright += 1;
                    assert!(result[1].abs_diff(128) <= 1);
                    assert!(result[2].abs_diff(64) <= 1);
                } else {
                    assert_eq!(result[0], (f32::from(color[0]) * 0.08).round() as u8);
                }
            }
            assert_eq!(bright, (f32::from(color[0]) / 255. * 16.).round() as u8);
        }
        assert_eq!(quantize([0, 0, 0, 127], 0), [0, 0, 0, 127]);
        assert_eq!(quantize([255, 255, 255, 127], 15), [255, 255, 255, 127]);
        for value in [0, 255] {
            let image = RgbaImage::from_pixel(16, 16, image::Rgba([value, value, value, 127]));
            assert_eq!(
                apply(&image, 2, 100),
                image,
                "no independent noise over flat black or white"
            );
        }
    }

    #[test]
    fn cells_sample_the_image_and_use_solid_colors_at_every_size_including_partial_edges() {
        for cell in 1..=12 {
            let source = RgbaImage::from_fn(53, 31, |x, y| {
                image::Rgba([(x * 4) as u8, (y * 6) as u8, 30, (x + y) as u8])
            });
            let original = source.clone();
            let output = apply(&source, cell, 100);
            assert_eq!(source, original, "never modify the original artwork");
            for y in 0..source.height() {
                for x in 0..source.width() {
                    let cell_x = x / cell * cell;
                    let cell_y = y / cell * cell;
                    let sample =
                        source.get_pixel((cell_x + cell / 2).min(52), (cell_y + cell / 2).min(30));
                    let expected = quantize(
                        sample.0,
                        BAYER[(y / cell % 4) as usize][(x / cell % 4) as usize],
                    );
                    assert_eq!(&output.get_pixel(x, y).0[..3], &expected[..3]);
                    assert_eq!(output.get_pixel(x, y)[3], source.get_pixel(x, y)[3]);
                }
            }
        }
    }

    #[test]
    fn intensity_blends_the_treatment_without_changing_alpha_and_zero_is_an_exact_bypass() {
        let source = RgbaImage::from_pixel(8, 8, image::Rgba([128, 64, 32, 123]));
        assert_eq!(apply(&source, 2, 0), source);
        let full = apply(&source, 2, 100);
        assert_eq!(full.get_pixel(0, 0).0, [255, 128, 64, 123]);
        assert_eq!(full.get_pixel(0, 2).0, [10, 5, 3, 123]);
        let half = apply(&source, 2, 50);
        assert_eq!(half.get_pixel(0, 0).0, [192, 96, 48, 123]);
        assert!(half.pixels().all(|pixel| pixel[3] == 123));
        assert_eq!(apply(&RgbaImage::new(0, 0), 2, 100).dimensions(), (0, 0));
    }

    #[test]
    fn size_is_bounded_and_invalid_values_keep_the_default() {
        assert_eq!(cell_size(f32::NAN), 3);
        assert_eq!(cell_size(0.), 1);
        assert_eq!(cell_size(99.), 12);
    }
}
