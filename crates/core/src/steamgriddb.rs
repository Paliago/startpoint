use reqwest::{blocking::Client, redirect::Policy};
use serde::{Deserialize, de::DeserializeOwned};
use startpoint_sdk::{
    ApiError, ArtworkCandidate, ArtworkGame, ArtworkKind, ArtworkProvider, Game, JobContext,
    ProviderDescriptor,
};
use std::{io::Read, time::Duration};

pub struct SteamGridDbProvider {
    client: Client,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
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
        Ok(envelope.data)
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
        let images: Vec<RemoteImage> = self.get(
            &format!("{resource}/{target}?types=static&mimes={mimes}&nsfw=false&humor=false&epilepsy=false&limit=50{dimensions}"),
            credential,
            context,
        )?;
        let mut candidates = images
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
                id: format!("steamgriddb:{resource}:{}", image.id),
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
        candidates.truncate(24);
        Ok(candidates)
    }
}
