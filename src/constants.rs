//! Direction vectors, buffers and frame constants (mirrors `manim.constants`).

use glam::DVec3;

/// A point / vector in scene space. Manim is 2D here: `z` is kept at 0.
pub type Point = DVec3;

pub const ORIGIN: Point = DVec3::new(0.0, 0.0, 0.0);
pub const UP: Point = DVec3::new(0.0, 1.0, 0.0);
pub const DOWN: Point = DVec3::new(0.0, -1.0, 0.0);
pub const RIGHT: Point = DVec3::new(1.0, 0.0, 0.0);
pub const LEFT: Point = DVec3::new(-1.0, 0.0, 0.0);
pub const OUT: Point = DVec3::new(0.0, 0.0, 1.0);
pub const IN: Point = DVec3::new(0.0, 0.0, -1.0);
pub const UL: Point = DVec3::new(-1.0, 1.0, 0.0);
pub const UR: Point = DVec3::new(1.0, 1.0, 0.0);
pub const DL: Point = DVec3::new(-1.0, -1.0, 0.0);
pub const DR: Point = DVec3::new(1.0, -1.0, 0.0);

pub const PI: f64 = std::f64::consts::PI;
pub const TAU: f64 = std::f64::consts::TAU;
pub const DEGREES: f64 = TAU / 360.0;

pub const FRAME_HEIGHT: f64 = 8.0;
/// Frame width for the default 16:9 aspect ratio.
pub const FRAME_WIDTH: f64 = FRAME_HEIGHT * 16.0 / 9.0;
pub const FRAME_X_RADIUS: f64 = FRAME_WIDTH / 2.0;
pub const FRAME_Y_RADIUS: f64 = FRAME_HEIGHT / 2.0;

pub const SMALL_BUFF: f64 = 0.1;
pub const MED_SMALL_BUFF: f64 = 0.25;
pub const MED_LARGE_BUFF: f64 = 0.5;
pub const LARGE_BUFF: f64 = 1.0;
pub const DEFAULT_MOBJECT_TO_EDGE_BUFFER: f64 = MED_LARGE_BUFF;
pub const DEFAULT_MOBJECT_TO_MOBJECT_BUFFER: f64 = MED_SMALL_BUFF;

pub const DEFAULT_STROKE_WIDTH: f64 = 4.0;
pub const DEFAULT_DOT_RADIUS: f64 = 0.08;
pub const DEFAULT_FONT_SIZE: f64 = 48.0;

/// Shorthand to build a point.
pub fn pt(x: f64, y: f64) -> Point {
    DVec3::new(x, y, 0.0)
}
