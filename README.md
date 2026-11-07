# Startpoint

A native Rust game launcher built with **Rune, Rune Kit and Rune FX**. Steam, Epic, GOG, Xbox PC and manual games share one library, editable metadata and artwork, launch configuration, activity history, Markdown journals and independent per-game tasks.

Rune is the only frontend. The former GPUI application and its dependencies have been removed; `startpoint` is the executable on every platform.

## Run

Rust 1.89+ (tested with stable 1.99). Windows requires MSVC, Visual Studio Build Tools (**Desktop development with C++**) and the Windows SDK.

```sh
# Isolated 45-game example library with simulated launches
cargo run --locked --release -- --example-library

# Installed-game discovery and real launch routes
cargo run --locked --release
```

Windows uses wgpu's DX12 backend; macOS uses Rune's native Metal backend. No GPUI shader compiler or `runtime-shaders` feature is needed. Linux enables Vulkan/GLES but is not a packaged release target.

### Existing libraries

Existing Rune data is preserved: when a `rune-preview/library.sqlite3` database exists under Startpoint's data directory, the application continues using it and its matching cache. Example mode checks `rune-preview/examples/` separately. Otherwise Startpoint uses its standard data/cache directories, or their `examples/` subdirectories. Existing canonical libraries are not overwritten or combined with a Rune library. Settings displays the active paths.

Artwork selections, metadata, preferences, journal entries and tasks remain in the existing database. Schema 4 adds independent tasks and protects them from older executables. Back up the database before using an older build; older executables reject newer schemas.

SteamGridDB artwork is optional; its API key stays in the OS credential store. Example launches are simulated and cannot launch manually added or real-provider games. Headless scenes use temporary example databases without credentials, network artwork providers or real launches.

## Library and artwork

- **Classic Recents:** selected landscape artwork, direct Play/favourite actions and a rotation at the bottom of the first screen. Optional activity, collection-progress and journal widgets begin below it.
- **Infinite Wall:** virtual portrait rows with dwell hover, lift/scale, surrounding dimming, Play controls and matched artwork expansion below the top toolbar.
- **Library:** continuous cover, landscape, pill and compact-table layouts; title/recent/source sorting; source, favourite, completion, collection and hidden-game filters; an independently closable overview.
- **Activity:** recorded launches, a calendar heatmap, ranked games and date-filtered history. No playtime or save-game integration is inferred.

Search opens the native command panel rather than filtering Recents. Use **Cmd/Ctrl+F/P/K**. Escape clears a nonempty query, then closes the panel on the next press. Back/forward navigation restores the library context. **Cmd/Ctrl+B** toggles the sidebar.

Right-click a game for launch, favourite, editing, artwork, launch settings, journal, completion, hide/unhide and directory actions. The editor has Details, Artwork and Launch sections. Journal is a separate modal with dated/author-attributed Markdown entries, formatting shortcuts, preview and an independent To-dos tab.

**Refresh metadata and missing artwork for all games** preserves manual edits and selected artwork. Identified Steam, GOG and Epic games use their public catalogues without additional account credentials; SteamGridDB fills missing artwork when a key is configured. Saved artwork supports local upload, category/history selection, server pagination and game-name or direct URL/#ID identification.

### Launch through Steam

Open the game editor → **Launch → Launch via**. Choose a saved **Steam non-Steam shortcut**, a numeric **Steam launch URL**, an executable/application bundle, or restore the launcher default. `.url` import only changes the draft until saved. Startpoint reads Steam's saved shortcut/account IDs without modifying Steam's files; it never silently falls back to an executable when a linked shortcut disappears.

## Developer tools

```sh
cargo run --locked --release -- --example-library --developer-tools --scene component-effects
```

The **Component effects** page is available in debug builds, tooling scenes or sessions explicitly started with `--developer-tools`. Open it from **Settings → Developer tools** or search **Developer: Component effects**. Controls, Surfaces and Data samples exercise real Rune components with GPU materials, pause/reduced motion, disabled states, contrast protection and session-local configuration. See [desktop architecture](docs/desktop.md) and [native tooling](docs/automation.md).

For terminal-only discovery information:

```sh
cargo run --locked -- --example-library --diagnostics
cargo run --locked -- --example-library --diagnostics --refresh-metadata
```

## Check

```sh
cargo fmt --all -- --check
cargo test --locked --workspace --features self-update
cargo build --locked --release --features self-update
python3 -m unittest discover -s scripts -p 'test_release.py' -v
```

The application lives in `crates/desktop`; `crates/sdk` defines the API and `crates/core` provides discovery, persistence, artwork and launch services. The default workspace member is the Rune desktop.

## Install and release

Download Windows x64 or Apple Silicon macOS packages from [GitHub Releases](https://github.com/Paliago/startpoint/releases). Extract the entire portable package, not just its executable. Intel Macs are not supported by the packaged release channel.

The existing installer identity and update feeds are retained. Packaged `self-update` builds check shortly after launch; download and restart actions live in Settings and Activity & Notifications. Development, developer-tool, diagnostic and example sessions do not perform automatic update checks. Plain standalone executables require a packaged installation to update.

**Actions → Windows test executable** builds a selected branch. **Actions → Release** with a blank tag creates a package preview without publishing. Publishing requires a matching tested version tag and validates both platforms before making one release public. Main pushes run checks and Windows tests, not automatic release publication. See [release procedures](docs/releases.md) for signing, packaging and updater testing.

Rune uses a **blue published-release icon** (`startpoint-rune.*`) and an **amber local-development icon** (`startpoint-rune-dev.*`). Local builds—including plain `cargo build --release`—default to **Startpoint Rune Dev**, with a separate Windows taskbar identity and automatic updates disabled. GitHub release packaging explicitly builds with `--features self-update,production-branding` to select the blue icon and existing installed-app/update identity. Debug builds and the compatibility `dev-icon` flag always select amber, even if `production-branding` is also requested. Storage paths do not depend on branding.

The editable SVG sources are in `crates/desktop/assets/icons/`; regenerate PNG/ICO exports with `cargo run --locked --example generate_icon`. The older green/violet icon assets remain unchanged for historical compatibility, but the Rune application and its packages use the new blue/amber pair. Windows embeds the matching ICO in the executable and sets the window icons explicitly; macOS uses the corresponding PNG at runtime and the blue icon in packaged bundles.
