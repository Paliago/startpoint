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

## Library views

**Cover grid** is the default. Use the palette button in the Library toolbar and its **Layout** dropdown to choose **Pills**, **Cover grid**, **Landscape tiles** or **Compact list**. Size/spacing, source labels, last-played dates, favourite buttons and the right-hand detail panel are configurable and saved automatically. Previously saved layout choices are kept.

Double-click a game to launch it, or right-click for play, metadata/collections, artwork, launch settings, journal, re-identification, favourite, completion, hide/unhide and folder actions. Manually added games can also be removed with confirmation. Hover over grid artwork for Play/Manage buttons and an empty favourite star; favourited stars are filled and always visible unless disabled in customization. The detail pane is overview-only and has a close button; selecting a game reopens it when enabled. Metadata, images, launch settings, journal and re-identification always open in the centered game-editor modal, whether the detail pane is enabled, disabled or closed. The overview remains behind the modal, and editors opened from Recents or Activity stay on that screen. Close editors with × or Escape.

Compact rows include developer/genre metadata, collections, completion and Startpoint launch counts, alongside availability and optional source/date information. Source labels and launch dates are independent: games with no recorded launch date leave that field empty. Switching layout or density returns the library to its heading and filters.

Hidden games are available from the sidebar only. Favourites and Manually added filters appear when populated; the first favourite reveals its filter and removing the last collapses it with a short animation (disabled with reduced motion).

Window size, location and maximized state are saved locally in `window-state.json` in the application's data directory. If a monitor is removed, the next launch moves the window back onto an available display. Example-library placement stays with its isolated data.

The detail overview uses a game's logo instead of its text title when available. It prefers **Background** (SteamGridDB hero) artwork for clean scenery. If there is a logo but no background, it uses a neutral surface rather than a cover with another baked-in title; **Choose background…** opens that artwork category. Background files can still contain text—choose a text-free image in the Images editor. Cover art is a fallback only when no separate logo is available.

Logos are centered using their visible artwork rather than transparent file padding, and their display height adapts to fit the available width. Normalized copies are prepared off the UI thread in the logo cache; originals and artwork history are unchanged. Extremely tall logos are height-limited to keep the description and launch controls visible. Favourite and game options sit beside Start game, with last-launched information beneath the action row.

## Check

```sh
cargo test --locked --workspace
```

Code: `crates/sdk` (API), `crates/core` (backend), `crates/desktop` (GPUI).

Shared desktop presentation lives in `crates/desktop/src/components.rs`: flat buttons, icon buttons, segmented choices, checked dropdowns, labeled fields, floating panels, section labels, widget surfaces and image fallbacks. Views keep their own state/action wiring. `detail_artwork.rs` centralizes the overview's clean-background and logo/title selection.

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
