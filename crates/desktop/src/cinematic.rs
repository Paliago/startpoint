//! Cinematic composition and finite transitions. Images are decoded once on an
//! asset worker; GPUI crossfades cached textures directly, with no readback or
//! continuous lighting simulation. No library or launch state lives here.
use gpui::{App, Asset, Context, RenderImage, Window};
use image::{Frame, RgbaImage};
use startpoint_sdk::{ArtworkKind, Game};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) const TRANSITION: Duration = Duration::from_millis(480);
pub(crate) const NEUTRAL_ACCENT: [f32; 3] = [0.78, 0.70, 0.54];

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ArtworkKey {
    Landscape(PathBuf),
    SoftCover(PathBuf),
    Portrait(PathBuf),
    Book {
        background: Option<PathBuf>,
        cover: Option<PathBuf>,
    },
    Diagonal {
        path: PathBuf,
        width: u16,
        height: u16,
        slot: u8,
    },
}

pub(crate) fn artwork_key(game: &Game) -> Option<ArtworkKey> {
    game.artwork
        .get(&ArtworkKind::Background)
        .map(|art| ArtworkKey::Landscape(art.image_path.clone()))
        .or_else(|| {
            game.artwork
                .get(&ArtworkKind::Cover)
                .map(|art| ArtworkKey::SoftCover(art.image_path.clone()))
        })
}

#[derive(Clone)]
pub(crate) struct Artwork {
    pub image: Option<Arc<RenderImage>>,
    pub accent: [f32; 3],
    pub luminance: f32,
}
impl Default for Artwork {
    fn default() -> Self {
        Self {
            image: None,
            accent: NEUTRAL_ACCENT,
            luminance: 0.5,
        }
    }
}

#[derive(Default)]
pub(crate) struct ArtworkCache {
    recent: VecDeque<ArtworkKey>,
    pinned: Vec<ArtworkKey>,
    cleanup_scheduled: bool,
    pending: Vec<ArtworkKey>,
    capacity: usize,
}
impl ArtworkCache {
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(4);
    }
    pub fn load(
        &mut self,
        key: &ArtworkKey,
        pinned: &[ArtworkKey],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Artwork> {
        let known = self.recent.contains(key);
        if !known && self.pending.len() >= 2 {
            return None;
        }
        self.recent.retain(|old| old != key);
        self.recent.push_back(key.clone());
        for key in pinned.iter().chain(std::iter::once(key)) {
            if !self.pinned.contains(key) {
                self.pinned.push(key.clone());
            }
        }
        if !self.cleanup_scheduled {
            self.cleanup_scheduled = true;
            let entity = cx.weak_entity();
            window.on_next_frame(move |window, cx| {
                let _ = entity.update(cx, |cache, cx| cache.finish_frame(window, cx));
            });
        }
        if !known {
            self.pending.push(key.clone());
            let source = key.clone();
            let task = cx.fetch_asset::<ArtworkAsset>(key).0;
            cx.spawn(async move |cache, cx| {
                task.await;
                let _ = cache.update(cx, |cache, cx| {
                    cache.pending.retain(|key| key != &source);
                    // Explicitly invalidate the owning view: preparation runs
                    // before element painting, outside a nested current-view
                    // scope. Asset completion must not wait for user input.
                    cx.notify();
                });
            })
            .detach();
        }
        window.get_asset::<ArtworkAsset>(key, cx)
    }

    fn finish_frame(&mut self, window: &mut Window, cx: &mut App) {
        self.cleanup_scheduled = false;
        let pinned = std::mem::take(&mut self.pinned);
        let mut changed = false;
        // At most three composited frames plus the loading destination. All
        // images are bounded to 1920×1200, independent of source file size/DPI.
        while self.recent.len() > self.capacity.max(4) {
            let Some(index) = self.recent.iter().position(|key| !pinned.contains(key)) else {
                break;
            };
            let key = self.recent.remove(index).unwrap();
            if let Some(art) = window.get_asset::<ArtworkAsset>(&key, cx)
                && let Some(image) = art.image
            {
                let _ = window.drop_image(image);
            }
            cx.remove_asset::<ArtworkAsset>(&key);
            changed = true;
        }
        if changed {
            window.refresh();
        }
    }
}

struct ArtworkAsset;
impl Asset for ArtworkAsset {
    type Source = ArtworkKey;
    type Output = Artwork;
    fn load(
        key: ArtworkKey,
        _: &mut App,
    ) -> impl std::future::Future<Output = Artwork> + Send + 'static {
        async move {
            let (path, soften, portrait, book, diagonal) = match key {
                ArtworkKey::Landscape(path) => (path, false, false, false, None),
                ArtworkKey::SoftCover(path) => (path, true, false, false, None),
                ArtworkKey::Portrait(path) => (path, false, true, false, None),
                ArtworkKey::Book { background, cover } => {
                    let suitable_background = background.as_ref().is_some_and(|path| {
                        image::image_dimensions(path).is_ok_and(|(width, height)| {
                            f64::from(width) / f64::from(height.max(1)) <= 1.8
                        })
                    });
                    let path = if suitable_background {
                        background.or(cover)
                    } else {
                        cover.or(background)
                    };
                    let Some(path) = path else {
                        return Artwork::default();
                    };
                    (path, false, false, true, None)
                }
                ArtworkKey::Diagonal {
                    path,
                    width,
                    height,
                    slot,
                } => (path, false, false, false, Some((width, height, slot))),
            };
            let Ok(source) = image::open(path) else {
                return Artwork::default();
            };
            let sample = source.thumbnail(48, 32).to_rgba8();
            let accent = artwork_accent(&sample);
            let luminance = sample
                .pixels()
                .map(|p| {
                    (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2]))
                        / 255.
                })
                .sum::<f32>()
                / (sample.width() * sample.height()).max(1) as f32;
            let mut pixels = if let Some((width, height, slot)) = diagonal {
                crate::art_scenes::composition::diagonal_pixels(source, width, height, slot)
            } else if soften {
                source
                    .resize_to_fill(960, 600, image::imageops::FilterType::Triangle)
                    .blur(24.)
                    .to_rgba8()
            } else if portrait {
                source
                    .resize(1024, 1536, image::imageops::FilterType::Triangle)
                    .to_rgba8()
            } else if book {
                source
                    .resize(1920, 1400, image::imageops::FilterType::Triangle)
                    .to_rgba8()
            } else {
                source
                    .resize(1920, 1200, image::imageops::FilterType::Triangle)
                    .to_rgba8()
            };
            for pixel in pixels.pixels_mut() {
                let alpha = f32::from(pixel[3]) / 255.;
                let shade = if soften { 0.48 } else { 1. };
                for channel in &mut pixel.0[..3] {
                    *channel = (f32::from(*channel) * alpha * shade) as u8;
                }
                if diagonal.is_none() {
                    pixel[3] = 255;
                }
                pixel.0.swap(0, 2); // GPUI's native BGRA texture storage.
            }
            Artwork {
                image: Some(Arc::new(RenderImage::new([Frame::new(pixels)]))),
                accent,
                luminance,
            }
        }
    }
}

fn artwork_accent(pixels: &RgbaImage) -> [f32; 3] {
    // Favor atmospheric midtones over black letterboxing and white logos.
    // A small weighted histogram avoids averaging complementary colors to gray.
    let mut bins = [[0f32; 4]; 12];
    for pixel in pixels.pixels() {
        if pixel[3] < 128 {
            continue;
        }
        let rgb = [pixel[0], pixel[1], pixel[2]].map(|v| f32::from(v) / 255.);
        let max = rgb.into_iter().fold(0f32, f32::max);
        let min = rgb.into_iter().fold(1f32, f32::min);
        if max < 0.10 || min > 0.88 {
            continue;
        }
        let delta = max - min;
        let hue = if delta < 0.001 {
            0.
        } else if max == rgb[0] {
            ((rgb[1] - rgb[2]) / delta).rem_euclid(6.)
        } else if max == rgb[1] {
            (rgb[2] - rgb[0]) / delta + 2.
        } else {
            (rgb[0] - rgb[1]) / delta + 4.
        };
        let bin = ((hue * 2.) as usize).min(11);
        let weight = 0.1 + delta * (1. - (max - 0.55).abs());
        for channel in 0..3 {
            bins[bin][channel] += rgb[channel] * weight;
        }
        bins[bin][3] += weight;
    }
    let best = bins
        .into_iter()
        .max_by(|a, b| a[3].total_cmp(&b[3]))
        .unwrap();
    if best[3] < 0.01 {
        return NEUTRAL_ACCENT;
    }
    let mut color = [best[0] / best[3], best[1] / best[3], best[2] / best[3]];
    let max = color.into_iter().fold(0f32, f32::max);
    for channel in &mut color {
        *channel = (*channel / max.max(0.01) * 0.72 + 0.24).clamp(0., 1.);
    }
    color
}

#[derive(Clone)]
pub(crate) struct Layer {
    pub key: Option<ArtworkKey>,
    pub artwork: Artwork,
    pub weight: f32,
}

#[derive(Default)]
pub(crate) struct Scene {
    from: Vec<Layer>,
    target: Option<Layer>,
    started: Option<Instant>,
    paused_at: Option<Instant>,
    duration: Option<Duration>,
}
impl Scene {
    pub fn luminance(&self, now: Instant) -> f32 {
        let layers = self.layers(now);
        if layers.is_empty() {
            0.5
        } else {
            layers.iter().map(|l| l.artwork.luminance * l.weight).sum()
        }
    }
    pub fn set_duration(&mut self, duration: Duration) {
        self.duration = Some(duration);
    }
    fn progress(&self, now: Instant) -> f32 {
        let now = self.paused_at.unwrap_or(now);
        let t = self
            .started
            .map_or(1., |start| {
                now.saturating_duration_since(start).as_secs_f32()
                    / self.duration.unwrap_or(TRANSITION).as_secs_f32()
            })
            .clamp(0., 1.);
        t * t * (3. - 2. * t)
    }
    pub fn layers(&self, now: Instant) -> Vec<Layer> {
        let t = self.progress(now);
        let mut layers = self
            .from
            .iter()
            .cloned()
            .map(|mut layer| {
                layer.weight *= 1. - t;
                layer
            })
            .collect::<Vec<_>>();
        if let Some(target) = &self.target {
            let mut target = target.clone();
            target.weight = t;
            layers.push(target);
        }
        layers.retain(|layer| layer.weight > 0.0001);
        layers
    }
    pub fn keys(&self) -> Vec<ArtworkKey> {
        self.from
            .iter()
            .chain(self.target.iter())
            .filter_map(|layer| layer.key.clone())
            .collect()
    }
    pub fn update(
        &mut self,
        key: Option<ArtworkKey>,
        artwork: Artwork,
        reduced: bool,
        now: Instant,
    ) {
        if self.target.as_ref().is_some_and(|target| target.key == key) {
            if reduced || self.progress(now) >= 1. {
                self.from.clear();
                self.started = None;
            }
            return;
        }
        let mut from = self.layers(now);
        // Retarget from the displayed blend, not from an arbitrary old game.
        // Bound rapid keyboard scrubbing to three full-screen textures.
        if from.len() > 2 {
            from.sort_by(|a, b| b.weight.total_cmp(&a.weight));
            from.truncate(2);
            let total: f32 = from.iter().map(|layer| layer.weight).sum();
            for layer in &mut from {
                layer.weight /= total;
            }
        }
        self.started = (!reduced && !from.is_empty()).then_some(self.paused_at.unwrap_or(now));
        self.from = if self.started.is_some() { from } else { vec![] };
        self.target = Some(Layer {
            key,
            artwork,
            weight: 1.,
        });
    }
    pub fn animating(&self, now: Instant) -> bool {
        self.paused_at.is_none() && self.started.is_some() && self.progress(now) < 1.
    }
    pub fn set_paused(&mut self, paused: bool, now: Instant) {
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(now),
            (false, Some(at)) => {
                if let Some(start) = &mut self.started {
                    *start += now.saturating_duration_since(at);
                }
                self.paused_at = None;
            }
            _ => {}
        }
    }
    pub fn accent(&self, now: Instant) -> [f32; 3] {
        let layers = self.layers(now);
        if layers.is_empty() {
            return NEUTRAL_ACCENT;
        }
        let mut accent = [0.; 3];
        for layer in layers {
            for (channel, value) in accent.iter_mut().enumerate() {
                *value += layer.artwork.accent[channel] * layer.weight;
            }
        }
        accent
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Layout {
    pub width: f32,
    pub bottom: f32,
    pub cover_height: f32,
    pub cover_width: f32,
    pub step: f32,
    pub left: f32,
    pub title_top: f32,
    pub title_size: f32,
}
#[derive(Clone, Copy)]
pub(crate) struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub focus: f32,
    pub opacity: f32,
}
impl Layout {
    pub fn new(width: f32, height: f32) -> Self {
        let width = width.max(1.);
        let height = height.max(1.);
        let cover_height = (height * 0.226).clamp(112., 224.).min(width * 0.21);
        let cover_width = cover_height * 2. / 3.;
        let bottom = height - (height * 0.038).clamp(20., 38.);
        let step = cover_width + (width * 0.027).clamp(16., 42.);
        Self {
            width,
            bottom,
            cover_height,
            cover_width,
            step,
            left: (width * 0.03).clamp(24., 48.),
            title_top: bottom - cover_height - 88.,
            title_size: (width * 0.023).clamp(26., 38.),
        }
    }
    pub fn place(&self, offset: f32) -> Placement {
        let focus = (1. - offset.abs()).clamp(0., 1.);
        let scale = 1. + focus * 0.10;
        let width = self.cover_width * scale;
        let height = self.cover_height * scale;
        Placement {
            x: self.width / 2. + offset * self.step - width / 2.,
            y: self.bottom - height - focus * 8.,
            width,
            height,
            focus,
            opacity: (3. - offset.abs()).clamp(0., 1.),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filmstrip_fits_and_reserves_title_space_at_desktop_sizes() {
        for (w, h) in [(960., 620.), (1280., 820.), (1920., 1080.), (2560., 1440.)] {
            let layout = Layout::new(w, h);
            assert!(layout.title_top > 44.);
            assert!(layout.title_top + layout.title_size * 1.25 < layout.place(0.).y);
            assert!(layout.place(-2.).x > 16.);
            assert!(layout.place(2.).x + layout.cover_width < w - 16.);
            assert!(layout.bottom < h - 16.);
            assert!(layout.place(0.).y < layout.place(1.).y);
        }
    }
    #[test]
    fn palette_ignores_letterboxing_and_follows_artwork_hue() {
        let red = artwork_accent(&RgbaImage::from_pixel(
            48,
            32,
            image::Rgba([180, 45, 20, 255]),
        ));
        let blue = artwork_accent(&RgbaImage::from_pixel(
            48,
            32,
            image::Rgba([20, 45, 180, 255]),
        ));
        assert!(red[0] > red[2] + 0.4 && blue[2] > blue[0] + 0.4);
        assert_eq!(artwork_accent(&RgbaImage::new(48, 32)), NEUTRAL_ACCENT);
    }
    fn art(color: [f32; 3]) -> Artwork {
        Artwork {
            image: None,
            accent: color,
            luminance: 0.5,
        }
    }
    fn key(name: &str) -> Option<ArtworkKey> {
        Some(ArtworkKey::Landscape(name.into()))
    }
    #[test]
    fn crossfade_retargets_from_displayed_lighting_and_freezes_in_background() {
        let now = Instant::now();
        let mut scene = Scene::default();
        scene.update(key("a"), art([1., 0., 0.]), false, now);
        scene.update(key("b"), art([0., 0., 1.]), false, now);
        let halfway = now + TRANSITION / 2;
        assert_eq!(scene.accent(halfway), [0.5, 0., 0.5]);
        let before = scene.accent(halfway);
        scene.update(key("c"), art([0., 1., 0.]), false, halfway);
        assert_eq!(scene.accent(halfway), before);
        scene.set_paused(true, halfway);
        let later = halfway + Duration::from_secs(600);
        assert_eq!(scene.accent(later), before);
        assert!(!scene.animating(later));
        scene.set_paused(false, later);
        assert_eq!(scene.accent(later), before);
        assert_eq!(scene.accent(later + TRANSITION), [0., 1., 0.]);
        scene.update(key("d"), art([1., 1., 0.]), true, later);
        assert_eq!(scene.accent(later), [1., 1., 0.]);
        assert!(!scene.animating(later));
        for index in 0..100 {
            scene.update(
                key(&index.to_string()),
                art([1., 0., 0.]),
                false,
                later + Duration::from_millis(index * 20),
            );
            assert!(scene.layers(later).len() <= 3);
        }
    }
}
