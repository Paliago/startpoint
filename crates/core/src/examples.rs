use startpoint_sdk::{
    ApiError, DiscoveredGame, DiscoveryProvider, DiscoveryReport, DiscoverySettings, JobContext,
    LaunchTarget, ProviderDescriptor,
};

pub struct ExampleProvider;

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
        context.report("Loading 45 example games · launches are simulated");
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
            (638_970, "Yakuza 0"),
            (257_350, "Baldur's Gate II: Enhanced Edition"),
            (292_030, "The Witcher 3: Wild Hunt"),
            (638_230, "Journey"),
            (753_640, "Outer Wilds"),
            (814_380, "Sekiro: Shadows Die Twice"),
            (1_174_180, "Red Dead Redemption 2"),
            (870_780, "Control"),
            (1_593_500, "God of War"),
            (1_151_640, "Horizon Zero Dawn"),
            (1_361_320, "The Room 4: Old Sins"),
            (460_950, "Katana ZERO"),
            (250_900, "The Binding of Isaac: Rebirth"),
            (311_690, "Enter the Gungeon"),
            (268_910, "Cuphead"),
            (391_540, "Undertale"),
            (1_057_090, "Ori and the Will of the Wisps"),
            (237_930, "Transistor"),
            (107_100, "Bastion"),
            (462_770, "Pyre"),
            (387_290, "Ori and the Blind Forest: Definitive Edition"),
            (1_250_410, "Microsoft Flight Simulator"),
            (632_470, "Disco Elysium"),
            (427_520, "Factorio"),
            (294_100, "RimWorld"),
            (548_430, "Deep Rock Galactic"),
            (632_360, "Risk of Rain 2"),
            (413_420, "Danganronpa 2: Goodbye Despair"),
            (264_710, "Subnautica"),
            (203_160, "Tomb Raider"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_library_has_45_distinct_games_with_simulated_launches() {
        let context = JobContext::new(std::sync::Arc::default(), |_| {});
        let report = ExampleProvider
            .discover(&DiscoverySettings::default(), &context)
            .unwrap();
        assert!(report.complete);
        assert_eq!(report.games.len(), 45);
        let ids: std::collections::BTreeSet<_> = report
            .games
            .iter()
            .map(|game| &game.source_game_id)
            .collect();
        assert_eq!(ids.len(), report.games.len());
        assert!(report.games.iter().all(|game| matches!(
            &game.launch_target, LaunchTarget::Uri(uri) if uri.starts_with("startpoint-example://game/")
        )));
    }
}
