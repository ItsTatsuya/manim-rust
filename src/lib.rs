//! # manim (Rust)
//!
//! A Rust reimplementation of the [Manim](https://github.com/ManimCommunity/manim)
//! animation engine, built for speed:
//!
//! - **Two backends**: a tiny-skia CPU rasterizer that renders frames in parallel across
//!   cores, and a wgpu GPU renderer. `RendererKind::Auto` probes for a hardware GPU and
//!   falls back to the CPU.
//! - **Frame dedup**: static stretches are rasterized once and repeated.
//! - **No Python, Cairo or Pango**: text comes from system font outlines.
//!
//! ```no_run
//! use manim::prelude::*;
//!
//! fn main() -> anyhow::Result<()> {
//!     manim::render("SquareToCircle", |s| {
//!         let square = s.add(Square::new(2.0).rotate(PI / 4.0));
//!         s.play(Create::new(square));
//!         s.play(Transform::new(square, Circle::new(1.0).set_fill(PINK, 0.5)));
//!         s.play(FadeOut::new(square));
//!     })
//! }
//! ```

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
pub mod image;
#[cfg(feature = "math")]
pub mod math;
pub mod mobject;
pub mod output;
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
