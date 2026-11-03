//! A small, deterministic polished-concrete material, baked once per device.
//! Channels store roughness, reflectivity, fine grain/pitting and albedo wear.
//! Surface normals stay level: imperfections scatter light, not warp the floor.
use image::{Rgba, RgbaImage};

pub const SIZE: u32 = 256;
pub const MIPS: u32 = 9;

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut bits = (x as u32).wrapping_mul(1973) ^ (y as u32).wrapping_mul(9277) ^ seed;
    bits = (bits ^ (bits >> 16)).wrapping_mul(0x7feb352d);
    bits = (bits ^ (bits >> 15)).wrapping_mul(0x846ca68b);
    (bits ^ (bits >> 16)) as f32 / u32::MAX as f32
}

fn noise(u: f32, v: f32, periods: [i32; 2], seed: u32) -> f32 {
    let x = u * periods[0] as f32;
    let y = v * periods[1] as f32;
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let sx = fx * fx * (3. - 2. * fx);
    let sy = fy * fy * (3. - 2. * fy);
    let sample = |dx: i32, dy: i32| {
        hash(
            (ix + dx).rem_euclid(periods[0]),
            (iy + dy).rem_euclid(periods[1]),
            seed,
        )
    };
    let a = sample(0, 0) * (1. - sx) + sample(1, 0) * sx;
    let b = sample(0, 1) * (1. - sx) + sample(1, 1) * sx;
    a * (1. - sy) + b * sy
}

fn smoothstep(from: f32, to: f32, value: f32) -> f32 {
    let t = ((value - from) / (to - from)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn sample(u: f32, v: f32, x: i32, y: i32) -> [f32; 4] {
    // Overlapping broad polish and smaller trowel/wear marks, not isolated
    // white-noise pixels. All value-noise octaves wrap seamlessly at tile edges.
    let broad = noise(u, v, [4, 4], 0x68bc21eb);
    let middle = noise(u, v, [13, 17], 0x32b176ad);
    let small = noise(u, v, [39, 43], 0xa4c17d83);
    let polish = smoothstep(0.30, 0.71, broad * 0.52 + middle * 0.34 + small * 0.14);
    let trowel = noise(u, v, [18, 101], 0x47c89b1d);
    let fine = hash(x, y, 0x746f2b15);
    let pits = if hash(x, y, 0x815c2a37) < 0.018 {
        0.12
    } else {
        1.
    };
    [
        (0.23 + (1. - polish) * 0.47 + (trowel - 0.5) * 0.06).clamp(0.2, 0.76),
        (0.16 + polish * 0.78 * (0.83 + small * 0.17)).clamp(0., 1.),
        (0.22 + fine * 0.40 + trowel * 0.38) * pits,
        0.35 + broad * 0.20 + middle * 0.24 + trowel * 0.12,
    ]
}

pub fn pixels() -> RgbaImage {
    RgbaImage::from_fn(SIZE, SIZE, |x, y| {
        let channels = sample(
            (x as f32 + 0.5) / SIZE as f32,
            (y as f32 + 0.5) / SIZE as f32,
            x as i32,
            y as i32,
        );
        Rgba(channels.map(|channel| (channel * 255.).round() as u8))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concrete_has_spatially_coherent_polished_and_matte_regions() {
        let pixels = pixels();
        let roughness = pixels.pixels().map(|pixel| pixel[0]).collect::<Vec<_>>();
        assert!(roughness.iter().copied().min().unwrap() < 85);
        assert!(roughness.iter().copied().max().unwrap() > 160);
        assert!(pixels.pixels().any(|pixel| pixel[1] > 190));
        assert!(pixels.pixels().any(|pixel| pixel[1] < 75));
        let neighbor_delta: f32 = pixels
            .enumerate_pixels()
            .filter(|(x, _, _)| *x + 1 < SIZE)
            .map(|(x, y, pixel)| {
                (f32::from(pixel[0]) - f32::from(pixels.get_pixel(x + 1, y)[0])).abs()
            })
            .sum();
        assert!(
            neighbor_delta / (((SIZE - 1) * SIZE) as f32) < 5.,
            "roughness must form patches, not sparkling pixel noise"
        );
    }
    #[test]
    fn concrete_is_repeatable_and_macro_channels_tile_without_a_seam() {
        assert_eq!(pixels(), pixels());
        for step in 0..64 {
            let t = step as f32 / 64.;
            let left = sample(0., t, 0, step);
            let right = sample(1., t, 0, step);
            let top = sample(t, 0., step, 0);
            let bottom = sample(t, 1., step, 0);
            for channel in [0, 1, 3] {
                assert!((left[channel] - right[channel]).abs() < 0.0001);
                assert!((top[channel] - bottom[channel]).abs() < 0.0001);
            }
        }
    }
}
