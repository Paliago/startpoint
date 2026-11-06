# Artwork launcher designs — reference and implementation plans

> Historical design archive. These plans describe the retired frontend and are not the current feature list or implementation instructions. The sole Rune application supports Classic and Infinite Wall; see [desktop architecture](../desktop.md). Original reference images are retained for future design work.

Saved before implementation. These are three **additional selectable designs**, not replacements for Classic, Editorial, Gallery or Cinematic.

## Non-negotiable shared behavior

- **Keep the existing three-island top bar unchanged in every design.** Reference corner icons describe visual restraint, not permission to replace the toolbar. Use its existing Search, Library, Settings and management actions.
- Use installed, non-hidden games from all connected providers; preserve game IDs, existing launch routes/overrides, context menus and accessible keyboard input. Never invent games or save-state integration.
- Preserve the original reference PNGs below. They are design documentation, not bundled launcher artwork. Actual scenes use the user's game art, with honest missing-art fallbacks.
- Use cached native GPUI image compositing and geometric animation where sufficient. wgpu is available for effects that genuinely need it, but do not introduce readback or per-frame image processing for simple transitions.
- Finite motion is interruptible and uses local, pausable time. No bounce, elastic snapping, exaggerated tilt or perpetual motion in Wall/Reel. Reduced motion bypasses spatial animations. Minimized/other-fullscreen protection and the saved background-pause setting apply to all new animation clocks.
- Bounded decode concurrency, texture caches and visible geometry; virtualize the wall. No work proportional to library size on every animation frame. Never evict images used by the current displayed frame.
- Stage launch animations separately from actual launch dispatch. Dispatch once, to the original game ID, only if the launch is still valid; Escape cancels pending transitions. Errors must not leave the UI locked or an expanded scene permanently covering controls.

## Plans and original references

1. [Infinite Wall](infinite-wall.md) — [original PNG](references/infinite-wall.png)
2. [Film Reel](film-reel.md) — [original PNG](references/film-reel.png)
3. [Art Book](art-book.md) — [original PNG](references/art-book.png)

## Delivery / verification

Implement shared bounded motion/layout primitives, then Wall, Reel and Art Book. Add persistence/migration and design-switching tests; pointer/keyboard launch, preview reversal, scrolling bounds, pause/resume and reduced-motion regressions. Validate actual Windows screenshots for all three, native toolbar controls, settled/minimized GPU behavior and updater-feature compilation. Rebuild `target/release/startpoint.exe`. Record implementation deviations rather than silently substituting a simpler interaction.

Art Book's final request about slowly moving wallpaper ripples is interpreted as **very slow, tiny movement of the artwork's feathered contours**, not animated paper grain, page curls or cloth waves. Paper remains stationary. This effect pauses in background and is removed under reduced motion.

## Implementation status

### Requested revision

- Remove scene-level reserved header strips: the existing three islands float over full-bleed content, with no full-width backing.
- Wall: keep unhovered covers consistently dark; expand into a landscape hero with real metadata/description and Play, crossfading from the anchored portrait and reversing on close.
- Reel: remove empty top/bottom bands, overlay title/Play on the full-height reel, and add a slow Balatro-inspired marbled background. Continuous motion remains reduced-motion/pause-aware.
- Book: derive paper, ink and island colors from the selected artwork; use a live feathered tear mask rather than moving a pre-baked image; center the crop inside the visible artwork region, not the window. Paper grain remains stationary.
- Verify the revised executable, then commit all three designs and their saved plans/references and push the current branch.

All three designs are implemented as persisted interface styles. The shared three-island toolbar is retained, including native window controls, Search, Settings and normal Library navigation. See [implementation and validation](implementation.md) for rendering choices, measured resource use and limitations.
