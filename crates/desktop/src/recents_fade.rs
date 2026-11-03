use gpui::{App, Asset, Context, Hsla, ImageCacheError, Pixels, RenderImage, Rgba, Size, Window};
use image::{Frame, RgbaImage};
use std::{collections::VecDeque, path::PathBuf, sync::Arc};

// Recents uses one settled appearance: an eased fade at the former 75% length.
const FADE_LENGTH: f32 = 75.;
const LENGTH_SCALE: f32 = 0.55 + 0.9 * FADE_LENGTH / 100.;
const EDGE_SPAN: f32 = 180. * LENGTH_SCALE;
const PANEL_TOP: f32 = 306.;
const PANEL_RIGHT: f32 = 440.;
// Larger than half the edge span so the *opaque* region remains rounded too.
const CORNER_RADIUS: f32 = 190.;

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FadeKey {
    color: u32,
}

#[derive(Default)]
pub(crate) struct RecentsFade {
    recent: VecDeque<BackdropKey>,
    frame: Vec<BackdropKey>,
    cleanup_scheduled: bool,
}

impl RecentsFade {
    /// Flatten artwork, color dithering and the settled fade off-thread, then apply shell
    /// alpha exactly once. Stacked translucent quads would approach opacity.
    pub fn image(
        &mut self,
        path: Option<PathBuf>,
        color: Hsla,
        size: Size<Pixels>,
        dither_strength: f32,
        dither_size: f32,
        fade: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        self.request(
            path,
            color,
            size,
            dither_strength,
            dither_size,
            fade,
            false,
            window,
            cx,
        )
    }

    /// The Library uses the same cached worker without flattening source alpha
    /// or applying the Recents fade. Its existing gradient stays in the view.
    pub fn artwork(
        &mut self,
        path: PathBuf,
        size: Size<Pixels>,
        dither_strength: f32,
        dither_size: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        self.request(
            Some(path),
            gpui::transparent_black(),
            size,
            dither_strength,
            dither_size,
            false,
            true,
            window,
            cx,
        )
    }

    fn request(
        &mut self,
        path: Option<PathBuf>,
        color: Hsla,
        size: Size<Pixels>,
        dither_strength: f32,
        dither_size: f32,
        fade: bool,
        preserve_alpha: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        let width = f32::from(size.width).max(1.);
        let height = f32::from(size.height).max(1.);
        let scale = (1920. / width).min(1200. / height).min(1.);
        let dither_strength = if dither_strength.is_finite() {
            dither_strength.clamp(0., 1.)
        } else {
            startpoint_sdk::DEFAULT_DITHER_STRENGTH
        };
        let key = BackdropKey {
            path,
            color: Rgba::from(color).into(),
            // Bound work during live window/sidebar resizing instead of decoding
            // full artwork again for every one-pixel layout change.
            width: ((width * scale / 64.).round().max(1.) * 64.) as u32,
            height: ((height * scale / 64.).round().max(1.) * 64.) as u32,
            dither_strength: (dither_strength * 100.).round() as u8,
            dither_size: crate::dithering::cell_size(dither_size) as u8,
            fade,
            preserve_alpha,
        };
        self.recent.retain(|old| old != &key);
        self.recent.push_back(key.clone());
        if !self.frame.contains(&key) {
            self.frame.push(key.clone());
        }
        if !self.cleanup_scheduled {
            self.cleanup_scheduled = true;
            let entity = cx.weak_entity();
            window.on_next_frame(move |window, cx| {
                let _ = entity.update(cx, |state, cx| state.finish_frame(window, cx));
            });
        }
        let image = window
            .use_asset::<BackdropAsset>(&key, cx)
            .and_then(Result::ok);
        if image.is_some() {
            return image;
        }
        // Keep the matching ready surface pinned while new dithering is composited.
        let fallback = self.recent.iter().rev().find_map(|previous| {
            if previous.path != key.path
                || previous.color != key.color
                || previous.width != key.width
                || previous.height != key.height
                || previous.fade != key.fade
                || previous.preserve_alpha != key.preserve_alpha
            {
                return None;
            }
            window
                .get_asset::<BackdropAsset>(previous, cx)
                .and_then(Result::ok)
                .map(|image| (previous.clone(), image))
        });
        fallback.map(|(previous, image)| {
            if !self.frame.contains(&previous) {
                self.frame.push(previous);
            }
            image
        })
    }

    fn finish_frame(&mut self, window: &mut Window, cx: &mut App) {
        self.cleanup_scheduled = false;
        let pinned = std::mem::take(&mut self.frame);
        let mut changed = false;
        while self.recent.len() > 3 {
            let Some(index) = self.recent.iter().position(|key| !pinned.contains(key)) else {
                break;
            };
            let old = self.recent.remove(index).unwrap();
            if let Some(Ok(image)) = window.get_asset::<BackdropAsset>(&old, cx) {
                let _ = window.drop_image(image);
            }
            cx.remove_asset::<BackdropAsset>(&old);
            if !self.recent.iter().any(|key| {
                key.path == old.path && key.width == old.width && key.height == old.height
            }) {
                if let Some(path) = old.path {
                    cx.remove_asset::<DecodedArtwork>(&(path, old.width, old.height));
                }
            }
            changed = true;
        }
        if changed {
            window.refresh();
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct BackdropKey {
    path: Option<PathBuf>,
    color: u32,
    width: u32,
    height: u32,
    dither_strength: u8,
    dither_size: u8,
    fade: bool,
    preserve_alpha: bool,
}

struct BackdropAsset;

struct DecodedArtwork;

impl Asset for DecodedArtwork {
    type Source = (PathBuf, u32, u32);
    type Output = Option<Arc<RgbaImage>>;

    fn load(
        (path, width, height): Self::Source,
        _: &mut App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        async move {
            image::open(path).ok().map(|image| {
                Arc::new(
                    image
                        .resize_to_fill(width, height, image::imageops::FilterType::Lanczos3)
                        .to_rgba8(),
                )
            })
        }
    }
}

impl Asset for BackdropAsset {
    type Source = BackdropKey;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    fn load(
        source: BackdropKey,
        cx: &mut App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        let artwork = source.path.as_ref().map(|path| {
            cx.fetch_asset::<DecodedArtwork>(&(path.clone(), source.width, source.height))
                .0
        });
        async move {
            let artwork = match artwork {
                Some(task) => task.await,
                None => None,
            };
            let mut pixels = backdrop_pixels(&source, artwork.as_deref());
            // RenderImage uses BGRA, just like GPUI's normal image decoder.
            for pixel in pixels.pixels_mut() {
                pixel.0.swap(0, 2);
            }
            Ok(Arc::new(RenderImage::new([Frame::new(pixels)])))
        }
    }
}

fn backdrop_pixels(key: &BackdropKey, artwork: Option<&RgbaImage>) -> RgbaImage {
    let treated = artwork.filter(|_| key.dither_strength > 0).map(|image| {
        crate::dithering::apply(image, u32::from(key.dither_size), key.dither_strength)
    });
    if key.preserve_alpha {
        return treated
            .or_else(|| artwork.cloned())
            .unwrap_or_else(|| RgbaImage::new(key.width, key.height));
    }
    let artwork = treated.as_ref().or(artwork);
    let theme = [
        (key.color >> 24) as u8,
        (key.color >> 16) as u8,
        (key.color >> 8) as u8,
    ];
    RgbaImage::from_fn(key.width, key.height, |x, y| {
        let mut rgb = theme.map(f32::from);
        if let Some(artwork) = artwork {
            let pixel = artwork.get_pixel(x, y);
            let alpha = f32::from(pixel[3]) / 255.;
            for channel in 0..3 {
                rgb[channel] = rgb[channel] * (1. - alpha) + f32::from(pixel[channel]) * alpha;
            }
        }
        if key.fade {
            let alpha = combined_alpha(
                (x as f32 + 0.5) / key.width as f32,
                (y as f32 + 0.5) / key.height as f32,
            );
            for channel in 0..3 {
                rgb[channel] = rgb[channel] * (1. - alpha) + f32::from(theme[channel]) * alpha;
            }
        }
        image::Rgba([
            rgb[0].round() as u8,
            rgb[1].round() as u8,
            rgb[2].round() as u8,
            key.color as u8,
        ])
    })
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

#[cfg(test)]
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
    fn artwork_dithering_and_fade_never_make_the_translucent_backdrop_opaque() {
        let artwork = RgbaImage::from_pixel(64, 40, image::Rgba([160, 80, 40, 255]));
        for alpha in [204u8, 255] {
            for dither in [false, true] {
                for fade in [false, true] {
                    let key = BackdropKey {
                        path: None,
                        color: 0x20304000 | u32::from(alpha),
                        width: 64,
                        height: 40,
                        dither_strength: if dither { 12 } else { 0 },
                        dither_size: 2,
                        fade,
                        preserve_alpha: false,
                    };
                    for image in [
                        backdrop_pixels(&key, Some(&artwork)),
                        backdrop_pixels(&key, None),
                    ] {
                        assert!(image.pixels().all(|pixel| pixel[3] == alpha));
                        if fade {
                            assert_eq!(image.get_pixel(63, 39).0, [32, 48, 64, alpha]);
                        }
                    }
                    let image = backdrop_pixels(&key, Some(&artwork));
                    assert_eq!(
                        image.get_pixel(0, 0).0[..3],
                        if dither { [171, 86, 43] } else { [160, 80, 40] }
                    );
                }
            }
        }
    }

    #[test]
    fn dithering_strength_changes_pixels_without_changing_alpha_or_the_settled_fade() {
        let artwork = RgbaImage::from_pixel(64, 40, image::Rgba([160, 80, 40, 255]));
        let mut previous = 160;
        for strength in [0, 4, 12, 35, 70, 100] {
            let mut key = BackdropKey {
                path: None,
                color: 0x203040cc,
                width: 64,
                height: 40,
                dither_strength: strength,
                dither_size: 2,
                fade: false,
                preserve_alpha: false,
            };
            let image = backdrop_pixels(&key, Some(&artwork));
            assert!(image.get_pixel(0, 0)[0] >= previous);
            previous = image.get_pixel(0, 0)[0];
            assert!(image.pixels().all(|pixel| pixel[3] == 204));
            if strength == 0 {
                assert_eq!(
                    image,
                    RgbaImage::from_pixel(64, 40, image::Rgba([160, 80, 40, 204]))
                );
            }
            key.fade = true;
            assert_eq!(
                backdrop_pixels(&key, Some(&artwork)).get_pixel(63, 39).0,
                [32, 48, 64, 204]
            );
            assert!(
                backdrop_pixels(&key, None)
                    .pixels()
                    .all(|pixel| pixel.0 == [32, 48, 64, 204])
            );
        }
        assert_eq!(previous, 255);
    }

    #[test]
    fn dither_size_enlarges_the_pattern_without_changing_intensity_alpha_or_fade() {
        let artwork = RgbaImage::from_pixel(48, 48, image::Rgba([160, 80, 40, 255]));
        let mut key = BackdropKey {
            path: None,
            color: 0x203040cc,
            width: 48,
            height: 48,
            dither_strength: 12,
            dither_size: 2,
            fade: false,
            preserve_alpha: false,
        };
        let original = backdrop_pixels(&key, Some(&artwork));
        key.dither_size = 6;
        let enlarged = backdrop_pixels(&key, Some(&artwork));
        assert_ne!(original, enlarged);
        for y in 0..16 {
            for x in 0..16 {
                for dy in 0..3 {
                    for dx in 0..3 {
                        assert_eq!(
                            original.get_pixel(x, y),
                            enlarged.get_pixel(x * 3 + dx, y * 3 + dy)
                        );
                    }
                }
            }
        }
        for size in 1..=12 {
            key.dither_size = size;
            let image = backdrop_pixels(&key, Some(&artwork));
            assert!(image.pixels().all(|pixel| pixel[3] == 204));
            assert_eq!(image.get_pixel(0, 0), original.get_pixel(0, 0));
            key.fade = true;
            assert_eq!(
                backdrop_pixels(&key, Some(&artwork)).get_pixel(47, 47).0,
                [32, 48, 64, 204]
            );
            key.fade = false;
            key.dither_strength = 0;
            assert!(
                backdrop_pixels(&key, Some(&artwork))
                    .pixels()
                    .all(|pixel| pixel.0 == [160, 80, 40, 204])
            );
            key.dither_strength = 12;
        }
    }

    #[test]
    fn library_treatment_preserves_source_alpha_and_does_not_apply_the_recents_fade() {
        let artwork = RgbaImage::from_fn(64, 40, |x, y| image::Rgba([128, 64, 32, (x + y) as u8]));
        let key = BackdropKey {
            path: None,
            color: 0x203040cc,
            width: 64,
            height: 40,
            dither_strength: 100,
            dither_size: 2,
            fade: false,
            preserve_alpha: true,
        };
        let image = backdrop_pixels(&key, Some(&artwork));
        assert_eq!(image, crate::dithering::apply(&artwork, 2, 100));
        for (original, treated) in artwork.pixels().zip(image.pixels()) {
            assert_eq!(original[3], treated[3]);
        }
        assert!(
            backdrop_pixels(&key, None)
                .pixels()
                .all(|pixel| pixel.0 == [0, 0, 0, 0])
        );
    }

    #[test]
    fn recents_and_library_use_the_same_color_dithering_at_the_shared_defaults() {
        let settings = startpoint_sdk::AppearanceSettings::default();
        let artwork = RgbaImage::from_fn(64, 40, |x, y| {
            image::Rgba([(x * 4) as u8, (y * 6) as u8, 80, 255])
        });
        let mut key = BackdropKey {
            path: None,
            color: 0x203040ff,
            width: 64,
            height: 40,
            dither_strength: (settings.dither_recents_strength * 100.).round() as u8,
            dither_size: crate::dithering::cell_size(settings.dither_recents_size) as u8,
            fade: false,
            preserve_alpha: false,
        };
        let recents = backdrop_pixels(&key, Some(&artwork));
        key.preserve_alpha = true;
        key.dither_strength = (settings.dither_artwork_strength * 100.).round() as u8;
        key.dither_size = crate::dithering::cell_size(settings.dither_artwork_size) as u8;
        assert_eq!(recents, backdrop_pixels(&key, Some(&artwork)));
        assert_eq!(recents, crate::dithering::apply(&artwork, 3, 20));
        assert_ne!(recents, artwork);
    }

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
