use super::*;

impl LauncherView {
    pub(super) fn design(&self) -> &'static crate::design::Profile {
        crate::design::resolve(&self.appearance)
    }

    pub(super) fn collection_overlay_open(&self) -> bool {
        self.settings_open
            || self.command_mode.is_some()
            || self.customization_open
            || self.theme_editor.is_some()
            || self.jobs_open
            || self.game_menu_open
            || self.editor_is_modal()
    }

    pub(super) fn recents_layout_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.weak_entity();
        choice_dropdown(
            "recents-layout",
            match self.appearance.recents_layout {
                RecentsLayout::Editorial => RecentsLayout::Carousel,
                layout => layout,
            },
            [
                RecentsLayout::Carousel,
                RecentsLayout::Gallery,
                RecentsLayout::Cinematic,
                RecentsLayout::InfiniteWall,
                RecentsLayout::FilmReel,
                RecentsLayout::ArtBook,
                RecentsLayout::Diagonal,
                RecentsLayout::Mosaic,
            ]
            .into_iter()
            .map(|layout| {
                let mut settings = self.appearance.clone();
                settings.recents_layout = layout;
                (layout, crate::design::resolve(&settings).name.into())
            })
            .collect(),
            self.snapshot.ready,
            move |recents_layout, window, cx| {
                let _ = owner.update(cx, |view, cx| {
                    view.set_appearance(
                        AppearanceSettings {
                            recents_layout,
                            ..view.appearance.clone()
                        },
                        window,
                        cx,
                    );
                    view.focus.focus(window, cx);
                });
            },
            cx,
        )
    }
}
