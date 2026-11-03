# Artwork scene implementation and validation

## Delivery

Select **Infinite Wall**, **Film Reel** or **Art Book** in **Settings → Interface style** (also available in Customize). They add to, rather than replace, the existing designs. All three retain the exact shared toolbar layout and use installed, non-hidden games from all connected providers independently of Library filters. Launch commands retain original game IDs and existing routes/overrides.

Plans and exact original PNG copies were saved before implementation. SHA-256 comparisons against all three uploaded files pass. The reference images are documentation only, not runtime game artwork.

## Rendering and power

- Native GPUI GPU compositing handles cached images, opacity, translation, scaling, clipping and bounded vector masks. No additional wgpu worker, frame readback, per-frame image filtering or moving paper texture is introduced. The three toolbar islands float over full-bleed content rather than a reserved header strip.
- Collection ordering and ID positions are cached against snapshot `Arc` identity. The Wall renders visible rows plus overscan; Reel checks at most five cyclic neighboring slots. Same-sized collection reorderings preserve selected identity and correct centering.
- The scene derivative cache retains six recent entries plus assets pinned by the displayed frame, with at most two concurrent decodes. Original artwork remains unchanged. Wall cells use the existing native image cache.
- Local animation clocks freeze under the centralized background/minimized/other-fullscreen policy. Wall stops when settled. Reel's 32-path marbling and Book's 20-layer changing torn-edge mask share the existing 100 ms timer and become static under reduced motion. Book artwork/paper grain do not move; the seam itself changes shape.
- Book's paper, ink and island palette follows cached artwork hue/brightness without overwriting the saved app theme; scoped chrome colors clear on navigation. Native cropping is centered in the left-hand artwork region. Wall's expanded view crossfades to a landscape hero and displays real metadata/description instead of stretching a cover throughout the detail view.
- Missing artwork uses an honest cover/title fallback or empty paper composition; no generated replacement game artwork. Cold destinations hold the previous composition until ready instead of wiping to an empty background.

## Automated checks

Validated on Windows:

```powershell
cargo check --locked --workspace
cargo test --locked --workspace
cargo check --locked --workspace --features self-update
cargo build --locked --release
cargo fmt --all -- --check
git diff --check
```

Workspace tests: **155 passed, 0 failed, 3 intentionally ignored**. Coverage includes persisted style round-trips, the shared-toolbar invariant, pause/reverse/reduced-motion clocks, bounded full-bleed geometry, stable Wall shading and interrupted hover, reversible hero blending, live material tessellation/continuity, image-palette contrast, snapshot cache scope/reordering, native preview reversal, non-bubbling Wall Play, once-only Reel dispatch/cancellation, Book overview selection and scoped chrome cleanup on Settings navigation. The ignored checks are one opt-in wgpu adapter test and two live catalogue tests. Builds report the existing `proc-macro-error2` future-compatibility notice.

## Native review

The Windows example library is used for native artwork/input review; its launch commands are simulated and do not start installed games. Verified shared toolbar, all three style selectors, Wall hover/preview/shared Back, Reel centered neighbors and Play expansion, and Book overview selection. Native macOS execution has not been validated on this Windows host.

The initial optimized `target/release/startpoint.exe` was reviewed natively at a 1296×828 outer window size, using an NVIDIA GeForce RTX 4070 SUPER. Wall hover, full-screen cover expansion, shared Back reversal and wheel settling were exercised. Reel neighbor selection, the centered destination and full-screen simulated Play expansion were captured. Book overview selection returned to the correct Elden Ring page after a cold decode and after minimize/restore.

**Pre-revision baseline only:** ten one-second Windows `GPU Engine` counter samples per state, filtered to the release process and summed across its engines. These figures predate the requested continuous Reel marbling and live Book tear, and are not presented as measurements of those revised effects:

| State | Mean GPU utilization | Maximum sample |
|---|---:|---:|
| Infinite Wall, foreground and settled | 0% | 0% |
| Film Reel, foreground and settled | 0% | 0% |
| Art Book, foreground, 10 Hz contour drift | 0.895% | 1.140% |
| Art Book, minimized | 0% | 0% |

The foreground states were checked against the native foreground window; minimization was checked with `IsIconic`. These are short, machine-specific observations, not cross-device frame-rate guarantees or measurements of active transitions. Background/fullscreen suspension policy has automated coverage; a separate foreground-transfer probe did not successfully move native focus and is **not** treated as a valid background resource measurement. macOS still requires on-device validation.

Optimized native captures were saved during review as `infinite-wall-release.png`, `infinite-wall-release-preview.png`, `infinite-wall-release-return.png`, `infinite-wall-release-scrolled.png`, `film-reel-release-next.png`, `film-reel-release-launch.png`, `art-book-release.png`, `art-book-release-overview.png`, and `art-book-release-selection.png` in the local OpenCode temporary directory. Original reference PNGs remain unchanged in this directory's `references/` folder.

## Revised native validation

The revised optimized executable was reviewed at the same window size/GPU. Confirmed artwork behind all three toolbar gaps, full-height Reel frames and overlay controls, consistently dark Wall covers, Hollow Knight's landscape detail view with real metadata/description, the native artwork chooser, and both Cyberpunk's dark Book palette and Elden Ring's light Book palette. The chooser now opens its Background category directly; updated editor metadata/hero paths are reflected in an already-open Wall detail view.

Two settled Book captures were compared without navigating or moving the artwork. In the seam region, **8,252 of 13,400 sampled pixels changed**; in an interior artwork region, **0 of 33,400 changed**. This verifies locally changing mask geometry rather than translating the whole artwork. Paper grain is stationary.

Fresh ten-second process-specific GPU counter samples for the revised living materials:

| Revised state | Mean GPU utilization | Maximum sample |
|---|---:|---:|
| Film Reel, foreground marbling | 3.808% | 4.110% |
| Film Reel, minimized | 0% | 0% |
| Art Book, foreground live tear | 1.937% | 2.861% |
| Art Book, minimized | 0% | 0% |

Native minimize/restore was also exercised; motion resumed after restoration. These observations include all work attributed to the process, vary with GPU clocks/system load and are not a fixed utilization or frame-rate guarantee. Revised captures use the `revised-wall-*`, `revised-reel-*`, `revised-book-*` and `book-live-*` prefixes in the local OpenCode temporary directory.
