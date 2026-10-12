# Windows distribution and updates

Windows x64 releases use Velopack **1.2.161**, pinned in both Cargo and the release workflow. The .NET SDK is needed only to run the packaging CLI; users do not need .NET.

## One-time setup

The default destination is the public source repository, **`Paliago/startpoint`**. No extra release repository or token is needed for this setup. If you later want to keep source private, create an **initialized public releases-only repository**, for example `Paliago/startpoint-releases`. It need not contain source code.

In the source repository's **Settings → Secrets and variables → Actions**:

- Variable `STARTPOINT_RELEASE_REPOSITORY`: `owner/repository` for that public destination. If absent, the workflow uses the source repository, which must then be public.
- Secret `STARTPOINT_RELEASE_TOKEN`: a fine-grained token with **Contents: write** on the destination. Not needed when publishing into the same public source repository. The token is used only in CI, never included in the executable.

The destination is compiled into the app. Keep it stable across releases; moving it requires leaving an update pointing existing installations to a build configured for the new destination. Keep the app ID **`Paliago.Startpoint.App`** stable too. Its default install directory is `%LOCALAPPDATA%\Paliago.Startpoint.App`, separate from `%LOCALAPPDATA%\Startpoint` library/settings/cache data. **Do not override installation or portable extraction into the data directory.**

### Code signing

Unsigned test builds work but can produce SmartScreen/antivirus warnings. Configure signing before broad public distribution. For Azure Artifact Signing, set:

- Variable `STARTPOINT_SIGNING_METADATA_JSON`: JSON with `Endpoint`, `CodeSigningAccountName`, `CertificateProfileName` (see [Velopack signing](https://docs.velopack.io/packaging/signing)).
- Secrets `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` for an identity authorized to sign with that certificate profile.

Velopack signs the app, updater and installer during packaging. No signing credentials ship in the app. SHA-256 package verification detects corruption; it does **not** independently authenticate a compromised release repository/feed. Protect release credentials and repository access.

## Publish a release

1. Set `[workspace.package].version` in `Cargo.toml` to the new stable version and update `Cargo.lock` (`cargo check --features self-update`). Commit the version change together with the intended release changes.
2. Create and push the matching tag, for example **`v0.1.0`**. The **Windows release** workflow tests, builds and packages it, uploads build artifacts, then publishes a GitHub Release in the configured destination.
3. For a dry run, use **Actions → Windows release → Run workflow**, select an existing tag and leave **publish** unchecked. The public destination must still be configured so the packaged app has a usable update URL.

The tag must exactly match the Cargo version; prereleases/build suffixes are intentionally not supported yet. Packages are uploaded into a draft before publishing so update checks cannot see an incomplete release. Existing releases are never overwritten, and publishing refuses to move the stable feed backwards. If publishing fails, inspect the draft and either finish publishing its verified assets or remove only that unpublished draft before rerunning. Never replace assets of a version already delivered to users.

Users download the generated `*-Setup.exe` or portable ZIP from the destination's Releases page. Extract the **entire** portable ZIP into a writable folder and launch its entry point; copying out `startpoint.exe` alone loses the updater layout. Existing standalone test executables do not gain self-update support: install/extract a packaged release once. The original **Windows test executable** workflow still produces standalone binaries; local PowerShell helpers are no longer included in its artifacts.

## In-app behavior

- Release packages include the `self-update` feature. Normal `cargo run`, debug builds, example-library runs and non-Windows builds do not check for updates.
- Two seconds after window creation, a dedicated worker checks the latest stable release feed. No network calls or downloads run on the UI thread or library/artwork executor. Automatic failures are quiet and shown only in Settings.
- A toolbar badge and **Settings → Startpoint** offer **Download update**, then **Restart to update**. Downloads use full packages, a version-tag-pinned URL, size/SHA-256 verification and a 15-minute request timeout. Checks time out after 10 seconds. Partial/failed downloads do not replace the installed app and can be retried.
- Restart is disabled while application jobs are active, an editor is open, or an appearance save is pending. Restart goes through GPUI's graceful quit handlers, preserving window placement. Games are not deliberately terminated by Startpoint's restart.
- A downloaded update can also be applied at the next normal launch, using Velopack's local startup hook. This hook performs local package housekeeping; **an update-applying launch can take longer**, but startup never waits for an online check. A failed apply leaves a pending update actionable offline.
- No update daemon, periodic polling, forced download, restart of an already-running app without user action, telemetry upload, or administrator requirement is added. GitHub will receive normal update HTTP requests. Updates cannot downgrade versions.

The minimal Velopack startup hook runs before database/diagnostics initialization and must not be moved onto a worker: installer hooks must exit before opening the application. Benchmark packaged launch time, including this local hook, before making a zero-overhead startup claim.

## Local packaging

Install .NET SDK 8+ and `dotnet tool install --global vpk --version 1.2.161`. In PowerShell:

```powershell
$env:STARTPOINT_RELEASE_REPOSITORY = 'Paliago/startpoint'
cargo build --locked --release --features self-update
New-Item -ItemType Directory -Path target/package
Copy-Item target/release/startpoint.exe target/package/startpoint.exe
vpk pack --packId Paliago.Startpoint.App --packTitle Startpoint --packVersion 0.1.0 --packDir target/package --mainExe startpoint.exe --runtime win-x64 --channel win-x64-stable --icon crates/desktop/assets/icons/startpoint.ico --outputDir target/distribution/0.1.0
```

Use the Cargo version, not an arbitrary packaging version. Before `vpk pack`, create `target/package` and copy **only** `target/release/startpoint.exe` into it; do not package the whole build directory. Pass `--azureTrustedSignFile` for an Azure signing metadata file. The workflow does staging and package hash/size verification automatically. Crash symbols are not included in release packages. On a public source repository, Actions artifacts may also be publicly downloadable; use external private storage if your symbols must be confidential.

PowerShell helper files (`*.ps1`) are local-only and ignored by Git. The release workflow does not depend on them.

## Validation before first public release

```powershell
cargo test --locked --workspace
cargo test --locked --workspace --features self-update
cargo check --locked --release --features self-update
```

Use a disposable public release repository and test Windows account/VM for end-to-end acceptance:

1. Package/install A, publish newer B, launch A: the window must appear without waiting for the check. Compare first-window times online/offline, with and without a pending package.
2. Download B from the toolbar and Settings, keep using the library, restart: verify B's version, existing library, credentials, themes and window placement.
3. Download then quit without clicking restart: next normal launch should apply B locally, also offline.
4. Disconnect midway through download, corrupt a test feed hash, or run two download attempts from separate app instances: the current installation must remain usable and retry must work.
5. Try restart with an active job or editor; try with another installed instance open. Editors/jobs must not be silently discarded; verify Velopack's multi-instance behavior in the VM.
6. Uninstall A/B and confirm `%LOCALAPPDATA%\Startpoint` is untouched. Test the portable layout separately.

macOS packaging/signing and preview channels are not part of this initial Windows release pipeline.
