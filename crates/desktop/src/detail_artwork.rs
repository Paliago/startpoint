use crate::logo_cache::{LogoCache, State};
use gpui::{prelude::*, *};
use gpui_component::ActiveTheme;
use startpoint_sdk::{ArtworkAsset, ArtworkKind, Game};

const LOGO_HEIGHT: f32 = 160.;

fn logo_frame(height: f32) -> Div {
    div()
        .relative()
        .w_full()
        .h(px(height))
        .flex_shrink_0()
        .overflow_hidden()
}

/// Heroes/backgrounds are intended as clean scenery, unlike branded box covers.
/// Without one, use a neutral surface when a separate logo is available rather
/// than stacking that logo over a cover with its own baked-in title.
pub(crate) fn background(game: &Game) -> Option<&ArtworkAsset> {
    game.artwork.get(&ArtworkKind::Background).or_else(|| {
        if game.artwork.contains_key(&ArtworkKind::Logo) {
            None
        } else {
            game.artwork.get(&ArtworkKind::Cover)
        }
    })
}

fn logo_layout(
    width: f32,
    image_width: u32,
    image_height: u32,
    height_limit: f32,
) -> (f32, f32, f32) {
    let width = width.max(1.);
    let height =
        (width * image_height.max(1) as f32 / image_width.max(1) as f32).min(height_limit.max(1.));
    let image_width = height * image_width.max(1) as f32 / image_height.max(1) as f32;
    (image_width.min(width), height, height.max(LOGO_HEIGHT))
}

pub(crate) fn heading(
    game: &Game,
    width: f32,
    height_limit: f32,
    logos: &Entity<LogoCache>,
    cx: &mut App,
) -> Div {
    let title = game.title().to_string();
    let fallback_title = title.clone();
    let text = move || {
        div()
            .w_full()
            .text_3xl()
            .font_weight(FontWeight::BOLD)
            .child(fallback_title.clone())
    };
    match game.artwork.get(&ArtworkKind::Logo) {
        Some(logo) => match logos.update(cx, |cache, cx| cache.load(logo, cx)) {
            State::Ready(logo) => {
                let (image_width, image_height, frame_height) =
                    logo_layout(width, logo.width, logo.height, height_limit);
                logo_frame(frame_height).child(
                    img(logo.path)
                        // Explicit bounds avoid GPUI's intrinsic/percentage image sizing.
                        .absolute()
                        .left(px((width - image_width) / 2.))
                        .top(px((frame_height - image_height) / 2.))
                        .w(px(image_width))
                        .h(px(image_height))
                        .object_fit(ObjectFit::Contain)
                        .with_fallback(move || text().into_any_element()),
                )
            }
            State::Pending => logo_frame(LOGO_HEIGHT),
            State::Failed => text(),
        },
        None => div()
            .w_full()
            .text_3xl()
            .text_color(cx.theme().foreground)
            .font_weight(FontWeight::BOLD)
            .child(title),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[::core::prelude::v1::test]
    fn logo_frame_reserves_nonshrinking_height_above_the_description() {
        let mut frame = logo_frame(LOGO_HEIGHT);
        let style = frame.style();
        assert_eq!(style.size.height, Some(px(LOGO_HEIGHT).into()));
        assert_eq!(style.flex_shrink, Some(0.));
        assert_eq!(style.overflow.x, Some(Overflow::Hidden));
        assert_eq!(style.overflow.y, Some(Overflow::Hidden));
    }

    #[::core::prelude::v1::test]
    fn tall_and_wide_logos_use_full_width_and_remain_centered_within_their_frames() {
        for (image_width, image_height) in [(440, 320), (1200, 300), (1600, 900)] {
            let (width, height, frame) = logo_layout(345., image_width, image_height, 320.);
            assert!((width - 345.).abs() < 0.01);
            assert!(frame >= height);
            assert!(frame >= LOGO_HEIGHT);
            assert!((width / height - image_width as f32 / image_height as f32).abs() < 0.01);
        }
        let (width, height, frame) = logo_layout(345., 100, 800, 250.);
        assert_eq!(height, 250.);
        assert_eq!(frame, 250.);
        assert!(
            width < 345.,
            "extremely tall logos fit rather than distort or overlap the text"
        );
    }

    fn asset(kind: ArtworkKind) -> ArtworkAsset {
        ArtworkAsset {
            id: kind.label().into(),
            kind,
            image_path: PathBuf::from(format!("{}.png", kind.label())),
            thumbnail_path: PathBuf::new(),
            provider_id: "test".into(),
            source_url: None,
            author: "test".into(),
        }
    }

    #[::core::prelude::v1::test]
    fn background_prefers_clean_heroes_and_avoids_cover_branding_with_a_separate_logo() {
        let mut game: Game = serde_json::from_value(serde_json::json!({
            "id": "steam:42", "provider_id": "steam", "source_game_id": "42", "discovered_title": "Game",
            "install_directory": "", "default_launch": {"Uri": "steam://rungameid/42"}, "launch_override": null,
            "metadata": {}, "favourite": false, "available": true, "last_launched": null,
            "artwork": {}, "artwork_history": [], "artwork_game_ids": {}
        })).unwrap();
        assert!(background(&game).is_none());
        game.artwork
            .insert(ArtworkKind::Cover, asset(ArtworkKind::Cover));
        assert_eq!(background(&game).unwrap().kind, ArtworkKind::Cover);
        game.artwork
            .insert(ArtworkKind::Logo, asset(ArtworkKind::Logo));
        assert!(
            background(&game).is_none(),
            "do not layer separate and baked-in logos"
        );
        game.artwork
            .insert(ArtworkKind::Background, asset(ArtworkKind::Background));
        assert_eq!(background(&game).unwrap().kind, ArtworkKind::Background);
        game.artwork.remove(&ArtworkKind::Logo);
        assert_eq!(background(&game).unwrap().kind, ArtworkKind::Background);
    }
}
