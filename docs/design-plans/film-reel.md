# Film Reel

Reference: [film-reel.png](references/film-reel.png), supplied as Image 2 (`29a958af-Image_2.png`).

## Composition

A continuous horizontal row of large, wide landscape scenes. The selected scene occupies roughly 70% of the viewport width, centered vertically with substantial negative space above and below. Both neighbors peek in at the sides. Keep artwork rich and clean, with a dark, softly coloured ambient field drawn from the selected artwork. Place only a small tracked title and quiet Play action below the reel. **Use the shared three-island top bar**, not replacement corner controls.

## Motion / interaction

| Trigger | Motion | Duration / behavior |
|---|---|---|
| Browse | Horizontal camera-like glide; focused scene gains slight scale and neighbors recede | about 500 ms, smooth monotonic easing, shortest cyclic route |
| Ambient change | Crossfade selected-artwork colour/glow | 800 ms, independent of reel position |
| Title change | Previous text fades out; next enters slightly from below | out first, then in after midpoint; drift only a few pixels |
| Play / Enter | Selected landscape expands to fill the client area as title/action dissolve | about 420–500 ms; dispatch original launch route once at completion |
| Cancel | Return from expansion without launching | reversible, no snap |

Clicking a neighbor selects it; wheel/trackpad and arrows browse. Direct search selection enters the same reel. Momentum must settle to one exact centered game; no elastic or spring overshoot. Never auto-advance.

## Rendering and implementation

Reuse the bounded landscape decode/palette cache from Cinematic. Native GPU texture quads are sufficient; do not add a second wgpu/readback scene for a flat reel. Pure world/index-to-screen positions drive both rendering and native hit targets. Keep only the selected scene, neighboring scenes and transition assets resident; ambient colour uses cached palette data, not per-frame pixel reads. Use independent local clocks for the 500 ms spatial movement, 800 ms ambient fade and finite launch expansion.

## Acceptance

- Centering is exact after repeated, wrapped and interrupted input.
- Ambient transitions outlast reel travel without keeping the scene animating afterward.
- Titles do not display both games at full opacity or jump across the layout.
- Launch sends the selected game ID once, after the expansion; cancellation sends nothing.
- Pause freezes all clocks; reduced motion bypasses expansion while preserving launch behavior.

## Delivered

Implemented with native cached landscape textures, a cached tinted SVG glow, 500 ms monotonic reel motion, independent 800 ms ambient fade and 420 ms Play expansion. Only five cyclic candidate positions are inspected per scene frame, with duplicates removed for small collections. Cold artwork holds the previous displayed composition until decoding completes. Search, wheel, arrows, context menus and existing launch IDs/routes are retained; Escape or navigation cancels pending dispatch, and completion dispatches once.

**Revised after native feedback:** the cached glow is replaced by a genuinely changing, Balatro-inspired liquid contour field: 32 bounded native color paths at 10 Hz, with slow radial warping and rotation. The selected landscape and its neighbors extend from top to bottom behind the floating islands; title and Play overlay a lower readability gradient instead of reserving empty header/footer bands. This continuous atmospheric motion supersedes the original no-idle-motion brief for Reel, but remains subject to reduced motion and background/minimized/fullscreen suspension. Solid path fills avoid backend-specific triangle-gradient seams.

No secondary wgpu scene or GPU-to-CPU readback is needed for the flat reel. See [validation](implementation.md).
