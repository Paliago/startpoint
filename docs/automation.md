# Non-activating GPUI automation

This opt-in macOS integration runs the real `LauncherView` inside the normal
`gpui_component::Root`. It does not substitute an editor demo or a mock backend.

## Compatibility

The optional dependency is pinned to
`Paliago/gpui-automation@dbe801f5564e383fe4cf0ca2f9882ecec28fc2d6`.
Startpoint, the driver and the component library resolve to the same
`zeronsh/zui@361ff20b919fdfb80199ee6d9aed725300cb7d54` GPUI package.
The component revision remains
`zeronsh/gpui-component@701ca97af7983bf85343d2d1d404ab528b18ec96`.

The driver's dependency-level Cargo patches do **not** propagate to this workspace.
They were reviewed rather than copied blindly: the documented Paliago GPUI commit
`827dc2fbbb585f59e95559dbe79d149790dc072e` differs from the existing GPUI revision
only in `crates/gpui_web/src/window.rs`. No macOS adaptation or GPUI upgrade was
needed. `cargo tree --locked --features automation,runtime-shaders -i gpui`
verifies the shared runtime dependency.

## Build and launch

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

Normal and automation builds/checks passed. The normal build rejected an
automation launch with exit code 2 without creating a socket. Its normal
dependency tree does not contain `gpui-automation`.

The unfiltered desktop suite is **not green**: the existing Gallery and Cinematic
headless tests panic because GPUI TestWindows do not provide a native platform
window handle. Excluding those two, the automation-enabled workspace suite passed
161 tests (including an inline write-policy regression check), with three
existing GPU/live-service tests ignored. No standalone test files were added.

#### Prepared upstream fix — not applied to the configured dependency

[`patches/gpui-test-window-native-handles.patch`](../patches/gpui-test-window-native-handles.patch)
contains the minimal fix against GPUI revision
`361ff20b919fdfb80199ee6d9aed725300cb7d54`. It replaces the two `unimplemented!`
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

Per review scope, the patch has **not** been pushed, applied to a shared checkout,
or enabled in this application's manifests/lockfile. The configured unfiltered
suite therefore still has the two dependency failures until a reviewed upstream
fix is adopted. All GPUI packages must continue to resolve together when that
happens; do not patch only one package or add a read-only-input workaround.

To check the artifact against a fresh disposable checkout of the stated GPUI
revision, use `git apply --check /absolute/path/to/startpoint/patches/gpui-test-window-native-handles.patch`.

```sh
cargo fmt -p startpoint -p startpoint-core -p startpoint-sdk -- --check
cargo check --locked --features runtime-shaders
cargo check --locked --features automation,runtime-shaders
cargo test --locked --workspace --features automation,runtime-shaders -- \
  --skip gallery_browses_all_sources_without_panels_and_preserves_launch_and_navigation_context \
  --skip cinematic_unifies_sources_and_keeps_pointer_search_launch_and_settings_native
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
