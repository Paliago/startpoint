use crate::{components::*, state::State};
use rune::{El, FileDialogOptions, TextEdit, col, el, row};
use rune_kit::{Kit, SegmentedControl};
use startpoint_sdk::*;

pub struct ArtworkBrowser {
    pub kind: ArtworkKind,
    pub online: bool,
    pub identifying: bool,
    pub query: TextEdit,
    pub matches: Vec<ArtworkGame>,
    pub match_limit: usize,
    pub remote: Option<ArtworkGame>,
    pub candidates: Vec<ArtworkCandidate>,
    pub page: u32,
    pub saved_page: usize,
    pub has_more: bool,
    pub search_job: Option<JobId>,
    pub page_job: Option<JobId>,
}

impl Default for ArtworkBrowser {
    fn default() -> Self {
        Self {
            kind: ArtworkKind::Cover,
            online: false,
            identifying: false,
            query: TextEdit::new(""),
            matches: Vec::new(),
            match_limit: 24,
            remote: None,
            candidates: Vec::new(),
            page: 1,
            saved_page: 0,
            has_more: false,
            search_job: None,
            page_job: None,
        }
    }
}

impl State {
    pub fn search_artwork_games(&mut self) {
        let Some(game_id) = self.editor.game_id.clone() else {
            return;
        };
        let query = self.editor.artwork.query.text.trim().to_owned();
        if query.is_empty() {
            self.error = Some("Enter a game name, SteamGridDB URL or #ID.".into());
            return;
        }
        self.editor.artwork.matches.clear();
        self.editor.artwork.match_limit = 24;
        self.editor.artwork.search_job = self.submit(Command::SearchArtworkGames {
            provider_id: "steamgriddb".into(),
            game_id,
            query,
        });
    }

    pub fn load_artwork_page(&mut self, page: u32) {
        let Some(game_id) = self.editor.game_id.clone() else {
            return;
        };
        let remote_game_id = self
            .editor
            .artwork
            .remote
            .as_ref()
            .map(|game| game.id.clone());
        self.editor.artwork.page = page.max(1);
        self.editor.artwork.has_more = false;
        self.editor.artwork.candidates.clear();
        self.editor.artwork.page_job = self.submit(Command::ListArtwork {
            provider_id: "steamgriddb".into(),
            game_id,
            remote_game_id,
            kind: self.editor.artwork.kind,
            page: self.editor.artwork.page,
        });
    }

    pub fn set_artwork_kind(&mut self, kind: ArtworkKind) {
        self.editor.artwork.kind = kind;
        self.editor.artwork.saved_page = 0;
        self.editor.artwork.candidates.clear();
        self.editor.artwork.page_job = None;
        self.editor.artwork.page = 1;
        self.editor.artwork.has_more = false;
        if self.editor.artwork.online {
            self.load_artwork_page(1);
        }
    }
}

pub fn view(state: &State, kit: &Kit, _: &mut Vec<El>) -> El {
    let browser = &state.editor.artwork;
    let kinds = [
        ArtworkKind::Cover,
        ArtworkKind::Background,
        ArtworkKind::Logo,
        ArtworkKind::Icon,
    ];
    let index = kinds
        .iter()
        .position(|kind| kind == &browser.kind)
        .unwrap_or(0);
    let tabs = SegmentedControl::new(&["Cover", "Background", "Logo", "Icon"], Some(index))
        .width(None)
        .on_pick(move |s: &mut State, index, _| {
            if let Some(kind) = kinds.get(index) {
                s.set_artwork_kind(*kind);
            }
        })
        .build(kit)
        .key("artwork-kinds");
    let mut body = col().w_full().gap(14.0).child(tabs).child(
        row()
            .gap(8.0)
            .child(
                button("Your images")
                    .on_press(|s: &mut State| {
                        s.editor.artwork.online = false;
                        s.editor.artwork.identifying = false;
                    })
                    .build(kit),
            )
            .child(
                button("Upload…")
                    .on_press_tx(|s: &mut State, tx| {
                        let game_id = s.editor.game_id.clone();
                        let kind = s.editor.artwork.kind;
                        tx.open_file(
                            FileDialogOptions::default()
                                .filter("Artwork", &["png", "jpg", "jpeg", "webp"]),
                            move |s: &mut State, result, _| match result {
                                Ok(Some(path)) if s.editor.game_id == game_id => {
                                    if let Some(game_id) = game_id {
                                        s.submit(Command::ImportArtwork {
                                            game_id,
                                            kind,
                                            path,
                                        });
                                    }
                                }
                                Err(error) => s.error = Some(error.to_string()),
                                _ => {}
                            },
                        );
                    })
                    .build(kit)
                    .key("upload-artwork"),
            )
            .child(
                button("Find more artwork")
                    .disabled(state.headless)
                    .on_press(|s: &mut State| {
                        s.editor.artwork.online = true;
                        s.editor.artwork.identifying = false;
                        s.load_artwork_page(1);
                    })
                    .build(kit),
            )
            .child(
                button("Re-identify…")
                    .disabled(state.headless)
                    .on_press(|s: &mut State| s.editor.artwork.identifying = true)
                    .build(kit),
            ),
    );
    if browser.online || browser.identifying {
        body = body.child(search(state, kit));
        if !browser.identifying {
            body = body.child(pagination(state, kit));
            let mut images = row().w_full().flex_wrap().gap(12.0);
            for candidate in &browser.candidates {
                let candidate = candidate.clone();
                let thumbnail = candidate
                    .preview_path
                    .as_ref()
                    .map(|path| crate::library::image(state, path, kit))
                    .unwrap_or_else(|| el().fill(kit.c.layer_02));
                let title = format!(
                    "{} · {}×{}",
                    candidate.author, candidate.width, candidate.height
                );
                images = images.child(
                    col()
                        .key(format!("online-artwork-{}", candidate.id))
                        .w(150.0)
                        .gap(6.0)
                        .child(thumbnail.w_full().h(115.0).radius(kit.radius.md))
                        .child(hint(title, kit))
                        .child(
                            button("Use image")
                                .disabled(state.busy())
                                .on_press(move |s: &mut State| {
                                    if let Some(game_id) = s.editor.game_id.clone() {
                                        s.submit(Command::SelectArtwork {
                                            game_id,
                                            candidate: candidate.clone(),
                                        });
                                    }
                                })
                                .build(kit),
                        ),
                );
            }
            if browser.candidates.is_empty() {
                images = images.child(hint(
                    if browser.page_job.is_some_and(|id| state.job_active(id)) {
                        "Loading artwork…"
                    } else {
                        "No images on this page."
                    },
                    kit,
                ));
            }
            body = body.child(images).child(pagination(state, kit));
        }
    } else {
        let mut images = row().w_full().flex_wrap().gap(12.0);
        let saved = saved_assets(state);
        if let Some(game) = state.editor.game_id.as_ref().and_then(|id| state.game(id)) {
            for asset in saved.iter().skip(browser.saved_page * 24).take(24) {
                let selected = game
                    .artwork
                    .get(&browser.kind)
                    .is_some_and(|current| current.id == asset.id);
                let asset_id = asset.id.clone();
                let path = if state.images.contains_key(&asset.thumbnail_path) {
                    &asset.thumbnail_path
                } else {
                    &asset.image_path
                };
                images = images.child(
                    col()
                        .key(format!("saved-artwork-{}", asset.id))
                        .w(150.0)
                        .gap(6.0)
                        .child(
                            crate::library::image(state, path, kit)
                                .w_full()
                                .h(120.0)
                                .radius(kit.radius.md),
                        )
                        .child(
                            button(if selected { "✓ Current" } else { "Use image" })
                                .on_press(move |s: &mut State| {
                                    if let Some(game_id) = s.editor.game_id.clone() {
                                        s.submit(Command::SelectLocalArtwork {
                                            game_id,
                                            asset_id: asset_id.clone(),
                                        });
                                    }
                                })
                                .build(kit),
                        ),
                );
            }
        }
        if saved.is_empty() {
            images = images.child(hint(
                "No saved images. Upload an image or find artwork online.",
                kit,
            ));
        }
        body = body.child(images);
        if saved.len() > 24 {
            let last = saved.len().saturating_sub(1) / 24;
            body = body.child(
                row()
                    .gap(10.0)
                    .child(
                        button("Previous saved images")
                            .disabled(browser.saved_page == 0)
                            .on_press(|s: &mut State| {
                                s.editor.artwork.saved_page =
                                    s.editor.artwork.saved_page.saturating_sub(1)
                            })
                            .build(kit),
                    )
                    .child(
                        button("Next saved images")
                            .disabled(browser.saved_page >= last)
                            .on_press(move |s: &mut State| {
                                s.editor.artwork.saved_page =
                                    (s.editor.artwork.saved_page + 1).min(last)
                            })
                            .build(kit),
                    ),
            );
        }
    }
    body.on_drop_files(|s: &mut State, paths, _| {
        if let (Some(game_id), Some(path)) = (s.editor.game_id.clone(), paths.first()) {
            s.submit(Command::ImportArtwork {
                game_id,
                kind: s.editor.artwork.kind,
                path: path.clone(),
            });
        }
    })
}

pub fn saved_assets(state: &State) -> Vec<&ArtworkAsset> {
    let mut seen = std::collections::BTreeSet::new();
    state
        .editor
        .game_id
        .as_ref()
        .and_then(|id| state.game(id))
        .map(|game| {
            game.artwork
                .values()
                .chain(game.artwork_history.iter())
                .filter(|asset| asset.kind == state.editor.artwork.kind)
                .filter(|asset| seen.insert(&asset.id))
                .collect()
        })
        .unwrap_or_default()
}

fn search(state: &State, kit: &Kit) -> El {
    let browser = &state.editor.artwork;
    let mut results = col()
        .w_full()
        .gap(8.0)
        .child(field(
            "Game name, SteamGridDB URL or #ID",
            "artwork-search",
            &browser.query,
            |s| &mut s.editor.artwork.query,
            kit,
        ))
        .child(
            primary("Search games")
                .disabled(
                    state.headless || browser.search_job.is_some_and(|id| state.job_active(id)),
                )
                .on_press(|s: &mut State| s.search_artwork_games())
                .build(kit)
                .key("search-artwork-games"),
        );
    if let Some(remote) = &browser.remote {
        let remote_id = remote.id.clone();
        results = results
            .child(hint(format!("Browsing artwork for {}", remote.name), kit))
            .child(
                button("Save artwork match")
                    .on_press(move |s: &mut State| {
                        if let Some(game_id) = s.editor.game_id.clone() {
                            s.submit(Command::SetArtworkGame {
                                provider_id: "steamgriddb".into(),
                                game_id,
                                remote_game_id: remote_id.clone(),
                            });
                        }
                    })
                    .build(kit),
            );
    }
    for matched in browser.matches.iter().take(browser.match_limit) {
        let matched = matched.clone();
        results = results.child(button(&matched.name).on_press(move |s: &mut State| {
            if s.editor.artwork.identifying {
                if let Some(game_id) = s.editor.game_id.clone() {
                    s.confirm("Change game identity?", "Automatic metadata will be refreshed for this identity. Manual edits are retained.", Command::ReidentifyGame { game_id, matched: matched.clone() });
                }
            } else {
                s.editor.artwork.remote = Some(matched.clone());
                s.editor.artwork.matches.clear();
                s.load_artwork_page(1);
            }
        }).build(kit));
    }
    if browser.matches.len() > browser.match_limit {
        results = results.child(
            button("Load more games")
                .on_press(|s: &mut State| s.editor.artwork.match_limit += 24)
                .build(kit),
        );
    }
    results
}

fn pagination(state: &State, kit: &Kit) -> El {
    let browser = &state.editor.artwork;
    let loading = browser.page_job.is_some_and(|id| state.job_active(id));
    row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(
            button("Previous page")
                .disabled(browser.page <= 1 || loading)
                .on_press(|s: &mut State| {
                    s.load_artwork_page(s.editor.artwork.page.saturating_sub(1))
                })
                .build(kit),
        )
        .child(label(format!("Page {}", browser.page), 12.0, 400.0, kit))
        .child(
            button("Next page")
                .disabled(!browser.has_more || loading)
                .on_press(|s: &mut State| s.load_artwork_page(s.editor.artwork.page + 1))
                .build(kit),
        )
}
