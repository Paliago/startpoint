# Non-activating GPUI automation

This opt-in macOS integration runs the real `LauncherView` inside the normal
`gpui_component::Root`. It does not substitute an editor demo or a mock backend.

## Compatibility

The optional dependency is pinned to
`Paliago/gpui-automation@dbe801f5564e383fe4cf0ca2f9882ecec28fc2d6`.
Startpoint, the driver and the component library declare the same Zeron API base,
`361ff20b919fdfb80199ee6d9aed725300cb7d54`. The host's root Cargo patch table
resolves the matching GPUI packages together to
`Paliago/zui@fa44ef75782ec34a0c5490e56541832e7c595921`.
The component revision remains
`zeronsh/gpui-component@701ca97af7983bf85343d2d1d404ab528b18ec96`.

The driver's dependency-level Cargo patches do **not** propagate to this workspace.
The host explicitly applies the merged headless-window fix from
[Paliago/zui#1](https://github.com/Paliago/zui/pull/1). Its only difference from the
existing API base is the two native-handle error returns on `TestWindow`; native
platform windows and the automation driver are unchanged. The library's original
documented `827dc2f` patch is a separate web-only change and is not needed here.
`cargo tree --locked --features automation,runtime-shaders -i gpui` verifies the
shared runtime dependency. Do not update only one GPUI package and split types.

## Build and launch

For isolated design inspection, add `--automation-layout` with one of `carousel`,
`gallery`, `cinematic`, `infinite_wall`, `film_reel`, or
`mosaic`. This overrides only the session's observed appearance, not SQLite
preferences. Add `--automation-render-background` explicitly to exercise motion
without activating the window. It overrides only background pausing in this
private session and permits its deliberately hidden offscreen surface to render.
Other-app fullscreen protection and reduced-motion settings still apply; normal
windows retain their minimized protection. Neither option permits backend settings
writes or native actions.

```sh
cargo build --locked --features automation,runtime-shaders

RUN=$(mktemp -d "${TMPDIR%/}/startpoint.XXXXXX")
DRIVER="$RUN/driver"
SOCKET="$RUN/app.sock"
git clone --no-checkout https://github.com/Paliago/gpui-automation "$DRIVER"
git -C "$DRIVER" checkout --detach dbe801f5564e383fe4cf0ca2f9882ecec28fc2d6

./target/debug/startpoint --automation-socket "$SOCKET" \
  --automation-library "$HOME/Library/Application Support/Startpoint/examples/library.sqlite3"
```

Use a **new** clone destination and private session directory. `mktemp` creates
the directory with mode 0700. macOS socket paths must fit within 103 bytes.
The optional `--automation-library` argument imports game records from a read-only
SQLite connection; omit it for an empty real library. Credentials, preferences,
journals and launch-event history are not imported. Existing artwork paths are
read for display; derived caches go into the private session.

In another terminal, using the same `DRIVER` and `SOCKET` values:

```sh
node "$DRIVER/client.ts" "$SOCKET" '{"command":"wait","pointer":"/status/ready","equals":true}'
node "$DRIVER/client.ts" "$SOCKET" '{"command":"inspect"}'
node "$DRIVER/client.ts" "$SOCKET" '{"command":"key","key":"cmd-p"}'
node "$DRIVER/client.ts" "$SOCKET" '{"command":"wait","target":"command-search","state":"focused"}'
node "$DRIVER/client.ts" "$SOCKET" '{"command":"type","text":"Edit game metadata"}'
node "$DRIVER/client.ts" "$SOCKET" '{"command":"key","key":"enter"}'
node "$DRIVER/client.ts" "$SOCKET" '{"command":"wait","pointer":"/status/editorOpen","equals":true}'
node "$DRIVER/client.ts" "$SOCKET" '{"command":"focus","target":"field-title"}'
node "$DRIVER/client.ts" "$SOCKET" '{"command":"screenshot","path":"editor.png"}'
node "$DRIVER/client.ts" "$SOCKET" '{"command":"quit"}'
```

The session prints its exact backend database and capture directory before
initialization. Inspect `/status/backend/endpoint` and the permission fields before
attempting any mutation. Screenshot filenames must be new; captures are retained
after quit. The driver removes only its own socket.

## Host-owned safety

- Normal builds do not enable the dependency. Automation flags fail with exit
  code 2 before normal startup when the feature/platform is unsupported. Merely
  compiling with the feature does not start a socket or change activation policy.
- Explicit automation startup calls `prepare_application()` on GPUI's main
  thread before opening the driver's offscreen, `focus: false` window. It never
  calls `cx.activate` or generates global mouse/keyboard events.
- Each session has new data, preferences, window placement, diagnostics and cache
  directories. Pending jobs and backend-event receipts belong to that process.
  Database/schema setup, read-only seeding and private rendering caches are host
  initialization, not permission for subsequent business operations.
- The actual `LauncherBuilder`/SQLite backend and normal Steam/Epic/GOG/SteamGridDB
  providers are used. Automatic discovery is suppressed. No OS credentials are
  read or written; credential commands are independently rejected by the isolated
  core configuration.
- Every application command is denied by default at the `ApplicationApi`
  boundary. A separately authorized launch may add **`--automation-allow-metadata`**;
  that permits only `UpdateMetadata` in the new session database. Launches, artwork
  changes, discovery, journal changes and preference persistence remain denied.
  The flag never enables writes to the source library or an external endpoint.
- Known native entry points are guarded: file/confirmation dialogs, reveal-in-
  Finder, browser links, clipboard actions, character palette and native window
  dragging. Theme-file saves are also denied. Automation refuses a build combined
  with `self-update`.

Do not enable metadata permission merely to make a scenario pass. Obtain explicit
authorization for that exact private backend and operation first.

## Live observations and targeting

`status` reads the existing launcher and backend, including the selected game,
editor/command state, notices, actual draft values, appearance previews, current
jobs and a bounded history of **real** backend job events. It does not maintain
shadow editor state or turn a dispatched click into a success result.
`ready` means the launcher has received its library snapshot; `backendReady`
separately reports backend initialization. Appearance observations are live UI
previews, not proof that a denied preference change was persisted.

Shared buttons publish their existing element IDs as author accessibility IDs.
Editor adapters register existing focus handles and weak readers of the real
`InputState`/`TextareaState`, including current UTF-8 byte selections and enabled
states. Examples include `command-search`, `field-title`, `field-description`,
`field-developer`, `field-genres`, `artwork-search` and `artwork-api-key`.
Wrapper labels use an `editor` suffix to avoid matching both a wrapper and its
inner native input. Hidden editors are not registered during inspection.

Matching remains strict: repeated labels or IDs must be narrowed with properties
such as `role` and an ancestor structural `within` path. For example, inspect the
`game-editor-modal` group and scope `save-metadata` to its returned path. Structural
paths are runtime fallbacks, not stable IDs across layout changes.

## Evaluation results

The pinned driver was exercised against the real launcher with 45 imported game
records, not the independent example. Inline assertion scripts were used; no new
test files were created.

Passed checks:

- `windowActive=false` during inspection, logical input focus, navigation,
  screenshots, successful writes, failures and recovery.
- Single-line and multiline Unicode values, UTF-8 byte selections, and focusing
  another editor then returning without changing the original selection.
- Unique editor IDs, scoped metadata-save targeting, rejection of ambiguous
  button targets, and rejection of disabled/hidden input focus.
- Recents/Library navigation with selected-game preservation, real editor
  scrolling and draft preservation across editor tabs.
- Hover changed the offscreen capture. A real slider-thumb drag changed the
  rounding preview from 5 to 10; preference persistence was correctly denied.
- Default-denied metadata save retained drafts and did not queue a backend job.
- Authorized metadata save produced real `Queued → Running → Completed` events.
  A title over the core's 500-byte limit produced `Queued → Running → Failed`,
  retained unsaved drafts and left the old saved data intact. Correction produced
  a new completed job and preserved the same game/editor context.
- Direct read-only SQL verified recovered metadata, 45 records and zero launch
  events in the private database. Original normal/example database hashes were
  unchanged. Launch remained denied even in the metadata-authorized session.
- Socket and captures were mode 0600; session directories were mode 0700.

One initial slider assertion failed because the gesture hit the track rather
than the thumb: it changed the value to 5 without dragging. Targeting the current
thumb and sending held-pointer moves passed. An initial duplicate Developer
target was fixed in the host wrapper label; ambiguity rejection was preserved.

Receipts verified pending backend states, but the writes were too fast to capture
a live pending UI frame. Unsaved-draft survival across process restart was not
qualified. The app intentionally reports `renderPaused=true` while inactive, so
these captures do not qualify ambient motion or Gallery's active 3D frame loop.

Accessibility `enabled` is not a write permission: Save remains inspectable while
the host denies it. Native button disabled-state coverage is incomplete, and the
driver's `visible` flag is primarily a viewport-bounds check, not complete ancestor
clipping or occlusion testing.

Offscreen Metal captures preserve alpha, not final desktop blur/compositing.
This evaluation does not qualify OS clipboard/dialogs/login, real OS focus/caret,
microphone/permission behavior, or full-rate animation. Those require a dedicated
macOS/VM workflow; do not test them on the working desktop through this driver.

### Checks and retained evidence

#### Artwork-design revision verification

The next presentation pass removes Book's top/bottom alpha borders and replaces
block-like brush noise with continuous directional bristle coverage. Mosaic now
prepares the requested, displayed and adjacent screens (at most four screens / 24
artworks), with two artwork decodes in flight. It also prepares normalized logos
and their native image-cache entries. A cold destination does not replace the
displayed screen until all of its artwork and logo loads have settled; failed
loads use the existing fallbacks. The initial empty screen uses a loading state.

This pass's private report is
`/private/var/folders/3g/vsqpcg555g1ftv2w9765vnzh0000gn/T/opencode/rev.WMgoUd/report.json`.
It verified initial warm pages `[0,1,7]`, an already-ready next screen, and a
rapid cold jump to screen 4 that retained complete screen 1 until screen 4 was
ready. The Book capture checks the unbordered top/bottom and revised side brush.
All inspected windows remained inactive and both sessions exited cleanly.

The follow-up revision removes Editorial's enum and fallback entirely, defaults
control outlines off, and retains interrupted Wall shade fades. Book banners are
framed around a bounded detail analysis rather than substituted with portrait
covers or blindly center-cropped; covers are used only when a background is
unavailable. Cached bristle alpha and warm, low-contrast paper grain replace the
wavy edge. No stored artwork is modified.

Follow-up captures of Witcher 3, Journey, Outer Wilds and Disco Elysium are indexed
by `focal-report.json` under
`/private/var/folders/3g/vsqpcg555g1ftv2w9765vnzh0000gn/T/opencode/art.nh9r9U/`.
`visual-report.json` in that directory also retains a Wall exit frame between
fully shaded and fully restored, confirming fade-out rather than a snap. The
existing tests exercise interrupted hover/re-entry, default/explicit outline
values and off-center artwork detail. These full suites pass 155 tests normally
and 156 with automation, with three existing ignored checks per configuration.

After removing Editorial and replacing the old Reel vector material, the full
workspace suites pass **154 tests normally** and **155 with automation**, with
zero failures and three existing ignored checks in each. The removed tests were
specific to the deleted Editorial renderer and the replaced vector material.
The existing native GPU check additionally exercises liquid frames at different
times, opacity and real readback. Its liquid render/readback measured 2.23 ms
median and 2.50 ms p95 in the recorded run; this excludes GPUI upload and display.

Private real-view captures and reports are retained under
`/private/var/folders/3g/vsqpcg555g1ftv2w9765vnzh0000gn/T/opencode/ui.bsKiGG/`:
`verification-report.json`, `launch-report.json`, and `reel-report.json`.
Observed checks include fixed Mosaic labels/bounds across repeated selection and
screen changes, settled Book navigation, living torn-edge movement, expanded
Diagonal hover, surrounding Wall darkening, eight Gallery library selections,
and a launch reaching the default-deny policy without backend dispatch.
Pixel comparisons confirmed Cinematic foreground hover with an unchanged empty
button background. Every inspected window remained inactive. Original normal and
example database hashes were unchanged, and completed sessions quit through their
own sockets.

A seven-process parallel capture attempt lost the Gallery and Reel endpoints
without a retained application panic; the cause was not established. Subsequent
individual GPU runs completed and exited cleanly. Concurrent multi-process GPU
stability and end-to-end foreground frame pacing are not qualified by this pass.

#### Earlier dependency-adoption checks

Normal and automation builds/checks passed. The normal build rejected an
automation launch with exit code 2 without creating a socket. Its normal
dependency tree does not contain `gpui-automation`.

At dependency adoption, both unfiltered workspace suites passed with the fix:
**162 passed** without automation and **163 passed** with automation, each with
zero failures and three existing GPU/live-service tests ignored. Neither Gallery
nor Cinematic requires exclusion. No standalone test files were added.

#### Adopted upstream fix

[The merged change](https://github.com/Paliago/zui/commit/fa44ef75782ec34a0c5490e56541832e7c595921)
replaces the two `unimplemented!`
native-handle methods on `TestWindow` with `Err(HandleError::Unavailable)`.
`gpui-base` already handles that error and skips the unsupported native autofill
bridge. Input editing, focus, search and the application test assertions remain
unchanged. A dependency's `cfg(test)` is not enabled merely because its host is
being tested, which is why the macOS bridge is reached by these downstream tests.

The patch was validated using a private GPUI source copy and an archived host
snapshot at `18fe0c4`. Temporary Cargo command-line patches resolved all 19 GPUI
packages to that same copy. The **unfiltered** automation-enabled workspace suite
then passed **163 tests, 0 failures, 3 existing ignored tests**, including both
previously failing Gallery/Cinematic cases.

Validation copies are retained under:

```text
/private/var/folders/3g/vsqpcg555g1ftv2w9765vnzh0000gn/T/opencode/gpui-fix.EhE4o0/
  gpui/          # original GPUI source plus the two-line patch
  application/   # isolated host snapshot and its temporary lockfile
```

The fix was subsequently merged upstream and pinned in this application's root
manifest and lockfile. The temporary patch artifact was removed because the
published commit now provides the fix. No application test assertions were
removed, no focused input was made read-only, and no shared checkout was edited.

A post-adoption smoke run against the real root with a fresh, write-denied
session also verified Unicode command input, UTF-8 byte selection (including a
selected emoji), focus preservation and Metal capture with `windowActive=false`.
Its capture is retained at
`session-c8a3fc88-a0e4-4207-95ed-3eed65f1f877/merged-gpui-smoke.png`
beneath the validation directory above. This is not a framerate
or final desktop-compositing qualification.

```sh
cargo fmt -p startpoint -p startpoint-core -p startpoint-sdk -- --check
cargo check --locked --features runtime-shaders
cargo check --locked --features automation,runtime-shaders
cargo test --locked --workspace --features runtime-shaders
cargo test --locked --workspace --features automation,runtime-shaders
```

Evaluation artifacts were retained privately under:

```text
/private/var/folders/3g/vsqpcg555g1ftv2w9765vnzh0000gn/T/opencode/gpui.TSPjqn/
  driver/                                     # clean checkout at the exact pin
  session-feb6e4b9-6504-42c2-9e8a-0eb71dfc4314/
    readonly-report.json
    readonly-final.png
    before-hover.png
    after-hover.png
    scrolled-editor.png
  session-ecf639bd-4201-4f24-8c84-cc7c0584bd58/
    metadata-report.json
    metadata-failure.png
    metadata-recovered.png
    data/library.sqlite3
```

All evaluation application processes were stopped through their own driver
sessions. No unrelated process was stopped, and the driver source was not edited.

For routine UI workflows this replaces the previous activating/global-input
method: it is scoped, assertable, non-activating and exercises native application
views. It complements rather than replaces unit tests, native GPU checks and
dedicated desktop/performance acceptance.
