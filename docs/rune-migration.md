# Rune migration

`startpoint-rune` is a separate native frontend using the existing `startpoint-sdk` contract and `startpoint-core` services. GPUI remains the default executable and production release target. Both can run simultaneously, with separate databases and preferences.

Rune, rune-kit and rune-fx are pinned together to [`781e2b6d87cd5f32d6f4365956fad4baaf24ee50`](https://github.com/anomalyco/rune/tree/781e2b6d87cd5f32d6f4365956fad4baaf24ee50). The starting references were `examples/hello` and `crates/rune/README.md`; the expanded frontend uses rune-kit's public component APIs rather than importing GPUI renderers.

## Run

```sh
# Existing application
cargo run --locked -p startpoint -- --example-library

# Rune with simulated example games
cargo run --locked --release -p startpoint-rune -- --example-library

# Rune with installed-game discovery and real launch routes
cargo run --locked -p startpoint-rune
```

On macOS without the Metal compiler, add `--features runtime-shaders` to the **GPUI** command. Rune uses its own Metal backend and does not need that feature. Windows has not yet been validated for this frontend.

## Storage and safety

- Interactive Rune runs use `rune-preview/` under Startpoint's standard data/cache directories. Example runs use `rune-preview/examples/`. Settings displays the exact paths.
- Libraries, journals, artwork selections, appearance preferences and history are **not synchronized** with GPUI. Do not share a writable database between processes.
- Real-library runs use the existing OS credential service. **Saving, verifying or removing a SteamGridDB key affects the credential shared with GPUI.** Keys are not stored in theme files or local preferences.
- Interactive example mode uses the existing credential service and automatically fills missing metadata/artwork when a credential is available. Example-provider launches remain simulated. Non-example game launches are blocked in example sessions, including manually added games.
- Every headless run forces a fresh temporary example database, omits artwork providers and disables OS credentials and external application opening. Local artwork imports and SDK mutations operate only on that temporary database. Real installed-game launches are not permitted headlessly.

## Implemented application workflows

### Navigation and library

- Compact floating toolbar surfaces for native window controls/sidebar/back/forward, Recents/Library/Activity navigation, and search/application actions. macOS uses real traffic lights relocated into the first surface, not simulated buttons. Rune drag regions preserve titlebar dragging and double-click behaviour without stealing presses from controls.
- Bounded back/forward navigation restores screens, filters, sources, collections, selection, sort order and overview visibility. Cmd+[ / Cmd+] navigate on macOS; Alt+Left / Alt+Right are available on other platforms. New navigation after going back clears the forward branch.
- Native sidebar rows for all installed games, favourites, manually added games, completed/unfinished games, hidden games, discovery sources and collections.
- Search, title/recent/source ordering, selection and a closable game overview.
- Cover grid, landscape tiles, pills and a **rune-kit virtualized Table** for compact lists. The table provides selection, keyboard navigation, resizable/sortable columns and Return activation. Artwork layouts use continuous, virtualized rows without pagination, with bounded nearby-thumbnail retention. Redundant library-count captions are removed.
- Inline grid/tile/compact switching, sort reversal and an overview visibility toggle. The overview can be reopened without going through customization. Compact-table sort indicators stay synchronized with the library's sort controls, including date direction.
- Play, double-click activation, favourite changes, completion, hide/unhide, installation-directory opening and game-management context menus.
- Direct completion/unfinished marking in the overview preserves the game's existing collections.
- Recorded activity with game/journal links and a date selection from the Recents activity widget.
- Optional activity, collection progress and journal preview widgets.
- Clicking or focusing toolbar search expands it into a centered native command panel through matched geometry. Search never filters Recents. The panel groups games and commands, supports Up/Down/Return, reveals selected games in Library, and closes with Escape or an outside click. Cmd/Ctrl+F/P/K opens it directly. Refresh and Add live in the command panel rather than the toolbar.
- Cmd/Ctrl+B folds the sidebar through the same animation as its button; reduced motion makes both immediate.

### Game editor

The editor stays above its originating screen and uses rune-kit's Dialog and SegmentedControl:

- **Details:** title, description, developer, genres, collections, completion, hide/unhide and confirmed manual-game removal. Unchanged automatic metadata is not converted to manual overrides.
- **Artwork:** saved-image selection/history pagination, local upload and drag/drop, Cover/Background/Logo/Icon categories, SteamGridDB game-name or explicit URL/#ID search, match persistence, re-identification and remote image selection. Both ends of the result list expose server pagination, including filtered-empty pages with `has_more`.
- **Launch:** restore launcher defaults; executable/application bundle, individual argument lines and working directory; numeric Steam launch URLs; `.url` shortcut import; saved Steam non-Steam shortcuts identified by account and app ID; shortcut refresh and warnings. Imported routes remain drafts until saved. Core validation remains authoritative.
- **Journal:** create, edit and confirmed deletion, with SDK persistence. Saving an existing draft updates its entry rather than repeatedly creating copies.

Remote artwork responses and imported shortcut URLs are accepted only when their job ID and game match the current editor; artwork pages also match the requested category and page. Moving to another editor invalidates the previous request association.

### Settings and customization

- Saved startup screen.
- Secure native text entry for SteamGridDB credentials, verify/unlock, confirmed removal and the API-key acquisition link.
- Visible SteamGridDB connection/download prompts in Classic Recents and Library, plus a Settings download action. Successful credential connection or verification queues metadata/artwork refresh after active work finishes; existing artwork choices are preserved. Activity & Notifications exposes progress, errors and cancellation.
- Automatic/resolved discovery directories, native folder pickers, reset to automatic, discovery refresh and metadata/artwork refresh.
- Local data/cache directory actions.
- Saved interface style, System/Light/Dark, built-in themes, layout/density, detail/source/date/favourite options and widgets.
- Theme import, copy/edit, rename, validation and live colour preview. Theme files use the existing Startpoint JSON shape, live only in Rune's storage, have bounded import sizes and are written on workers. Built-ins require saving under a new name.
- Control rounding uses Rune's range accessibility/step handlers; ordinary buttons remain rune-kit buttons with only application-specific radius adaptation.
- Saved translucency, desktop backdrop blur, Recents/selected-artwork Bayer strength and pixel size, and background-effect pause controls.
- Activity & Notifications lists backend jobs, errors and cancellation actions. Completed notifications can be dismissed individually or cleared together without deleting backend job records; running work retains its cancellation control.

### Interface presentations

Rune offers **Classic, Gallery, Cinematic and Infinite Wall**. Film Reel and Mosaic have been removed from Rune; their SDK variants and GPUI implementations remain compatible, and older Rune selections render Classic. The retained styles share navigation, game selection, launch routes, command search and editors. Rune owns layout, target-property motion, focus, hit testing, scrolling, image decoding and rendering.

These are native Rune compositions, **not rendering-parity claims**. Gallery does not yet reproduce the GPUI/wgpu perspective floor/reflections. Cinematic uses Rune image elements rather than GPUI GPU handles. Infinite Wall uses virtual portrait rows, 10-point gaps, dwell hover emphasis, surrounding dimming, hover Play controls and click-to-expand artwork. The SDK on this branch has no separate Editorial, Art Book or Diagonal variants.

### Glass and artwork treatment

- A transparent Rune window uses native macOS system glass when translucency and desktop blur are enabled; other platforms use the tinted fallback. A separate, mostly opaque contrast layer sits above the native material: Rune replaces a material node's own fill on macOS, so its fill cannot serve as that tint. Headless snapshots cannot validate desktop materials.
- Floating toolbar surfaces use restrained flat controls, translucent tint, Rune backdrop blur and a soft edge/shadow rather than refractive glass lenses. Recents artwork extends behind the floating controls in every presentation, with reserved native-window-control hit areas. Dialogs use Rune Kit's own surface; selected backgrounds provide a softly blurred ambient image on other screens. Theme surfaces explicitly follow the Startpoint theme rather than deriving a saturated tint from its foreground colour.
- The library sizes artwork columns to the available window/sidebar/detail-panel width. Its selected-game panel gives more height to artwork, removes redundant headings, and uses compact management/progress icons beside launch/favourite controls. Nearby backgrounds/logos are prefetched, and the previous complete hero is retained until the selected background/logo pair is ready. Artwork controls use explicit overlay colours for light-mode contrast.
- Classic Recents uses edge-to-edge selected artwork beneath the floating toolbars, a contrast scrim, left-aligned logo/title, readiness/last-played information, a short description and direct play/favourite/manage actions. Its landscape rotation distinguishes the selected game. A smoked sidebar stays readable over the artwork. Image readiness never changes the page composition: the previous ready background remains while its replacement decodes, and only the incoming image fades. Nearby backgrounds/logos are prefetched.
- Both side panels fold through clipped containers with fixed-width contents and a 200 ms drawer curve. Progress is retargeted from its current value and advanced on Rune's frame clock, with wake requests only while a fold is active, so background commits cannot settle it prematurely. Sidebar buttons and shortcuts animate in both directions; reduced motion is immediate. Page replacement disables inherited entry/exit fades to avoid flashing during Recents-to-Library navigation. Recents forces the dark variant for all controls and toolbars; other screens follow the saved/system appearance. The bottom application/status footer is removed.
- Form buttons, inputs and select fields use coordinated heights, spacing and field borders. Dialogs use Rune Kit's own centered surface, bounded scrolling and content-fit sizing, with outside-click and Escape dismissal. Cancelling a confirmation returns to its parent dialog.
- Full-size artwork uses optional rune-fx Bayer treatment, preserving normal thumbnail rendering. Strength follows a squared response for a subtle low-end treatment; zero omits the effect slot and 100% retains the full effect. The neutral palette treatment is not the hue-preserving GPUI dither algorithm.
- The artwork compositor tracks device/format changes and passes Rune clipping and blend metadata to rune-fx. Obsolete Film Reel animation and clock resources are removed.

## Rune abstractions used

- `rune-kit`: Button, TextInput, TextArea, Select, Switch, SegmentedControl, Dialog, Menu/MenuItem, Sidebar/SidebarRow/SidebarSection and virtualized Table/TableColumn/Cell.
- `rune`: native Open/Folder dialogs, secure TextEdit, file drops, accessibility roles/ranges, modal focus management, keyboard routing, scroll requests, clipping, aspect fitting, gradients and ImageCache.
- Core events wake the UI through Rune's Waker. Network operations and game mutations stay on the existing core workers. Theme reads/writes and directory/browser opening use host workers. There is no permanent frame timer.
- Decoded image handles are retained only for the current artwork page, nearby/displayed library games, selected game and current editor. Headless runs wait for local image decoding; interactive runs never do.

## Internal headless checks

```sh
cargo build --locked -p startpoint-rune
mkdir -p target/rune-preview
target/debug/startpoint-rune --scene editor --snap target/rune-preview/editor.png --tree target/rune-preview/editor.json
target/debug/startpoint-rune --scene compact --snap target/rune-preview/compact.png --tree target/rune-preview/compact.json
target/debug/startpoint-rune --scene infinite-wall --snap target/rune-preview/infinite-wall.png --tree target/rune-preview/infinite-wall.json
```

`--list-scenes` includes the application screens, editor sections, customization, manual addition, commands, notifications, compact table and all interface presentations. `--script` drives the same input handlers as the window. Nested fields are addressed by scoped keys such as `editor-title/value/input` and `journal-text/scroll/input`. Scripted `type` events take virtual time per character; use `paste` or leave enough time before clicking Save. Select options have scoped `option0`, `option1`, etc. keys beneath their popup.

Current internal evidence is written under `target/rune-preview/`, not committed as fixtures. Visual checks include populated 45-game light/dark library renders, minimum-window layouts and native macOS screenshots of the populated application. Native captures confirm that the contrast layer remains visible above the system material. Headless artwork checks import copies of cached images through the application's file-dialog handlers; they do not share writable artwork paths or a database with the interactive library. User acceptance and broader platform checks remain part of the migration.

## Remaining parity work — not ready for production replacement

1. Remaining renderer parity: Gallery perspective/reflections, exact cinematic transitions, hue-preserving dithering, native desktop-material acceptance and complete precision-control geometry parity. Glass and Bayer treatment are implemented but are not exact GPUI replicas. Infinite Wall expansion currently uses a standard dialog rather than GPUI's matched artwork transition.
2. Native window position/maximized-state restoration, theme export/removal actions, full sidebar resize behavior and release-grade native menu/branding integration.
3. Packaged Rune update channel, installer/signing/updater integration. Development builds intentionally show automatic updates as unavailable rather than using the GPUI production updater on the wrong executable.
4. Live network/credential acceptance, actual launch routes on Windows/macOS, large-library profiling, native accessibility/IME and platform suspension behavior. Headless example checks do not prove those integrations.

Keep GPUI as the stable application until these gaps and platform acceptance are resolved. The current branch substantially expands functional coverage, but does not claim complete migration or production readiness.
