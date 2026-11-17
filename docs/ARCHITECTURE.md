# Architecture

```
            scene thread                     render thread                 encode thread       ffmpeg
 ┌──────────────────────────────┐   ┌─────────────────────────────┐   ┌──────────────────┐   ┌────────┐
 │ construct closure            │   │ Renderer::render_batch       │   │ RGBA → YUV 4:2:0 │   │ x264 / │
 │  play/wait → Animation.apply │──►│  CPU: rayon × skia|vello     │──►│ (rayon, BT.601/  │──►│ NVENC  │
 │  updaters                    │   │  GPU: lyon → wgpu → readback │   │  BT.709)         │   │        │
 │  Camera → DisplayList        │   │  Auto: time both, keep best  │   │ GIF / PNG writer │   └────────┘
 │  dedup identical frames      │   └─────────────────────────────┘   └──────────────────┘
 └──────────────────────────────┘     bounded channels give backpressure between stages
```

## Data model (`src/mobject`, `src/world.rs`)

- A `VMobject` is a list of `Subpath`s (runs of cubic Béziers, each optionally closed),
  plus fill/stroke style, a z-index and child `VMobject`s. Groups are just `VMobject`s
  with children and no paths.
- The `World` owns every mobject in a `Vec` and hands out `MobjectId(usize)` handles. It
  also keeps the display list (which ids are on screen, in what order) and the value
  trackers. Python Manim shares mutable references instead; in Rust an arena with ids
  keeps borrows simple and makes animations and updaters plain data plus closures.
- Builder methods (`shift`, `set_fill`, …) consume and return a `VMobject`, so
  construction chains like Python's. `World::modify(id, f)` applies them in place via
  `std::mem::take`, with no clone.

## Animations (`src/animation`)

```rust
trait Animation {
    fn resolve(&mut self, w: &World);            // fill config that depends on the world (Write timing)
    fn begin(&mut self, w: &mut World);          // capture start state, display the mobject
    fn apply(&mut self, w: &mut World, a: f64);  // a = rate_func(progress)
    fn clean_up(&mut self, w: &mut World);       // restore structure, hide removers
}
```

- Most animations are a `Morph`: `begin` computes a start state and an end state, `align`
  makes them structurally identical, and `apply` interpolates point by point (straight or
  along an arc).
- **Alignment** (`mobject::align`) flattens both sides into leaves and pads the shorter
  side by repeating leaves. Within each leaf pair it pads subpath counts with degenerate
  subpaths, then subdivides curves so the counts match. This is the same strategy as
  Manim's `align_data`.
- `AnimationGroup` computes start and end times from `lag_ratio`. It begins each child
  lazily when its start time arrives, so `Succession` of two `.animate` calls on one
  mobject composes correctly. `Scene::play` wraps everything in a group.
- Frame counts follow Manim CE: `play` renders ⌈t·fps⌉ frames, `wait` renders ⌊t·fps⌋.

## Rendering (`src/camera.rs`, `src/renderer`)

- The `Camera` turns the world into a `DisplayList`: pixel-space cubic paths with resolved
  fill/stroke RGBA and stroke width in pixels (1 stroke unit = 0.01 scene units, as in
  Cairo Manim). Non-finite geometry is dropped here.
- `DisplayList` derives `PartialEq`, which is how identical frames are detected cheaply
  before any pixels are touched.
- **CPU:** each frame of a batch goes to a rayon worker. Two rasterizers are compiled in:
  tiny-skia (lowest fixed cost per frame) and vello_cpu (sparse strips + SIMD; ~3× faster
  once a frame has a couple of thousand curves, ~15% slower on light frames). `--renderer cpu`
  buckets frames by ⌊log2(curves)⌋, times both on the first 3 frames of each bucket and then
  uses the faster one for that bucket. Frames with raster images always use tiny-skia.
  Tests and `-s` snapshots use tiny-skia only, so their output is deterministic.
- **GPU:** frames are tessellated in parallel with lyon into one vertex/index buffer per
  frame (NDC coordinates and per-vertex color), drawn in one call with alpha blending in
  sRGB space into a 4× MSAA target, resolved, then copied to a mapped buffer. Chunks of 8
  frames are double-buffered, so the CPU reads back chunk *k* while the GPU renders
  *k+1*.
- **Auto:** below 1080p it uses the CPU directly. Otherwise it alternates the first
  batches between the backends, keeps the faster per-frame time, and logs the decision.

## Pipeline (`src/pipeline.rs`)

1. **Scene thread:** builds a display list per frame and collapses repeats into
   `(DisplayList, repeat_count)`. The last entry is held back so a following identical
   frame only bumps its count.
2. **Render thread:** rasterizes batches (`Renderer::render_batch_with`). Each frame is
   passed through the writer's `FramePreparer` (RGBA → YUV 4:2:0 with yuvutils-rs for MP4,
   NeuQuant palette + indices for GIF) on the thread that drew it, then the RGBA buffer
   goes back to a `FramePool`. Writers without a preparer (PNG, none) get shared RGBA frames.
3. **Encode thread:** only moves bytes: writes each prepared frame `repeat` times to ffmpeg's
   stdin, or merges repeats into one GIF frame with a longer delay.

All channels are bounded, so a slow encoder applies backpressure instead of buffering
gigabytes of frames. `plan_queues` sizes the batch and the encoder queue by bytes
(`FRAME_MEMORY_BUDGET`, 512 MiB: the batch being drawn, the one finished before it and the
encoder queue together), so 4K frames get a smaller batch than 1080p ones. Frame
buffers are recycled through `FramePool` (only the last holder of a shared buffer returns it).
With `-s` only the final display list is kept and rasterized.

## Output (`src/output.rs`)

- `find_ffmpeg`: `MANIM_FFMPEG` → `PATH` → winget's package folder (Windows).
- `Encoder::resolve`: below 1080p30 always x264. Otherwise the first of NVENC, QSV and
  AMF that passes a real one-frame test encode (ffmpeg builds list encoders whose hardware
  isn't present).
- Writers delete their file if no frames were written, so a still-only scene leaves just
  its PNG.

## The `manim-rs` script runner (`src/bin/manim-rs.rs`)

Rust can't interpret a script, so `manim-rs scene.rs` does this:

1. Writes a tiny Cargo project into the cache (`%LOCALAPPDATA%\manim-rs`, `~/Library/Caches/manim-rs`
   or `~/.cache/manim-rs`). Its `[[bin]] path` points at **your** file, so compiler errors
   reference it directly. It depends on the engine source the runner was built from (`MANIM_RS_SOURCE` overrides it);
   when that directory no longer exists, as after `cargo install --git`, it depends on the
   GitHub repository (the crate is not published to crates.io).
2. Builds it in release mode with a cache-wide `CARGO_TARGET_DIR`. Every script shares
   the compiled engine, and incremental compilation keeps rebuilds to about a second.
3. Runs the executable from the script's folder with `-o <script folder>` (unless you
   gave `-o` or `--media_dir`), so the video lands next to the script.
4. Deletes the scene executable and its debug info, including cargo's copy in `deps/`,
   whether the run succeeded or failed. Only the engine's library artifacts stay cached.

## Source map

| Path | Contents |
|---|---|
| `src/constants.rs`, `color.rs`, `bezier.rs`, `rate_functions.rs` | Math, palette, easing |
| `src/mobject/mod.rs` | `VMobject`, layout, style, `partial`, `align`, interpolation |
| `src/mobject/geometry.rs`, `graphing.rs`, `src/text.rs` | Shape library, axes and plots, text |
| `src/animation/{mod,creation,morph,movement}.rs` | Animation trait, groups, every animation |
| `src/world.rs`, `scene.rs`, `camera.rs` | Arena, timeline, display lists |
| `src/pipeline.rs` (hidden from rustdoc), `renderer/`, `output.rs`, `config.rs` | Threads, backends, writers, CLI |
| `src/guard.rs` | Panic containment for host calls |
| `src/bin/manim-rs.rs` | The script runner |

## Additions for host applications

- **World snapshots.** The world stores mobjects behind `Arc` and mutates them
  copy-on-write, so cloning a world copies pointers. With `Config::record`, each `play`/
  `wait` stores a snapshot plus a fresh clone of its animation (animations are `Clone`;
  `.animate` closures run once and their result is shared). `Scene::seek(t)` restores the
  snapshot before `t` and replays at most one play's frames without rasterizing. The result matches exported frame
  `floor(t * fps)` (`Scene::frame_at`, `ScriptHost::frame_at`).
- **Frame stepping.** `play`/`wait` are built on `begin_play`/`begin_wait`, `step` and
  `end_frame`, so a host (the script engine) can run its own per-frame code between them
  without re-entering the scene.
- **Layers.** Besides the z-sorted scene layer the world has a background layer (theme
  patterns; its display list is shared by `Arc` across frames and the CPU rasterizer caches
  it rasterized) and an overlay layer (watermarks). `clear()` and layout checks skip both.
- **Images.** Image mobjects carry pixels and three anchor points that move with every
  transform; the camera turns them into an affine image draw. The CPU rasterizer draws them;
  the GPU path hands frames that contain images to the CPU.
- **Script host** (`src/script`). TypeScript is type-stripped by oxc (with a line map for
  error messages) and evaluated by QuickJS. JS mobjects are handles into a registry
  (detached values, world ids, or views into a group's children); arguments cross as JSON.
  The `manim` JS module (`prelude.js`) builds the Manim-style API on top.
