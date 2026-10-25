use reqwest::{blocking::Client, redirect::Policy};
use serde::{Deserialize, de::DeserializeOwned};
use startpoint_sdk::{
    ApiError, ArtworkCandidate, ArtworkGame, ArtworkKind, ArtworkPage, ArtworkProvider, Game,
    JobContext, ProviderDescriptor,
};
use std::{io::Read, time::Duration};

pub struct SteamGridDbProvider {
    client: Client,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    limit: Option<u32>,
}

const IMAGE_PAGE_SIZE: u32 = 24;

fn direct_game_id(query: &str) -> Result<Option<u64>, ApiError> {
    let invalid = || {
        ApiError::Validation(
            "Use a SteamGridDB game URL or # followed by its numeric game ID.".into(),
        )
    };
    if let Some(id) = query
        .strip_prefix('#')
        .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return id
            .parse::<u64>()
            .ok()
            .filter(|id| *id != 0)
            .map(Some)
            .ok_or_else(invalid);
    }
    if query.contains("://") {
        let url = reqwest::Url::parse(query).map_err(|_| invalid())?;
        let parts = url.path().trim_matches('/').split('/').collect::<Vec<_>>();
        if url.scheme() != "https"
            || !matches!(
                url.host_str(),
                Some("steamgriddb.com" | "www.steamgriddb.com")
            )
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || parts.len() != 2
            || parts[0] != "game"
        {
            return Err(invalid());
        }
        return parts[1]
            .parse::<u64>()
            .ok()
            .filter(|id| *id != 0)
            .map(Some)
            .ok_or_else(invalid);
    }
    Ok(None)
}

#[derive(Deserialize)]
struct RemoteAuthor {
    name: String,
}

#[derive(Deserialize)]
struct RemoteGame {
    id: u64,
    name: String,
}

#[derive(Deserialize)]
struct RemoteImage {
    id: u64,
    url: String,
    thumb: String,
    author: RemoteAuthor,
    #[serde(default)]
    nsfw: bool,
    #[serde(default)]
    humor: bool,
    #[serde(default)]
    score: i32,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
}

impl SteamGridDbProvider {
    pub fn new() -> Result<Self, ApiError> {
        let client = Client::builder()
            .user_agent("Startpoint/0.1")
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .redirect(Policy::none())
            .build()
            .map_err(|error| ApiError::Provider(error.to_string()))?;
        Ok(Self { client })
    }

    fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        credential: &str,
        context: &JobContext,
    ) -> Result<T, ApiError> {
        self.get_envelope(path, credential, context)
            .map(|envelope: Envelope<T>| envelope.data)
    }

    fn get_envelope<T: DeserializeOwned>(
        &self,
        path: &str,
        credential: &str,
        context: &JobContext,
    ) -> Result<Envelope<T>, ApiError> {
        context.checkpoint()?;
        let response = self
            .client
            .get(format!("https://www.steamgriddb.com/api/v2/{path}"))
            .bearer_auth(credential)
            .send()
            .map_err(|error| ApiError::Provider(error.to_string()))?;
        match response.status().as_u16() {
            401 | 403 => return Err(ApiError::InvalidCredential("SteamGridDB rejected the API key. Update it in Settings.".into())),
            429 => return Err(ApiError::Provider("SteamGridDB is rate limiting requests. Try again later; cached artwork remains available.".into())),
            404 => return Err(ApiError::NoArtwork),
            _ if !response.status().is_success() => return Err(ApiError::Provider(format!("SteamGridDB returned HTTP {}", response.status()))),
            _ => {},
        }
        let mut bytes = Vec::new();
        response
            .take(2_000_001)
            .read_to_end(&mut bytes)
            .map_err(|error| ApiError::Provider(error.to_string()))?;
        if bytes.len() > 2_000_000 {
            return Err(ApiError::Provider(
                "SteamGridDB returned an unexpectedly large response".into(),
            ));
        }
        context.checkpoint()?;
        let envelope: Envelope<T> = serde_json::from_slice(&bytes).map_err(|error| {
            ApiError::Provider(format!("Invalid SteamGridDB response: {error}"))
        })?;
        Ok(envelope)
    }
}

impl ArtworkProvider for SteamGridDbProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "steamgriddb".into(),
            name: "SteamGridDB".into(),
        }
    }

    fn image_hosts(&self) -> Vec<String> {
        [
            "cdn2.steamgriddb.com",
            "cdn.steamgriddb.com",
            "www.steamgriddb.com",
            "steamgriddb.com",
            "s3.amazonaws.com",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    fn validate_credential(&self, credential: &str, context: &JobContext) -> Result<(), ApiError> {
        self.search("Portal", credential, context).map(|_| ())
    }

    fn game(
        &self,
        game: &Game,
        credential: &str,
        context: &JobContext,
    ) -> Result<ArtworkGame, ApiError> {
        let target = if let Some(id) = game.artwork_game_ids.get("steamgriddb") {
            if id.parse::<u64>().is_err() {
                return Err(ApiError::Validation(
                    "Invalid SteamGridDB game identifier".into(),
                ));
            }
            format!("id/{id}")
        } else if matches!(game.provider_id.as_str(), "steam" | "example")
            && game.source_game_id.parse::<u32>().is_ok()
        {
            format!("steam/{}", game.source_game_id)
        } else {
            let mut matches = self
                .search(&game.discovered_title, credential, context)?
                .into_iter()
                .filter(|candidate| candidate.name.eq_ignore_ascii_case(&game.discovered_title));
            let candidate = matches.next().ok_or(ApiError::NoArtwork)?;
            return if matches.next().is_none() {
                Ok(candidate)
            } else {
                Err(ApiError::NoArtwork)
            };
        };
        let game: RemoteGame = self.get(&format!("games/{target}"), credential, context)?;
        Ok(ArtworkGame {
            id: game.id.to_string(),
            name: game.name,
        })
    }

    fn search(
        &self,
        query: &str,
        credential: &str,
        context: &JobContext,
    ) -> Result<Vec<ArtworkGame>, ApiError> {
        let query = query.trim();
        if query.is_empty() || query.len() > 200 {
            return Err(ApiError::Validation(
                "Enter a game title of 1–200 bytes".into(),
            ));
        }
        if let Some(id) = direct_game_id(query)? {
            let game: RemoteGame = self.get(&format!("games/id/{id}"), credential, context)?;
            return Ok(vec![ArtworkGame {
                id: game.id.to_string(),
                name: game.name,
            }]);
        }
        let mut url =
            reqwest::Url::parse("https://www.steamgriddb.com/api/v2/search/autocomplete/")
                .map_err(|error| ApiError::Provider(error.to_string()))?;
        url.path_segments_mut()
            .map_err(|_| ApiError::Provider("Invalid search endpoint".into()))?
            .pop_if_empty()
            .push(query);
        let path = url
            .path()
            .strip_prefix("/api/v2/")
            .ok_or_else(|| ApiError::Provider("Invalid search endpoint".into()))?;
        let games: Vec<RemoteGame> = self.get(path, credential, context)?;
        Ok(games
            .into_iter()
            .map(|game| ArtworkGame {
                id: game.id.to_string(),
                name: game.name,
            })
            .collect())
    }

    fn artwork(
        &self,
        game: &Game,
        remote_game_id: Option<&str>,
        kind: ArtworkKind,
        credential: &str,
        context: &JobContext,
    ) -> Result<Vec<ArtworkCandidate>, ApiError> {
        self.artwork_page(game, remote_game_id, kind, 0, credential, context)
            .map(|page| page.images)
    }

    fn artwork_page(
        &self,
        game: &Game,
        remote_game_id: Option<&str>,
        kind: ArtworkKind,
        page: u32,
        credential: &str,
        context: &JobContext,
    ) -> Result<ArtworkPage, ApiError> {
        let resource = match kind {
            ArtworkKind::Cover => "grids",
            ArtworkKind::Background => "heroes",
            ArtworkKind::Logo => "logos",
            ArtworkKind::Icon => "icons",
        };
        let target = match remote_game_id
            .or_else(|| game.artwork_game_ids.get("steamgriddb").map(String::as_str))
        {
            Some(id) if id.parse::<u64>().is_ok() => format!("game/{id}"),
            Some(_) => {
                return Err(ApiError::Validation(
                    "Invalid SteamGridDB game identifier".into(),
                ));
            }
            None if matches!(game.provider_id.as_str(), "steam" | "example")
                && game.source_game_id.parse::<u32>().is_ok() =>
            {
                format!("steam/{}", game.source_game_id)
            }
            None => {
                return Err(ApiError::Validation(
                    "Search for this game on SteamGridDB and choose the correct match first".into(),
                ));
            }
        };
        let dimensions = match kind {
            ArtworkKind::Cover => "&dimensions=600x900",
            ArtworkKind::Background => "&dimensions=3840x1240,1920x620,1600x650",
            _ => "",
        };
        let mimes = match kind {
            ArtworkKind::Logo => "image/png,image/webp",
            ArtworkKind::Icon => "image/png",
            _ => "image/png,image/jpeg,image/webp",
        };
        let response: Envelope<Vec<RemoteImage>> = self.get_envelope(
            &format!("{resource}/{target}?types=static&mimes={mimes}&nsfw=false&humor=false&epilepsy=false&limit={IMAGE_PAGE_SIZE}&page={page}{dimensions}"),
            credential,
            context,
        )?;
        image_page(response, kind, page)
    }
}

fn image_page(
    response: Envelope<Vec<RemoteImage>>,
    kind: ArtworkKind,
    page: u32,
) -> Result<ArtworkPage, ApiError> {
    if response.page.is_some_and(|returned| returned != page) {
        return Err(ApiError::Provider(
            "SteamGridDB returned a different artwork page. Try again.".into(),
        ));
    }
    // Use the raw result count/provider total, not the count left after
    // filtering oversized/unsafe images: later pages can still be usable.
    let limit = response
        .limit
        .filter(|limit| *limit > 0)
        .unwrap_or(IMAGE_PAGE_SIZE);
    let has_more = response
        .total
        .map(|total| (u64::from(page) + 1) * u64::from(limit) < total)
        .unwrap_or(response.data.len() >= limit as usize);
    let mut candidates = response
        .data
        .into_iter()
        .filter(|image| {
            !image.nsfw
                && !image.humor
                && image.width <= crate::artwork::MAXIMUM_IMAGE_DIMENSION
                && image.height <= crate::artwork::MAXIMUM_IMAGE_DIMENSION
                && u64::from(image.width) * u64::from(image.height) * 4
                    <= crate::artwork::MAXIMUM_DECODE_BYTES
        })
        .map(|image| ArtworkCandidate {
            id: format!(
                "steamgriddb:{}:{}",
                match kind {
                    ArtworkKind::Cover => "grids",
                    ArtworkKind::Background => "heroes",
                    ArtworkKind::Logo => "logos",
                    ArtworkKind::Icon => "icons",
                },
                image.id
            ),
            provider_id: "steamgriddb".into(),
            kind,
            image_url: image.url,
            thumbnail_url: image.thumb,
            author: image.author.name,
            preview_path: None,
            score: image.score,
            width: image.width,
            height: image.height,
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|image| {
        std::cmp::Reverse((
            kind == ArtworkKind::Background && image.width >= 3000,
            image.score,
        ))
    });
    Ok(ArtworkPage {
        images: candidates,
        has_more,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(
        page: u32,
        total: Option<u64>,
        count: u32,
        unsafe_images: bool,
    ) -> Envelope<Vec<RemoteImage>> {
        serde_json::from_value(serde_json::json!({
            "page": page, "limit": IMAGE_PAGE_SIZE, "total": total,
            "data": (0..count).map(|index| serde_json::json!({
                "id": u64::from(page) * u64::from(IMAGE_PAGE_SIZE) + u64::from(index),
                "url": "https://cdn2.steamgriddb.com/full.png", "thumb": "https://cdn2.steamgriddb.com/thumb.png",
                "author": {"name":"Artist"}, "score": index,
                "width": 600, "height": 900, "nsfw": unsafe_images,
            })).collect::<Vec<_>>()
        })).unwrap()
    }

    #[test]
    fn image_pages_keep_all_results_and_stop_at_the_provider_total() {
        let first = image_page(response(0, Some(49), 24, false), ArtworkKind::Cover, 0).unwrap();
        let second = image_page(response(1, Some(49), 24, false), ArtworkKind::Cover, 1).unwrap();
        let last = image_page(response(2, Some(49), 1, false), ArtworkKind::Cover, 2).unwrap();
        assert!(first.has_more && second.has_more && !last.has_more);
        assert_eq!(
            first.images.len() + second.images.len() + last.images.len(),
            49
        );
        let ids = first
            .images
            .iter()
            .chain(&second.images)
            .chain(&last.images)
            .map(|image| &image.id)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            ids.len(),
            49,
            "Pagination must neither repeat nor truncate results"
        );
    }

    #[test]
    fn filtered_empty_pages_can_still_advance_to_later_images() {
        let page = image_page(response(0, Some(48), 24, true), ArtworkKind::Cover, 0).unwrap();
        assert!(page.images.is_empty());
        assert!(page.has_more);
        let terminal = image_page(response(1, Some(48), 24, true), ArtworkKind::Cover, 1).unwrap();
        assert!(!terminal.has_more);
    }

    #[test]
    fn missing_pagination_metadata_uses_raw_page_size_and_rejects_wrong_pages() {
        assert!(
            image_page(response(0, None, 24, false), ArtworkKind::Cover, 0)
                .unwrap()
                .has_more
        );
        assert!(
            !image_page(response(1, None, 2, false), ArtworkKind::Cover, 1)
                .unwrap()
                .has_more
        );
        assert!(image_page(response(0, Some(100), 24, false), ArtworkKind::Cover, 1).is_err());
    }

    #[test]
    fn direct_identification_only_accepts_explicit_game_ids_and_trusted_game_urls() {
        assert_eq!(direct_game_id("#5461202").unwrap(), Some(5461202));
        assert_eq!(
            direct_game_id("https://www.steamgriddb.com/game/5461202").unwrap(),
            Some(5461202)
        );
        assert_eq!(
            direct_game_id("https://steamgriddb.com/game/5461202/").unwrap(),
            Some(5461202)
        );
        for title in ["Cyberpunk 2077", "1942", "#DRIVE"] {
            assert_eq!(direct_game_id(title).unwrap(), None);
        }
        for query in [
            "#0",
            "#18446744073709551616",
            "https://example.com/game/5461202",
            "file:///game/5461202",
            "https://steamgriddb.com/grid/5461202",
            "https://steamgriddb.com/game/0",
        ] {
            assert!(direct_game_id(query).is_err(), "{query}");
        }
    }
}
