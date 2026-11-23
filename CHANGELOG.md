# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to follow
[Semantic Versioning](https://semver.org/). The crate is distributed from GitHub only
(`publish = false`); it is not published to crates.io.

## [Unreleased]

### Added

- Camera frame animation: `MoveCamera` (and `World::frame`, `Camera::live_frame`) zooms and moves
  the frame, including a "zoom on a point while it glides to an anchor" mode. The frame lives in the
  world, so seeking restores it. `VMobject::pin` keeps a mobject in the base frame;
  `VMobject::clip_to` clips it to a rectangle. Scripts: `MoveCamera({...})`, `pin()`, `clipTo()`.
- `effects` module (`Effects`, `Dash`, `Gradient`, `Shadow`): dash patterns cut by arc length at
  draw time with an animatable phase (`set_dash`), linear gradient fills (`set_gradient`), soft drop
  shadows (`set_shadow`) and background blur (`set_backdrop_blur`). `DrawPath` gained `gradient`,
  `shadow`, `backdrop_blur` and `clip` (and `Default`); `DisplayList::needs_reference` says when a
  frame needs them. Such frames are drawn by tiny-skia on every renderer.
- Script methods `setDash`, `setGradient`, `setShadow`, `setBackdropBlur`, `pin`, `clipTo` (and
  their `clear` forms), all usable in `.animate`.
- `Config::x264` ([`X264Options`](src/output.rs): preset and CRF), with `--x264-preset` and
  `--crf` on the command line and `output::create_writer_with` / `FfmpegWriter::with_options`.
  Defaults stay `fast` and CRF 18.
- `Config::script_cancel` and `Config::script_deadline`: a second cancel flag and deadline
  honoured next to `Config::cancel` / `Config::deadline`. `ScriptHost` uses them, so a host's
  flags and the script's no longer overwrite each other.
- `World::try_get`, `try_get_mut`, `try_set`, `try_value`, `try_set_value` and
  `World::contains`: fallible access that reports a stale or foreign id as an error instead of
  panicking.
- `Animation::validate` (default method): an animation can reject a bad world or its own
  configuration; `Scene::play` records the error and plays nothing.
- New default methods on `FrameWriter` (`preparer`, `write_prepared`) and `Renderer`
  (`render_batch_with`, `use_pool`), so converting frames (RGBA to YUV, GIF palettes) runs on
  the render threads and frame buffers are recycled. Existing implementations keep working.
- Resource limits for hosts: nesting depth of 256 (`mobject::MAX_NESTING`), 1,000,000 curves
  per script-built mobject, a 512 MiB JS heap by default (`script::DEFAULT_MEMORY_LIMIT`),
  Typst markup of at most 64 KiB and a 30 s compile cap that also stops on cancel, and a
  16384-pixel frame side limit. Exceeding one is an error.
- Panic containment (`guard`): panics in scene code, animations and script callbacks become
  errors, without printing to stderr.
- Frame memory budget: the batch size and encoder queue are sized by bytes
  (about 512 MiB in flight), so 4K renders use smaller batches.
- Crate-level documentation with an example, doc comments on every public item
  (`missing_docs` is now a lint), a `[lints]` table, an MSRV job and a rustdoc job in CI,
  and this changelog.
- `MANIM_RS_SOURCE` documented as the override for the source the `.rs` runner builds against.

### Changed

- **Behaviour change for hosts:** `Scene::seek`, `Scene::frame_at`, `ScriptHost::seek` and
  `ScriptHost::frame_at` now show exactly exported frame `floor(t * fps)`. Frame `k` is the
  state after `k + 1` steps, so `frame_at(k / fps)` matches video frame `k`. Earlier versions
  could differ by a frame.
- Public enums that are expected to grow are `#[non_exhaustive]`: `OutputFormat`, `Encoder`,
  `RendererKind`, `renderer::Frame`, `Quality`, `SnapshotAt`, `ProgressMode`, `AudioSource`,
  `CodeBackground`, `TokenKind`, `Pattern`, `TitlePosition`, `BoxStyle` and `TextRole`.
  Matching on them from another crate needs a `_` arm.
- `HostOptions::timeout` is a per-call limit (script load, `run`/`render`, `seek`,
  `frame_at`), restarting on every call. It does not cover `finish` (encoder flush, audio mux);
  stop that with `ScriptHost::cancel_handle`.
- `Pipeline::new` returns `Result`. `RasterImage::from_rgba` and `ImageMobject::from_rgba`
  return `Result` and reject a pixel buffer of the wrong length.
- The `dist` profile no longer sets `panic = "abort"`. Hosts must use unwinding for panic
  containment to work.
- Minimum supported Rust version is 1.97 (was documented as 1.89; the `script` feature's
  `oxc` dependency requires 1.97).
- `png` 0.17 to 0.18 and `gif` 0.13 to 0.14, matching `image`; `tiny-skia` is built without
  its own PNG support, so one `png` and one `gif` remain in the dependency tree.
- The `manim-rs` runner builds `.rs` scenes against the source it was built from, or the
  GitHub repository when that directory no longer exists.
- Internals that were public only for tests and the CLI are hidden from the docs:
  the `pipeline` module (with `Stats`, `Finished`), `renderer::cpu::rasterize`,
  `output::rgba_to_yuv420` and `YuvMatrix`. `renderer::vello`, `renderer::cpu::rasterize_pooled`,
  `output::{YuvPreparer, GifPreparer, NullWriter}` and the `FfmpegWriter::encoder` field are
  no longer public.

### Fixed

- Script `Axes.c2p` / `p2c` ignored scaling of the axes after creation; they now follow scale as well as
  moves.
- Script `group.get(i)` made a new handle on every call; it now returns the same object for the same member.
- Hangs, panics and lost output on hostile or broken input: non-finite geometry, deeply nested
  groups, huge frames, runaway scripts and Typst compiles, and stale ids now end in an error,
  and a failed render removes its partial output.
- A played member of an off-screen group stays on screen.
- A CI race between two script tests.
