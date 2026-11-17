# Contributing

## Build and test

```bash
cargo build --release --examples
cargo test --release                                  # full suite
cargo test --release -- --ignored                     # + end-to-end manim-rs runner test
cargo test --release --no-default-features            # without GPU / text
MANIM_FUZZ=20 cargo test --release --test fuzz        # 20× randomized soak
cargo clippy --all-targets --all-features --locked -- -D warnings && cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps   # every public item needs a doc comment
```

The tests skip themselves (with a message) when an optional dependency is missing: no
GPU adapter, no system fonts, or no ffmpeg.

## Test suites

| File | Covers |
|---|---|
| `src/**` unit tests | Bézier math, rate functions, alignment, config parsing, YUV coefficients |
| `tests/animations.rs` | End state and timing of every animation; groups, `Succession`, lag; updaters, trackers, `always_redraw`; frame counts matching Manim CE |
| `tests/mobjects.rs` | Shape dimensions and defaults, layout helpers, styles, partials, alignment, axes, text layout, colors, rate functions |
| `tests/rendering.rs` | Pixel-exact fills, blending, even-odd holes, z-order, determinism, NaN/∞ robustness, odd sizes, CPU/GPU parity, pipelined GPU batches |
| `tests/output.rs` | Pipeline dedup across batch boundaries, GIF timing, PNG sequences, MP4 frame counts via ffprobe, YUV color round trip, still-only scenes, `-o` output, and (ignored by default) `manim-rs` leaving only the script and video |
| `tests/fuzz.rs` | Random shapes, morphs and animation timelines must never panic or produce non-finite geometry |

## Adding an animation

1. Implement `Animation` (use the `impl_config!()` macro for the config accessors).
2. Most visual animations should be a `Morph` constructor in `src/animation/morph.rs`:
   produce a start and end `VMobject`, and alignment and interpolation come for free.
3. Restore the mobject's structure in `clean_up`, and hide it if it's a "remover".
4. Add an end-state test to `tests/animations.rs` and a case in `tests/fuzz.rs`.

## Style

`cargo fmt` (120 columns, see `rustfmt.toml`), a clean `cargo clippy --all-targets --all-features`, and a doc comment on every public item (`missing_docs` is a lint here).
The MSRV is `rust-version` in `Cargo.toml` (set by the newest dependency; CI checks it). Items public only for the tests or the CLI are `#[doc(hidden)]` and not part of the supported API.
