//! Resolve discovery locations on the runtime worker, never during a UI frame.
use startpoint_sdk::{Game, SettingsSnapshot};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) fn resolve(
    settings: &SettingsSnapshot,
    games: &[Game],
) -> BTreeMap<String, Vec<PathBuf>> {
    let steam = settings.steam_directory.clone().or_else(|| {
        steamlocate::SteamDir::locate()
            .ok()
            .map(|steam| steam.path().to_path_buf())
    });
    let mut gog = crate::stores::gog_directories(&settings.gog_directory);
    if settings.gog_directory.is_none() {
        // Windows GOG discovery also reads installer records. Show the resolved
        // installation directories retained by discovery, not a made-up root.
        gog.extend(
            games
                .iter()
                .filter(|game| game.provider_id == "gog" && game.available)
                .map(|game| game.install_directory.clone()),
        );
        gog.sort();
        gog.dedup();
    }
    BTreeMap::from([
        ("steam".into(), steam.into_iter().collect()),
        (
            "epic".into(),
            crate::stores::epic_directories(&settings.epic_directory),
        ),
        ("gog".into(), gog),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_paths_match_the_locations_used_by_discovery() {
        let settings = SettingsSnapshot {
            steam_directory: Some(PathBuf::from("steam-root")),
            epic_directory: Some(PathBuf::from("epic-manifests")),
            gog_directory: Some(PathBuf::from("gog-games")),
            ..SettingsSnapshot::default()
        };
        let paths = resolve(&settings, &[]);
        assert_eq!(paths["steam"], vec![settings.steam_directory.unwrap()]);
        assert_eq!(paths["epic"], vec![settings.epic_directory.unwrap()]);
        assert_eq!(paths["gog"], vec![settings.gog_directory.unwrap()]);
    }
}
