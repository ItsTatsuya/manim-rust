//! Direction vectors, buffers and frame constants (mirrors `manim.constants`).

use glam::DVec3;

/// A point / vector in scene space. Manim is 2D here: `z` is kept at 0.
pub type Point = DVec3;

/// The origin, `(0, 0, 0)`.
pub const ORIGIN: Point = DVec3::new(0.0, 0.0, 0.0);
/// Unit vector pointing up (+y).
pub const UP: Point = DVec3::new(0.0, 1.0, 0.0);
/// Unit vector pointing down (-y).
pub const DOWN: Point = DVec3::new(0.0, -1.0, 0.0);
/// Unit vector pointing right (+x).
pub const RIGHT: Point = DVec3::new(1.0, 0.0, 0.0);
/// Unit vector pointing left (-x).
pub const LEFT: Point = DVec3::new(-1.0, 0.0, 0.0);
/// Unit vector pointing out of the screen toward the viewer (+z).
pub const OUT: Point = DVec3::new(0.0, 0.0, 1.0);
/// Unit vector pointing into the screen (-z).
pub const IN: Point = DVec3::new(0.0, 0.0, -1.0);
/// Up-left diagonal, `(-1, 1, 0)`.
pub const UL: Point = DVec3::new(-1.0, 1.0, 0.0);
/// Up-right diagonal, `(1, 1, 0)`.
pub const UR: Point = DVec3::new(1.0, 1.0, 0.0);
/// Down-left diagonal, `(-1, -1, 0)`.
pub const DL: Point = DVec3::new(-1.0, -1.0, 0.0);
/// Down-right diagonal, `(1, -1, 0)`.
pub const DR: Point = DVec3::new(1.0, -1.0, 0.0);

/// Half a turn, in radians.
pub const PI: f64 = std::f64::consts::PI;
/// A full turn (2Ï€), in radians.
pub const TAU: f64 = std::f64::consts::TAU;
/// Radians per degree: multiply degrees by this to convert.
pub const DEGREES: f64 = TAU / 360.0;

/// Height of the default camera frame, in scene units.
pub const FRAME_HEIGHT: f64 = 8.0;
/// Frame width for the default 16:9 aspect ratio.
pub const FRAME_WIDTH: f64 = FRAME_HEIGHT * 16.0 / 9.0;
/// Half the frame width, in scene units.
pub const FRAME_X_RADIUS: f64 = FRAME_WIDTH / 2.0;
/// Half the frame height, in scene units.
pub const FRAME_Y_RADIUS: f64 = FRAME_HEIGHT / 2.0;

/// Small gap between mobjects (0.1).
pub const SMALL_BUFF: f64 = 0.1;
/// Medium-small gap (0.25).
pub const MED_SMALL_BUFF: f64 = 0.25;
/// Medium-large gap (0.5).
pub const MED_LARGE_BUFF: f64 = 0.5;
/// Large gap (1.0).
pub const LARGE_BUFF: f64 = 1.0;
/// Default gap between a mobject and the frame edge when aligning to it.
pub const DEFAULT_MOBJECT_TO_EDGE_BUFFER: f64 = MED_LARGE_BUFF;
/// Default gap between two mobjects when arranging or placing one next to another.
pub const DEFAULT_MOBJECT_TO_MOBJECT_BUFFER: f64 = MED_SMALL_BUFF;

/// Default stroke width of new shapes, in Manim's stroke units.
pub const DEFAULT_STROKE_WIDTH: f64 = 4.0;
/// Default radius of a `Dot`, in scene units.
pub const DEFAULT_DOT_RADIUS: f64 = 0.08;
/// Default font size of `Text`, in points.
pub const DEFAULT_FONT_SIZE: f64 = 48.0;

/// Shorthand to build a point.
pub fn pt(x: f64, y: f64) -> Point {
    DVec3::new(x, y, 0.0)
}
