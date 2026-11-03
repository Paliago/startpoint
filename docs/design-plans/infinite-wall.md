# Infinite Wall

Reference: [infinite-wall.png](references/infinite-wall.png), supplied as Image 1 (`174098a2-Image_1.png`).

## Composition

Dense portrait artwork fills the client area below the unchanged shared top bar. Narrow charcoal gutters, consistent 2:3 framing, small corner radius, no captions, platform badges, statistics or dashboard panels. Cover size adapts to window width; maintain readable hit targets at minimum window size. The hovered/keyboard-focused card is painted above its neighbors, never causing grid reflow. A small Play action is overlaid inside it.

The wall contains the actual installed collection, not a five-game shortlist and not fabricated infinite duplicates. Virtual rows and overscan make large libraries feel continuous without loading every cover.

## Motion / interaction

| Trigger | Motion | Duration / behavior |
|---|---|---|
| Hover/focus | Lift about 6 px and scale to roughly 1.06; nearby cards dim softly | 200 ms, eased, retargetable |
| Play reveal | Opacity only; no artwork/layout shift | 140–200 ms |
| Scroll | Immediate input response with short, decaying residual movement | bounded momentum; no overscroll bounce; stop at real collection bounds |
| Cover selection | Same cover expands from its displayed cell into an artwork-focused viewport view | about 420 ms; preserve center crop/focal anchoring |
| Close / Escape | Reverse to the same game cell and scroll position | about 420 ms, reversible even mid-transition |

Play must not bubble into selection/preview. Right-click retains native game options. Keyboard arrows navigate the grid, Enter launches, and Escape closes the expanded view before dismissing unrelated UI. Scrolling is held while expanded. Resize and library refresh must retain game identity or safely close a removed selection.

## Rendering and implementation

Native GPUI artwork textures, translated/scaled rectangles and opacity; no wgpu needed for the initial wall. Pure layout computes column count, row range and hit geometry. Cache only visible rows plus overscan. A finite scalar expansion interpolates the source rectangle to the viewport; use the same source image during the anchored part of the transition. A separate small Play button remains native and accessible.

## Acceptance

- Hover never relays out adjacent cards; fade does not shift art.
- Large-library rendering remains viewport-bounded.
- Mid-animation Escape reverses continuously and restores the exact cell/scroll anchor.
- No launch on ordinary cover selection; no double launch from Play bubbling.
- No idle frame requests after hover/scroll/preview finishes; pause and reduced-motion behavior are covered by tests.

## Delivered

Implemented with native GPU image composition, viewport-bounded rows, cached installed-collection ordering, 200 ms hover emphasis, a 160 ms bounded scroll settle, and 420 ms reversible expansion. The preview retains its source cover and origin; no landscape-image swap breaks the anchored movement. Up/Down moves one grid row, Left/Right one game, Enter launches, and Escape/shared Back reverses the preview. Play does not bubble into preview selection.

**Revised after native feedback:** artwork now continues behind the floating toolbar islands, with no empty header strip. Unhovered covers retain a constant 26% dark overlay; the focused cover eases to 4%, and interrupted hover fades preserve their displayed strength. During expansion the anchored portrait crossfades into the game's landscape hero (the same artwork type as Reel); closing reverses the blend and geometry. The expanded view includes the real title, provider, developer, genres, last-launched date and available description, plus Play and access to the existing artwork chooser. No information is invented for missing fields.

"Infinite" describes continuous browsing of the real collection: the wall has finite bounds, not repeated fabricated games. See [validation](implementation.md).
