use super::*;

impl LauncherView {
    pub(crate) fn automation_status(&self, cx: &App) -> serde_json::Value {
        let snapshot = self.api.snapshot();
        let selected = self
            .selected
            .as_ref()
            .and_then(|id| snapshot.games.iter().find(|game| &game.id == id));
        serde_json::json!({
            "ready": self.snapshot.ready, "backendReady": snapshot.ready, "initializationError": snapshot.initialization_error,
            "screen": format!("{:?}", self.screen), "settingsOpen": self.settings_open,
            "editorOpen": self.editor_is_modal(), "editorTab": format!("{:?}", self.detail_tab),
            "commandOpen": self.command_mode.is_some(), "jobsOpen": self.jobs_open,
            "customizationOpen": self.customization_open, "notice": self.notice,
            "selected": selected.map(|game| serde_json::json!({"id": game.id, "title": game.title(), "metadata": game.metadata})),
            "gameCount": snapshot.games.len(), "jobs": snapshot.jobs.iter().map(crate::automation::job_status).collect::<Vec<_>>(),
            "paths": {"data": snapshot.paths.data_directory, "cache": snapshot.paths.cache_directory},
            "renderPaused": self.render_power.paused(), "sidebarWidth": self.appearance.sidebar_width,
            "cinematicLaunchHovered": self.cinematic_launch_hovered,
            "scene": {
                "presentedGame": self.art_scenes.selected,
                "progress": self.art_scenes.page.value(Instant::now()),
                "hoveredGame": self.art_scenes.hovered,
                "hoverAmount": self.art_scenes.hovered.as_ref().map_or(0., |id| self.art_scenes.emphasis(id, Instant::now())),
                "gpuReady": self.gallery_gpu.read(cx).frame().is_some(),
                "gpuLiquid": self.gallery_gpu.read(cx).frame().and_then(|frame| frame.scene.liquid),
                "gpuCoverCount": self.gallery_gpu.read(cx).frame().map(|frame| frame.scene.covers.len()),
                "mosaicReadyPages": self.art_scenes.mosaic_ready_pages,
                "mosaicPage": self.art_scenes.selected.as_ref().and_then(|id| self.art_scenes.position(id)).map(|index| index / 6)
            },
            "appearance": self.appearance,
            "draft": {
                "title": self.title.read(cx).value().to_string(),
                "description": self.description.read(cx).value().to_string(),
                "developer": self.developer.read(cx).value().to_string(),
                "genres": self.genres.read(cx).value().to_string(),
                "journal": self.journal_text.read(cx).value().to_string()
            },
            "commandQuery": self.command_search.read(cx).value().to_string()
        })
    }
}
