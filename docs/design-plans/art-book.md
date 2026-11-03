# Art Book

Reference: [art-book.png](references/art-book.png), supplied as Image 3 (`0aedc880-Image_3.png`).

## Composition

Warm ivory paper, generous whitespace and elegant restrained serif type. Dramatically cropped artwork occupies about two-thirds of the viewport, feathering irregularly into the paper rather than ending at a hard rectangular card edge. Place the title and quiet Play action in the remaining right-hand space, with a small position/collection-size index near the lower right. Keep the paper background and its fine texture stationary. **Keep the unchanged shared top bar** with its normal search/collection access.

Each page follows the selected game, not a separate dashboard. Prefer clean landscape art; fall back to a cover or a neutral paper composition without pretending unavailable artwork exists. Crop and blend treatments are cached derivatives, never changes to original artwork.

## Motion / interaction

| Trigger | Motion | Duration / behavior |
|---|---|---|
| Browse | Artwork moves sideways while a soft feathered wipe reveals the next composition | 650 ms total, horizontal and monotonic |
| Text change | Fade title and Play out first; next text arrives after artwork nearly settles | staged within the 650 ms transition |
| Play hover/focus | Fine underline slowly extends from the text baseline | about 250–300 ms, no button plate or text movement |
| Collection overview | Current composition shrinks slightly into a gallery of games | about 400 ms, reversible; preserve selection and return to its composition |
| Artwork contour atmosphere | Optional tiny slow horizontal drift of feathered artwork contours | very low amplitude/rate; paper grain never moves; foreground only |

The last atmospheric request is **not** permission for rippling paper, page curls, elastic movement or a constantly deforming whole image. Reduced motion gives static compositions, instantaneous page changes and immediate underline/overview states. Background/minimized/fullscreen suspension stops contour motion and local clocks.

## Rendering and implementation

Start with GPUI native image compositing and a worker-baked irregular alpha edge on a bounded landscape derivative. Generate stationary paper texture once and cache it. Animate crop/translation, mask reveal and text opacity on the GPU without regenerating pixels. If native composition cannot achieve the feathered wipe economically, isolate that mask as a small wgpu effect, retaining native controls and bounded presentation; do not make the entire page a readback animation by default.

Overview is a native virtualized collection layer over the shrinking composition, not a new sidebar. Game selection closes overview back into the same art-book presentation. Existing context menus and real launch routing remain intact.

## Acceptance

- Art remains about two-thirds of the composition, with readable title/action at narrow and wide desktop sizes.
- Soft edge is irregular and painterly, without decorative card frames or moving paper noise.
- Old text is gone before new text arrives; full transition is about 650 ms.
- Overview entry/exit preserves selected game and keyboard focus; no click-through launch.
- Ambient contour motion is tiny, cheap, pause-aware and removed under reduced motion; document its measured resource cost.

## Delivered

Implemented with a worker-baked irregular alpha feather, a stationary cached paper texture, italic serif typography, a 650 ms horizontal reveal with staged text, a 280 ms underline, and a 400 ms shrinking-composition overview. The collection index opens that overview; the shared Library tab deliberately retains ordinary Library navigation. Overview selection returns to the chosen page without launching. Cold destinations retain the previous composition until decoded.

The atmospheric interpretation is a **1.5 px, slow horizontal drift of the feathered artwork**, sampled at 10 Hz by the existing shared timer. It is not a deforming wallpaper simulation; paper grain stays stationary. Reduced motion removes the drift, and background/minimized/fullscreen suspension freezes it. See [validation and measured cost](implementation.md).

**Revised after native feedback:** the earlier baked feather and whole-image drift are replaced by a live vector tear mask. Twenty incremental-coverage paths form a smooth feather; each part of the seam moves differently over time, while the cached image and paper grain remain stationary. The image is no longer pre-cropped to a fixed portrait-like aspect ratio: native cropping is centered in the left-hand 70% artwork region, not at the window center. Paper, ink, action and toolbar-island colors follow the selected image's dominant hue and brightness, with contrast-checked actions and automatic light/dark paper. These scene colors do not overwrite the saved application theme and clear on Settings/Library navigation. Artwork extends behind the floating islands with no reserved header strip.
