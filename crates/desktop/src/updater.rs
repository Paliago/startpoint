//! Release builds opt in with `self-update`. All network/package work uses dedicated
//! threads, never the UI thread or the executor shared with library/artwork loading.
use crate::components::{button, hint};
use gpui::{prelude::*, *};
use gpui_component::{Disableable, button::ButtonVariants};
use std::{path::Path, sync::mpsc, time::Duration};
use velopack::{UpdateCheck, UpdateInfo, UpdateManager, VelopackAsset};

const APP_ID: &str = "Paliago.Startpoint.App";
const REPOSITORY: &str = match option_env!("STARTPOINT_RELEASE_REPOSITORY") {
    Some(repository) => repository,
    None => "Paliago/startpoint",
};
const STARTUP_DELAY: Duration = Duration::from_secs(2);

pub enum Event {
    RestartRequested,
    RestartPrepared,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Ready,
    Restarting,
    Unavailable,
}

impl Phase {
    fn busy(self) -> bool {
        matches!(self, Self::Checking | Self::Downloading | Self::Restarting)
    }

    fn badge_visible(self) -> bool {
        matches!(
            self,
            Self::Available | Self::Downloading | Self::Ready | Self::Restarting
        )
    }
}

pub struct Updater {
    phase: Phase,
    manager: Option<UpdateManager>,
    available: Option<Box<UpdateInfo>>,
    pending: Option<VelopackAsset>,
    progress: i16,
    error: Option<&'static str>,
    unavailable_reason: &'static str,
}

impl EventEmitter<Event> for Updater {}

impl Updater {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let enabled = cfg!(target_os = "windows")
            && !cfg!(debug_assertions)
            && !std::env::args()
                .any(|arg| matches!(arg.as_str(), "--example-library" | "--diagnostics"));
        if enabled {
            cx.spawn_in(window, async move |updater, cx| {
                cx.background_executor().timer(STARTUP_DELAY).await;
                let _ = updater.update_in(cx, |updater, window, cx| {
                    // A manual check may have run before the startup timer fired.
                    if updater.phase == Phase::Idle && updater.error.is_none() {
                        updater.check(window, cx);
                    }
                });
            })
            .detach();
        }
        Self {
            phase: if enabled {
                Phase::Idle
            } else {
                Phase::Unavailable
            },
            manager: None,
            available: None,
            pending: None,
            progress: 0,
            error: None,
            unavailable_reason: "Automatic updates are disabled in development, example and non-Windows builds.",
        }
    }

    fn check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.phase.busy() || matches!(self.phase, Phase::Unavailable | Phase::Ready) {
            return;
        }
        let Ok(result) = worker(check_for_update) else {
            self.error = Some("Could not start the update checker. Try again.");
            cx.notify();
            return;
        };
        self.phase = Phase::Checking;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |updater, cx| {
            let result = result.recv().await.unwrap_or(Err(CheckFailure::Network));
            let _ = updater.update_in(cx, |updater, _, cx| {
                match result {
                    Ok((manager, outcome)) => {
                        updater.manager = Some(manager);
                        updater.available = None;
                        updater.pending = None;
                        match outcome {
                            Outcome::Current => updater.phase = Phase::Current,
                            Outcome::Available(update) => {
                                updater.available = Some(update);
                                updater.phase = Phase::Available;
                            }
                            Outcome::Pending(asset) => {
                                updater.pending = Some(asset);
                                updater.phase = Phase::Ready;
                            }
                        }
                    }
                    Err(CheckFailure::Unavailable(reason)) => {
                        updater.phase = Phase::Unavailable;
                        updater.unavailable_reason = reason;
                    }
                    Err(CheckFailure::Network) => {
                        // Startup failures stay quiet: no badge, dialog or retry loop.
                        updater.phase = Phase::Idle;
                        updater.error = Some(
                            "Could not check for updates. Check your connection and try again.",
                        );
                        crate::diagnostics::record("update check failed");
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.phase != Phase::Available {
            return;
        }
        let (Some(manager), Some(update)) = (self.manager.clone(), self.available.clone()) else {
            return;
        };
        let asset = update.TargetFullRelease.clone();
        let (progress, progress_rx) = mpsc::channel();
        let Ok(result) = worker(move || manager.download_updates(&update, Some(progress)).is_ok())
        else {
            self.error = Some("Could not start the download. Try again.");
            cx.notify();
            return;
        };
        self.phase = Phase::Downloading;
        self.progress = 0;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |updater, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let percent = progress_rx.try_iter().last();
                let running = updater.update_in(cx, |updater, _, cx| {
                    if let Some(percent) = percent {
                        updater.progress = percent.clamp(0, 100);
                        cx.notify();
                    }
                    updater.phase == Phase::Downloading
                });
                if !matches!(running, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
        cx.spawn_in(window, async move |updater, cx| {
            let success = result.recv().await.unwrap_or(false);
            let _ = updater.update_in(cx, |updater, _, cx| {
                if success {
                    updater.pending = Some(asset);
                    updater.phase = Phase::Ready;
                    crate::diagnostics::record("update downloaded and verified");
                } else {
                    updater.phase = Phase::Available;
                    updater.error = Some("Download or verification failed. Your current installation is unchanged; try again.");
                    crate::diagnostics::record("update download failed");
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Called by the launcher only after checking for open editors and active jobs.
    pub fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.phase != Phase::Ready {
            return;
        }
        let (Some(manager), Some(asset)) = (self.manager.clone(), self.pending.clone()) else {
            return;
        };
        let Ok(result) = worker(move || {
            // Do not use apply_updates_and_restart: it calls process::exit and skips
            // GPUI's quit handlers (including saving the window placement).
            manager
                .wait_exit_then_apply_updates(&asset, false, true, Vec::<String>::new())
                .is_ok()
        }) else {
            self.error = Some("Could not start the updater. Try again.");
            cx.notify();
            return;
        };
        self.phase = Phase::Restarting;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |updater, cx| {
            let success = result.recv().await.unwrap_or(false);
            let _ = updater.update_in(cx, |updater, _, cx| {
                if success {
                    cx.emit(Event::RestartPrepared);
                } else {
                    updater.phase = Phase::Ready;
                    updater.error = Some("Could not apply the update. Close other Startpoint instances and try again.");
                    crate::diagnostics::record("update restart failed");
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.phase {
            Phase::Available => self.download(window, cx),
            Phase::Ready => cx.emit(Event::RestartRequested),
            Phase::Idle | Phase::Current => self.check(window, cx),
            _ => {}
        }
    }

    fn version(&self) -> &str {
        self.pending
            .as_ref()
            .map(|asset| asset.Version.as_str())
            .or_else(|| {
                self.available
                    .as_ref()
                    .map(|update| update.TargetFullRelease.Version.as_str())
            })
            .unwrap_or(env!("CARGO_PKG_VERSION"))
    }

    fn status(&self) -> String {
        match self.phase {
            Phase::Idle => "Checks automatically shortly after launch.".into(),
            Phase::Checking => "Checking for updates…".into(),
            Phase::Current => "You're up to date.".into(),
            Phase::Available => format!("Startpoint {} is available.", self.version()),
            Phase::Downloading => format!("Downloading {} — {}%", self.version(), self.progress),
            Phase::Ready => format!(
                "Startpoint {} is ready. Restart to finish updating.",
                self.version()
            ),
            Phase::Restarting => "Restarting to update…".into(),
            Phase::Unavailable => self.unavailable_reason.into(),
        }
    }

    fn action_label(&self) -> String {
        match self.phase {
            Phase::Available => "Download update".into(),
            Phase::Downloading => format!("Downloading {}%", self.progress),
            Phase::Ready => "Restart to update".into(),
            Phase::Restarting => "Restarting…".into(),
            Phase::Checking => "Checking…".into(),
            _ => "Check for updates".into(),
        }
    }

    pub fn badge(&self, restart_allowed: bool, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("update-island")
            .when(self.phase.badge_visible(), |row| {
                row.occlude()
                    .mr_2()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        button("update-badge")
                            .h(px(28.))
                            .px_2()
                            .label(self.action_label())
                            .disabled(
                                self.phase.busy()
                                    || (self.phase == Phase::Ready && !restart_allowed),
                            )
                            .tooltip(if self.phase == Phase::Ready && !restart_allowed {
                                "Finish active jobs and close editors before restarting."
                                    .to_string()
                            } else {
                                self.status()
                            })
                            .on_click(
                                cx.listener(|updater, _, window, cx| updater.action(window, cx)),
                            ),
                    )
            })
            .into_any_element()
    }

    pub fn settings(&self, restart_allowed: bool, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(hint(&self.status()))
            .when_some(self.error, |section, error| section.child(hint(error)))
            .when(self.phase != Phase::Unavailable, |section| {
                section.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            button("update-action")
                                .secondary()
                                .h(px(32.))
                                .px_3()
                                .label(self.action_label())
                                .disabled(
                                    self.phase.busy()
                                        || (self.phase == Phase::Ready && !restart_allowed),
                                )
                                .on_click(
                                    cx.listener(|updater, _, window, cx| {
                                        updater.action(window, cx)
                                    }),
                                ),
                        )
                        .when(self.phase == Phase::Ready && !restart_allowed, |row| {
                            row.child(hint(
                                "Finish active jobs and close editors before restarting.",
                            ))
                        }),
                )
            })
            .into_any_element()
    }
}

fn worker<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> std::io::Result<async_channel::Receiver<T>> {
    let (sender, receiver) = async_channel::bounded(1);
    std::thread::Builder::new()
        .name("startpoint-update".into())
        .spawn(move || {
            let _ = sender.send_blocking(work());
        })?;
    Ok(receiver)
}

enum Outcome {
    Current,
    Available(Box<UpdateInfo>),
    Pending(VelopackAsset),
}

#[derive(Debug)]
enum CheckFailure {
    Unavailable(&'static str),
    Network,
}

fn check_for_update() -> Result<(UpdateManager, Outcome), CheckFailure> {
    if !valid_repository(REPOSITORY) {
        return Err(CheckFailure::Unavailable(
            "This build has an invalid release repository.",
        ));
    }
    let manager = UpdateManager::new(ReleaseSource, None, None).map_err(|_| {
        CheckFailure::Unavailable(
            "Install Startpoint using the release installer or portable package to enable updates.",
        )
    })?;
    query_manager(manager)
}

fn query_manager(manager: UpdateManager) -> Result<(UpdateManager, Outcome), CheckFailure> {
    if manager.get_app_id() != APP_ID {
        return Err(CheckFailure::Unavailable(
            "This installation has an unexpected application ID.",
        ));
    }
    // A downloaded update remains actionable even offline, including after an
    // unsuccessful apply. Partial downloads are not returned by Velopack.
    if let Some(asset) = manager.get_update_pending_restart() {
        return Ok((manager, Outcome::Pending(asset)));
    }
    let outcome = match manager
        .check_for_updates()
        .map_err(|_| CheckFailure::Network)?
    {
        UpdateCheck::UpdateAvailable(update) => Outcome::Available(update),
        UpdateCheck::NoUpdateAvailable => Outcome::Current,
        UpdateCheck::RemoteIsEmpty => return Err(CheckFailure::Network),
    };
    Ok((manager, outcome))
}

/// The latest stable release is a static HTTP feed: no GitHub API token/rate limit.
/// Downloads are pinned to a version tag so publishing another release mid-download
/// cannot change the URL. Full packages avoid dependencies on older release assets.
#[derive(Clone)]
struct ReleaseSource;

impl velopack::sources::UpdateSource for ReleaseSource {
    fn get_release_feed(
        &self,
        channel: &str,
        app: &velopack::bundle::Manifest,
        staged_user_id: &str,
    ) -> Result<velopack::VelopackAssetFeed, velopack::Error> {
        let source = http_source("latest/download", 10_000);
        let mut feed = source.get_release_feed(channel, app, staged_user_id)?;
        feed.Assets
            .retain(|asset| asset.Type.eq_ignore_ascii_case("Full"));
        for asset in &feed.Assets {
            validate_asset(asset)?;
        }
        Ok(feed)
    }

    fn download_release_entry(
        &self,
        asset: &VelopackAsset,
        local_file: &Path,
        progress: Option<mpsc::Sender<i16>>,
    ) -> Result<(), velopack::Error> {
        validate_asset(asset)?;
        http_source(&format!("download/v{}", asset.Version), 15 * 60 * 1_000)
            .download_release_entry(asset, local_file, progress)
    }
}

fn http_source(path: &str, timeout: u64) -> velopack::sources::HttpSource {
    velopack::sources::HttpSource::new_with_options(
        format!("https://github.com/{REPOSITORY}/releases/{path}"),
        velopack::HttpOptions {
            TimeoutMilliseconds: timeout,
            ..Default::default()
        },
    )
}

fn valid_repository(repository: &str) -> bool {
    let parts = repository.split('/').collect::<Vec<_>>();
    parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
}

fn validate_asset(asset: &VelopackAsset) -> Result<(), velopack::Error> {
    let version = semver::Version::parse(&asset.Version)?;
    let safe_name = asset.FileName.ends_with("-full.nupkg")
        && !asset.FileName.contains("..")
        && asset
            .FileName
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if asset.PackageId != APP_ID
        || !asset.Type.eq_ignore_ascii_case("Full")
        || !safe_name
        || !version.pre.is_empty()
        || !version.build.is_empty()
        || asset.Size == 0
        || asset.SHA256.len() != 64
        || !asset.SHA256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(velopack::Error::InvalidPackage(
            "Unexpected release metadata".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{APP_ID, Outcome, Phase, query_manager, valid_repository, validate_asset};
    use std::{
        io::Write,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc::Sender,
        },
    };
    use velopack::{
        UpdateCheck, UpdateManager, VelopackAsset, VelopackAssetFeed, sources::UpdateSource,
    };

    fn asset() -> VelopackAsset {
        VelopackAsset {
            PackageId: APP_ID.into(),
            Version: "0.2.0".into(),
            Type: "Full".into(),
            FileName: format!("{APP_ID}-0.2.0-full.nupkg"),
            SHA256: "a".repeat(64),
            Size: 123,
            ..Default::default()
        }
    }

    #[test]
    fn accepts_stable_full_release() {
        assert!(validate_asset(&asset()).is_ok());
    }

    #[test]
    fn rejects_unsafe_package_paths() {
        for name in [
            "../evil-full.nupkg",
            "C:\\evil-full.nupkg",
            "/evil-full.nupkg",
            "https://evil/full.nupkg",
            "evil?x-full.nupkg",
        ] {
            let mut asset = asset();
            asset.FileName = name.into();
            assert!(validate_asset(&asset).is_err(), "{name}");
        }
    }

    #[test]
    fn rejects_wrong_app_and_unverified_packages() {
        let mut wrong_app = asset();
        wrong_app.PackageId = "Startpoint".into();
        assert!(validate_asset(&wrong_app).is_err());
        for hash in ["", "short", &"z".repeat(64)] {
            let mut asset = asset();
            asset.SHA256 = hash.into();
            assert!(validate_asset(&asset).is_err());
        }
        let mut empty = asset();
        empty.Size = 0;
        assert!(validate_asset(&empty).is_err());
    }

    #[test]
    fn rejects_prerelease_build_metadata_and_invalid_versions() {
        for version in [
            "0.2.0-beta.1",
            "0.2.0+build.1",
            "../../evil",
            "not-a-version",
        ] {
            let mut asset = asset();
            asset.Version = version.into();
            assert!(validate_asset(&asset).is_err());
        }
    }

    #[test]
    fn rejects_delta_packages() {
        let mut asset = asset();
        asset.Type = "Delta".into();
        assert!(validate_asset(&asset).is_err());
    }

    #[test]
    fn release_repository_is_only_an_owner_and_name() {
        assert!(valid_repository("Paliago/startpoint-releases"));
        for repository in [
            "",
            "owner",
            "owner/repo/path",
            "https://github.com/owner/repo",
            "../repo",
            "owner/repo?token=secret",
        ] {
            assert!(!valid_repository(repository), "{repository}");
        }
    }

    #[test]
    fn automatic_checks_and_failures_do_not_show_a_badge() {
        for phase in [
            Phase::Idle,
            Phase::Checking,
            Phase::Current,
            Phase::Unavailable,
        ] {
            assert!(!phase.badge_visible());
        }
        assert!(Phase::Available.badge_visible());
        assert!(Phase::Ready.badge_visible());
    }

    #[test]
    fn operations_disable_duplicate_actions() {
        for phase in [Phase::Checking, Phase::Downloading, Phase::Restarting] {
            assert!(phase.busy());
        }
        assert!(!Phase::Available.busy());
        assert!(!Phase::Ready.busy());
    }

    #[derive(Clone)]
    struct FixtureSource {
        package: PathBuf,
        asset: VelopackAsset,
        offline: Arc<AtomicBool>,
        interrupted: Arc<AtomicBool>,
    }

    impl UpdateSource for FixtureSource {
        fn get_release_feed(
            &self,
            _: &str,
            _: &velopack::bundle::Manifest,
            _: &str,
        ) -> Result<VelopackAssetFeed, velopack::Error> {
            if self.offline.load(Ordering::Relaxed) {
                return Err(velopack::Error::Other("offline fixture".into()));
            }
            Ok(VelopackAssetFeed {
                Assets: vec![self.asset.clone()],
            })
        }

        fn download_release_entry(
            &self,
            _: &VelopackAsset,
            local: &Path,
            _: Option<Sender<i16>>,
        ) -> Result<(), velopack::Error> {
            if self.interrupted.load(Ordering::Relaxed) {
                std::fs::write(local, b"incomplete download")?;
                return Err(velopack::Error::Io(std::io::Error::from(
                    std::io::ErrorKind::Interrupted,
                )));
            }
            std::fs::copy(&self.package, local)?;
            Ok(())
        }
    }

    struct Installation {
        _directory: tempfile::TempDir,
        current: PathBuf,
        packages: PathBuf,
        source: FixtureSource,
        manager: UpdateManager,
    }

    fn manifest(version: &str) -> String {
        format!(
            r#"<?xml version="1.0"?><package><metadata><id>{APP_ID}</id><version>{version}</version><title>Startpoint</title><authors>test</authors><description>test</description><mainExe>startpoint.exe</mainExe><channel>win-x64-stable</channel></metadata></package>"#
        )
    }

    impl Installation {
        fn new(current_version: &str, remote_version: &str, invalid_hash: bool) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("installed");
            let current = root.join("current");
            let packages = root.join("packages");
            std::fs::create_dir_all(&current).unwrap();
            std::fs::create_dir_all(&packages).unwrap();
            std::fs::write(current.join("sq.version"), manifest(current_version)).unwrap();
            std::fs::write(current.join("startpoint.exe"), b"original installation").unwrap();
            std::fs::write(root.join("Update.exe"), b"fixture, never executed").unwrap();
            let remote = directory
                .path()
                .join(format!("{APP_ID}-{remote_version}-full.nupkg"));
            let mut archive = zip::ZipWriter::new(std::fs::File::create(&remote).unwrap());
            archive
                .start_file(
                    format!("{APP_ID}.nuspec"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            archive
                .write_all(manifest(remote_version).as_bytes())
                .unwrap();
            archive
                .start_file(
                    "lib/app/startpoint.exe",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            archive.write_all(b"new installation").unwrap();
            archive.finish().unwrap();
            let (sha1, sha256) = velopack::misc::calculate_sha1_sha256(&remote).unwrap();
            let source = FixtureSource {
                package: remote.clone(),
                asset: VelopackAsset {
                    PackageId: APP_ID.into(),
                    Version: remote_version.into(),
                    Type: "Full".into(),
                    FileName: remote.file_name().unwrap().to_str().unwrap().into(),
                    SHA1: sha1,
                    SHA256: if invalid_hash { "0".repeat(64) } else { sha256 },
                    Size: remote.metadata().unwrap().len(),
                    ..Default::default()
                },
                offline: Arc::new(AtomicBool::new(false)),
                interrupted: Arc::new(AtomicBool::new(false)),
            };
            let manager = UpdateManager::new(
                source.clone(),
                None,
                Some(velopack::locator::VelopackLocatorConfig {
                    RootAppDir: root.clone(),
                    UpdateExePath: root.join("Update.exe"),
                    PackagesDir: packages.clone(),
                    ManifestPath: current.join("sq.version"),
                    CurrentBinaryDir: current.clone(),
                    IsPortable: true,
                }),
            )
            .unwrap();
            Self {
                _directory: directory,
                current,
                packages,
                source,
                manager,
            }
        }

        fn update(&self) -> Box<velopack::UpdateInfo> {
            match self.manager.check_for_updates().unwrap() {
                UpdateCheck::UpdateAvailable(update) => update,
                _ => panic!("expected newer fixture release"),
            }
        }

        fn assert_installed_unchanged(&self) {
            assert_eq!(
                std::fs::read(self.current.join("startpoint.exe")).unwrap(),
                b"original installation"
            );
            assert_eq!(self.manager.get_current_version_as_string(), "0.1.0");
        }
    }

    #[test]
    fn corrupt_download_cannot_become_pending_or_replace_installation() {
        let installation = Installation::new("0.1.0", "0.2.0", true);
        let error = installation
            .manager
            .download_updates(&installation.update(), None)
            .unwrap_err();
        assert!(matches!(error, velopack::Error::ChecksumInvalid(..)));
        assert!(installation.manager.get_update_pending_restart().is_none());
        assert!(
            !installation
                .packages
                .join(&installation.source.asset.FileName)
                .exists()
        );
        installation.assert_installed_unchanged();
    }

    #[test]
    fn interrupted_download_stays_partial_and_can_be_retried() {
        let installation = Installation::new("0.1.0", "0.2.0", false);
        let update = installation.update();
        installation
            .source
            .interrupted
            .store(true, Ordering::Relaxed);
        assert!(
            installation
                .manager
                .download_updates(&update, None)
                .is_err()
        );
        assert!(installation.manager.get_update_pending_restart().is_none());
        let target = installation
            .packages
            .join(&installation.source.asset.FileName);
        assert!(!target.exists());
        assert!(target.with_extension("partial").exists());
        installation.assert_installed_unchanged();
        installation
            .source
            .interrupted
            .store(false, Ordering::Relaxed);
        installation
            .manager
            .download_updates(&update, None)
            .unwrap();
        assert!(target.exists());
        assert_eq!(
            installation
                .manager
                .get_update_pending_restart()
                .unwrap()
                .Version,
            "0.2.0"
        );
        installation.assert_installed_unchanged();
    }

    #[test]
    fn verified_pending_update_is_available_without_network() {
        let installation = Installation::new("0.1.0", "0.2.0", false);
        installation
            .manager
            .download_updates(&installation.update(), None)
            .unwrap();
        installation.source.offline.store(true, Ordering::Relaxed);
        let (_, outcome) = query_manager(installation.manager.clone()).unwrap();
        assert!(matches!(outcome, Outcome::Pending(asset) if asset.Version == "0.2.0"));
        installation.assert_installed_unchanged();
    }

    #[test]
    fn older_remote_release_cannot_downgrade_installation() {
        let installation = Installation::new("0.2.0", "0.1.0", false);
        let (_, outcome) = query_manager(installation.manager.clone()).unwrap();
        assert!(matches!(outcome, Outcome::Current));
        assert_eq!(
            installation.manager.get_current_version_as_string(),
            "0.2.0"
        );
    }
}
