//! # manim (Rust)
//!
//! A Rust reimplementation of the [Manim](https://github.com/ManimCommunity/manim)
//! animation engine: describe a scene with mobjects and animations, and render it to
//! MP4, GIF or PNG frames. No Python, Cairo or Pango.
//!
//! - **Scenes** are built through [`Scene`]: `add` mobjects (the [`mobject`] module) and
//!   `play` [`animation`]s. Mobjects are plain values; a scene stores them and hands back
//!   [`MobjectId`](world::MobjectId) handles.
//! - **Two backends**: a CPU rasterizer (tiny-skia and vello_cpu) that renders frames in
//!   parallel across cores, and a wgpu GPU renderer. [`renderer::RendererKind::Auto`]
//!   picks by measurement. Static stretches are rasterized once and repeated.
//! - **Output** goes through [`output`]: MP4 needs an `ffmpeg` executable (found on `PATH`,
//!   or set with `MANIM_FFMPEG` or [`Config::ffmpeg`](config::Config::ffmpeg)); GIF and PNG
//!   sequences are written in pure Rust.
//! - **Hosts**: with the `script` feature, [`script::ScriptHost`] runs TypeScript or
//!   JavaScript scenes in an embedded engine, with timeouts, a memory limit, cancellation,
//!   seeking ([`script::ScriptHost::frame_at`]) and structured [`events`]. The `manim-rs`
//!   binary renders script files and, by compiling them on the fly, Rust scene files.
//!
//! ## Example
//!
//! Render a scene without writing anything (`dry_run` runs the scene logic only, so this
//! needs no ffmpeg):
//!
//! ```
//! use manim::prelude::*;
//!
//! fn main() -> anyhow::Result<()> {
//!     let config = Config { dry_run: true, ..Config::default() };
//!     manim::render_with(&config, "SquareToCircle", |s| {
//!         let square = s.add(Square::new(2.0).rotate(PI / 4.0));
//!         s.play(Create::new(square));
//!         s.play(Transform::new(square, Circle::new(1.0).set_fill(PINK, 0.5)));
//!         s.play(FadeOut::new(square));
//!     })
//! }
//! ```
//!
//! A scene binary calls [`render`] instead, which reads the options (`-ql`, `--renderer`,
//! `-o`, ...) from the command line and writes `media/videos/<scene>/...`:
//!
//! ```no_run
//! use manim::prelude::*;
//!
//! fn main() -> anyhow::Result<()> {
//!     manim::render("SquareToCircle", |s| {
//!         let square = s.add(Square::new(2.0));
//!         s.play(Create::new(square));
//!     })
//! }
//! ```
//!
//! ## Cargo features
//!
//! All are on by default; each one drops its dependencies when disabled.
//!
//! | feature | adds |
//! |---|---|
//! | `gpu` | the wgpu renderer |
//! | `vello` | the vello_cpu rasterizer (chosen per frame against tiny-skia) |
//! | `text` | `Text`, `Code`, `Table`, `Matrix` from system font outlines |
//! | `svg` | `SVGMobject` |
//! | `math` | `MathTex` / `Typst` equations (embedded Typst compiler) |
//! | `script` | the TypeScript / JavaScript scene host |
//!
//! ## Notes for embedding
//!
//! The minimum supported Rust version is 1.97 (set by the `script` feature's dependencies).
//! Errors are [`anyhow::Error`]s. Rendering is guarded against panics from scene code, but
//! that relies on unwinding, so do not build a host with `panic = "abort"`.

pub mod animation;
pub mod audio;
pub mod bezier;
pub mod camera;
pub mod check;
pub mod color;
pub mod config;
pub mod constants;
pub mod defaults;
pub mod events;
mod guard;
pub mod image;
#[cfg(feature = "math")]
pub mod math;
pub mod mobject;
pub mod output;
#[doc(hidden)]
pub mod pipeline;
pub mod rate_functions;
pub mod renderer;
pub mod scene;
#[cfg(feature = "script")]
pub mod script;
#[cfg(feature = "svg")]
pub mod svg;
#[cfg(feature = "text")]
pub mod text;
pub mod theme;
pub mod world;

pub mod prelude;

pub use scene::{render, render_with, Scene};

/// Re-exported so scene scripts can write `fn main() -> manim::anyhow::Result<()>`
/// without adding a dependency.
pub use anyhow;
