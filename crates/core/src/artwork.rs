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
            write_image(
                &image.thumbnail(image.width().min(3840), image.height().min(2160)),
                &asset.image_path,
            )?;
        }
        cache_images(&image, &asset)?;
    }
    Ok(asset)
}

fn cache_missing(asset: &ArtworkAsset) -> bool {
    !asset.thumbnail_path.is_file()
}

fn cache_images(image: &image::DynamicImage, asset: &ArtworkAsset) -> Result<(), ApiError> {
    if !asset.thumbnail_path.is_file() {
        write_image(&image.thumbnail(640, 960), &asset.thumbnail_path)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artwork_preserves_original_pixels_and_restores_thumbnails() {
        let directory =
            std::env::temp_dir().join(format!("startpoint-artwork-{}", uuid::Uuid::new_v4()));
        let paths = ApplicationPaths {
            data_directory: directory.join("data"),
            cache_directory: directory.join("cache"),
        };
        let original = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            768,
            248,
            image::Rgba([80, 120, 160, 200]),
        ));
        let mut bytes = Cursor::new(Vec::new());
        original
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("encoded artwork");
        let asset = save_asset(
            bytes.get_ref(),
            ArtworkKind::Background,
            "local",
            None,
            "Local".into(),
            &paths,
        )
        .expect("saved artwork");
        assert!(
            image::open(&asset.image_path)
                .expect("original image")
                .to_rgba8()
                == original.to_rgba8()
        );
        let thumbnail = image::open(&asset.thumbnail_path)
            .expect("thumbnail")
            .to_rgba8();
        assert_eq!(thumbnail.dimensions(), (640, 207));
        assert!(!restore_thumbnail(&asset, &paths).expect("existing thumbnail"));
        fs::remove_file(&asset.thumbnail_path).expect("removed disposable thumbnail");
        assert!(restore_thumbnail(&asset, &paths).expect("restored thumbnail"));
        assert_eq!(
            image::open(&asset.thumbnail_path)
                .expect("restored image")
                .to_rgba8(),
            thumbnail
        );
        assert!(matches!(
            restore_thumbnail(
                &ArtworkAsset {
                    thumbnail_path: paths.cache_directory.join("outside.png"),
                    ..asset
                },
                &paths
            ),
            Err(ApiError::Validation(_))
        ));
        fs::remove_dir_all(directory).expect("removed temporary artwork");
    }
}
