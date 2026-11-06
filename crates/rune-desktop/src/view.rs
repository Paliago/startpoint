use crate::{
    components::*,
    effects, motion,
    state::{Overlay, Screen, State},
    toolbar,
};
use rune::{Color, Cx, El, ScrollSpec, Spring, SystemMaterial, Transition, col, el, row};
use rune_kit::{Dialog, Kit};
use startpoint_sdk::ArtworkKind;

pub fn view(state: &State, cx: &mut Cx<State>) -> El {
    let kit = state.kit(cx.system_dark());
    let size = cx.layout_size("startpoint").unwrap_or([1280.0, 840.0]);
    let wall = state.screen == Screen::Recents
        && state.appearance.recents_layout == startpoint_sdk::RecentsLayout::InfiniteWall;
    let mut layers = Vec::new();
    let header = toolbar::view(state, cx, &kit, size[0]);
    let immersive = crate::library::immersive_recents(state);
    let mut content_kit = kit.clone();
    let artwork_recents = state.screen == Screen::Recents
        && state.appearance.recents_layout != startpoint_sdk::RecentsLayout::InfiniteWall;
    if immersive {
        content_kit.c.text_base = Color::WHITE;
        content_kit.c.text_muted = Color::WHITE.alpha(0.72);
        content_kit.c.text_faint = content_kit.c.text_muted;
        content_kit.c.icon_base = Color::WHITE;
        content_kit.c.bg_button_neutral = Color::BLACK.alpha(0.4);
    }
    let content = match state.screen {
        Screen::Recents | Screen::Library => {
            crate::library::view(state, cx, &content_kit, &mut layers)
        }
        Screen::Activity => scroll(crate::library::activity(state, &kit)),
        Screen::Settings => scroll(crate::settings::view(state, &kit, &mut layers)),
    };
    let mut workspace = row()
        .key("workspace")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(0.0);
    {
        let mut sidebar_kit = kit.clone();
        if immersive {
            sidebar_kit.c.text_base = Color::WHITE;
            sidebar_kit.c.text_muted = Color::WHITE.alpha(0.78);
            sidebar_kit.c.icon_base = Color::WHITE;
            sidebar_kit.c.icon_muted = sidebar_kit.c.text_muted;
            sidebar_kit.c.layer_03 = Color::WHITE.alpha(0.1);
        }
        let mut sidebar = crate::library::sidebar(state, &sidebar_kit)
            .instant_changes()
            .transition(Transition::none());
        if immersive {
            sidebar = sidebar
                .padding(8.0)
                .fill(Color::BLACK.alpha(0.74))
                .backdrop_blur(16.0)
                .border(0.5, Color::WHITE.alpha(0.1))
                .radius(12.0);
        }
        let sidebar_width = state.appearance.sidebar_width.clamp(140.0, 260.0);
        workspace = workspace.child(
            el().key("sidebar-fold")
                .w((sidebar_width + 24.0) * state.sidebar_fold.progress)
                .h_full()
                .instant_changes()
                .clip_rounded(0.0)
                .child(
                    sidebar
                        .absolute(0.0, 0.0)
                        .w(sidebar_width)
                        .h_full()
                        .pt(if wall { 64.0 } else { 0.0 }),
                ),
        );
    }
    workspace = workspace.child(
        el().key("main-content")
            .instant_changes()
            .transition(Transition::none())
            .grow(1.0)
            .min_w(0.0)
            .min_h(0.0)
            .clip_rounded(0.0)
            .child(content.w_full().h_full()),
    );
    let dark = cx.system_dark();
    let reduced = cx.is_reduced_motion();
    let background_alpha = if !state.appearance.window_translucency {
        1.0
    } else {
        1.0 - state.appearance.translucency_amount.clamp(0.0, 1.0) * 0.12
    };
    let mut root = el()
        .key("startpoint")
        .w_full()
        .h_full()
        .radius(12.0)
        .clip_rounded(12.0)
        .continuous_corners()
        .motion(Spring::NONE)
        .transition(Transition::none())
        .on_layout(move |s: &mut State, _| {
            s.system_dark = dark;
            s.window_width = size[0];
            s.reduced_motion = reduced;
        });
    if state.appearance.window_translucency && state.appearance.backdrop_blur && !state.headless {
        root = root.child(
            el().key("desktop-material")
                .transition(Transition::none())
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .radius(12.0)
                .system_material(SystemMaterial::Glass)
                .inert(),
        );
    }
    root = root.child(
        el().key("contrast-backdrop")
            .transition(Transition::none())
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(kit.c.page.alpha(if state.headless {
                1.0
            } else {
                background_alpha
            }))
            .inert(),
    );
    if artwork_recents {
        if state.selected_game().is_some() {
            root = root
                .child(
                    scene_background(state, &kit)
                        .key("immersive-artwork")
                        .transition(Transition::none())
                        .absolute(0.0, 0.0)
                        .w_full()
                        .h_full()
                        .inert(),
                )
                .child(
                    el().key("artwork-scrim")
                        .transition(Transition::none())
                        .absolute(0.0, 0.0)
                        .w_full()
                        .h_full()
                        .gradient(Color::BLACK.alpha(0.12), Color::BLACK.alpha(0.88))
                        .inert(),
                );
        }
    } else if let Some(game) = state.selected_game()
        && let Some(asset) = game.artwork.get(&ArtworkKind::Background)
        && state.images.contains_key(&asset.image_path)
    {
        root = root.child(
            crate::library::image(state, &asset.image_path, &kit)
                .key("ambient-artwork")
                .transition(Transition::none())
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .opacity(if kit.c.text_base.luminance() > kit.c.page.luminance() {
                    0.08
                } else {
                    0.025
                })
                .blur(72.0)
                .inert(),
        );
    }
    let frame = col()
        .key("application-frame")
        .transition(Transition::none())
        .w_full()
        .h_full()
        .padding(if wall { 0.0 } else { 24.0 })
        .pt(if wall { 0.0 } else { 72.0 })
        .pb(if wall { 0.0 } else { 24.0 })
        .child(workspace);
    root = root
        .child(frame)
        .child(header.pin(Some(12.0), Some(10.0), Some(12.0), None));
    overlays(state, &kit, size, &mut layers);
    crate::commands::context_menu(state, &kit, &mut layers);
    rune_kit::root(root, layers)
}

pub fn scene_background(state: &State, kit: &Kit) -> El {
    let mut background = el().w_full().h_full().fill(kit.c.page);
    if let Some(path) = &state.previous_background {
        background = background.child(
            crate::library::image(state, path, kit)
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .transition(Transition::none()),
        );
    }
    if let Some(path) = &state.presented_background {
        background = background.child(
            crate::library::image(state, path, kit)
                .key(format!("background-{}", path.display()))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .transition(motion::image_transition()),
        );
    }
    effects::artwork_surface(
        background,
        if state.appearance.dither_recents {
            state.appearance.dither_recents_strength.powi(2)
        } else {
            0.0
        },
        state.appearance.dither_recents_size,
    )
}

fn scroll(content: El) -> El {
    col()
        .w_full()
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content)
}

fn overlays(state: &State, kit: &Kit, size: [f32; 2], layers: &mut Vec<El>) {
    if state.overlay == Overlay::Commands {
        crate::commands::overlay(state, kit, size, layers);
        return;
    }
    let (title, body, width) = match state.overlay {
        Overlay::WallFocus => ("Artwork".into(), crate::wall::focus(state, kit), 420.0),
        Overlay::None => return,
        Overlay::Editor => {
            let title = state
                .editor
                .game_id
                .as_ref()
                .and_then(|id| state.game(id))
                .map(|game| game.title().to_owned())
                .unwrap_or_else(|| "Game editor".into());
            (title, crate::editor::view(state, kit, layers), 800.0)
        }
        Overlay::AddGame => (
            "Add a game".into(),
            crate::editor::manual_view(state, kit),
            640.0,
        ),
        Overlay::Customize => (
            "Customize".into(),
            crate::settings::customize(state, kit, layers),
            700.0,
        ),
        Overlay::Theme => (
            "Edit theme".into(),
            crate::settings::theme_editor(state, kit),
            640.0,
        ),
        Overlay::Commands => (
            "Search games and actions".into(),
            crate::commands::palette(state, kit),
            700.0,
        ),
        Overlay::Notifications => (
            "Activity & Notifications".into(),
            crate::commands::notifications(state, kit),
            700.0,
        ),
        Overlay::Confirm => {
            let title = state
                .confirmation
                .as_ref()
                .map(|confirmation| confirmation.title.clone())
                .unwrap_or_else(|| "Confirmation".into());
            let message = state
                .confirmation
                .as_ref()
                .map(|confirmation| confirmation.message.clone())
                .unwrap_or_default();
            (
                title,
                col().w_full().gap(20.0).child(hint(message, kit)).child(
                    row()
                        .gap(10.0)
                        .child(
                            primary("Confirm")
                                .on_press(|s: &mut State| s.accept_confirmation())
                                .build(kit)
                                .key("confirm-action"),
                        )
                        .child(
                            button("Cancel")
                                .on_press(|s: &mut State| {
                                    let overlay = s
                                        .confirmation
                                        .take()
                                        .map_or(Overlay::None, |confirmation| {
                                            confirmation.return_to
                                        });
                                    s.show_overlay(overlay);
                                })
                                .build(kit),
                        ),
                ),
                480.0,
            )
        }
    };
    let mut body = col().w_full().gap(12.0).child(body);
    if let Some(error) = &state.error {
        body = body.child(hint(error, kit).live_region(rune::LiveRegion::Assertive));
    }
    let maximum = match state.overlay {
        Overlay::Confirm => 240.0,
        Overlay::AddGame => 420.0,
        Overlay::Commands | Overlay::Notifications => 400.0,
        _ => 600.0,
    };
    let body_height = (size[1] - 180.0).clamp(180.0, maximum);
    let body = col()
        .w_full()
        .max_h(body_height)
        .min_h(0.0)
        .padding(24.0)
        .pt(8.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(body);
    let mut dialog_kit = kit.clone();
    dialog_kit.c.layer_01 = kit.c.layer_01;
    let mut dialog_layers = Vec::new();
    let mut dialog = Dialog::new("application", title)
        .open(true)
        .fit()
        .width(f32::min(width, size[0] - 48.0))
        .click_outside(true)
        .on_close(|s: &mut State| s.dismiss_overlay())
        .child(body);
    if state.overlay != Overlay::Confirm {
        dialog = dialog.description(match state.overlay {
            Overlay::Customize => "Appearance, layouts and artwork treatment",
            Overlay::Editor => "Details, artwork, launch options and journal",
            Overlay::AddGame => "Add a local game or application to your library",
            Overlay::Theme => "Create a personal colour palette",
            Overlay::Commands => "Find games and application actions",
            Overlay::WallFocus => "Selected cover artwork",
            _ => "Background work and recent notifications",
        });
    }
    dialog.build(&dialog_kit, &mut dialog_layers);
    dialog_layers.append(layers);
    *layers = dialog_layers;
}
