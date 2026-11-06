//! Release builds opt in with `self-update`. All network/package work uses dedicated
//! threads, never the UI thread or the executor shared with library/artwork loading.
use crate::{
    components::{button, hint},
    state::{HostEvent, State},
};
use rune::{El, Waker, col};
use rune_kit::Kit;
use std::{path::Path, sync::mpsc, time::Duration};
use velopack::{UpdateCheck, UpdateInfo, UpdateManager, VelopackAsset};

const APP_ID: &str = "Paliago.Startpoint.App";
const REPOSITORY: &str = match option_env!("STARTPOINT_RELEASE_REPOSITORY") {
    Some(repository) => repository,
    None => "Paliago/startpoint",
};
const STARTUP_DELAY: Duration = Duration::from_secs(2);

pub fn supported_platform() -> bool {
    release_channel().is_some()
}

fn release_channel() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("win-x64-stable"),
        ("macos", "aarch64") => Some("osx-arm64-stable"),
        _ => None,
    }
}

pub enum Event {
    Checked(Result<(UpdateManager, Outcome), CheckFailure>),
    Progress(i16),
    Downloaded(VelopackAsset, bool),
    RestartPrepared(bool),
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

    fn notification_visible(self) -> bool {
        matches!(
            self,
            Self::Available | Self::Downloading | Self::Ready | Self::Restarting
        )
    }

    fn action_disabled(self, restart_allowed: bool) -> bool {
        self.busy() || (self == Self::Ready && !restart_allowed)
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

impl Updater {
    pub fn new(enabled: bool) -> Self {
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
            unavailable_reason: "Automatic updates are disabled in development, example and unsupported platform builds.",
        }
    }

    pub fn check(&mut self, sender: async_channel::Sender<HostEvent>, wake: Waker, delayed: bool) {
        if self.phase.busy() || matches!(self.phase, Phase::Unavailable | Phase::Ready) {
            return;
        }
        if !dispatch(sender, wake, move |_, _| {
            if delayed {
                std::thread::sleep(STARTUP_DELAY);
            }
            Event::Checked(check_for_update())
        }) {
            self.error = Some("Could not start the update checker. Try again.");
            return;
        }
        self.phase = Phase::Checking;
        self.error = None;
    }

    fn download(&mut self, sender: async_channel::Sender<HostEvent>, wake: Waker) {
        if self.phase != Phase::Available {
            return;
        }
        let (Some(manager), Some(update)) = (self.manager.clone(), self.available.clone()) else {
            return;
        };
        let asset = update.TargetFullRelease.clone();
        if !dispatch(sender, wake, move |sender, wake| {
            let (progress, receiver) = mpsc::channel();
            let sender = sender.clone();
            let wake = wake.clone();
            let _ = std::thread::Builder::new()
                .name("startpoint-update-progress".into())
                .spawn(move || {
                    for value in receiver {
                        post(&sender, &wake, Event::Progress(value));
                    }
                });
            let success = manager.download_updates(&update, Some(progress)).is_ok();
            Event::Downloaded(asset, success)
        }) {
            self.error = Some("Could not start the download. Try again.");
            return;
        }
        self.phase = Phase::Downloading;
        self.progress = 0;
        self.error = None;
    }

    /// Called by the launcher only after checking for open editors and active jobs.
    fn restart(&mut self, sender: async_channel::Sender<HostEvent>, wake: Waker) {
        if self.phase != Phase::Ready {
            return;
        }
        let (Some(manager), Some(asset)) = (self.manager.clone(), self.pending.clone()) else {
            return;
        };
        if !dispatch(sender, wake, move |_, _| {
            Event::RestartPrepared(
                manager
                    .wait_exit_then_apply_updates(&asset, false, true, Vec::<String>::new())
                    .is_ok(),
            )
        }) {
            self.error = Some("Could not start the updater. Try again.");
            return;
        }
        self.phase = Phase::Restarting;
        self.error = None;
    }

    pub fn action(
        &mut self,
        sender: async_channel::Sender<HostEvent>,
        wake: Waker,
        restart_allowed: bool,
    ) {
        match self.phase {
            Phase::Available => self.download(sender, wake),
            Phase::Ready if restart_allowed => self.restart(sender, wake),
            Phase::Idle | Phase::Current => self.check(sender, wake, false),
            _ => {}
        }
    }

    pub fn handle(&mut self, event: Event) -> bool {
        match event {
            Event::Checked(Ok((manager, outcome))) => {
                self.manager = Some(manager);
                self.available = None;
                self.pending = None;
                self.phase = match outcome {
                    Outcome::Current => Phase::Current,
                    Outcome::Available(update) => {
                        self.available = Some(update);
                        Phase::Available
                    }
                    Outcome::Pending(asset) => {
                        self.pending = Some(asset);
                        Phase::Ready
                    }
                };
            }
            Event::Checked(Err(CheckFailure::Unavailable(reason))) => {
                self.phase = Phase::Unavailable;
                self.unavailable_reason = reason;
            }
            Event::Checked(Err(CheckFailure::Network)) => {
                self.phase = Phase::Idle;
                self.error =
                    Some("Could not check for updates. Check your connection and try again.");
                crate::diagnostics::record("update check failed");
            }
            Event::Progress(progress) if self.phase == Phase::Downloading => {
                self.progress = progress.clamp(0, 100)
            }
            Event::Progress(_) => {}
            Event::Downloaded(asset, true) => {
                self.pending = Some(asset);
                self.phase = Phase::Ready;
            }
            Event::Downloaded(_, false) => {
                self.phase = Phase::Available;
                self.error = Some(
                    "Download or verification failed. Your current installation is unchanged; try again.",
                );
            }
            Event::RestartPrepared(true) => return true,
            Event::RestartPrepared(false) => {
                self.phase = Phase::Ready;
                self.error = Some(
                    "Could not apply the update. Close other Startpoint instances and try again.",
                );
            }
        }
        false
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

    pub fn notification_status(&self) -> Option<String> {
        self.phase.notification_visible().then(|| self.status())
    }
}

pub fn settings(state: &State, kit: &Kit) -> El {
    let updater = &state.updater;
    let restart_allowed = !state.busy()
        && matches!(
            state.overlay,
            crate::state::Overlay::None | crate::state::Overlay::Notifications
        );
    let mut body = col().w_full().gap(12.0).child(hint(updater.status(), kit));
    if let Some(error) = updater.error {
        body = body.child(crate::components::error_message(error, kit));
    }
    if updater.phase != Phase::Unavailable {
        body = body.child(
            crate::components::form_actions().child(
                button(&updater.action_label())
                    .disabled(updater.phase.action_disabled(restart_allowed))
                    .on_press(|s: &mut State| {
                        if let Some(wake) = s.wake.clone() {
                            let allowed = !s.busy()
                                && matches!(
                                    s.overlay,
                                    crate::state::Overlay::None
                                        | crate::state::Overlay::Notifications
                                );
                            s.updater.action(s.host_sender.clone(), wake, allowed);
                        }
                    })
                    .build(kit)
                    .key("update-action"),
            ),
        );
    }
    if updater.phase == Phase::Ready && !restart_allowed {
        body = body.child(hint(
            "Finish active jobs and close editors before restarting.",
            kit,
        ));
    }
    body
}

fn post(sender: &async_channel::Sender<HostEvent>, wake: &Waker, event: Event) {
    let _ = sender.send_blocking(HostEvent::Update(event));
    wake();
}

fn dispatch(
    sender: async_channel::Sender<HostEvent>,
    wake: Waker,
    work: impl FnOnce(&async_channel::Sender<HostEvent>, &Waker) -> Event + Send + 'static,
) -> bool {
    std::thread::Builder::new()
        .name("startpoint-update".into())
        .spawn(move || {
            let event = work(&sender, &wake);
            post(&sender, &wake, event);
        })
        .is_ok()
}

pub enum Outcome {
    Current,
    Available(Box<UpdateInfo>),
    Pending(VelopackAsset),
}

#[derive(Debug)]
pub enum CheckFailure {
    Unavailable(&'static str),
    Network,
}

fn check_for_update() -> Result<(UpdateManager, Outcome), CheckFailure> {
    if !valid_repository(REPOSITORY) {
        return Err(CheckFailure::Unavailable(
            "This build has an invalid release repository.",
        ));
    }
    let channel = release_channel().ok_or(CheckFailure::Unavailable(
        "This platform does not have an update channel.",
    ))?;
    let options = velopack::UpdateOptions {
        ExplicitChannel: Some(channel.into()),
        MaximumDeltasBeforeFallback: -1,
        ..Default::default()
    };
    let manager = UpdateManager::new(ReleaseSource, Some(options), None).map_err(|_| {
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
    use super::{
        APP_ID, Event, Outcome, Phase, Updater, query_manager, valid_repository, validate_asset,
    };
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
    fn automatic_checks_and_failures_do_not_show_a_notification() {
        for phase in [
            Phase::Idle,
            Phase::Checking,
            Phase::Current,
            Phase::Unavailable,
        ] {
            assert!(!phase.notification_visible());
        }
        for phase in [
            Phase::Available,
            Phase::Downloading,
            Phase::Ready,
            Phase::Restarting,
        ] {
            assert!(phase.notification_visible());
        }
    }

    #[test]
    fn notification_tracks_download_progress_and_restart_readiness() {
        let mut updater = Updater {
            phase: Phase::Downloading,
            manager: None,
            available: None,
            pending: Some(asset()),
            progress: 42,
            error: None,
            unavailable_reason: "Unavailable",
        };
        assert_eq!(
            updater.notification_status().as_deref(),
            Some("Downloading 0.2.0 — 42%")
        );
        assert_eq!(updater.action_label(), "Downloading 42%");
        assert!(!updater.handle(Event::Progress(57)));
        assert_eq!(updater.action_label(), "Downloading 57%");
        assert!(!updater.handle(Event::Downloaded(asset(), true)));
        assert_eq!(
            updater.notification_status().as_deref(),
            Some("Startpoint 0.2.0 is ready. Restart to finish updating.")
        );
        assert_eq!(updater.action_label(), "Restart to update");
        assert!(updater.handle(Event::RestartPrepared(true)));
        updater.phase = Phase::Idle;
        updater.error = Some("Network failure");
        assert!(updater.notification_status().is_none());
    }

    #[test]
    fn notification_and_settings_actions_keep_restart_and_busy_guards() {
        for phase in [Phase::Checking, Phase::Downloading, Phase::Restarting] {
            assert!(phase.action_disabled(true));
            assert!(phase.action_disabled(false));
        }
        assert!(Phase::Ready.action_disabled(false));
        assert!(!Phase::Ready.action_disabled(true));
        assert!(!Phase::Available.action_disabled(false));
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
