use gpui::{App, Asset, Hsla, ImageCacheError, ImageSource, RenderImage, Rgba, Window};
use image::{Frame, RgbaImage};
use std::sync::Arc;

// Recents uses one settled appearance: an eased fade at the former 75% length.
const FADE_LENGTH: f32 = 75.;
const LENGTH_SCALE: f32 = 0.55 + 0.9 * FADE_LENGTH / 100.;
const EDGE_SPAN: f32 = 180. * LENGTH_SCALE;
const PANEL_TOP: f32 = 306.;
const PANEL_RIGHT: f32 = 440.;
// Larger than half the edge span so the *opaque* region remains rounded too.
const CORNER_RADIUS: f32 = 190.;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FadeKey {
    color: u32,
}

#[derive(Default)]
pub(crate) struct RecentsFade {
    current: Option<FadeKey>,
}

impl RecentsFade {
    pub fn image(&mut self, color: Hsla, window: &mut Window, cx: &mut App) -> ImageSource {
        let key = FadeKey {
            color: Rgba::from(color).into(),
        };
        if self.current != Some(key) {
            if let Some(old) = self.current.take() {
                if let Some(Ok(image)) = window.get_asset::<FadeAsset>(&old, cx) {
                    let _ = window.drop_image(image);
                }
                cx.remove_asset::<FadeAsset>(&old);
            }
            self.current = Some(key);
        }
        // Generate off the UI thread and retain one small texture per theme color.
        ImageSource::from(move |window: &mut Window, cx: &mut App| {
            window.use_asset::<FadeAsset>(&key, cx)
        })
    }
}

struct FadeAsset;

impl Asset for FadeAsset {
    type Source = FadeKey;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    fn load(
        source: FadeKey,
        _: &mut App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        async move {
            let mut pixels = fade_pixels(source);
            // RenderImage uses BGRA, just like GPUI's normal image decoder.
            for pixel in pixels.pixels_mut() {
                pixel.0.swap(0, 2);
            }
            Ok(Arc::new(RenderImage::new([Frame::new(pixels)])))
        }
    }
}

// Signed distance to a lower-left panel with a circular upper-right corner.
// The panel extends beyond the viewport's left and bottom edges to avoid seams.
fn panel_distance(x: f32, y: f32) -> f32 {
    let qx = x - (PANEL_RIGHT - CORNER_RADIUS);
    let qy = PANEL_TOP + CORNER_RADIUS - y;
    qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - CORNER_RADIUS
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn panel_alpha(x: f32, y: f32) -> f32 {
    smoothstep(0.5 - panel_distance(x * 1000., y * 600.) / EDGE_SPAN)
}

fn floor_alpha(y: f32) -> f32 {
    // The eased bottom transition ends at 93%, followed by a solid 7% strip.
    let span = 0.20 * LENGTH_SCALE;
    smoothstep((y - (0.93 - span)) / span)
}

fn combined_alpha(x: f32, y: f32) -> f32 {
    1. - (1. - panel_alpha(x, y)) * (1. - floor_alpha(y))
}

fn fade_pixels(key: FadeKey) -> RgbaImage {
    RgbaImage::from_fn(512, 320, |x, y| {
        let alpha = combined_alpha((x as f32 + 0.5) / 512., (y as f32 + 0.5) / 320.);
        image::Rgba([
            (key.color >> 24) as u8,
            ((key.color >> 16) & 255) as u8,
            ((key.color >> 8) & 255) as u8,
            (alpha * 255.).round() as u8,
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eased_fade_keeps_artwork_clear_and_the_footer_solid() {
        let image = fade_pixels(FadeKey { color: 0x203040ff });
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        assert_eq!(image.get_pixel(511, 0)[3], 0);
        assert!(image.get_pixel(0, 220)[3] > image.get_pixel(400, 220)[3]);
        for x in 0..512 {
            for y in 300..320 {
                assert_eq!(image.get_pixel(x, y)[3], 255);
            }
        }
        for pixel in image.pixels() {
            assert_eq!(&pixel.0[..3], &[32, 48, 64], "no experimental fade grain");
        }
    }

    #[test]
    fn eased_bottom_uses_the_fixed_seventy_five_percent_length() {
        assert_eq!(FADE_LENGTH, 75.);
        let start = 0.93 - 0.20 * LENGTH_SCALE;
        assert_eq!(floor_alpha(start), 0.);
        assert!((floor_alpha((start + 0.93) * 0.5) - 0.5).abs() < 0.0001);
        assert_eq!(floor_alpha(0.93), 1.);
        assert_eq!(floor_alpha(1.), 1.);
    }

    #[test]
    fn opaque_panel_retains_a_rounded_upper_right_corner() {
        let center_x = PANEL_RIGHT - CORNER_RADIUS;
        let center_y = PANEL_TOP + CORNER_RADIUS;
        let solid_radius = CORNER_RADIUS - EDGE_SPAN * 0.5;
        assert!(solid_radius > 70., "fade must not consume the rounding");
        // The intersection of the opaque top/side lines must NOT be opaque.
        let square_x = PANEL_RIGHT - EDGE_SPAN * 0.5;
        let square_y = PANEL_TOP + EDGE_SPAN * 0.5;
        assert!(panel_alpha(square_x / 1000., square_y / 600.) < 0.95);
        // Points inside the inset quarter-circle are fully opaque; those just
        // outside it enter a smooth transition rather than a sharp square edge.
        for angle in [0.2f32, 0.5, 0.8, 1.1, 1.4] {
            let inside_x = center_x + (solid_radius - 2.) * angle.cos();
            let inside_y = center_y - (solid_radius - 2.) * angle.sin();
            assert_eq!(panel_alpha(inside_x / 1000., inside_y / 600.), 1.);
            let outside_x = center_x + (solid_radius + 20.) * angle.cos();
            let outside_y = center_y - (solid_radius + 20.) * angle.sin();
            let alpha = panel_alpha(outside_x / 1000., outside_y / 600.);
            assert!(alpha > 0.9 && alpha < 1.);
        }
    }
}
