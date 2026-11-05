use super::*;
use startpoint_sdk::BackgroundStyle;

impl LauncherView {
    pub(super) fn background_selectable(&self) -> bool {
        matches!(
            self.appearance.recents_layout,
            RecentsLayout::Carousel
                | RecentsLayout::Gallery
                | RecentsLayout::Cinematic
                | RecentsLayout::InfiniteWall
        )
    }

    pub(super) fn liquid_background_active(&self) -> bool {
        self.background_selectable()
            && self.appearance.background_style == BackgroundStyle::Balatro
            && !self.settings_open
            && (self.screen == StartScreen::Recents || self.gallery_active())
    }

    pub(super) fn background_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.weak_entity();
        choice_dropdown(
            "background-style",
            self.appearance.background_style,
            vec![
                (BackgroundStyle::Default, "Default".into()),
                (BackgroundStyle::Balatro, "Balatro".into()),
            ],
            self.snapshot.ready,
            move |background_style, window, cx| {
                let _ = owner.update(cx, |view, cx| {
                    view.set_appearance(
                        AppearanceSettings {
                            background_style,
                            ..view.appearance.clone()
                        },
                        window,
                        cx,
                    );
                });
            },
            cx,
        )
    }

    pub(super) fn liquid_parameters(&self) -> [f32; 4] {
        let accent = if self.cinematic_active() {
            self.cinematic_scene.accent(Instant::now())
        } else {
            self.art_scenes.ambient.accent(Instant::now())
        };
        [
            if appearance::reduced_motion() {
                0.
            } else {
                (self.background_clock.seconds() * 30.).floor() / 30.
            },
            accent[0],
            accent[1],
            accent[2],
        ]
    }

    pub(super) fn prepare_background(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.liquid_background_active() || self.gallery_active() || self.render_power.paused() {
            return;
        }
        let [seconds, r, g, b] = self.liquid_parameters();
        let scene = crate::gallery_gpu::scene::Scene::reel(
            self.main_content_width(window),
            f32::from(window.viewport_size().height),
            seconds,
            [r, g, b],
        );
        self.gallery_gpu
            .update(cx, |gpu, cx| gpu.request(scene, window, cx));
    }

    pub(super) fn liquid_background(&self, cx: &App) -> AnyElement {
        div()
            .absolute()
            .inset_0()
            .size_full()
            .bg(rgb(0x0b1414))
            .when_some(
                self.gallery_gpu
                    .read(cx)
                    .frame()
                    .filter(|frame| frame.scene.liquid.is_some() && frame.scene.covers.is_empty()),
                |stage, frame| stage.child(crate::gallery_gpu::frame_image(frame)),
            )
            .into_any_element()
    }
}
