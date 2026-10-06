use image::{ImageFormat, ImageReader, Limits};
use reqwest::{Url, blocking::Client};
use sha2::{Digest, Sha256};
use startpoint_sdk::{ApiError, ApplicationPaths, ArtworkAsset, ArtworkKind, JobContext};
use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

const MAXIMUM_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
pub(crate) const MAXIMUM_IMAGE_DIMENSION: u32 = 8192;
pub(crate) const MAXIMUM_DECODE_BYTES: u64 = 128 * 1024 * 1024;

pub(crate) fn image_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::Io(error.to_string())
}

pub(crate) fn download(
    client: &Client,
    url: &str,
    hosts: &[String],
    context: &JobContext,
) -> Result<Vec<u8>, ApiError> {
    context.checkpoint()?;
    let mut parsed = Url::parse(url).map_err(image_error)?;
    if !allowed_url(&parsed, hosts) {
        return Err(ApiError::Validation(
            "Artwork must use HTTPS on a registered provider's image host".into(),
        ));
    }
    let mut response = client.get(parsed.clone()).send().map_err(image_error)?;
    for _ in 0..4 {
        if !response.status().is_redirection() {
            break;
        }
        context.checkpoint()?;
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| ApiError::Provider("Invalid artwork redirect".into()))?;
        parsed = parsed.join(location).map_err(image_error)?;
        if !allowed_url(&parsed, hosts) {
            return Err(ApiError::Validation(
                "Artwork redirected outside the provider's approved hosts".into(),
            ));
        }
        response = client.get(parsed.clone()).send().map_err(image_error)?;
    }
    if !response.status().is_success() {
        return Err(ApiError::Provider(format!(
            "Artwork download returned HTTP {}",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAXIMUM_IMAGE_BYTES)
    {
        return Err(ApiError::Validation(
            "Artwork exceeds the 20 MB image limit".into(),
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(MAXIMUM_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(image_error)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_IMAGE_BYTES {
        return Err(ApiError::Validation(
            "Artwork exceeds the 20 MB image limit".into(),
        ));
    }
    context.checkpoint()?;
    Ok(bytes)
}

fn allowed_url(url: &Url, hosts: &[String]) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url
            .host_str()
            .is_some_and(|host| hosts.iter().any(|approved| approved == host))
}

pub(crate) fn read_local(path: &Path) -> Result<Vec<u8>, ApiError> {
    let file = fs::File::open(path).map_err(image_error)?;
    if file.metadata().map_err(image_error)?.len() > MAXIMUM_IMAGE_BYTES {
        return Err(ApiError::Validation(
            "Artwork exceeds the 20 MB image limit".into(),
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAXIMUM_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(image_error)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_IMAGE_BYTES {
        return Err(ApiError::Validation(
            "Artwork exceeds the 20 MB image limit".into(),
        ));
    }
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Result<image::DynamicImage, ApiError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(image_error)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAXIMUM_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAXIMUM_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAXIMUM_DECODE_BYTES);
    reader.limits(limits);
    reader.decode().map_err(image_error)
}

pub(crate) fn save_preview(bytes: &[u8], paths: &ApplicationPaths) -> Result<PathBuf, ApiError> {
    let id = format!("{:x}", Sha256::digest(bytes));
    let path = paths
        .cache_directory
        .join("thumbnails")
        .join(format!("{id}.png"));
    if !path.is_file() {
        write_image(&decode(bytes)?.thumbnail(320, 480), &path)?;
    }
    Ok(path)
}

pub(crate) fn save_asset(
    bytes: &[u8],
    kind: ArtworkKind,
    provider_id: &str,
    source_url: Option<String>,
    author: String,
    paths: &ApplicationPaths,
) -> Result<ArtworkAsset, ApiError> {
    let id = format!("{:x}", Sha256::digest(bytes));
    let image_path = paths
        .data_directory
        .join("artwork")
        .join(format!("{id}.full-v2.png"));
    let thumbnail_path = paths
        .cache_directory
        .join("thumbnails")
        .join(format!("{id}.v2.png"));
    let asset = ArtworkAsset {
        id,
        kind,
        image_path,
        thumbnail_path,
        provider_id: provider_id.into(),
        source_url,
        author,
    };
    if !asset.image_path.is_file() || cache_missing(&asset) {
        let image = decode(bytes)?;
        if !asset.image_path.is_file() {
            write_image(&image.thumbnail(3840, 2160), &asset.image_path)?;
        }
        cache_images(&image, &asset)?;
    }
    Ok(asset)
}

fn cache_missing(asset: &ArtworkAsset) -> bool {
    !asset.thumbnail_path.is_file()
        || !asset.dithered_path().is_file()
        || (matches!(asset.kind, ArtworkKind::Cover | ArtworkKind::Background)
            && (!asset.recents_path().is_file() || !asset.recents_dithered_path().is_file()))
}

fn cache_images(image: &image::DynamicImage, asset: &ArtworkAsset) -> Result<(), ApiError> {
    if !asset.thumbnail_path.is_file() {
        write_image(&image.thumbnail(640, 960), &asset.thumbnail_path)?;
    }
    if !asset.dithered_path().is_file() {
        write_image(
            &dither(&image.thumbnail(1600, 1600)),
            &asset.dithered_path(),
        )?;
    }
    if matches!(asset.kind, ArtworkKind::Cover | ArtworkKind::Background) {
        for (path, dithered) in [
            (asset.recents_path(), false),
            (asset.recents_dithered_path(), true),
        ] {
            if !path.is_file() {
                write_image(&recents_fade(&image.thumbnail(3840, 2160), dithered), &path)?;
            }
        }
    }
    Ok(())
}

fn write_image(image: &image::DynamicImage, path: &Path) -> Result<(), ApiError> {
    let directory = path
        .parent()
        .ok_or_else(|| ApiError::Io("Missing image directory".into()))?;
    fs::create_dir_all(directory).map_err(image_error)?;
    let temporary = directory.join(format!("{}.partial", uuid::Uuid::new_v4()));
    let result = image
        .save_with_format(&temporary, ImageFormat::Png)
        .map_err(image_error)
        .and_then(|_| fs::rename(&temporary, path).map_err(image_error));
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    if result.is_err() && path.is_file() {
        return Ok(());
    }
    result
}

pub(crate) fn restore_thumbnail(
    asset: &ArtworkAsset,
    paths: &ApplicationPaths,
) -> Result<bool, ApiError> {
    if !asset
        .thumbnail_path
        .starts_with(paths.cache_directory.join("thumbnails"))
        || asset
            .thumbnail_path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(ApiError::Validation("Invalid cached thumbnail path".into()));
    }
    if !cache_missing(asset) {
        return Ok(false);
    }
    let bytes = read_local(&asset.image_path)?;
    let image = decode(&bytes)?;
    cache_images(&image, asset)?;
    Ok(true)
}

fn recents_fade(image: &image::DynamicImage, dithered: bool) -> image::DynamicImage {
    let mut pixels = image.to_rgba8();
    let width = pixels.width().max(1);
    let height = pixels.height().max(1);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let horizontal = x * 256 / width;
        let vertical = y * 256 / height;
        let information_fade =
            (256 - smooth_step(77, 174, horizontal)) * smooth_step(51, 159, vertical) / 256;
        let bottom_fade = smooth_step(174, 252, vertical);
        let coverage = 256 - information_fade.max(bottom_fade);
        if !dithered {
            pixel[3] = u8::try_from(u32::from(pixel[3]) * coverage / 256).unwrap_or_default();
            continue;
        }
        let mut noise = (x / 2).wrapping_mul(0x9e37_79b9) ^ (y / 2).wrapping_mul(0x85eb_ca6b);
        noise ^= noise >> 16;
        noise = noise.wrapping_mul(0x7feb_352d);
        noise ^= noise >> 15;
        noise = noise.wrapping_mul(0x846c_a68b);
        noise ^= noise >> 16;
        let threshold = noise >> 24;
        if threshold >= coverage {
            pixel[3] = 0;
        }
    }
    image::DynamicImage::ImageRgba8(pixels)
}

fn smooth_step(start: u32, end: u32, value: u32) -> u32 {
    // Inputs and opacity use a 0–256 fixed-point scale.
    let fraction = (value.saturating_sub(start) * 256 / (end - start)).min(256);
    fraction * fraction * (768 - 2 * fraction) / 65_536
}

fn dither(image: &image::DynamicImage) -> image::DynamicImage {
    const THRESHOLDS: [[u16; 4]; 4] =
        [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let mut pixels = image.to_rgba8();
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let column = usize::try_from(x / 2 % 4).unwrap_or_default();
        let row = usize::try_from(y / 2 % 4).unwrap_or_default();
        let threshold = (THRESHOLDS[row][column] * 2 + 1) * 255;
        let peak = u16::from(pixel[0].max(pixel[1]).max(pixel[2]));
        for channel in &mut pixel.0[..3] {
            let value = if peak * 32 > threshold {
                u16::from(*channel) * 255 / peak.max(1)
            } else {
                u16::from(*channel) / 12
            };
            *channel = u8::try_from(value).unwrap_or(255);
        }
    }
    image::DynamicImage::ImageRgba8(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dithering_preserves_dimensions_alpha_and_original_pixels() {
        let original = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            8,
            8,
            image::Rgba([180, 90, 45, 170]),
        ));
        let result = dither(&original).to_rgba8();
        assert_eq!(result.dimensions(), (8, 8));
        assert!(result.pixels().all(|pixel| pixel[3] == 170));
        assert!(result.pixels().any(|pixel| pixel[0] == 255));
        assert!(result.pixels().any(|pixel| pixel[0] == 15));
        assert_eq!(original.to_rgba8().get_pixel(0, 0).0, [180, 90, 45, 170]);
        let banner = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            384,
            124,
            image::Rgba([80, 120, 160, 200]),
        ));
        let faded = recents_fade(&banner, true).to_rgba8();
        assert_eq!(faded.dimensions(), (384, 124));
        assert_eq!(faded.get_pixel(250, 10).0, [80, 120, 160, 200]);
        assert_eq!(faded.get_pixel(0, 100)[3], 0);
        assert_eq!(faded.get_pixel(250, 123)[3], 0);
        assert!(
            faded
                .enumerate_pixels()
                .filter(|(x, y, _)| *x <= 100 && *y >= 80)
                .all(|(_, _, pixel)| pixel[3] == 0)
        );
        assert!(
            faded
                .pixels()
                .all(|pixel| pixel.0[..3] == [80, 120, 160] && matches!(pixel[3], 0 | 200))
        );
        assert_eq!(faded, recents_fade(&banner, true).to_rgba8());
        let smooth = recents_fade(&banner, false).to_rgba8();
        assert_eq!(smooth.get_pixel(250, 10).0, [80, 120, 160, 200]);
        assert_eq!(smooth.get_pixel(0, 100)[3], 0);
        assert_eq!(smooth.get_pixel(250, 123)[3], 0);
        assert!(smooth.pixels().any(|pixel| pixel[3] > 0 && pixel[3] < 200));
    }
}
