# Releases: main → tested tag → one GitHub Release

The **Release** workflow builds Windows x64 and macOS Apple Silicon from the **same commit**. Intel Macs are not supported. Velopack **1.2.161** packages installers, portable apps and full update packages. Python 3.12 and .NET 8 are CI tools only; end users need neither.

## Flow

- **Push/merge to `main`:** version/provenance checks → two parallel native builds and tests → packaging/validation → one combined Actions artifact. Nothing is published. These are installation previews, not a separate update feed.
- **Push `vX.Y.Z`:** require a stable version matching `Cargo.toml` and `Cargo.lock`, with its commit reachable from `main`. Build all platforms, assemble and validate their hashes/source/signing state, create a draft, upload everything, then publish once as the latest release. If any platform fails, **no partial public release** appears.
- **Manual run:** Actions → Release → Run workflow on `main`. Leave `tag` blank for a preview; enter an existing tag and select `publish` to retry publication. `allow_unsigned` is an explicit testing opt-in. Publishing without a tag is rejected.
- **Unsigned tags:** an annotated tag with `[unsigned-test]` on its own message line allows an explicitly labelled testing release. Otherwise publication requires signing on every platform. Main previews permit unsigned packages; incomplete signing configurations still fail rather than silently falling back.

The concise **1, 2, 3** release guide is in [README.md](../README.md#make-a-release--1-2-3). For `0.1.0`, the workspace version is already correct. To download a preview, open its successful Actions run and extract `startpoint-v0.1.0-all-platforms`; installers are inside.

## One-time GitHub setup

Default destination: the public **Paliago/startpoint** repository. The workflow's built-in token publishes there; no extra publishing credential is necessary. Enable GitHub Actions. Branch protections should require the release preview jobs before merging release candidates. Protect `v*` tags from deletion/force-push, allowing only release maintainers to create them.

Optional separate public release destination:

- Actions variable `STARTPOINT_RELEASE_REPOSITORY`: initialized public `owner/repository`.
- Actions secret `STARTPOINT_RELEASE_TOKEN`: fine-grained token with **Contents: write** on that repository.

Keep the destination stable: it is compiled into installed apps. Publishing tokens are never embedded. Signing/publishing secrets must not be made available to untrusted code; the release workflow deliberately does not run on pull requests.

## Production signing

Configure these under **Settings → Secrets and variables → Actions** before pushing a normal production tag. `0.1.0` can use the explicit unsigned-testing annotation instead.

**No developer accounts for testing:** leave the signing secrets/variables unset. Main previews work without them. Publish `0.1.0` with the README's `[unsigned-test]` annotated tag; the release will be clearly labelled as untrusted testing software. The Mac app is ad-hoc signed locally with identity `-`, requiring no Apple account or certificate; the `.pkg` is unsigned and neither is notarized. Prefer the app ZIP in `~/Applications` for testing and explicitly approve macOS security warnings if allowed. Ad-hoc signing verifies bundle integrity, not who published it. Developer ID signing/notarization is optional future work for trusted public distribution and requires Apple Developer Program credentials.

### Windows: Azure Trusted Signing

Actions variable:

- `STARTPOINT_SIGNING_METADATA_JSON`: Azure metadata JSON containing `Endpoint`, `CodeSigningAccountName`, `CertificateProfileName` (and optional `CorrelationId`).

Actions secrets:

- `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`: service principal authorized to sign with that profile.

Velopack signs the executable, updater and installer. Signing does not guarantee immediate SmartScreen reputation.

### macOS: Developer ID + notarization

Export the **Developer ID Application** and **Developer ID Installer** certificates with their private keys as separate password-protected `.p12` files, using the same export password. Store base64-encoded contents in secrets:

- `MACOS_APP_CERTIFICATE_BASE64`
- `MACOS_INSTALLER_CERTIFICATE_BASE64`
- `MACOS_CERTIFICATE_PASSWORD`
- `APPLE_ID`, `APPLE_APP_PASSWORD` (Apple app-specific password), `APPLE_TEAM_ID`

Actions variables:

- `MACOS_APP_SIGNING_IDENTITY`: full identity, e.g. `Developer ID Application: Your Name (TEAMID)`.
- `MACOS_INSTALLER_SIGNING_IDENTITY`: `Developer ID Installer: Your Name (TEAMID)`.

The build imports certificates into an ephemeral keychain, packages/signs the app and `.pkg`, notarizes both, staples tickets and verifies signatures/notarization. Keychain/certificate files are cleaned up and never uploaded. The native Rust app uses no JIT/App Sandbox entitlement; macOS builds target **14.0+**. Unsigned test builds use ad-hoc app signatures, **not** Developer ID trust or notarization.

## Install and test 0.1.0

1. Download your architecture's installer from the `v0.1.0` release (or the combined preview artifact).
   - **Windows:** run `Paliago.Startpoint.App-win-x64-stable-Setup.exe`. The default installation is per-user, separate from `%LOCALAPPDATA%\Startpoint` library data. For portable use, extract the whole ZIP into a writable folder and run its top-level launcher.
   - **Apple Silicon Mac:** use `osx-arm64-stable`. Run the `.pkg`, or extract its portable ZIP and move the **entire `Startpoint.app`** into a writable `~/Applications` folder before opening it. Do not run directly from the Downloads preview or inside an archive. Installation into system `/Applications` can require authorization for installation/updates.
2. For an **unsigned testing release**, review the download source/checksums before approving OS warnings. Windows may offer **More info → Run anyway**. On macOS, attempt to open the installer/app, then use **System Settings → Privacy & Security → Open Anyway** if offered. macOS policies may still block unsigned software; use a signed/notarized release in managed environments. Do not disable Gatekeeper globally.
3. Open Settings and verify **0.1.0**. Launch normally to exercise the real library, or use `--example-library` for isolated sample data (automatic updates are disabled in example mode). Check the library/settings survive a close/reopen.
4. Publish **0.1.1** using the same README steps. Keep the installed `0.1.0` open/reopen it normally, let the deferred check run (or use **Check for updates**), choose **Download update**, then **Restart to update**. In newer builds, update notifications and their actions appear under the toolbar bell (also available in Settings), rather than as a separate navbar button. Verify the new version and preserved library/settings. An app at `0.1.0` cannot test an upgrade until a newer release exists.
5. Test offline startup, interrupted-download retry, and restart with an editor/background task active. The app prevents update restart while edits/work are active. Windows uninstall should remove the installation without removing the separate library data; macOS app removal should likewise leave user data alone.

Do not overwrite versioned update packages. `SHA256SUMS` and `release-manifest.json` record every platform's assets, hashes, signing state and exact source commit. Actions artifacts/symbols in this public repository are not confidential.

## Update behavior

- Windows x64: `win-x64-stable`; Apple Silicon: `osx-arm64-stable`.
- The app requests `https://github.com/{repository}/releases/latest/download/releases.{channel}.json` after the window opens (two-second delay), on a dedicated thread. Downloaded full packages are pinned to `/releases/download/v{version}/{filename}` and checked against SHA-256/size.
- Network checks/downloads never block the UI/library/artwork executor. Velopack's pre-UI startup hook does local housekeeping and can apply a previously downloaded update; zero startup overhead has not been benchmarked.
- Downloads require user action. Restart uses GPUI shutdown so existing save/flush hooks run. A downloaded update may also apply on the next ordinary launch. No running app is forcibly restarted; no update daemon, periodic polling or telemetry is added.
- Partial/corrupt downloads do not become pending updates. A verified pending update remains actionable offline; older releases cannot downgrade an installation. Full packages do not depend on delta chains or older assets.

## Failures and recovery

- **A build fails:** fix on `main`, preview again, then tag a new version if the old tag was already shared. Never force-move a release tag. Before any release is public, an existing tag's unchanged build can be retried from Actions.
- **Uploading fails:** the release stays a draft. The publisher can resume only if existing draft assets exactly match the rebuilt hashes and source marker. Rebuilt installers/signatures may not be byte-identical. Inspect the unpublished draft; download/recover its matching artifacts or remove only that draft's incomplete assets/draft before retrying. Do not delete or replace public release assets.
- **Already public version:** publication refuses to overwrite it. Bump the version (including `Cargo.lock`) and publish a newer release.
- **One platform must be withdrawn:** do not publish a partial latest release missing an update channel. Fix it and publish a complete newer version for all platforms.

## Local packaging

On the target OS, install Python 3.11+, Rust/MSVC or Xcode tools, and .NET SDK; install the pinned CLI:

```sh
dotnet tool install vpk --version 1.2.161 --tool-path .local-vpk
cargo test --locked --workspace --features self-update
cargo build --locked --release --features self-update
python scripts/release.py package --platform win-x64 --version 0.1.0 --commit <40-character-source-commit> --policy testing --vpk .local-vpk/vpk --output target/distribution/0.1.0-win-x64
```

Use `osx-arm64` on an Apple Silicon Mac instead. Signing uses the same environment variables as CI. Output must be empty; only application files are staged. Python scripts are tracked; local PowerShell helpers (`*.ps1`) stay ignored and are not required by CI. The original Windows standalone test workflow is still available but its binaries do not acquire packaged self-update support.
