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

- Steam installations are discovered automatically. Use **+** to add another executable or a macOS application bundle.
- Find games with **Cmd/Ctrl F** or **/**. Use **↑ / ↓** or **← / →** to select and **Enter** to start from the artwork overview. **Escape** returns to that overview; **Cmd/Ctrl ,** opens Settings. The launch shortcut is disabled while editing game details or viewing notifications.
- The selected game's **…** button opens metadata, artwork and launch configuration. Arguments are individual values, one per line, without shell quoting or expansion.
- Settings offers **Library / Recents** as the saved start screen. Recents shows a large artwork banner and up to five recently launched games, with ready-to-play games as the initial fallback. Search-bar tabs switch views without changing your startup preference.
- Settings offers **System / Light / Dark** appearance and separate artwork-dithering controls for Library and Recents. Recents dithering is opt-in and off by default; its fine stippled fade clears the title area without pixelating the image or cards. Windows and macOS currently use opaque windows; macOS keeps native traffic lights. Windows appearance still needs runtime verification.
- An optional personal [SteamGridDB API key](https://www.steamgriddb.com/profile/preferences) enables community artwork. Settings shows whether the saved key is verified, checking, rejected or temporarily unavailable; an existing key does not need to be entered again. The key stays in Windows Credential Manager or macOS Keychain, never in the library database.
- With a key supplied, background refresh fills missing covers, high-resolution banners, logos and icons from popular SteamGridDB choices. Banners prefer 3840-pixel artwork when available. Descriptions, developers and genres for Steam-linked games come from Steam's public catalogue; SteamGridDB itself only supplies titles and artwork. Your local edits and selected images take precedence.
- Selecting a SteamGridDB game match is saved immediately and reused when switching image categories or reopening the app. Uploaded images and saved artwork work offline.
- Launch timestamps represent launches from Startpoint, not a store's complete play history. **All games** includes saved games whose installations are no longer available, not an online store catalogue.

Discovery, network requests, image processing and persistence run on workers. The top-bar bell opens Activity and notifications, reports progress and supports cooperative cancellation. There is no bottom status bar. Partial or failed scans retain previously saved games and user edits.

## Architecture

| Crate | Responsibility |
| --- | --- |
| `crates/sdk` (`startpoint-sdk`) | UI-independent game models, commands, events and provider contracts |
| `crates/core` (`startpoint-core`) | SQLite persistence, platform launching, credentials, Steam discovery and SteamGridDB artwork |
| `crates/desktop` (`startpoint`) | GPUI window, appearance, virtualized game rows, forms and bounded image cache |

The desktop uses `ApplicationApi::{snapshot, submit, subscribe, cancel}`. Compiled extensions can consume the same API and register `DiscoveryProvider` or `ArtworkProvider` integrations through `LauncherBuilder`. Provider identifiers must be unique; URI schemes and image hosts are explicitly allow-listed.

`API_VERSION` versions this application contract. It is **not** a stable Rust dynamic-library ABI. Dynamic plugin loading, Epic and GOG discovery are not implemented.

SQLite and selected artwork live in the OS application-data directory. Selected artwork retains up to 3840 × 2160 pixels. Thumbnails and derived dither/fade images live in the OS cache directory and are regenerated when missing. Settings exposes both directories. On macOS these are `~/Library/Application Support/Startpoint` and `~/Library/Caches/Startpoint`; example data uses an `examples` subdirectory in each.

## Verify

```sh
cargo fmt --all -- --check
cargo check --workspace --features runtime-shaders
cargo test -p startpoint-sdk -p startpoint-core
cargo clippy --workspace --features runtime-shaders -- -D warnings
```

The macOS development build uses `runtime-shaders`. The SDK has passed an `x86_64-pc-windows-msvc` check. Cross-checking the core from macOS is blocked by missing Windows C headers (`assert.h` in `ring`); a full desktop build and runtime verification still need to be performed with the Windows SDK and MSVC tools on Windows.

## Implementation references

[Zeron](https://github.com/zeronsh/zeron) informed the native title bar, system appearance handling and cached ordered-dither treatment. Window transparency is currently disabled. Startpoint keeps its own smaller launcher implementation rather than importing Zeron's application architecture.
