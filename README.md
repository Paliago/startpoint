# Startpoint

Native Rust/GPUI game launcher. Windows first; macOS supported.
Steam, Epic, GOG and manual games; editable metadata, artwork, themes and local activity/journals.

## Run

Rust 1.89+ (tested with stable 1.99). Windows requires MSVC, Visual Studio Build Tools (**Desktop development with C++**) and the Windows SDK.

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

## Install

Download from [GitHub Releases](https://github.com/Paliago/startpoint/releases): **Windows x64** `*-Setup.exe` or portable ZIP; **macOS 14+ on Apple Silicon (arm64)** `.pkg` or app ZIP. Intel Macs are not supported. Extract the entire portable package, not just its executable.

Packaged apps check for updates in the background after launch; use **Download update → Restart to update** in the bell's **Activity & Notifications** panel or Settings. The bell shows a notification dot for available, downloading and ready updates; update actions do not add a separate navbar button. Development/example builds and standalone test executables do not automatically check. Unsigned testing releases show OS security warnings; see [installation and update testing](docs/releases.md#install-and-test-010).

## Make a release — 1, 2, 3

1. **Prepare `main`.** Set `[workspace.package].version` in `Cargo.toml` (already `0.1.0` for the first release), run `cargo check --workspace --features self-update` to update `Cargo.lock`, then commit/merge and push to `main`. Wait for **Actions → Release** to pass on that exact commit. It produces installable Windows + Apple Silicon Mac previews without publishing.
2. **Tag that tested commit.** Normal, signed release: `git tag -a v0.1.0 -m "Startpoint 0.1.0"`, then `git push origin v0.1.0`. For the first **unsigned testing release**, use `git tag -a v0.1.0 -m "Startpoint 0.1.0" -m "[unsigned-test]"` instead, then push the tag. Use the matching new version on subsequent releases. Never move/reuse a published tag.
3. **Install the published release.** Wait for the tag's **Release** run: both platforms must pass before **one GitHub Release** becomes public. Download your installer from [Releases](https://github.com/Paliago/startpoint/releases). To exercise the updater, install `0.1.0`, then release `0.1.1` and use **Download update → Restart to update** without reinstalling.

Production signing setup, preview downloads, failure recovery and local packaging: [docs/releases.md](docs/releases.md). The standalone Windows test-executable workflow remains available.

## Library views

**Cover grid** is the default. Use the palette button in the Library toolbar and its **Layout** dropdown to choose **Pills**, **Cover grid**, **Landscape tiles** or **Compact list**. Size/spacing, source labels, last-played dates, favourite buttons and the right-hand detail panel are configurable and saved automatically. Previously saved layout choices are kept.

Double-click a game to launch it, or right-click for play, metadata/collections, artwork, launch settings, journal, re-identification, favourite, completion, hide/unhide and folder actions. Manually added games can also be removed with confirmation. Hover over grid artwork for Play/Manage buttons and an empty favourite star; favourited stars are filled and always visible unless disabled in customization. The detail pane is overview-only and has a close button; selecting a game reopens it when enabled. Metadata, images, launch settings, journal and re-identification always open in the centered game-editor modal, whether the detail pane is enabled, disabled or closed. The overview remains behind the modal, and editors opened from Recents or Activity stay on that screen. Close editors with × or Escape.

Compact rows include developer/genre metadata, collections, completion and Startpoint launch counts, alongside availability and optional source/date information. Source labels and launch dates are independent: games with no recorded launch date leave that field empty. Switching layout or density returns the library to its heading and filters.

Hidden games are available from the sidebar only. Favourites and Manually added filters appear when populated; the first favourite reveals its filter and removing the last collapses it with a short animation (disabled with reduced motion).

Window size, location and maximized state are saved locally in `window-state.json` in the application's data directory. If a monitor is removed, the next launch moves the window back onto an available display. Example-library placement stays with its isolated data.

The detail overview uses a game's logo instead of its text title when available. It prefers **Background** (SteamGridDB hero) artwork for clean scenery. If there is a logo but no background, it uses a neutral surface rather than a cover with another baked-in title; **Choose background…** opens that artwork category. Background files can still contain text—choose a text-free image in the Images editor. Cover art is a fallback only when no separate logo is available.

Logos are centered using their visible artwork rather than transparent file padding, and their display height adapts to fit the available width. Normalized copies are prepared off the UI thread in the logo cache; originals and artwork history are unchanged. Extremely tall logos are height-limited to keep the description and launch controls visible. Favourite and game options sit beside Start game, with last-launched information beneath the action row.

## Smoked glass

The left, middle and right **toolbar islands** share the same glass surface, theme rounding, 36px height, equal 2px padding on all four sides and 2px control gaps. Screen tabs and toolbar icons use the same neutral, high-contrast hover/pressed/selected fills; the selected screen no longer uses a different accent-button style. Resting controls share the island's background, and labels/icons remain sharp. Windows caption controls match these highlights and rounding, except Close retains its red warning hover. Regular buttons and opened dropdown menus retain their opaque fills.

Resting action buttons, icon buttons and dropdown triggers use a stronger theme-derived fill: darker in light mode and lighter in dark mode, targeting at least 1.8:1 separation from the shell/panel tint while retaining 4.5:1 label contrast. Hover/pressed emphasis is capped to keep labels readable. Buttons remain flat and borderless; toolbar controls still share their containing surface.

Default buttons and navbar containers use the same live theme corner radius (`radius.lg`, 12px in Startpoint). This includes ordinary actions, dropdown triggers, toolbar controls and Windows caption controls; custom-theme edits update them together.

The app uses shared, theme-aware smoked-glass panels and **flat, borderless controls**. Floating navigation, the **Ctrl+K / Cmd+K** command palette, the metadata/images/launch/journal modal, customization panels, notifications and Recents widgets share 18px backdrop blur, 85% tint, pale hairlines, soft shadows and faint inset highlights. Buttons have solid fills, no gradients or shadows, compact theme-scaled corners, and distinct hover/pressed states. Sidebar navigation uses plain rows with no resting fill, highlighting only hovered/selected items. Inputs are darker and recessed without outer focus rings. Regular buttons, dropdown menus and fields stay opaque while containing panels are glass. Top navigation shares its island's resting background and highlights hovered/selected tabs separately. Keyboard-focus indicators on buttons are retained, and modal dimming covers the entire app.

**Customize Library** and **Customize Recents** contain theme selection/editing, a **Colour mode** dropdown (System/Light/Dark), separate saved **Translucency** and **App background blur** switches, and a **Translucency** amount slider. Library artwork dithering lives only in Customize Library. **App background blur** controls only the desktop behind the native app window: disabling it uses a sharp transparent backdrop instead of Acrylic/vibrancy. Modals, customization panels, notifications, the command palette and other glass panels always retain their local backdrop blur. The app-background blur control is available when native window translucency is enabled. Disabling translucency makes the shell/panels opaque without losing the saved amount or native-blur preference. Unsupported native-translucency platforms retain an opaque base. Regular buttons, menus, fields, text and icons remain opaque.

The amount slider has a visible **20% default notch**, calibrated to preserve the previous theme-aware glass appearance. Pointer adjustment snaps within two percentage points of it, and **Default** restores it. Click the slider or Tab to it, then use **Left/Right** or **Up/Down** for 1% steps, **Shift+arrow** for 5% steps, and **Home/End** for the endpoints. Keyboard adjustment can always leave the notch. Changes preview immediately and are saved after a short pause or on pointer release. Recents reuses its composited artwork texture while adjusting the amount, rather than decoding artwork on every slider movement. Window dragging and native caption actions are unchanged.

Recents composites artwork, ordered dithering and its settled fade off-thread, then applies shell translucency once to the result. Even its solid-colour footer stays translucent; images on disk are never modified. The three-backdrop cache retains visible/crossfading/preloaded frames and evicts only at frame boundaries. Right-click the Recents backdrop or a game card for the same game actions as Library.

**Edit theme…** previews colour and corner-radius changes live on shared controls and glass panels. **Border** affects field edges and panel hairlines; buttons intentionally have no borders. **Rename theme…** focuses the custom theme's name in the editor. Saving renames its existing file's theme without leaving an old duplicate; built-ins are always copied. Sidebar labels and icons align to the left. The component comparison page has been removed.

The desktop pins matching revisions of [Zeron's ZUI renderer](https://github.com/zeronsh/zui) and [component fork](https://github.com/zeronsh/gpui-component) for per-panel blur on Windows Direct3D and macOS Metal. Shared smoke styling and paint-layer ordering live in `crates/desktop/src/glass.rs`; the existing frame-safe artwork cache remains in use.

## Settings and UI vocabulary

Settings contains **Startup**, **SteamGridDB API key**, **Game discovery**, **Local storage**, and **Updates**, in that order. Preferred start screen is a dropdown; appearance belongs in customization. The key acquisition action is a text link; verification/unlocking and removal are in the key's Options menu. Discovery and storage use path rows with directory actions beside their paths. Automatic discovery shows the resolved Steam root, Epic manifest search directory, and GOG search/known installer locations; resolving them happens on the runtime worker. Recents always uses visible installed games in recent-launch order, without a Library filter dropdown or changes to the saved Library filters.

Settings and the Activity overview have no active game selection. The action command bar omits launch, hide, favourite, journal and editing commands there; it still offers global navigation and settings actions. Returning to a game page restores the previous selection when it is still available in that view. An explicitly opened game editor has its own selected-game context. Game search can still open a result in Library from any page, and game commands are revalidated on activation to reject stale targets.

Use these names when discussing the UI:

| Name | Meaning / shared code |
| --- | --- |
| **Screen tabs** | Recents / Library / Activity; `navigation_tab` |
| **Toolbar island** | Shared glass container and geometry for all three bars; `toolbar_island` |
| **Toolbar control** | Common neutral interaction styling for screen tabs and toolbar icons; `toolbar_control` |
| **Icon button** | Square action target; `toolbar_icon_button` for 32×32 toolbar icons, `icon_button` for ordinary actions |
| **Action button** | Flat text action with symmetric 12px side padding; `button` |
| **Choice group** | Related options with an accent-selected choice; `choice_group` / `choice_button` |
| **Choice dropdown** | Opaque trigger plus check-marked menu choices; `choice_dropdown` |
| **Path row** | Label/path at left, directory actions at right; `path_row` |
| **Glass panel** | Containing smoked-glass surface; `glass_panel` |
| **Customize panel** | Floating appearance and screen-layout controls; `floating_panel` |

## Check

```sh
cargo test --locked --workspace
```

Code: `crates/sdk` (API), `crates/core` (backend), `crates/desktop` (GPUI).

Shared desktop presentation lives in `crates/desktop/src/components.rs`: smoke button tokens, borderless buttons, icon buttons, navigation choices, dropdowns, recessed inputs/textareas, glass panels, section labels, widget surfaces and image fallbacks. `glass.rs` owns theme-derived materials, global effect policy and ordered blur painting. Views keep their own state/action wiring. `detail_artwork.rs` centralizes the overview's clean-background and logo/title selection.

## Launcher icon

`crates/desktop/assets/icons/startpoint.svg` is the editable logo source; `.png` is a transparent 512px export, and `.ico` includes nine sizes from 16–256px.
Windows builds embed the icon in `startpoint.exe` for Explorer, shortcuts, Start menu pins and the taskbar. No separate icon file is needed beside the executable.

After editing the SVG, regenerate the exports before building:

```sh
cargo run --locked --release -p startpoint --example generate_icon
```

## Crash diagnostics

Session logs are written to the application's data directory under `diagnostics` (Windows: `%LOCALAPPDATA%\Startpoint\data\diagnostics`). The latest ten sessions are retained, with routine logs limited to 2 MiB each. They record action labels, job states, GPUI warning/error locations and Rust panic backtraces. Arbitrary error/panic messages and command contents are omitted to avoid recording credentials or personal library data. Paths may still appear in stack traces. Logs are local only.

After a crash, collect the session logs and Windows Event Viewer crash report without launching the app. Review before sharing, and keep the matching `.exe` and `.pdb` privately before rebuilding. A panic hook cannot capture every native crash or forced termination. Windows minidumps can contain secrets from memory and should not be shared without review.

PowerShell diagnostic/packaging helpers are local-only: `*.ps1` is ignored by Git and these files are not distributed through GitHub or build artifacts.
