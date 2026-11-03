# Gallery rendering

The 2D Gallery checkpoint is commit `f48ac95`. It includes five fixed recent-game dots, the full Library's position/total indicator, shared toolbar islands, safe reflection texture sampling and Xbox GDK discovery.

## Scene

`crates/desktop/src/gallery_gpu/scene.rs` describes the perspective camera and world-space artwork planes. A low viewing angle places them on a common floor. Projection of their corners supplies GPUI's native selection/context-menu hit targets; the hit regions use the **displayed frame's** geometry rather than a newer pending animation pose.

`gallery.wgsl` performs analytic ray/plane intersections on the GPU. The scene genuinely uses world-space positions, surface normals, viewing direction and nearest-surface visibility; it is not a collection of scaled UI images. The simple geometry needs neither a mesh importer nor hardware ray-tracing extensions. The floor is level, imperfect polished concrete: broad, irregular patches change both reflection roughness and reflectivity, while smaller trowel marks, fine pitting and albedo wear break up the light. More polished patches retain clearer, brighter reflections; worn patches scatter and absorb them. A four-ray soft reflection cone, artwork mip sampling, grazing-angle Fresnel response, contact shading and localized artwork-tinted illumination keep the finish grounded. No rippled normals: the texture never distorts reflections like water. Opaque artwork pixels return before floor shading, and poster bases are precomputed on the CPU. These are bounded approximations, not full path-traced global illumination. Original artwork is never modified.

`material.rs` bakes a deterministic, seamlessly tiling 256×256 RGBA material once per device. Its channels encode roughness, reflectivity, micro-grain/pitting and albedo wear. The full mip chain is about 341 KiB; floor fragments use three cached material samples instead of evaluating procedural noise per frame. Reflections fade in coherence with distance from their contact. Feathered edges blend the two nearest reflected artworks rather than cutting dark, opaque trapezoids into the light; each reflection ray samples at most two artwork textures. Material tests check that matte and polished regions are spatially coherent and that macro channels tile without seams.

GPUI still renders the toolbar, selected title, Play button, fixed Recents dots or Library position indicator, menus and dialogs. Keyboard commands, launch routing, accessible buttons and reduced motion retain their existing controllers.

## Renderer and bridge

`renderer.rs` owns a wgpu device, scene pipeline, an eight-layer 512×768 artwork array with mipmaps, and reusable offscreen/readback targets. Frames are capped at 1600×1000 pixels; the UI itself remains full native DPI. Original images are decoded and uploaded only when a texture-cache entry changes.

`mod.rs` starts the worker only when Gallery first requests a nonempty scene. Its mailbox holds **one latest pending scene**, and its completion channel holds one frame. Superseded poses do not accumulate. Identical settled scenes do not submit new GPU work. GPU waits are bounded and happen on the worker, never inside GPUI rendering.

## Suspension and resource use

**Settings → Performance → Pause visual effects in background** is saved and defaults on, including for older settings files. It suspends Gallery rendering, Editorial ambient motion and Classic automatic browsing while unfocused. On Windows and macOS, minimized/hidden windows and another foreground fullscreen app always suspend effects, even if the setting is off. Fullscreen detection uses foreground client/monitor geometry on Windows and window metadata on macOS; it does not capture the screen or scan processes. Ordinary maximized Windows client areas and the desktop shell are excluded. Other platforms use focus-based suspension; native fullscreen/minimized detection is not implemented there.

Native geometry probes are cached for one second, with an immediate refresh on activation changes. While paused, the shared timer wakes at most once per second and does not request animation frames. The Gallery worker sleeps on a condition variable, clears queued work, rejects stale completion generations and releases offscreen/readback targets while retaining bounded artwork textures and the last displayed still. Rendering checks cancellation before artwork preparation, before submission and after readback. Already-submitted GPU work may finish; no further scene work is submitted while paused. Animation clocks freeze, so resuming does not spend background time. Scene submissions are capped at 60 Hz and stop entirely once settled.

The current bridge is **GPU → CPU readback → GPUI image upload**. It is a practical first implementation across GPUI's Direct3D 11 and Metal backends, but **not zero-copy**. At high resolutions/frame rates this adds bandwidth and latency. A future shared-texture/custom-render-pass bridge should replace only the presentation boundary; the scene description, shaders, native controls and input geometry need not be rewritten. Measure native transition performance before claiming a frame-rate target.

Scene frames have a sampling gutter; source UVs remain inside that gutter to prevent neighboring GPUI atlas textures bleeding into image edges. Replaced GPU image tiles are retired only at a frame boundary. If device initialization, shader validation or rendering fails, diagnostics record the error and the existing 2D presentation remains usable.

## Validation

```powershell
cargo test --locked --workspace
cargo test --locked -p startpoint gallery_gpu::renderer::tests::renders_real_gpu_scene_and_floor -- --ignored --nocapture
```

The ordinary suite validates projection, collection scope, native input, pause policy, fullscreen geometry, preference migration and mailbox cancellation without requiring a graphics adapter. The explicitly run smoke test initializes a real wgpu adapter, compiles WGSL, renders, reads back actual frames, checks cancellation and recreates released targets. It prints median/p95 timings after warm-up (readback and CPU copy included, GPUI upload/presentation excluded). Set `STARTPOINT_GPU_CAPTURE` to an image path to save its output. No network/store credentials are needed for this test. macOS native detection requires separate on-device validation.
