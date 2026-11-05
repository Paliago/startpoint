//! Exhibition geometry, interruptible carousel motion and artwork-derived floor light.
//! No library or window state lives here; rendering consumes these shared roles.
use gpui::{
    App, Asset, Bounds, Context, Corners, ImageCacheError, IntoElement, Pixels, RenderImage,
    Styled, Window, canvas, point, px, size,
};
use image::{Frame, RgbaImage};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) const TRANSITION: Duration = Duration::from_millis(440);
const REFLECTION_WIDTH: u32 = 128;
const REFLECTION_HEIGHT: u32 = 96;
const REFLECTION_GUTTER: u32 = 4;

/// GPUI's DirectX sprite atlas does not add filtering gutters. Sampling a small
/// stretched sprite at its tile edge can pick up an unrelated adjacent image.
/// Keep transparent texels around the derivative AND crop the UVs to its inner
/// content, so bilinear filtering only ever sees our own transparent guard.
pub(crate) fn reflection_image(image: Arc<RenderImage>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let _ = window.paint_image_fitted(
                bounds,
                reflection_fitted(bounds),
                Corners::all(px(0.)),
                image.clone(),
                0,
                false,
            );
        },
    )
    .size_full()
}

fn reflection_fitted(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    let gutter_x = bounds.size.width * (REFLECTION_GUTTER as f32 / REFLECTION_WIDTH as f32);
    let gutter_y = bounds.size.height * (REFLECTION_GUTTER as f32 / REFLECTION_HEIGHT as f32);
    Bounds::new(
        point(bounds.origin.x - gutter_x, bounds.origin.y - gutter_y),
        size(
            bounds.size.width + gutter_x * 2.,
            bounds.size.height + gutter_y * 2.,
        ),
    )
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Layout {
    pub width: f32,
    pub cover_height: f32,
    pub baseline: f32,
    pub gap: f32,
    pub title_top: f32,
    pub title_size: f32,
    pub play_top: f32,
    pub dots_top: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub shade: f32,
    pub opacity: f32,
}

impl Layout {
    pub fn new(width: f32, height: f32) -> Self {
        let gap = (width * 0.007).clamp(8., 14.);
        let title_size = (height * 0.033).clamp(24., 36.);
        let title_gap = (height * 0.058).clamp(26., 64.);
        let cover_height = (height * 0.595)
            .min((width - 64. - gap * 4.) / (3.48 * 2. / 3.))
            .min(height - height * 0.155 - title_gap - title_size * 1.2 - 18. - 60. - 28.)
            .max(1.);
        let baseline = height * 0.155 + cover_height;
        let title_top = baseline + title_gap;
        let play_top = title_top + title_size * 1.2 + 18.;
        Self {
            width,
            cover_height,
            baseline,
            gap,
            title_top,
            title_size,
            play_top,
            dots_top: play_top + 60.,
        }
    }

    pub fn place(self, offset: f32) -> Placement {
        let distance = offset.abs();
        let width = self.cover_height * 2. / 3.;
        let scale = interpolate(distance, [1., 0.70, 0.54, 0.44]);
        let shift = interpolate(
            distance,
            [
                0.,
                width * 0.85 + self.gap,
                width * 1.47 + self.gap * 2.,
                width * 1.96 + self.gap * 3.,
            ],
        );
        let lift = self.cover_height * interpolate(distance, [0., 0.065, 0.085, 0.10]);
        Placement {
            x: self.width / 2. + shift * offset.signum() - width * scale / 2.,
            y: self.baseline - lift - self.cover_height * scale,
            width: width * scale,
            height: self.cover_height * scale,
            shade: interpolate(distance, [0., 0.08, 0.20, 0.40]),
            opacity: (3. - distance).clamp(0., 1.),
        }
    }
}

fn interpolate(distance: f32, values: [f32; 4]) -> f32 {
    let distance = distance.clamp(0., 3.);
    let index = (distance.floor() as usize).min(2);
    values[index] + (values[index + 1] - values[index]) * (distance - index as f32)
}

pub(crate) fn relative(index: usize, position: f32, count: usize) -> f32 {
    let count = count.max(1) as f32;
    (index as f32 - position + count / 2.).rem_euclid(count) - count / 2.
}

#[derive(Default)]
pub(crate) struct PageWheel {
    accumulated: f32,
    last_event: Option<Instant>,
    advanced: bool,
}

impl PageWheel {
    pub fn push(&mut self, x: f32, y: f32, now: Instant) -> Option<bool> {
        let delta = if x.abs() > y.abs() { x } else { y };
        if !delta.is_finite() || delta == 0. {
            return None;
        }
        if self
            .last_event
            .is_none_or(|last| now.saturating_duration_since(last) > Duration::from_millis(220))
        {
            self.accumulated = 0.;
            self.advanced = false;
        }
        self.last_event = Some(now);
        if self.advanced {
            return None;
        }
        if delta.signum() != self.accumulated.signum() {
            self.accumulated = 0.;
        }
        self.accumulated += delta;
        if self.accumulated.abs() < 48. {
            return None;
        }
        self.advanced = true;
        Some(self.accumulated < 0.)
    }
}

#[derive(Default)]
pub(crate) struct Wheel {
    accumulated: f32,
    last_event: Option<Instant>,
    last_step: Option<Instant>,
}

impl Wheel {
    pub fn push(&mut self, x: f32, y: f32, now: Instant) -> Option<bool> {
        if self
            .last_event
            .is_none_or(|last| now.saturating_duration_since(last) > Duration::from_millis(220))
        {
            self.accumulated = 0.;
        }
        self.last_event = Some(now);
        if self
            .last_step
            .is_some_and(|last| now.saturating_duration_since(last) < Duration::from_millis(180))
        {
            return None;
        }
        self.accumulated += if x.abs() > y.abs() { x } else { y };
        if self.accumulated.abs() < 48. {
            return None;
        }
        let forward = self.accumulated < 0.;
        self.accumulated = 0.;
        self.last_step = Some(now);
        Some(forward)
    }
}

#[derive(Default)]
pub(crate) struct Motion {
    target: usize,
    count: usize,
    from: f32,
    to: f32,
    started: Option<Instant>,
    paused_at: Option<Instant>,
}

impl Motion {
    pub fn set_paused(&mut self, paused: bool, now: Instant) {
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(now),
            (false, Some(since)) => {
                if let Some(start) = &mut self.started {
                    *start += now.saturating_duration_since(since);
                }
                self.paused_at = None;
            }
            _ => {}
        }
    }

    pub fn update(&mut self, index: usize, count: usize, reduced_motion: bool, now: Instant) {
        let now = self.paused_at.unwrap_or(now);
        let index = index.min(count.saturating_sub(1));
        if self.count != count || reduced_motion {
            *self = Self {
                target: index,
                count,
                from: index as f32,
                to: index as f32,
                started: None,
                paused_at: self.paused_at,
            };
        } else if self.target != index {
            let current = self.position(now);
            let delta = relative(index, current, count);
            self.from = current;
            self.to = current + delta;
            self.target = index;
            self.started = Some(now);
        } else if !self.animating(now) {
            self.from = index as f32;
            self.to = index as f32;
            self.started = None;
        }
    }

    pub fn position(&self, now: Instant) -> f32 {
        let now = self.paused_at.unwrap_or(now);
        let progress = self.started.map_or(1., |start| {
            (now.saturating_duration_since(start).as_secs_f32() / TRANSITION.as_secs_f32())
                .clamp(0., 1.)
        });
        let eased = progress * progress * (3. - 2. * progress);
        self.from + (self.to - self.from) * eased
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.paused_at.is_none()
            && self
                .started
                .is_some_and(|start| now.saturating_duration_since(start) < TRANSITION)
    }
}

/// Fixed-size, off-thread derivatives. Animate the resulting GPU image, never
/// decode or blur a cover on each animation frame or live-resize pixel.
#[derive(Default)]
pub(crate) struct Reflections {
    recent: VecDeque<PathBuf>,
    frame: Vec<PathBuf>,
    cleanup_scheduled: bool,
}

impl Reflections {
    pub fn image(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        self.recent.retain(|old| old != &path);
        self.recent.push_back(path.clone());
        if !self.frame.contains(&path) {
            self.frame.push(path.clone());
        }
        if !self.cleanup_scheduled {
            self.cleanup_scheduled = true;
            let entity = cx.weak_entity();
            window.on_next_frame(move |window, cx| {
                let _ = entity.update(cx, |state, cx| state.finish_frame(window, cx));
            });
        }
        window
            .use_asset::<Reflection>(&path, cx)
            .and_then(Result::ok)
    }

    fn finish_frame(&mut self, window: &mut Window, cx: &mut App) {
        self.cleanup_scheduled = false;
        let pinned = std::mem::take(&mut self.frame);
        let mut changed = false;
        while self.recent.len() > 12 {
            let Some(index) = self.recent.iter().position(|path| !pinned.contains(path)) else {
                break;
            };
            let path = self.recent.remove(index).unwrap();
            if let Some(Ok(image)) = window.get_asset::<Reflection>(&path, cx) {
                let _ = window.drop_image(image);
            }
            cx.remove_asset::<Reflection>(&path);
            changed = true;
        }
        if changed {
            window.refresh();
        }
    }
}

struct Reflection;
impl Asset for Reflection {
    type Source = PathBuf;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;

    fn load(
        path: Self::Source,
        _: &mut App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        async move {
            // Missing/unreadable art has no light, rather than an invented accent.
            let mut pixels = image::open(path)
                .ok()
                .map(|art| {
                    reflection_pixels(
                        &art.resize_to_fill(128, 192, image::imageops::FilterType::Triangle)
                            .to_rgba8(),
                    )
                })
                .unwrap_or_else(|| {
                    RgbaImage::new(
                        REFLECTION_WIDTH + REFLECTION_GUTTER * 2,
                        REFLECTION_HEIGHT + REFLECTION_GUTTER * 2,
                    )
                });
            for pixel in pixels.pixels_mut() {
                pixel.0.swap(0, 2);
            }
            Ok(Arc::new(RenderImage::new([Frame::new(pixels)])))
        }
    }
}

fn reflection_pixels(cover: &RgbaImage) -> RgbaImage {
    let floor = RgbaImage::from_fn(REFLECTION_WIDTH, REFLECTION_HEIGHT, |x, y| {
        let source_x = x * cover.width() / 128;
        let source_y = cover.height().saturating_sub(1 + y * cover.height() / 192);
        *cover.get_pixel(source_x.min(cover.width() - 1), source_y)
    });
    let blurred = image::imageops::blur(&floor, 6.);
    let reflection = RgbaImage::from_fn(REFLECTION_WIDTH, REFLECTION_HEIGHT, |x, y| {
        let mut pixel = *blurred.get_pixel(x, y);
        let vertical = (1. - y as f32 / 95.).powi(2);
        let edge = ((x.min(127 - x) as f32) / 20.).clamp(0., 1.);
        pixel[3] = (f32::from(pixel[3]) * 0.38 * vertical * edge).round() as u8;
        if pixel[3] == 0 {
            pixel.0 = [0; 4];
        }
        pixel
    });
    let mut padded = RgbaImage::new(
        REFLECTION_WIDTH + REFLECTION_GUTTER * 2,
        REFLECTION_HEIGHT + REFLECTION_GUTTER * 2,
    );
    image::imageops::replace(
        &mut padded,
        &reflection,
        i64::from(REFLECTION_GUTTER),
        i64::from(REFLECTION_GUTTER),
    );
    padded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_portraits_fit_with_space_for_title_play_and_navigation() {
        for (width, height) in [
            (960., 620.),
            (1280., 820.),
            (1586., 992.),
            (2560., 1440.),
            (960., 1200.),
        ] {
            let layout = Layout::new(width, height);
            let center = layout.place(0.);
            assert!(center.y >= 64.);
            assert!(layout.place(-2.).x >= 16.);
            let last = layout.place(2.);
            assert!(last.x + last.width <= width - 16.);
            assert!(layout.dots_top + 22. < height);
            assert!(layout.title_top > center.y + center.height);
            for offset in [0., 1., 2.] {
                let card = layout.place(offset);
                assert!((card.width / card.height - 2. / 3.).abs() < 0.001);
            }
            assert!(center.height > layout.place(1.).height);
            assert!(layout.place(1.).height > last.height);
        }
    }

    #[test]
    fn interrupted_motion_is_continuous_and_wraps_by_the_shortest_route() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.update(0, 15, false, now);
        motion.update(14, 15, false, now);
        let midway = now + TRANSITION / 2;
        assert!((motion.position(midway) + 0.5).abs() < 0.001);
        let before = motion.position(midway);
        motion.update(13, 15, false, midway);
        assert_eq!(motion.position(midway), before);
        assert!(motion.animating(midway));
        assert!((relative(13, motion.position(midway + TRANSITION), 15)).abs() < 0.001);
        motion.update(4, 15, true, midway);
        assert_eq!(motion.position(midway), 4.);
        assert!(!motion.animating(midway));
        motion.update(0, 0, false, now);
        assert!(motion.position(now).is_finite());
    }

    #[test]
    fn floor_uses_the_actual_artwork_colors_and_fades_to_transparent() {
        let red = RgbaImage::from_pixel(32, 48, image::Rgba([230, 30, 10, 255]));
        let floor = reflection_pixels(&red);
        let pixel = |x, y| floor.get_pixel(x + REFLECTION_GUTTER, y + REFLECTION_GUTTER);
        assert_eq!(&pixel(64, 0).0[..3], &[230, 30, 10]);
        assert!(pixel(64, 0)[3] > pixel(64, 48)[3]);
        assert_eq!(pixel(64, 95)[3], 0);
        assert_eq!(pixel(0, 0)[3], 0);
        let transparent = RgbaImage::new(32, 48);
        assert!(
            reflection_pixels(&transparent)
                .pixels()
                .all(|pixel| pixel[3] == 0)
        );
    }

    #[test]
    fn motion_freezes_and_resumes_without_spending_background_time() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.update(0, 5, false, now);
        motion.update(1, 5, false, now);
        let halfway = now + TRANSITION / 2;
        motion.set_paused(true, halfway);
        let frozen = motion.position(halfway);
        let later = halfway + Duration::from_secs(600);
        assert_eq!(motion.position(later), frozen);
        assert!(!motion.animating(later));
        motion.set_paused(false, later);
        assert_eq!(motion.position(later), frozen);
        assert!(motion.animating(later));
        assert!((motion.position(later + TRANSITION / 2) - 1.).abs() < 0.001);
    }

    #[test]
    fn reflection_uvs_stay_inside_a_clear_guard_at_every_size_and_fractional_position() {
        let floor = reflection_pixels(&RgbaImage::from_pixel(128, 192, image::Rgba([255; 4])));
        for (x, y, pixel) in floor.enumerate_pixels() {
            if x < REFLECTION_GUTTER
                || x >= floor.width() - REFLECTION_GUTTER
                || y < REFLECTION_GUTTER
                || y >= floor.height() - REFLECTION_GUTTER
            {
                assert_eq!(pixel.0, [0; 4]);
            }
        }
        for scale in [0.5, 1., 1.25, 1.5, 2., 3.] {
            let visible = Bounds::new(
                point(px(21.37 * scale), px(600.14 * scale)),
                size(px(377.21 * scale), px(156.42 * scale)),
            );
            let fitted = reflection_fitted(visible);
            let x = f32::from(visible.origin.x - fitted.origin.x) / f32::from(fitted.size.width)
                * floor.width() as f32;
            let y = f32::from(visible.origin.y - fitted.origin.y) / f32::from(fitted.size.height)
                * floor.height() as f32;
            let width =
                f32::from(visible.size.width) / f32::from(fitted.size.width) * floor.width() as f32;
            let height = f32::from(visible.size.height) / f32::from(fitted.size.height)
                * floor.height() as f32;
            assert_eq!(
                (
                    x.round() as u32,
                    y.round() as u32,
                    width.round() as u32,
                    height.round() as u32
                ),
                (
                    REFLECTION_GUTTER,
                    REFLECTION_GUTTER,
                    REFLECTION_WIDTH,
                    REFLECTION_HEIGHT
                )
            );
        }
    }

    #[test]
    fn page_wheel_advances_once_until_the_gesture_and_momentum_end() {
        let now = Instant::now();
        let mut wheel = PageWheel::default();
        assert_eq!(wheel.push(0., -24., now), None);
        assert_eq!(
            wheel.push(0., -26., now + Duration::from_millis(10)),
            Some(true)
        );
        for elapsed in (30..1600).step_by(30) {
            assert_eq!(
                wheel.push(0., -60., now + Duration::from_millis(elapsed)),
                None
            );
        }
        assert_eq!(
            wheel.push(60., 0., now + Duration::from_millis(1900)),
            Some(false)
        );
        assert_eq!(
            wheel.push(f32::NAN, 0., now + Duration::from_millis(2200)),
            None
        );
    }

    #[test]
    fn wheel_accepts_both_axes_and_does_not_skip_games_from_trackpad_momentum() {
        let now = Instant::now();
        let mut wheel = Wheel::default();
        assert_eq!(wheel.push(0., -24., now), None);
        assert_eq!(
            wheel.push(0., -26., now + Duration::from_millis(10)),
            Some(true)
        );
        assert_eq!(wheel.push(0., -400., now + Duration::from_millis(30)), None);
        assert_eq!(
            wheel.push(60., 0., now + Duration::from_millis(250)),
            Some(false)
        );
    }
}
