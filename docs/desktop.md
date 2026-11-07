# Rune desktop

Startpoint has one native application: `crates/desktop`, package and executable `startpoint`. Rune, Rune Kit and Rune FX are pinned together to `781e2b6d87cd5f32d6f4365956fad4baaf24ee50`. There is no GPUI build target, component dependency, renderer or automation driver.

## Architecture

- `startpoint-sdk` defines snapshots, game metadata, artwork, journal entries, independent tasks, commands and backend jobs.
- `startpoint-core` owns persistence, provider discovery, launch validation, credentials, catalogue requests, artwork downloads and worker execution.
- Rune owns layout, rendering, image decoding, focus, hit testing, scrolling, native dialogs and accessibility. Core and host-worker completion wake the UI through Rune's Waker. There is no permanent application frame timer.
- `components.rs` adapts Rune Kit buttons, Field groups, Select, dividers, action rows and bounded modal bodies. Standard dialogs share surface construction and aligned header/body insets. Native Menu and Select retain keyboard navigation, focus restoration and dismissal.
- `state.rs` owns navigation/history, ordering, bounded image retention, overlay state and backend/host event handling. Image completion does not refresh backend snapshots. Pending pixels remain mounted so image decoding can repaint without another input.
- `effects.rs` composes optional Bayer treatment and developer-preview materials through Rune FX while preserving Rune's clipping/blend metadata. Expensive material experiments are not applied to ordinary application components.

## Storage and promotion

The sole application uses the standard Startpoint library unless an existing Rune preview database is present. In that case it continues using `rune-preview/` and its matching cache; example mode checks `rune-preview/examples/` independently before falling back to `examples/`.

Promotion does not copy, delete, combine or overwrite databases or cached artwork. Canonical libraries remain untouched when an existing Rune library is selected. Settings shows the exact active data and cache paths. The credential service remains the existing OS store.

Schema 3 added nullable local journal authors without rewriting old entries. Schema 4 protects tasks persisted inside game documents; absent task fields deserialize as an empty list. Journal text and tasks are independent. Older executables reject a newer schema instead of modifying it.

Headless runs always replace the paths with a temporary example database and disable credentials, remote artwork providers, external application opening and real launches. They do not access the interactive library.

## Presentations and interaction

The top bar uses a leading navigation/search/settings island, adjacent screen tabs and a trailing application-action island. On Windows, minimize, maximize/restore and close share the trailing island; there is no second title strip. Close has a red hover treatment and uses Rune's orderly window-close request. macOS retains its own traffic lights and their reserved leading space.

`window_chrome.rs` adapts the existing resizable Windows window through Rune's per-window native-handle callback. It removes the non-client caption without removing system-menu/resize/minimize/maximize styles, preserves DPI-aware edge/corner resizing and the maximized monitor work area, and exposes the maximize hit target to Windows Snap Layouts. Native handles are borrowed only on the UI thread; the subclass is removed when the window is destroyed. Rune owns drag regions and title-bar double-click behavior. Caption painting and native hit testing use the same logical geometry. Maximized content has square outer corners.

Isolated Rune layout/input tests cover 900–1600px widths, 100–200% scales, left/right grouping, caption hit-target alignment, control clicks versus dragging, double-click requests, orderly close and macOS traffic-light reservation. Windows native drag/resize, Snap Layouts, maximize/restore, taskbar and accessibility behavior still require interactive acceptance; headless layout checks do not prove those OS behaviors.

Classic and Infinite Wall are the supported Recents presentations. Retired SDK presentation values remain readable for stored-document compatibility and normalize to Classic; their old implementations are removed.

Classic keeps its rotation at the bottom of the first viewport and optional widgets below it. The Library is continuously scrollable and virtualized, with one captured ordering shared by each grid's count, keys and contents. Compact mode uses Rune Kit Table. Nearby artwork is prefetched; complete backgrounds remain visible while replacements decode. Other screens omit the former large blurred ambient artwork layer.

Infinite Wall uses a 180 ms hover dwell and 160 ms smoothstep transition, 6-point lift, 1.06 scale, surrounding dimming and centered Play controls. Expanded artwork uses matched geometry in a layer clipped below the toolbar. Reduced motion removes spatial emphasis.

The standalone Journal modal provides Markdown formatting/preview, rendered dated entries, local author attribution and a separate independent To-dos tab. Editor sections remain Details, Artwork and Launch. Confirmations name the operation, use danger styling for destructive actions and return to their parent when cancelled.

## Developer component effects

Available in debug builds, tooling scenes and sessions explicitly started with `--developer-tools`. Ordinary release sessions hide its settings entry and command and reject navigation to the page.

```sh
cargo run --locked --release -- --example-library --developer-tools --scene component-effects
```

The page provides a neutral baseline plus aura glow, liquid metal, charged edges, warm embers, fluted glass, mesh gradients and moulded plastic. Controls, Surfaces and Data tabs cover Button, IconButton, TextInput, TextArea, Switch, Checkbox, radio/segmented controls, Select, Menu, Dialog, Card, Collapsible, Badge, Progress and virtualized Table.

The inspector and specimen surface scroll independently. Material, intensity, speed, rounding, contrast protection, disabled state and reduced motion are session-local. Copy configuration copies a text summary; Reset restores the preview values. No game or appearance mutations are dispatched by the samples.

Pause freezes the local effect clock. Reduced motion, inactive windows, menus, dialogs and navigation away stop continuous effect submissions. Resuming preserves phase; static materials do not request continuous frames. Pixel comparisons verify running versus paused/reduced-motion output, in addition to interaction and layout checks.

## Packaging and updates

The application keeps its existing `startpoint` binary name, Windows resource manifest, installer identity, Velopack channels and package validation. Rune branding uses a blue published-release icon and an amber local-development icon, without changing the play-mark design or storage locations. Ordinary local builds (debug or optimized) use development branding; GitHub packaging explicitly opts into `production-branding`. Windows sets a separate development AppUserModelID before window creation and applies the matching embedded small/large icons to every new native window; Rune's app-icon helper supplies the macOS Dock icon. The older green/violet assets remain intact. Windows explicitly enables DX12, macOS uses Rune's Metal backend, and Linux enables Vulkan/GLES. Native Windows acceptance remains a platform check; a macOS build does not prove it.

The `self-update` feature uses dedicated workers for checks, verified downloads and update preparation. Results and progress wake Rune through host events. Settings and the notification panel expose actions; development/example/headless/developer-tool sessions disable automatic checks. Restart requests use Rune's orderly quit after backend work and editor guards, not `process::exit`.

Local diagnostics retain privacy-filtered session/panic logs and omit credentials, queries, library contents and panic payloads. The icon generator and icon/update/package validation tests are retained independently of the removed frontend.

Remaining platform work includes native window-placement restoration, complete menu integration, theme export/removal, sidebar resizing, and broader Windows/accessibility/IME/suspension acceptance. Promotion does not imply those gaps have been completed.
