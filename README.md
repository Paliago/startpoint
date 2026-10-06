# Startpoint

A native Rust / GPUI launcher for games already installed on your computer. Windows is the primary target; macOS is supported where practical.

## Run

Install Rust 1.88 or newer. On Windows, use the MSVC toolchain and install Visual Studio Build Tools with the **Desktop development with C++** workload and Windows SDK.

```sh
cargo run --release
```

macOS normally requires Xcode's Metal compiler. If that component is unavailable, compile shaders at runtime for development:

```sh
cargo run --features runtime-shaders
```

Headless discovery diagnostics:

```sh
cargo run --features runtime-shaders -- --diagnostics
```

Separate 15-game example library for interface checks (launches are simulated; your installed library is unchanged):

```sh
cargo run --features runtime-shaders -- --example-library
```

The example library uses its own data/cache directories and the existing OS-stored SteamGridDB key. Example favourites, metadata edits, artwork selections and simulated launch history persist separately.

## Use

- Installed **Steam, Epic Games and GOG** games are discovered from local records. Use **+** to add another executable or a macOS application bundle, including older installations without supported records.
- **Cmd/Ctrl F** or **/** opens game search; **Cmd/Ctrl K** opens actions. Use **↑ / ↓** and **Enter** in the command bar; selecting a search result opens that game in Library. **Escape** or clicking outside closes the bar and restores the previous input focus without changing editor drafts.
- Outside the command bar, **↑ / ↓** or **← / →** selects games and **Enter** starts the selected game from its overview. **Escape** and the navbar Back button return from Settings or game editing; **Cmd/Ctrl ,** opens Settings. The launch shortcut is disabled while editing, searching or viewing notifications.
- **Cmd/Ctrl B** or the navbar sidebar button toggles library sources on any screen. Drag the sidebar edge to resize it from 192–400 pixels; its width and visibility are saved. The resize handle also supports **Left / Right** and **Home / End** when focused. Sidebar motion follows the OS reduced-motion setting on Windows and macOS.
- The selected game's **…** button opens metadata, artwork, launch configuration and its journal. In **Metadata**, add collection names (one per line), mark completion and save progress. Collections appear in the sidebar as filters. Background metadata filling updates untouched editor fields without overwriting an unsaved draft. Arguments are individual values, one per line, without shell quoting or expansion.
- Settings offers **Library / Recents** as the saved start screen. Recents shows edge-to-edge artwork behind the floating window controls and the Recently played section, with small readable surfaces behind the title, Start button and controls. **Fade Recents background** adds a smooth fade through the title area into Recently played, becoming solid behind the cards and continuing to the bottom of the window. A translucent title surface keeps the text readable. This preference is saved and off by default. Up to five games are shown, with recently launched games first and other installed games filling the remaining slots, so autoplay still works after your first launch. Its carousel advances every eight seconds as the visible timer drains, using a 240 ms artwork crossfade after the next image has loaded. Pause/Resume explicitly controls playback; hovering over titles, controls or cards does not stop it. Keyboard focus within the carousel, an open panel, or an inactive window temporarily pauses the timer. Resume releases carousel focus so playback actually continues. Reduced-motion users start with autoplay paused and skip transitions even if resumed. Overflowing titles scroll gently back and forth in the selected-game panel and recent cards, with a full-title tooltip; reduced motion keeps titles stationary. Manual and keyboard selection stay immediate and reset the timer. Navbar tabs switch views without changing your startup preference. Settings is centered within the available window width.
- The **palette button on Recents** opens in-place customization: theme, System / Light / Dark mode, dithering, background fade and optional widgets. Appearance and widget changes are saved immediately without leaving Recents. **Activity calendar**, **Collection progress** and **Game journal** widgets are all off by default; enabled widgets sit below the recent cards and the page scrolls when needed. The carousel pauses while customization is open.
- Settings offers built-in **Startpoint, Forest, Ocean and Violet** themes and Library artwork dithering. **Edit theme…** opens a live visual editor over Recents with separate light/dark variants, color pickers, font family, font size and corner radius. **Save theme** writes and selects a custom theme; **Cancel** restores the saved appearance. Built-in themes cannot be overwritten, and another custom theme cannot be overwritten accidentally. Text contrast is displayed; primary-button text is chosen automatically for contrast. Every built-in theme includes contrast-checked light/dark palettes. The ordered texture is a tiled, 12%-opacity overlay matching `awsesh`, not a processed copy of the artwork. Originals and game-card artwork are unchanged. Windows and macOS use opaque windows; macOS keeps native traffic lights. Windows appearance still needs runtime verification.
- The **Activity** tab shows a monthly launch calendar with previous/next navigation and per-day games, manually marked collection progress, and recent journal notes. The bell remains background jobs and notifications. Launch history begins with this version: existing last-launch timestamps are not expanded into invented events, failed launches are not counted, and timestamps are grouped by local calendar date. These are launch counts, not playtime. Example launches are simulated and recorded only in the example library. Journals support adding, editing and confirmed deletion of local notes. Journal drafts survive refreshes and failed saves.
- An optional personal [SteamGridDB API key](https://www.steamgriddb.com/profile/preferences) enables community artwork. Settings distinguishes saved, checking, verified, rejected, approval-required and temporarily unavailable keys; an existing key does not need to be entered again. The key stays in Windows Credential Manager or macOS Keychain, never in the library database. The cached library loads before credential checks. Startup and automatic artwork requests never display a Keychain authorization prompt. If macOS denies silent access, choose **Unlock saved key** in Settings to authorize access explicitly. Unsigned development rebuilds can change the application's Keychain identity; a consistently signed distribution avoids that rebuild-specific approval. Windows reads the current user's Credential Manager without a macOS-style password dialog.
- With a key supplied, background refresh fills missing covers, high-resolution banners, logos and icons from popular SteamGridDB choices. Banners prefer 3840-pixel artwork when available. Descriptions, developers and genres for Steam-linked games come from Steam's public catalogue; SteamGridDB itself only supplies titles and artwork. Your local edits and selected images take precedence.
- Selecting a SteamGridDB game match is saved immediately and reused when switching image categories or reopening the app. Uploaded images and saved artwork work offline.
- Launch timestamps represent launches from Startpoint, not a store's complete play history. **All games** includes saved games whose installations are no longer available, not an online store catalogue.

Discovery, network requests, image processing and persistence run on workers. The top-bar bell opens background jobs and notifications, reports progress and supports cooperative cancellation. There is no bottom status bar. Partial or failed scans retain previously saved games and user edits.

## Local store discovery

- **Epic Games:** reads `.item` JSON manifests from `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Manifests` on Windows or `~/Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests` on macOS. Settings can select a different manifest directory. Incomplete downloads, non-launchable add-ons, Unreal Engine installations and plugins are excluded. Games launch through the allow-listed Epic launcher URI with their catalogue identifiers; Epic handles any authentication the game needs.
- **GOG on Windows:** reads the current-user and local-machine installer records in both registry views, including `SOFTWARE\GOG.com\Games` and GOG uninstall records. Default `GOG Galaxy\Games` folders under Program Files are also checked. Local `goggame-*.info` files identify games and their primary launch tasks, including offline-installer games without Galaxy. Non-standalone DLC is excluded. Executable arguments retain native Windows quoting and working directories; no shell is invoked.
- **GOG on macOS:** checks `/Applications` and `~/Applications` for identifiable native bundles. GOG manifests are read at the bundle root, `Contents/Resources` or `Contents/Resources/game`; the containing `.app` launches through LaunchServices. Folder-based tasks also support native executables and `.app` targets. This is not Windows-game emulation.
- **Custom GOG locations:** choose a games directory in Settings. The selected folder and its immediate game folders are checked, along with native bundle resources. This replaces automatic GOG detection. There is no recursive whole-drive search; for deeper layouts, select a closer folder or add the executable manually. Refresh installed games after changing any discovery directory. Directory preferences persist with the active library.

Both providers operate offline without storing store credentials, querying owned-game catalogues, downloading games or modifying another launcher's records. GOG launches use the local primary task rather than Galaxy's cloud-sync/statistics features. Records are size-limited and validated; tasks cannot escape their installation directory. A malformed or unreadable record produces a partial scan: valid games are imported, but missing games are not marked unavailable. A successful complete scan can mark absent games unavailable without deleting their metadata, launch overrides, artwork, collections, completion or journal. The example library continues to use only its simulated provider, not installed-store discovery.

## Custom themes

Use **Settings → Open themes directory**, add a `.json` file, then choose **Reload** and select its name. Theme files load on a worker at startup and on explicit reload; theme selection is saved independently of light/dark mode. Invalid files show a warning in Settings without preventing startup. A missing selected theme falls back to Startpoint without discarding the saved selection.

Themes use GPUI Component's `ThemeSet` format. Each file needs a unique `name` and one `light` and one `dark` variant. Omitted properties and color tokens inherit the matching Startpoint defaults. Colors are `#RRGGBB` or `#RRGGBBAA`; window and popover backgrounds and foregrounds must be opaque. Optional `font.family`, `font.size` (10–32 px), `radius` and `radius.lg` (0–48 px) configure component styling.

For example, save this as `custom.json`:

```json
{
  "name": "Custom",
  "themes": [
    {
      "mode": "light",
      "colors": {
        "primary.background": "#245fa8",
        "primary.hover.background": "#1e518e",
        "primary.active.background": "#194476",
        "primary.foreground": "#ffffff",
        "accent.background": "#dce9fb",
        "accent.foreground": "#194476",
        "ring": "#245fa8",
        "caret": "#245fa8"
      }
    },
    {
      "mode": "dark",
      "colors": {
        "primary.background": "#8bbaff",
        "primary.hover.background": "#a1c8ff",
        "primary.active.background": "#74a9ee",
        "primary.foreground": "#17202b",
        "accent.background": "#263a54",
        "accent.foreground": "#8bbaff",
        "ring": "#8bbaff",
        "caret": "#8bbaff"
      }
    }
  ]
}
```

The complete default palette is centralized in `crates/desktop/assets/themes/startpoint.json`; `crates/desktop/src/theme.rs` handles validation and inheritance. Components, status indicators, artwork fades and image placeholders use those semantic tokens. The OS-native font remains the default unless a theme specifies its own. No scripts, remote theme downloads or file watcher are needed.

Themes live under the active library's data directory: `%LOCALAPPDATA%\Startpoint\data\themes` on Windows and `~/Library/Application Support/Startpoint/themes` on macOS. Example-library themes remain isolated under `examples/themes`.

## Architecture

| Crate | Responsibility |
| --- | --- |
| `crates/sdk` (`startpoint-sdk`) | UI-independent game models, commands, events and provider contracts |
| `crates/core` (`startpoint-core`) | SQLite persistence, platform launching, credentials, Steam/Epic/GOG discovery and SteamGridDB artwork |
| `crates/desktop` (`startpoint`) | GPUI window, appearance, virtualized game rows, forms and bounded image cache |

The desktop uses `ApplicationApi::{snapshot, submit, subscribe, cancel}`. Compiled extensions can consume the same API and register `DiscoveryProvider` or `ArtworkProvider` integrations through `LauncherBuilder`. Provider identifiers must be unique; URI schemes and image hosts are explicitly allow-listed.

`API_VERSION` versions this application contract. It is **not** a stable Rust dynamic-library ABI. `SteamProvider`, `EpicProvider` and `GogProvider` register through `LauncherBuilder`; dynamic plugin loading remains deferred.

SQLite and selected artwork live in the OS application-data directory. Schema version 2 adds indexed `launch_events` and `journal_entries` tables in a transaction without replacing game documents. Collections and completion are backward-compatible fields in game documents. Removing a manual game also removes its associated launch events and notes. Selected artwork retains up to 3840 × 2160 pixels. Disposable thumbnails live in the OS cache directory and are regenerated when missing. Dithering and fades are desktop overlays; older cached derivatives are no longer used. Settings exposes both directories. On macOS these are `~/Library/Application Support/Startpoint` and `~/Library/Caches/Startpoint`; example data uses an `examples` subdirectory in each.

## Verify

```sh
cargo fmt --all -- --check
cargo check --workspace --features runtime-shaders
cargo test --workspace --features runtime-shaders
cargo clippy --workspace --features runtime-shaders --all-targets -- -D warnings
```

The macOS development build uses `runtime-shaders`. Existing tests exercise local Epic/GOG records, primary launch arguments, native macOS bundle discovery, invalid/oversized records, cancellation, partial-scan safety and preservation of user data. The SDK and the standalone Epic/GOG discovery module have passed `x86_64-pc-windows-msvc` checks, including Windows registry and native argument-parsing code. Cross-checking the complete core from macOS is blocked by missing Windows C headers (`assert.h` in `ring`); a full desktop build and actual store-launch verification still need the Windows SDK and MSVC tools on Windows.

## Implementation references

The desktop implementation in `/Users/alvinjohansson/code/elva/tui/awsesh/packages/desktop` is the design reference for the control capsule, 200 ms interruptible sidebar transition, resize limits and debounce, virtualized command bar, refresh icon, and tiled ordered-dither overlay. Startpoint reuses those small visual assets without importing the application architecture. [Zeron](https://github.com/zeronsh/zeron) informed the earlier native appearance work. Window transparency remains disabled.
