//! Discover registered Xbox PC (GDK) games without walking protected WindowsApps
//! folders or guessing executable paths. Launch through the registered app ID.
use startpoint_sdk::{
    ApiError, DiscoveryProvider, DiscoveryReport, DiscoverySettings, JobContext, ProviderDescriptor,
};

pub struct XboxProvider;

impl DiscoveryProvider for XboxProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "xbox".into(),
            name: "Xbox".into(),
        }
    }

    fn discover(
        &self,
        _: &DiscoverySettings,
        context: &JobContext,
    ) -> Result<DiscoveryReport, ApiError> {
        context.checkpoint()?;
        #[cfg(target_os = "windows")]
        {
            context.report("Reading registered Xbox PC games");
            let root = std::env::var_os("SystemRoot")
                .map(std::path::PathBuf::from)
                .ok_or_else(|| {
                    ApiError::Provider("Windows system directory is unavailable".into())
                })?;
            let json = registered_games(&root, context)?;
            parse_report(&json, &root.join("explorer.exe"), context)
        }
        #[cfg(not(target_os = "windows"))]
        {
            Ok(DiscoveryReport {
                games: vec![],
                complete: false,
                warnings: vec![],
            })
        }
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(serde::Deserialize)]
struct RegisteredGame {
    title: String,
    family: String,
    application: String,
    directory: std::path::PathBuf,
}

#[cfg(any(target_os = "windows", test))]
#[derive(serde::Deserialize)]
struct RegisteredReport {
    games: Vec<RegisteredGame>,
    warnings: Vec<String>,
}

#[cfg(any(target_os = "windows", test))]
fn parse_report(
    json: &[u8],
    explorer: &std::path::Path,
    context: &JobContext,
) -> Result<DiscoveryReport, ApiError> {
    context.checkpoint()?;
    let registered: RegisteredReport = serde_json::from_slice(json).map_err(|_| {
        ApiError::Provider("Windows returned an unreadable Xbox registration report".into())
    })?;
    let mut report = DiscoveryReport {
        games: vec![],
        complete: registered.warnings.is_empty(),
        warnings: registered.warnings,
    };
    let mut seen = std::collections::BTreeSet::new();
    for record in registered.games {
        context.checkpoint()?;
        let valid_id = |value: &str| {
            !value.is_empty()
                && value.len() <= 255
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        };
        if !valid_id(&record.family)
            || !valid_id(&record.application)
            || record.title.trim().is_empty()
            || record.title.len() > 512
            || record.title.chars().any(char::is_control)
            || record
                .title
                .to_ascii_lowercase()
                .starts_with("ms-resource:")
            || !record.directory.is_absolute()
            || !record.directory.is_dir()
            || !explorer.is_absolute()
            || !explorer.is_file()
        {
            report.complete = false;
            report.warnings.push(
                "An Xbox game registration was invalid or inaccessible; cached games were retained"
                    .into(),
            );
            continue;
        }
        let app_id = format!("{}!{}", record.family, record.application);
        if !seen.insert(app_id.clone()) {
            continue;
        }
        report.games.push(startpoint_sdk::DiscoveredGame {
            source_game_id: app_id.clone(),
            title: record.title.trim().into(),
            install_directory: record.directory,
            launch_target: startpoint_sdk::LaunchTarget::Executable(
                startpoint_sdk::ExecutableLaunch {
                    executable: explorer.to_path_buf(),
                    arguments: vec![format!("shell:AppsFolder\\{app_id}")],
                    working_directory: None,
                },
            ),
            local_cover: None,
        });
    }
    Ok(report)
}

#[cfg(target_os = "windows")]
const REGISTERED_GAMES: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$games = [System.Collections.Generic.List[object]]::new()
$warnings = [System.Collections.Generic.List[string]]::new()
foreach ($package in @(Get-AppxPackage -PackageTypeFilter Main)) {
    try {
        if ([string]::IsNullOrWhiteSpace($package.InstallLocation)) { continue }
        $configPath = Join-Path $package.InstallLocation 'MicrosoftGame.config'
        if (!(Test-Path -LiteralPath $configPath -PathType Leaf)) { continue }
        [xml]$config = Get-Content -LiteralPath $configPath -Raw
        $manifest = Get-AppxPackageManifest -Package $package.PackageFullName
        $executables = @($config.Game.ExecutableList.Executable | Where-Object { $_.IsDevOnly -ne 'true' -and $_.Id })
        $matched = $false
        foreach ($application in @($manifest.Package.Applications.Application)) {
            if (!($executables | Where-Object { $_.Id -eq $application.Id })) { continue }
            $matched = $true
            $title = [string]$config.Game.ShellVisuals.DefaultDisplayName
            if ([string]::IsNullOrWhiteSpace($title) -or $title.StartsWith('ms-resource:', [StringComparison]::OrdinalIgnoreCase)) {
                $title = [string]$application.VisualElements.DisplayName
            }
            if ([string]::IsNullOrWhiteSpace($title) -or $title.StartsWith('ms-resource:', [StringComparison]::OrdinalIgnoreCase)) {
                $title = [string]$package.Name
            }
            $games.Add([pscustomobject]@{
                title = $title; family = [string]$package.PackageFamilyName
                application = [string]$application.Id; directory = [string]$package.InstallLocation
            })
        }
        if (!$matched) { $warnings.Add('An Xbox game has no matching registered launch target; cached games were retained') }
    } catch { $warnings.Add('An Xbox game registration could not be read; cached games were retained') }
}
[pscustomobject]@{ games = @($games.ToArray()); warnings = @($warnings.ToArray()) } | ConvertTo-Json -Depth 5 -Compress
"#;

#[cfg(target_os = "windows")]
fn registered_games(root: &std::path::Path, context: &JobContext) -> Result<Vec<u8>, ApiError> {
    use std::{
        io::Read,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let mut child = Command::new(root.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            REGISTERED_GAMES,
        ])
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| {
            ApiError::Provider("Windows Xbox registration service could not be started".into())
        })?;
    let stdout = child.stdout.take().unwrap();
    // Drain concurrently: a large installation list must not block on a full pipe.
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(8 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let started = Instant::now();
    let outcome = loop {
        if let Err(error) = context.checkpoint() {
            break Err(error);
        }
        if started.elapsed() > Duration::from_secs(30) {
            break Err(ApiError::Provider(
                "Reading Xbox registrations timed out; cached games were retained".into(),
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(_)) => {
                break Err(ApiError::Provider(
                    "Windows could not enumerate Xbox games; cached games were retained".into(),
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(error) => break Err(ApiError::Io(error.to_string())),
        }
    };
    if outcome.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let bytes = reader
        .join()
        .map_err(|_| ApiError::Provider("Xbox registration reader stopped".into()))?
        .map_err(|error| ApiError::Io(error.to_string()))?;
    outcome?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use startpoint_sdk::LaunchTarget;
    use std::sync::{Arc, atomic::AtomicBool};

    #[test]
    fn xbox_registration_launches_the_exact_app_without_shell_interpolation_or_executable_guessing()
    {
        let directory = tempfile::tempdir().unwrap();
        let explorer = directory.path().join("explorer.exe");
        std::fs::write(&explorer, []).unwrap();
        let context = JobContext::new(Arc::new(AtomicBool::new(false)), |_| {});
        let good = serde_json::json!({"title": "Example Xbox Game", "family": "Example.Game_12345", "application": "Game", "directory": directory.path()});
        let json =
            serde_json::to_vec(&serde_json::json!({"games": [good.clone(), good], "warnings": []}))
                .unwrap();
        let report = parse_report(&json, &explorer, &context).unwrap();
        assert!(report.complete);
        assert_eq!(report.games.len(), 1);
        assert_eq!(report.games[0].source_game_id, "Example.Game_12345!Game");
        let LaunchTarget::Executable(launch) = &report.games[0].launch_target else {
            panic!("registered shell activation expected")
        };
        assert_eq!(launch.executable, explorer);
        assert_eq!(
            launch.arguments,
            ["shell:AppsFolder\\Example.Game_12345!Game"]
        );
    }

    #[test]
    fn partial_invalid_or_cancelled_xbox_scans_never_uninstall_cached_games() {
        let directory = tempfile::tempdir().unwrap();
        let explorer = directory.path().join("explorer.exe");
        std::fs::write(&explorer, []).unwrap();
        let cancellation = Arc::new(AtomicBool::new(false));
        let context = JobContext::new(cancellation.clone(), |_| {});
        let bad = serde_json::json!({"title": "Example", "family": "bad; start calc", "application": "Game", "directory": directory.path()});
        let json =
            serde_json::to_vec(&serde_json::json!({"games": [bad], "warnings": []})).unwrap();
        let report = parse_report(&json, &explorer, &context).unwrap();
        assert!(!report.complete && report.games.is_empty());
        assert!(parse_report(b"not json", &explorer, &context).is_err());
        cancellation.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(matches!(
            parse_report(&json, &explorer, &context),
            Err(ApiError::Cancelled)
        ));
    }
}
