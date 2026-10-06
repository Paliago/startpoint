use startpoint_sdk::{
    ApiError, DiscoveredGame, DiscoveryProvider, DiscoveryReport, DiscoverySettings, JobContext,
    LaunchTarget, ProviderDescriptor,
};

pub(crate) struct ExampleProvider;

impl DiscoveryProvider for ExampleProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "example".into(),
            name: "Example library".into(),
        }
    }

    fn discover(
        &self,
        _: &DiscoverySettings,
        context: &JobContext,
    ) -> Result<DiscoveryReport, ApiError> {
        context.report("Loading 15 example games · launches are simulated");
        let games = [
            (1_145_350, "Hades II"),
            (1_091_500, "Cyberpunk 2077"),
            (367_520, "Hollow Knight"),
            (2_379_780, "Balatro"),
            (1_245_620, "ELDEN RING"),
            (1_145_360, "Hades"),
            (413_150, "Stardew Valley"),
            (1_086_940, "Baldur's Gate 3"),
            (504_230, "Celeste"),
            (646_570, "Slay the Spire"),
            (1_794_680, "Vampire Survivors"),
            (400, "Portal"),
            (620, "Portal 2"),
            (1_055_540, "A Short Hike"),
            (588_650, "Dead Cells"),
        ]
        .into_iter()
        .map(|(app_id, title)| DiscoveredGame {
            source_game_id: app_id.to_string(),
            title: title.into(),
            install_directory: std::path::PathBuf::new(),
            launch_target: LaunchTarget::Uri(format!("startpoint-example://game/{app_id}")),
            local_cover: None,
        })
        .collect();
        Ok(DiscoveryReport {
            games,
            complete: true,
            warnings: Vec::new(),
        })
    }
}
