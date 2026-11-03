# Startpoint

Native Rust/GPUI game launcher. Windows first; macOS supported.
Steam, Epic, GOG, Xbox PC and manual games; editable metadata, artwork, themes and local activity/journals.

## Run

Rust 1.89+ (tested with stable 1.99). Windows requires MSVC, Visual Studio Build Tools (**Desktop development with C++**) and the Windows SDK.

```sh
cargo run --locked -- --example-library
cargo run --locked
```

The example library has isolated data and simulated launches. Use `--diagnostics` for discovery details.
Debug builds use a violet **Startpoint Dev** icon and separate taskbar identity. For an optimized local dev build, use `cargo build --locked --release --features dev-icon` (executable: `target/release/startpoint.exe`). Regular release builds without `dev-icon` retain the green production icon. Branding does not change library/data locations. Regenerate both icon exports with `cargo run --locked -p startpoint --example generate_icon`.
SteamGridDB artwork is optional; API keys stay in the OS credential store.

During metadata/artwork refresh, Steam games use their app ID for catalogue text. Identified GOG games use GOG's public product catalogue; Epic games use Epic's public catalogue, checking the installed game's namespace where available. These require no additional account credentials. GOG responses are verified against the installed product ID, so storefront renames (such as Witcher 3 GOTY to Remastered) do not discard valid text; choosing an unrelated identity still prevents reuse of the installed game's metadata. Other identified games, or missing native listings, can fall back to Steam with an unambiguous title match. Known edition labels such as **The Final Cut** are handled without treating sequels, expansions or soundtracks as the same game. Lookups are cached for a day; older negative Steam title caches do not prevent native-store repair. Manual metadata overrides remain untouched; re-identification clears only the previous identity's automatic text before refilling it.

The image browser has **Previous page / Next page** controls above and below its results, loading up to 24 SteamGridDB images per server page. Pages and previews are cached separately for each game match and image type; changing either resets to page one. A page containing only filtered-out images can still advance when SteamGridDB reports more results. Game-name search is not server-paginated by SteamGridDB: returned matches are shown in batches with **Load more games** when applicable. To identify a game outside those suggestions, paste its `https://www.steamgriddb.com/game/<ID>` URL or `#<ID>` into the same search field, then select the returned match.

### Launch a GOG/Epic/manual game through Steam

Open the game's editor → **Launch** → **Launch via**. Choose **Steam non-Steam shortcut**, select the saved shortcut (labelled with its Steam account ID), then **Save launch configuration**. Startpoint reads `userdata/<account ID>/config/shortcuts.vdf` on a worker, without changing Steam's files. **Refresh Steam shortcuts** reloads the list; restart Steam first if a newly added shortcut has not been written yet. The Steam directory override in Settings applies to this picker too.

Alternatively choose **Steam launch URL**, paste `steam://rungameid/…`, or import the `.url` desktop shortcut Steam created. Import only fills the draft; save it to apply. Only numeric Steam game-launch URLs are accepted—not installation, uninstallation or other Steam commands. Steam uses its own launch options and Steam Input configuration. Sign in to the appropriate Steam account first; Startpoint does not switch accounts (on Windows it blocks a known account mismatch for a linked shortcut).

This changes only the launch route: the original store, metadata, artwork, notes and history stay on the same game. Linked shortcuts use Steam's saved app ID, not a title-derived guess. A removed/changed shortcut produces a relink error and never silently falls back to an executable. **Restore launcher default** removes the Steam route and executable override; **Executable / application bundle** retains direct-launch support. Existing libraries keep their original launch behaviour until you save a different route.

On macOS without the Metal compiler, add `--features runtime-shaders` to Cargo commands.

## Windows executable

**Actions → Windows test executable → Run workflow**: choose a branch. For branches without the workflow, run from `main` and fill in `branch`.
Download and extract the run's artifact for `startpoint.exe` (unsigned; retained seven days).

## Install

Download from [GitHub Releases](https://github.com/Paliago/startpoint/releases): **Windows x64** `*-Setup.exe` or portable ZIP; **macOS 14+ on Apple Silicon (arm64)** `.pkg` or app ZIP. Intel Macs are not supported. Extract the entire portable package, not just its executable.

Packaged apps check for updates in the background after launch; use **Download update → Restart to update** in the bell's **Activity & Notifications** panel or Settings. The bell shows a notification dot for available, downloading and ready updates; update actions do not add a separate navbar button. Development/example builds and standalone test executables do not automatically check. Unsigned testing releases show OS security warnings; see [installation and update testing](docs/releases.md#install-and-test-010).

## Make a release — 1, 2, 3

1. **Prepare `main`.** Set `[workspace.package].version` in `Cargo.toml`, run `cargo check --workspace --features self-update` to update `Cargo.lock`, then commit/merge and push to `main`. Wait for **Actions → CI** on that exact commit. Main pushes run formatting, release-tooling tests and Windows workspace/updater tests only—no release builds, installers or artifact uploads. For a full Windows + Apple Silicon Mac preview, manually run **Actions → Release → Run workflow** on `main` with `tag` blank and `publish` unchecked.
2. **Tag that tested commit.** Normal, signed release: `git tag -a v0.1.0 -m "Startpoint 0.1.0"`, then `git push origin v0.1.0`. For the first **unsigned testing release**, use `git tag -a v0.1.0 -m "Startpoint 0.1.0" -m "[unsigned-test]"` instead, then push the tag. Use the matching new version on subsequent releases. Never move/reuse a published tag.
3. **Install the published release.** Wait for the tag's **Release** run: both platforms must pass before **one GitHub Release** becomes public. Download your installer from [Releases](https://github.com/Paliago/startpoint/releases). To exercise the updater, install `0.1.0`, then release `0.1.1` and use **Download update → Restart to update** without reinstalling.

Production signing setup, preview downloads, failure recovery and local packaging: [docs/releases.md](docs/releases.md). The standalone Windows test-executable workflow remains available.

## Library views

### Gallery

Choose **Settings → Interface style → Gallery**, or **Customize → Interface style**. Gallery is an artwork-first exhibition: oversized portrait covers, a larger centered selection, progressively receding neighbors, a near-black canvas and serif typography. The scene is rendered by **wgpu**: a perspective camera sees upright artwork planes above a world-space textured floor with rough reflections, artwork-coloured illumination and contact shading. Camera projection also determines the native cover hit targets. Only a handful of neighboring covers are rendered, even with a large installed collection.

The GPU renderer starts lazily on a worker and stops rendering when the scene settles. Its eight-layer mipmapped artwork cache and frame resolution are bounded; artwork decoding, GPU waits and readback never block GPUI's UI thread. The initial portable integration uses **offscreen GPU rendering plus a bounded CPU frame-copy bridge into GPUI**, not zero-copy shared textures. Native toolbar, text, controls, menus and accessibility remain GPUI. The previous 2D Gallery is retained as the loading/unsupported-device fallback. See [Gallery rendering](docs/gallery-rendering.md) for the implementation boundaries and native GPU validation command.

**Settings → Performance → Pause visual effects in background** defaults on and freezes artwork motion and automatic browsing when unfocused. On Windows and macOS, minimized windows and another foreground fullscreen app always pause effects, even with this preference off. Gallery cancels queued frames and sleeps, retaining a still image; settled scenes already submit no GPU frames.

### Cinematic

Choose **Settings → Interface style → Cinematic**, or change the style from **Customize**. The selected game's landscape artwork fills the window, with a gentle lower gradient, a lower-left title and **Continue / Play**, and a compact floating filmstrip. Its home includes **every visible installed game across connected libraries**, not a five-game shortlist; Left/Right, wheel/trackpad scrolling, cover clicks and Search reach the whole collection. The selected cover lifts into an artwork-coloured highlight. **Continue** uses the game's normal launch route—it does not promise save-game integration.

Scenery and accent lighting crossfade together through cached native GPU textures. There is no wgpu readback bridge, continuous animation, description panel, sidebar or platform badge on the cinematic home. **Every design uses the same three-island top bar**, including Cinematic: the left navigation/search/Settings island, the Recents/Library/Activity island, and the right actions/native-controls island. Missing landscape art uses a subdued, softened cover fallback. Reduced motion and background/fullscreen suspension apply. See [Cinematic rendering](docs/cinematic-rendering.md).

### Infinite Wall, Film Reel and Art Book

The [saved plans and original image references](docs/design-plans/README.md) describe three additional interface styles. **Infinite Wall** virtualizes the installed collection as a consistently darkened portrait wall with 200 ms hover lifts, gentle scroll settling and reversible expansion into landscape artwork, game information and Play. **Film Reel** glides between full-height landscapes over 500 ms, transitions ambient colour over 800 ms and expands the selected scene before dispatching Play. A slow Balatro-inspired marbled field moves behind the reel; title and Play float over the artwork, without empty top/bottom bands. **Art Book** derives its paper, ink and toolbar-island palette from the artwork, centers the crop inside its left-hand art region and animates a genuinely changing feathered tear. It retains a 650 ms page reveal, an extending Play underline and a shrinking-composition overview. Both living materials update at 10 Hz only while visible and active; paper grain never moves.

All three use the **same three floating toolbar islands over full-bleed content**, without a full-width backing or a reserved empty header strip. Existing launch routes, installed games from all providers, bounded artwork caches and pause-aware native GPU compositing are retained. Reduced motion removes spatial/ambient motion. They are selectable from **Settings → Interface style** or **Customize**.

Click a neighboring cover to select it; use **Left/Right**, the mouse wheel or horizontal trackpad scrolling to browse. **Play**, **Enter**, or double-click launches through the game's existing store/override route. Transitions can reverse or be interrupted without jumping, wrap at both ends, and respect the OS reduced-motion setting. Gallery never advances automatically. Its home exhibition contains the **five latest visible installed games**, with one fixed dot per game. The dots always represent the same five positions, not a sliding window. Library retains the complete collection and shows an honest **position / total** indicator instead of dots.

Gallery uses the **same three floating toolbar islands as Classic and Editorial**, retaining its dark palette: utility controls, Recents/Library/Activity tabs, and actions/window controls. There is no alternate corner-only header or full-width toolbar fill. **Ctrl/Cmd+F** or **/** opens game search; **Ctrl/Cmd+P/K** opens actions, including **Show hidden games**. Right-click artwork for game management and source/launch details in the editor. There are no sidebars, detail panels, widgets, platform badges or per-cover captions in the exhibition. Windows caption controls and macOS traffic-light clearance remain native. Gallery's dark palette applies to menus, dialogs, search and Settings too; switching designs retains the previous theme, sidebar, layout and widget preferences.

On Windows, Xbox discovery reads registered **GDK games with `MicrosoftGame.config`**, matching their application IDs to the Windows package manifest. Games launch through Windows' registered AppsFolder activation rather than a guessed executable. Enumeration is bounded and cancellable; unreadable/partial registrations retain cached games. Older UWP games without that configuration are not automatically detected; use **Add a game** for unsupported installations. Steam, Epic and GOG continue to use their existing discovery services. No store account credentials are requested.

### Editorial Recents

Choose **Settings → Interface style → Editorial**, or use **Customize → Interface style** from Recents or Library. **Classic** remains the default. Editorial shows six most recently launched visible installed games; Classic's carousel keeps five. Both are independent of Library filters. Editorial uses an opaque warm-ivory canvas, asymmetric landscape artwork with native organic curves, quiet serif captions and a circular Play button on hovered/selected artwork.

Editorial applies across **Recents, Library, Activity, Settings, editors, command search and customization** with warm paper surfaces, ink-coloured controls and editorial headings. The top navigation uses the same three floating toolbar islands as Classic: sidebar/back/search/settings on the left, Recents/Library/Activity tabs beside them, and customization/refresh/add/notifications plus Windows caption controls on the right. Editorial keeps its warm colours rather than adopting Classic's dark palette. **Startpoint** sits below the toolbar as the scrolling Recents heading, not in a full-width navigation strip. **Ctrl/Cmd+P** (or **Ctrl/Cmd+K**) opens the action command center. Click artwork to select, double-click to launch, or use Play. Right-click artwork or open its caption's options button for game management; hover artwork for the full title and source. Arrow keys select and reveal games; Enter launches.

Only the individual Editorial islands have translucent, softly blurred surfaces; the spaces between them are entirely clear, with no full-width tint. Pages and dialogs stay opaque paper, independently of desktop translucency. The sidebar button or Ctrl/Cmd+B works on Recents as well as Library. Artwork reflows into the remaining width, retaining its scroll position; selecting a sidebar source/filter opens that Library view rather than silently changing an invisible filter. Both designs share the same native title-bar geometry: macOS keeps its traffic lights at the top left and reserves 82px before app controls outside fullscreen; fullscreen reclaims that clearance. Windows caption controls remain at the right. Native dragging and caption behavior are unchanged.

Contours move continuously on a dedicated UI tick without requiring hover, including behind menus and dialogs. Hover adds only a little extra movement. Captions and button hitboxes stay fixed; masks use the exact opaque page colour, without rectangular artwork shadows or caption fades. Reduced motion disables the ambient animation. Switching back to Classic restores the saved colour theme, mode, translucency, sidebar and carousel preferences; choosing Editorial never overwrites them.

Visual designs are resolved centrally in `crates/desktop/src/design.rs`. Profiles declare their shell, material, palette, typography, Recents presentation/count, sidebar policy and ambient-motion cadence. Shared appearance, panel and shell renderers consume these roles; pages do not inspect theme names. The existing `recents_layout` setting remains readable for compatibility, but its choice now resolves an app-wide profile. Future designs add a descriptor and, only where needed, a new shared shell/material strategy rather than page-by-page conditionals.

**Cover grid** is the default. Use the palette button in the Library toolbar and its **Layout** dropdown to choose **Pills**, **Cover grid**, **Landscape tiles** or **Compact list**. Size/spacing, source labels, last-played dates, favourite buttons and the right-hand detail panel are configurable and saved automatically. Previously saved layout choices are kept.

Double-click a game to launch it, or right-click for play, metadata/collections, artwork, launch settings, journal, re-identification, favourite, completion, hide/unhide and folder actions. Manually added games can also be removed with confirmation. Hover over grid artwork for Play/Manage buttons and an empty favourite star; favourited stars are filled and always visible unless disabled in customization. The detail pane is overview-only and has a close button; selecting a game reopens it when enabled. Metadata, images, launch settings, journal and re-identification always open in the centered game-editor modal, whether the detail pane is enabled, disabled or closed. The overview remains behind the modal, and editors opened from Recents or Activity stay on that screen. Close editors with × or Escape.

The editor uses a game-identity rail with **Details**, **Artwork**, **Launch**, and **Journal** sections. Playback and favourite actions stay in the overview instead of competing with editing. **Re-identify game…** is available in Details and Artwork. Details opens on the main fields; **Library organization** expands to collections, completion, hide/unhide and manual-game removal. There is no separate sidebar options menu. Launch opens on the current launcher, with executable, arguments and directory fields behind **Customize launch…**; saved overrides expand automatically.

Editors, widgets and Customize panels omit instructional subtitles and reassurance captions. Labels, metadata, concise empty states, operational feedback, errors and destructive-action warnings remain visible.

Artwork opens on **Your images**. Select an image tile to use it; the current image has a small check and caption rather than a selection outline or disabled action row. Upload remains beside the artwork categories. **Find more artwork** opens a separate SteamGridDB browsing surface; credentials, matching and network requests are not mixed into the saved-image view. **Re-identify game…** opens the same dedicated identification flow from either saved or online artwork. Saved images and uploads continue to work without an API key.

Compact rows include developer/genre metadata, collections, completion and Startpoint launch counts, alongside availability and optional source/date information. Source labels and launch dates are independent: games with no recorded launch date leave that field empty. Switching layout or density returns the library to its heading and filters.

Hidden games are available from the sidebar only. Favourites and Manually added filters appear when populated; the first favourite reveals its filter and removing the last collapses it with a short animation (disabled with reduced motion).

Window size, location and maximized state are saved locally in `window-state.json` in the application's data directory. If a monitor is removed, the next launch moves the window back onto an available display. Example-library placement stays with its isolated data.

The detail overview uses a game's logo instead of its text title when available. It prefers **Background** (SteamGridDB hero) artwork for clean scenery. If there is a logo but no background, it uses a neutral surface rather than a cover with another baked-in title; **Choose background…** opens that artwork category. Background files can still contain text—choose a text-free image in the Images editor. Cover art is a fallback only when no separate logo is available.

Logos are centered using their visible artwork rather than transparent file padding, and their display height adapts to fit the available width. Normalized copies are prepared off the UI thread in the logo cache; originals and artwork history are unchanged. Extremely tall logos are height-limited to keep the description and launch controls visible. Favourite and game options sit beside Start game, with last-launched information beneath the action row.

## Precision controls and smoked glass

The left, middle and right **toolbar islands** share the same glass surface, theme rounding, 36px height, equal 2px padding on all four sides and 2px control gaps. Screen tabs and toolbar icons use the same neutral, high-contrast hover/pressed/selected fills; the selected screen no longer uses a different accent-button style. Resting controls share the island's background, and labels/icons remain sharp. Windows caption controls match these highlights and rounding, except Close retains its red warning hover. Regular buttons and opened dropdown menus retain their opaque fills.

Action buttons, icon buttons, dropdown triggers, text fields and appearance-slider focus frames share **Control rounding** (6px by default), fine theme-derived outlines, quiet opaque neutral fills, medium-weight labels and distinct hover/pressed feedback. Teal remains reserved for primary actions and small selection indicators. Secondary buttons no longer compete with the artwork; optional filled secondary buttons retain stronger surface separation. Both treatments preserve readable labels. Toolbar controls still share their containing glass surface.

Corner-radius roles are centralized in `crates/desktop/src/rounding.rs`: artwork and inset surfaces follow the theme's base radius (8px in Startpoint), while floating panels use its larger radius (12px). Artwork cards have a larger outer radius for their 4px inset; square themes keep square cards. Library covers, landscape/compact thumbnails, Recents images, editor posters, artwork previews and command-search thumbnails apply the artwork radius directly to both the image and its placeholder—GPUI's rectangular overflow clipping alone does not round child images. Notification blocks and journal notes share the inset-surface radius. Controls remain independently customizable; status dots and progress pills intentionally stay circular, and full-bleed backgrounds remain unrounded.

Customize Library and Customize Recents share **Precision controls**: **Control rounding** (0–12px), **Button padding** (8–20px), and **Outline strength** (25–100%) sliders, plus **Control outlines**, **Filled secondary buttons**, and **Compact launch controls** switches. Adjustments preview immediately and are saved after a short pause or pointer release. Sliders support arrow keys, Shift+arrow, Home/End and accessibility increment/decrement actions. Each slider has a Default action; the section's Reset restores only control styling, not theme, glass or layout preferences. Container rounding remains theme-owned, independently of control rounding. Saved themes and existing layout settings are retained.

Classic uses shared, theme-aware smoked-glass panels and **flat, finely outlined controls**. Floating navigation, the **Ctrl+P / Cmd+P** (also Ctrl/Cmd+K) command palette, the game-editor modal, customization panels and notifications share 18px backdrop blur, 85% tint, pale hairlines, soft shadows and faint inset highlights. Editorial instead resolves these shared surfaces to opaque warm paper with fine borders and no smoked tint or inset shadow. Buttons have solid fills with no gradients or shadows. Sidebar navigation uses plain rows with no resting fill, highlighting only hovered/selected items. Inputs are recessed without outer focus rings. Regular buttons, dropdown menus and fields stay opaque. Keyboard-focus indicators on buttons are retained, and modal dimming covers the entire app. Card titles are static, truncated with full-title tooltips; the Recents heading wraps instead of automatically scrolling. Zero launch timestamps are treated as missing history, and older launch dates use calendar dates instead of very large week counts.

Game cards use neutral surface changes for hover, press and selection, not accent outlines. Artwork keeps a 4px outer gutter; grid and Recents captions sit 2px inside the image edges and add an 8px bottom inset, keeping 12px beneath the text. Missing last-played dates do not reserve a blank footer. Library rows retain aligned artwork and consistent row spacing, while Recents cards do not stretch their backgrounds to match a neighboring card's extra text. Grid/compact and Recents cards share state styling: selected backgrounds are opaque and selected hover blends halfway toward the theme's hover color for a subtle lift, with the theme's active color reserved for pressing a selected card. Ordinary unselected hover/press fills are unchanged. Headless GPUI tests check laid-out caption side/bottom padding and pointer enter/leave/click/release in light and dark themes through context-menu wrappers and a virtual list. Pointer focus does not add a decorative ring; keyboard navigation retains the theme's distinct focus indicator. Recents widgets are quiet, hairline-separated sections rather than individual glass cards: **Weekly activity** shows seven days of recorded launches and opens the selected day in Activity; **Collection progress** shows completion counts and links to collections; **Journal preview** shows the latest note and opens its journal. These remain independently enabled in Customize Recents. No playtime is inferred. Artwork and its fade scroll with the Recents hero so the widgets and reading surfaces remain separate.

In Classic, **Customize Library** and **Customize Recents** contain theme selection/editing, a **Colour mode** dropdown (System/Light/Dark), separate saved **Translucency** and **App background blur** switches, and a **Translucency** amount slider. Editorial hides those inactive controls while retaining their saved values; both designs expose **Interface style** and control geometry. Library artwork dithering lives only in Customize Library. **App background blur** controls only the desktop behind the native app window: disabling it uses a sharp transparent backdrop instead of Acrylic/vibrancy. Classic glass panels retain their local backdrop blur; Editorial paper panels remain opaque and unblurred. The app-background blur control is available when native window translucency is enabled. Disabling translucency makes the shell/panels opaque without losing the saved amount or native-blur preference. Unsupported native-translucency platforms retain an opaque base. Regular buttons, menus, fields, text and icons remain opaque.

The amount slider has a visible **20% default notch**, calibrated to preserve the previous theme-aware glass appearance. Pointer adjustment snaps within two percentage points of it, and **Default** restores it. Click the slider or Tab to it, then use **Left/Right** or **Up/Down** for 1% steps, **Shift+arrow** for 5% steps, and **Home/End** for the endpoints. Keyboard adjustment can always leave the notch. Changes preview immediately and are saved after a short pause or on pointer release. Recents reuses its composited artwork texture while adjusting the amount, rather than decoding artwork on every slider movement. Window dragging and native caption actions are unchanged.

Selecting a Recents card or carousel position icon pauses automatic cycling, including when clicking the already-selected game. Previous/next navigation also pauses it. Use **Resume carousel** to continue; automatic advancement itself never enables the manual pause.

Recents composites artwork, ordered dithering and its settled fade off-thread, then applies shell translucency once to the result. Even its solid-colour footer stays translucent; images on disk are never modified. The three-backdrop cache retains visible/crossfading/preloaded frames and evicts only at frame boundaries. Right-click the Recents backdrop or a game card for the same game actions as Library.

**Dithering size** in Customize Recents and Customize Library changes the ordered pattern's cell size from **1–12px** (default **3px**): higher values make bigger, coarser dots, without changing intensity or window translucency. Both dithering surfaces use the same Zeron-style image-dependent Bayer color quantization, not a grain overlay: each cell samples the artwork and becomes a hue-preserving bright or dark dot according to its brightness and threshold. **Dithering intensity** blends the original and treated RGB (**0–100%**, default **20%**); **100%** shows the full treatment, and 0% bypasses it exactly. Size and intensity are saved independently for each screen, preview live, support keyboard adjustment and a Default action, and retain their values while dithering is off. Existing saved values are preserved; use each slider's **Default** action to adopt 3px / 20%. Both screens process images off-thread, reuse decoded artwork and keep ready textures during adjustments; cache eviction stays at frame boundaries. Image alpha, the existing fades, window translucency and original image files are unchanged.

**Edit theme…** previews colours and **Surface corner radius** live. **Border** affects field edges, panel hairlines and Precision control outlines; control rounding is adjusted independently in Customize. **Rename theme…** focuses the custom theme's name in the editor. Saving renames its existing file's theme without leaving an old duplicate; built-ins are always copied. Sidebar labels and icons align to the left. The component comparison page has been removed.

Development builds launched with `--example-library` also expose **Interface stress data** in Customize: **Demo data**, **Worst case**, **Empty**, **One game**, and **1,000 games**. These change only the displayed snapshot; stress modes block game mutations and launches, never overwrite saved game data, and are omitted from release builds. Switch back to Demo data for normal example-library interactions.
For repeatable development checks, add `--interface-data=worst`, `--interface-data=empty`, `--interface-data=one`, or `--interface-data=large` when launching the example library. These flags have no effect on the real library or in release builds.

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
| **Action button** | Finely outlined text action with adjustable symmetric padding; `button` |
| **Choice group** | Related options with a neutral selected choice; `choice_group` / `choice_button` |
| **Choice dropdown** | Opaque trigger plus check-marked menu choices; `choice_dropdown` |
| **Path row** | Label/path at left, directory actions at right; `path_row` |
| **Glass panel** | Containing smoked-glass surface; `glass_panel` |
| **Customize panel** | Floating appearance and screen-layout controls; `floating_panel` |

## Check

```sh
cargo test --locked --workspace
```

Code: `crates/sdk` (API), `crates/core` (backend), `crates/desktop` (GPUI).

Shared desktop presentation lives in `crates/desktop/src/components.rs`: Precision button tokens, outlined buttons, icon buttons, navigation choices, dropdowns, recessed inputs/textareas, glass panels, section labels, widget surfaces and image fallbacks. `adjustment_slider.rs` provides the shared keyboard-operable appearance sliders. `glass.rs` owns theme-derived materials, global effect policy and ordered blur painting. Views keep their own state/action wiring. `detail_artwork.rs` centralizes the overview's clean-background and logo/title selection.

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
