//! Display-only logo normalization. Originals and artwork history stay untouched.
use gpui::{App, Context};
use image::{DynamicImage, ImageFormat, ImageReader, Limits, RgbaImage};
use startpoint_sdk::ArtworkAsset;
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(crate) struct Logo {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone)]
pub(crate) enum State {
    Pending,
    Ready(Logo),
    Failed,
}

pub(crate) struct LogoCache {
    directory: PathBuf,
    entries: HashMap<PathBuf, State>,
    active: usize,
}

impl LogoCache {
    pub fn new(cache_directory: &Path) -> Self {
        Self {
            directory: cache_directory.join("logos"),
            entries: HashMap::new(),
            active: 0,
        }
    }

    pub fn load(&mut self, asset: &ArtworkAsset, cx: &mut Context<Self>) -> State {
        if let Some(state) = self.entries.get(&asset.image_path) {
            return state.clone();
        }
        // Bound simultaneous image decodes when selection changes rapidly.
        if self.active >= 2 {
            return State::Pending;
        }
        let source = asset.image_path.clone();
        let directory = self.directory.clone();
        self.entries.insert(source.clone(), State::Pending);
        self.active += 1;
        let task = cx
            .background_executor()
            .spawn(async move { prepare_logo(&source, &directory) });
        let source = asset.image_path.clone();
        cx.spawn(async move |cache, cx| {
            let result = task.await;
            let _ = cache.update(cx, |cache, cx| {
                cache.active = cache.active.saturating_sub(1);
                let state = match result {
                    Ok(logo) => State::Ready(logo),
                    Err(_) => {
                        crate::diagnostics::record(
                            "detail logo normalization failed; using text title",
                        );
                        State::Failed
                    }
                };
                cache.entries.insert(source, state);
                cx.notify();
                cx.refresh_windows();
            });
        })
        .detach();
        State::Pending
    }

    pub fn invalidate_failed(&mut self, _: &mut App) {
        self.entries
            .retain(|_, state| !matches!(state, State::Failed));
    }
}

fn prepare_logo(source: &Path, directory: &Path) -> Result<Logo, String> {
    let mut key = DefaultHasher::new();
    source.hash(&mut key);
    let path = directory.join(format!("{:016x}.trim-v1.png", key.finish()));
    if let Ok((width, height)) = image::image_dimensions(&path)
        && width > 0
        && height > 0
    {
        return Ok(Logo {
            path,
            width,
            height,
        });
    }
    let file = std::fs::File::open(source).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > 40 * 1024 * 1024 {
        return Err("Logo exceeds the display decode limit".into());
    }
    let mut reader = ImageReader::new(std::io::BufReader::new(file))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| error.to_string())?
        .into_rgba8();
    let (x, y, width, height) = visible_bounds(&image).ok_or("Logo has no visible pixels")?;
    let cropped = image::imageops::crop_imm(&image, x, y, width, height).to_image();
    let normalized = DynamicImage::ImageRgba8(cropped).thumbnail(width.min(1024), height.min(1024));
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let temporary = path.with_extension(format!("{}.partial", std::process::id()));
    let result = normalized
        .save_with_format(&temporary, ImageFormat::Png)
        .map_err(|error| error.to_string())
        .and_then(|_| std::fs::rename(&temporary, &path).map_err(|error| error.to_string()));
    let _ = std::fs::remove_file(&temporary);
    if result.is_err() && !path.is_file() {
        result?;
    }
    let (width, height) = image::image_dimensions(&path).map_err(|error| error.to_string())?;
    Ok(Logo {
        path,
        width,
        height,
    })
}

fn visible_bounds(image: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (mut left, mut top, mut right, mut bottom) = (image.width(), image.height(), 0, 0);
    let mut found = false;
    for (x, y, pixel) in image.enumerate_pixels() {
        // Ignore almost-invisible padding/glow noise, not dark logo colors.
        if pixel[3] <= 8 {
            continue;
        }
        found = true;
        left = left.min(x);
        top = top.min(y);
        right = right.max(x);
        bottom = bottom.max(y);
    }
    if !found {
        return None;
    }
    // Keep a small safety margin for antialiasing and soft outlines.
    left = left.saturating_sub(2);
    top = top.saturating_sub(2);
    right = right.saturating_add(2).min(image.width() - 1);
    bottom = bottom.saturating_add(2).min(image.height() - 1);
    Some((left, top, right - left + 1, bottom - top + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_asymmetric_transparent_padding_without_trimming_dark_opaque_artwork() {
        let mut image = RgbaImage::new(100, 80);
        for y in 20..50 {
            for x in 10..50 {
                image.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
            }
        }
        image.put_pixel(99, 0, image::Rgba([255, 255, 255, 1]));
        assert_eq!(visible_bounds(&image), Some((8, 18, 44, 34)));
        assert!(visible_bounds(&RgbaImage::new(10, 10)).is_none());
        assert_eq!(
            visible_bounds(&RgbaImage::from_pixel(5, 3, image::Rgba([0, 0, 0, 255]))),
            Some((0, 0, 5, 3))
        );
    }

    #[test]
    fn normalized_logo_is_a_cached_derivative_and_does_not_modify_the_original() {
        let directory =
            std::env::temp_dir().join(format!("startpoint-logo-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("original.png");
        let mut image = RgbaImage::new(100, 80);
        for y in 20..50 {
            for x in 10..50 {
                image.put_pixel(x, y, image::Rgba([60, 80, 100, 255]));
            }
        }
        image.save(&source).unwrap();
        let original = std::fs::read(&source).unwrap();
        let logo = prepare_logo(&source, &directory.join("cache")).unwrap();
        assert_eq!((logo.width, logo.height), (44, 34));
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert_eq!(
            prepare_logo(&source, &directory.join("cache"))
                .unwrap()
                .path,
            logo.path
        );
        std::fs::remove_file(logo.path).unwrap();
        std::fs::remove_file(source).unwrap();
        std::fs::remove_dir(directory.join("cache")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
