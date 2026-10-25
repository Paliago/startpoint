//! Public store metadata. No account tokens or artwork-provider credentials are used.
use reqwest::blocking::{Client, RequestBuilder};
use serde::{Deserialize, de::DeserializeOwned};
use startpoint_sdk::{ApiError, Game, JobContext, LaunchTarget, UserMetadata};
use std::io::Read;

fn normalized(title: &str) -> String {
    title
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Only remove known edition labels, never arbitrary suffixes, sequel numbers,
/// expansion names, soundtrack labels, or partial-title matches.
pub(crate) fn base_title(title: &str) -> String {
    let normalized = normalized(title);
    for suffix in [
        "the final cut",
        "definitive edition",
        "deluxe edition",
        "ultimate edition",
        "complete edition",
        "game of the year edition",
        "goty edition",
        "director s cut",
        "directors cut",
    ] {
        if let Some(base) = normalized.strip_suffix(&format!(" {suffix}")) {
            return base.to_string();
        }
    }
    normalized
}

pub(crate) fn titles_match(left: &str, right: &str) -> bool {
    let left = base_title(left);
    !left.is_empty() && left == base_title(right)
}

pub(crate) fn edition_search_title(title: &str) -> Option<String> {
    let base = base_title(title);
    (base != normalized(title) && !base.is_empty()).then_some(base)
}

pub(crate) fn epic_namespace(game: &Game) -> Option<String> {
    let LaunchTarget::Uri(uri) = &game.default_launch else {
        return None;
    };
    let uri = reqwest::Url::parse(uri).ok()?;
    if uri.scheme() != "com.epicgames.launcher" || uri.host_str() != Some("apps") {
        return None;
    }
    let path = uri
        .path()
        .trim_start_matches('/')
        .replace("%3A", ":")
        .replace("%3a", ":");
    let parts = path.split(':').collect::<Vec<_>>();
    if parts.len() != 3 || parts[2] != game.source_game_id {
        return None;
    }
    valid_id(parts[0]).then(|| parts[0].to_string())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}

fn get_json<T: DeserializeOwned>(
    request: RequestBuilder,
    provider: &str,
    context: &JobContext,
) -> Result<T, ApiError> {
    context.checkpoint()?;
    let response = request
        .header(
            "User-Agent",
            concat!("Startpoint/", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .map_err(|e| ApiError::Provider(format!("{provider} catalogue: {e}")))?;
    if response.status().as_u16() == 404 {
        return Err(ApiError::NoArtwork);
    }
    if !response.status().is_success() {
        return Err(ApiError::Provider(format!(
            "{provider} catalogue returned HTTP {}",
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    response
        .take(2_000_001)
        .read_to_end(&mut bytes)
        .map_err(|e| ApiError::Provider(e.to_string()))?;
    if bytes.len() > 2_000_000 {
        return Err(ApiError::Provider(format!(
            "{provider} catalogue response exceeded the size limit"
        )));
    }
    context.checkpoint()?;
    serde_json::from_slice(&bytes)
        .map_err(|e| ApiError::Provider(format!("Invalid {provider} catalogue response: {e}")))
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

#[derive(Deserialize)]
struct GogProduct {
    id: u64,
    title: String,
}

#[derive(Deserialize)]
struct GogEmbedded {
    product: GogProduct,
    #[serde(default)]
    developers: Vec<Named>,
    #[serde(default)]
    tags: Vec<Named>,
}

#[derive(Deserialize)]
struct GogGame {
    #[serde(default)]
    description: String,
    #[serde(default)]
    overview: String,
    #[serde(rename = "_embedded")]
    embedded: GogEmbedded,
}

pub(crate) fn gog_metadata(
    client: &Client,
    id: &str,
    context: &JobContext,
) -> Result<UserMetadata, ApiError> {
    let id = id
        .parse::<u64>()
        .map_err(|_| ApiError::Validation("Invalid GOG catalogue identifier".into()))?;
    let game: GogGame = get_json(
        client
            .get(format!("https://api.gog.com/v2/games/{id}"))
            .query(&[("locale", "en-US")]),
        "GOG",
        context,
    )?;
    gog_fields(game, id)
}

fn gog_fields(game: GogGame, id: u64) -> Result<UserMetadata, ApiError> {
    if game.embedded.product.id != id {
        return Err(ApiError::Provider(
            "GOG returned a different product identifier".into(),
        ));
    }
    let description = if game.description.trim().is_empty() {
        &game.overview
    } else {
        &game.description
    };
    let text = plain_text(description);
    // The first paragraph is GOG's synopsis; later paragraphs usually describe
    // individual features and embed promotional images.
    let synopsis = text
        .split("\n\n")
        .find(|part| !part.trim().is_empty())
        .unwrap_or("");
    Ok(UserMetadata {
        title: clean(&game.embedded.product.title, 500),
        description: clean(synopsis, 10_000),
        developer: clean(
            &game
                .embedded
                .developers
                .into_iter()
                .map(|v| v.name)
                .collect::<Vec<_>>()
                .join(", "),
            10_000,
        ),
        genres: clean(
            &game
                .embedded
                .tags
                .into_iter()
                .map(|v| v.name)
                .collect::<Vec<_>>()
                .join(", "),
            10_000,
        ),
    })
}

const EPIC_QUERY: &str = r#"query StartpointMetadata($title: String!) {
  Catalog { searchStore(keywords: $title, country: "US", locale: "en-US", count: 40) {
    elements { title description namespace tags { name groupName }
      customAttributes { key value } catalogNs { mappings { pageSlug pageType } } }
  } }
}"#;

#[derive(Deserialize)]
struct EpicResponse {
    data: Option<EpicData>,
}
#[derive(Deserialize)]
struct EpicData {
    #[serde(rename = "Catalog")]
    catalog: EpicCatalog,
}
#[derive(Deserialize)]
struct EpicCatalog {
    #[serde(rename = "searchStore")]
    search: EpicSearch,
}
#[derive(Deserialize)]
struct EpicSearch {
    elements: Vec<EpicOffer>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EpicOffer {
    title: String,
    namespace: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    tags: Vec<EpicTag>,
    #[serde(default)]
    custom_attributes: Vec<EpicAttribute>,
    catalog_ns: Option<EpicMappings>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EpicTag {
    name: String,
    group_name: Option<String>,
}
#[derive(Deserialize)]
struct EpicAttribute {
    key: String,
    value: String,
}
#[derive(Deserialize)]
struct EpicMappings {
    #[serde(default)]
    mappings: Vec<EpicMapping>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EpicMapping {
    page_slug: String,
    page_type: String,
}

pub(crate) fn epic_metadata(
    client: &Client,
    title: &str,
    namespace: Option<&str>,
    context: &JobContext,
) -> Result<UserMetadata, ApiError> {
    let variables = serde_json::json!({"title":title}).to_string();
    let response: EpicResponse = get_json(
        client
            .get("https://store.epicgames.com/graphql")
            .query(&[("query", EPIC_QUERY), ("variables", variables.as_str())]),
        "Epic",
        context,
    )?;
    let data = response
        .data
        .ok_or_else(|| ApiError::Provider("Epic catalogue did not return search data".into()))?;
    let offer = select_epic_offer(data.catalog.search.elements, title, namespace)?;
    let mut metadata = epic_fields(&offer);
    if metadata.developer.is_none() {
        let slug = offer
            .custom_attributes
            .iter()
            .find(|v| v.key == "com.epicgames.app.productSlug")
            .map(|v| v.value.split('/').next().unwrap_or(""))
            .or_else(|| {
                offer
                    .catalog_ns
                    .as_ref()?
                    .mappings
                    .iter()
                    .find(|v| v.page_type == "productHome")
                    .map(|v| v.page_slug.as_str())
            });
        if let Some(slug) = slug.filter(|slug| valid_id(slug)) {
            let content: Result<EpicContent, _> = get_json(
                client.get(format!(
                    "https://store-content.ak.epicgames.com/api/en-US/content/products/{slug}"
                )),
                "Epic",
                context,
            );
            match content {
                Ok(content) => {
                    if let Some(about) = epic_about(content, &offer) {
                        metadata.developer = clean(&about.developer_attribution, 10_000);
                        if metadata.description.is_none() {
                            metadata.description =
                                clean(&plain_text(&about.short_description), 10_000);
                        }
                    }
                }
                Err(ApiError::Cancelled) => return Err(ApiError::Cancelled),
                // Optional content must not discard the store's valid synopsis/genres.
                Err(_) => {}
            }
        }
    }
    Ok(metadata)
}

fn select_epic_offer(
    offers: Vec<EpicOffer>,
    title: &str,
    namespace: Option<&str>,
) -> Result<EpicOffer, ApiError> {
    let mut matches = offers
        .into_iter()
        .filter(|offer| {
            namespace.is_none_or(|ns| offer.namespace == ns) && titles_match(title, &offer.title)
        })
        .collect::<Vec<_>>();
    if matches
        .iter()
        .any(|offer| normalized(&offer.title) == normalized(title))
    {
        matches.retain(|offer| normalized(&offer.title) == normalized(title));
    }
    if matches.len() == 1 {
        Ok(matches.pop().unwrap())
    } else {
        Err(ApiError::NoArtwork)
    }
}

fn epic_fields(offer: &EpicOffer) -> UserMetadata {
    UserMetadata {
        title: clean(&offer.title, 500),
        description: clean(&plain_text(&offer.description), 10_000),
        developer: offer
            .custom_attributes
            .iter()
            .find(|v| v.key == "developerName")
            .and_then(|v| clean(&v.value, 10_000)),
        genres: clean(
            &offer
                .tags
                .iter()
                .filter(|tag| tag.group_name.as_deref() == Some("genre"))
                .map(|tag| tag.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            10_000,
        ),
    }
}

#[derive(Deserialize)]
struct EpicContent {
    namespace: String,
    #[serde(default)]
    pages: Vec<EpicPage>,
}
#[derive(Deserialize)]
struct EpicPage {
    namespace: String,
    data: EpicPageData,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EpicPageData {
    #[serde(default)]
    nav_title: String,
    about: Option<EpicAbout>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EpicAbout {
    #[serde(default)]
    developer_attribution: String,
    #[serde(default)]
    short_description: String,
}

fn epic_about(content: EpicContent, offer: &EpicOffer) -> Option<EpicAbout> {
    if content.namespace != offer.namespace {
        return None;
    }
    let mut pages = content.pages.into_iter().filter(|page| {
        page.namespace == offer.namespace
            && normalized(&page.data.nav_title) == normalized(&offer.title)
    });
    let page = pages.next()?;
    if pages.next().is_some() {
        return None;
    }
    page.data.about
}

fn clean(value: &str, maximum: usize) -> Option<String> {
    let mut value = value.replace('\0', "").trim().to_string();
    if value.len() > maximum {
        let mut end = maximum;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
    }
    (!value.is_empty()).then_some(value)
}

/// Store descriptions are displayed as plain text, never as executable HTML.
fn plain_text(html: &str) -> String {
    let mut output = String::new();
    let mut tag = String::new();
    let mut inside_tag = false;
    let mut quote = None;
    let mut hidden = None::<String>;
    for c in html.chars() {
        if inside_tag {
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
            } else if matches!(c, '\'' | '"') {
                quote = Some(c);
            } else if c == '>' {
                inside_tag = false;
                let name = tag
                    .trim()
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('/')
                    .to_ascii_lowercase();
                if matches!(name.as_str(), "script" | "style") {
                    hidden = Some(name.clone());
                } else if hidden
                    .as_ref()
                    .is_some_and(|hidden| name == format!("/{hidden}"))
                {
                    hidden = None;
                }
                if hidden.is_none()
                    && matches!(
                        name.trim_start_matches('/'),
                        "br" | "p" | "div" | "li" | "h1" | "h2" | "h3"
                    )
                {
                    output.push_str("\n\n");
                }
                tag.clear();
                continue;
            }
            tag.push(c);
        } else if c == '<' {
            inside_tag = true;
        } else if hidden.is_none() {
            output.push(c);
        }
    }
    for (entity, text) in [
        ("&nbsp;", " "),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&#039;", "'"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&amp;", "&"),
    ] {
        output = output.replace(entity, text);
    }
    output
        .split("\n\n")
        .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edition_aliases_preserve_sequel_expansion_and_soundtrack_boundaries() {
        assert!(titles_match(
            "Disco Elysium",
            "Disco Elysium - The Final Cut"
        ));
        assert!(titles_match("Control", "CONTROL™ Ultimate Edition"));
        assert_eq!(
            edition_search_title("Disco Elysium - The Final Cut").as_deref(),
            Some("disco elysium")
        );
        assert!(edition_search_title("Alan Wake 2").is_none());
        for (left, right) in [
            ("Alan Wake 2", "Alan Wake"),
            ("Cyberpunk 2077", "Cyberpunk 2077: Phantom Liberty"),
            ("Disco Elysium", "Disco Elysium - The Final Cut Soundtrack"),
            ("", ""),
        ] {
            assert!(!titles_match(left, right), "{left} / {right}");
        }
    }

    #[test]
    fn gog_metadata_uses_product_identity_and_a_plain_text_synopsis() {
        let document = serde_json::json!({
            "description":"<p>Disco Elysium is a detective RPG &amp; a story about choices.</p><img src='image.jpg'><p>More features.</p>",
            "_embedded": {"product":{"id":1771589310_u64,"title":"Disco Elysium - The Final Cut"},
                "developers":[{"name":"ZA/UM"}], "tags":[{"name":"Role-playing"},{"name":"Point-and-click"}]}
        });
        let metadata = gog_fields(
            serde_json::from_value(document.clone()).unwrap(),
            1771589310,
        )
        .unwrap();
        assert_eq!(
            metadata.description.as_deref(),
            Some("Disco Elysium is a detective RPG & a story about choices.")
        );
        assert_eq!(metadata.developer.as_deref(), Some("ZA/UM"));
        assert_eq!(
            metadata.genres.as_deref(),
            Some("Role-playing, Point-and-click")
        );
        assert!(gog_fields(serde_json::from_value(document).unwrap(), 1).is_err());
    }

    fn offer(title: &str, namespace: &str) -> EpicOffer {
        serde_json::from_value(serde_json::json!({
            "title":title, "namespace":namespace, "description":"A story about two connected heroes.",
            "tags":[{"name":"Horror","groupName":"genre"},{"name":"Action-Adventure","groupName":"genre"},
                {"name":"Halloween Sale","groupName":"event"},{"name":"Windows","groupName":"platform"}],
            "customAttributes":[{"key":"com.epicgames.app.productSlug","value":"alan-wake-2"}]
        })).unwrap()
    }

    #[test]
    fn epic_uses_the_exact_offer_and_namespace_and_only_genre_tags() {
        let selected = select_epic_offer(
            vec![
                offer("Alan Wake 2 Deluxe Edition", "namespace"),
                offer("Alan Wake 2", "namespace"),
            ],
            "Alan Wake 2",
            Some("namespace"),
        )
        .unwrap();
        let metadata = epic_fields(&selected);
        assert_eq!(metadata.title.as_deref(), Some("Alan Wake 2"));
        assert!(metadata.description.is_some());
        assert_eq!(metadata.genres.as_deref(), Some("Horror, Action-Adventure"));
        assert!(
            select_epic_offer(
                vec![offer("Alan Wake 2", "wrong")],
                "Alan Wake 2",
                Some("namespace")
            )
            .is_err()
        );
        assert!(
            select_epic_offer(
                vec![offer("Alan Wake", "namespace")],
                "Alan Wake 2",
                Some("namespace")
            )
            .is_err()
        );
        assert!(
            select_epic_offer(
                vec![
                    offer("Alan Wake 2", "namespace"),
                    offer("Alan Wake 2", "namespace")
                ],
                "Alan Wake 2",
                Some("namespace")
            )
            .is_err()
        );
    }

    #[test]
    fn epic_content_developer_is_read_from_the_matching_page_not_a_marketing_title() {
        let document = serde_json::json!({"namespace":"namespace", "pages":[
            {"namespace":"namespace","data":{"navTitle":"Alan Wake 2 Deluxe Edition","about":{"developerAttribution":"Wrong edition fixture"}}},
            {"namespace":"namespace","data":{"navTitle":"Alan Wake 2","about":{"title":"Alan Wake 2: Winner of awards", "developerAttribution":"Remedy Entertainment"}}}
        ]});
        let about = epic_about(
            serde_json::from_value(document.clone()).unwrap(),
            &offer("Alan Wake 2", "namespace"),
        )
        .unwrap();
        assert_eq!(about.developer_attribution, "Remedy Entertainment");
        assert!(
            epic_about(
                serde_json::from_value(document).unwrap(),
                &offer("Alan Wake 2", "other-namespace")
            )
            .is_none()
        );
    }

    #[test]
    fn store_text_does_not_expose_html_scripts_or_split_utf8_when_bounded() {
        assert_eq!(
            plain_text(
                "<style>hidden</style><script>hidden()</script><p>A &quot;quote&quot;.</p><p>Next.</p>"
            ),
            "A \"quote\".\n\nNext."
        );
        assert_eq!(clean("ééé", 3).as_deref(), Some("é"));
        assert!(clean("  ", 500).is_none());
    }

    #[test]
    #[ignore = "Requires the public GOG and Epic catalogue services"]
    fn live_disco_elysium_and_alan_wake_metadata() {
        use std::sync::{Arc, atomic::AtomicBool};
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .unwrap();
        let context = JobContext::new(Arc::new(AtomicBool::new(false)), |_| {});
        let disco = gog_metadata(&client, "1771589310", &context).unwrap();
        let alan = epic_metadata(
            &client,
            "Alan Wake 2",
            Some("c4763f236d08423eb47b4c3008779c84"),
            &context,
        )
        .unwrap();
        for metadata in [disco, alan] {
            assert!(metadata.description.is_some());
            assert!(metadata.developer.is_some());
            assert!(metadata.genres.is_some());
            println!(
                "{}: description, developer and genres available",
                metadata.title.unwrap()
            );
        }
    }
}
