# Cinematic

The Gallery / power-saving checkpoint is `e44a073`. Cinematic adds another persisted interface style without replacing Classic, Editorial or Gallery, and without modifying the user's saved authored palette or library layout.

## Presentation

`crates/desktop/src/cinematic.rs` owns composition, artwork-derived colour and finite animation state. `interface/cinematic.rs` supplies native controls, selection, launch routing and the connected library snapshot. Home includes all installed, non-hidden games from all providers, ordered by recent launch. The filmstrip shows five positions at rest and just their immediate moving neighbors during navigation; it is not five fixed games. Search results for installed games return to the cinematic home. Library remains a secondary, explicitly opened full browser.

Landscape artwork fills the viewport, with no persistent metadata, platform badges or sidebar. A subtle lower gradient gives the lower-left title and small Continue/Play action contrast. Continue is the standard launch command for a previously launched game, not a save-state/resume API. Right-click exposes existing per-game operations; double-click or Enter launches. **Cinematic uses the exact same shared three-island toolbar as Classic, Editorial and Gallery on every screen**—there is no design-specific toolbar branch. Navigation, search, Settings, customization, refresh, Add Game, activity, native controls and macOS traffic-light clearance retain their common geometry and behavior.

## Rendering and power

GPUI composites cached artwork textures directly. No separate wgpu device, readback, CPU frame generation, real-time blur or shader animation is needed. Landscapes decode once on the asset worker and are bounded to 1920×1200; the worker also extracts a small hue-weighted palette. At most two decodes run concurrently during rapid navigation; completion explicitly invalidates the launcher, so cold artwork does not wait for another input event. Missing landscapes use a one-time softened/dimmed 960×600 cover, and missing/unreadable images use a neutral surface. Authored artwork is never overwritten.

Crossfades last 480 ms and retarget from the currently displayed blend. Normal transitions composite two full-screen textures; rapid scrubbing is bounded to three. Source-over alpha is computed from normalized mixture weights, avoiding dark flashes. The same blend drives selection glow and action colour; it never restyles the shared toolbar. The artwork cache retains at most four bounded images, retiring unused atlas entries at frame boundaries and pinning the current transition's images. Portraits use the existing thumbnail cache.

There is no auto-advance or perpetual frame request. Identical settled scenes are static. The shared background/minimized/fullscreen policy freezes crossfades and filmstrip movement and prevents new artwork preparation while suspended. Resume preserves local animation time. OS reduced motion replaces transitions with immediate updates. No additional fullscreen probes or timers are introduced.

## Validation

`cargo test --locked --workspace` covers style persistence, source-independent installed scope, layout bounds, palette extraction, finite/interruptible crossfades, pause/resume and native selection, pointer launch, keyboard launch, search, Library access, modal click-through protection and design switching. Native Windows checks additionally verify actual artwork presentation, chrome, navigation and idle GPU activity. macOS native behavior still needs on-device validation.
