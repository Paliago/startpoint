# Startpoint

Native Rust/GPUI game launcher. Windows first; macOS supported.
Steam, Epic, GOG and manual games; editable metadata, artwork, themes and local activity/journals.

## Run

Rust 1.88+. Windows requires MSVC, Visual Studio Build Tools (**Desktop development with C++**) and the Windows SDK.

```sh
cargo run --locked -- --example-library
cargo run --locked
```

The example library has isolated data and simulated launches. Use `--diagnostics` for discovery details.
SteamGridDB artwork is optional; API keys stay in the OS credential store.

On macOS without the Metal compiler, add `--features runtime-shaders` to Cargo commands.

## Windows executable

**Actions → Windows test executable → Run workflow**: choose a branch. For branches without the workflow, run from `main` and fill in `branch`.
Download and extract the run's artifact for `startpoint.exe` (unsigned; retained seven days).

## Check

```sh
cargo test --locked --workspace
```

Code: `crates/sdk` (API), `crates/core` (backend), `crates/desktop` (GPUI).

## Launcher icon

`crates/desktop/assets/icons/startpoint.svg` is the editable logo source; `.png` is a transparent 512px export, and `.ico` includes nine sizes from 16–256px.
Windows builds embed the icon in `startpoint.exe` for Explorer, shortcuts, Start menu pins and the taskbar. No separate icon file is needed beside the executable.

After editing the SVG, regenerate the exports before building:

```sh
cargo run --locked --release -p startpoint --example generate_icon
```

## Crash diagnostics

Session logs are written to the application's data directory under `diagnostics` (Windows: `%LOCALAPPDATA%\Startpoint\data\diagnostics`). The latest ten sessions are retained, with routine logs limited to 2 MiB each. They record action labels, job states, GPUI warning/error locations and Rust panic backtraces. Arbitrary error/panic messages and command contents are omitted to avoid recording credentials or personal library data. Paths may still appear in stack traces. Logs are local only.

After a crash, collect logs and Windows crash reports without launching the app:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/Collect-Diagnostics.ps1 -Executable target/release/startpoint.exe
```

The script creates a dated folder and ZIP in your temporary directory; review before sharing. Keep the matching `.exe` and `.pdb` privately before rebuilding. A panic hook cannot capture every native crash or forced termination. To opt into Windows minidumps, run `scripts/Enable-CrashDumps.ps1` in an **administrator PowerShell**; it affects only `startpoint.exe`, keeps up to three minidumps and never uploads them. Pass `-Disable` to remove its settings. Dumps can contain secrets from memory and are **excluded** from collected bundles unless you explicitly pass `-IncludeDumps`.
