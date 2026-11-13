//! Function graphs, axes and number planes.

#![allow(clippy::new_ret_no_self)]

use super::geometry::{Line, Polygon};
use super::{Subpath, VMobject};
use crate::bezier;
use crate::color::*;
use crate::constants::*;

/// Smooth curve through `f(t)` sampled over `t_range`. Non-finite samples break the curve.
pub struct ParametricFunction;
impl ParametricFunction {
    pub fn new(f: impl Fn(f64) -> Point, t_range: [f64; 2]) -> VMobject {
        Self::with_samples(f, t_range, 200)
    }

    pub fn with_samples(f: impl Fn(f64) -> Point, t_range: [f64; 2], samples: usize) -> VMobject {
        let n = samples.max(2);
        let mut subpaths = Vec::new();
        let mut run: Vec<Point> = Vec::new();
        for i in 0..=n {
            let t = t_range[0] + (t_range[1] - t_range[0]) * i as f64 / n as f64;
            let p = f(t);
            if p.is_finite() {
                run.push(p);
            } else if run.len() > 1 {
                subpaths.push(Subpath::new(bezier::smooth_through(&run), false));
                run.clear();
            } else {
                run.clear();
            }
        }
        if run.len() > 1 {
            subpaths.push(Subpath::new(bezier::smooth_through(&run), false));
        }
        VMobject { stroke_color: YELLOW, fill_color: YELLOW, ..VMobject::from_subpaths(subpaths) }
    }
}

/// `y = f(x)` in scene coordinates.
pub struct FunctionGraph;
impl FunctionGraph {
    pub fn new(f: impl Fn(f64) -> f64, x_range: [f64; 2]) -> VMobject {
        ParametricFunction::new(|x| pt(x, f(x)), x_range)
    }
}

/// Coordinate axes with a mapping from graph coordinates to scene points.
#[derive(Clone, Debug)]
pub struct Axes {
    /// `[min, max, step]`
    pub x_range: [f64; 3],
    pub y_range: [f64; 3],
    pub x_length: f64,
    pub y_length: f64,
    pub center: Point,
    pub color: Color,
    pub include_tips: bool,
    pub include_numbers: bool,
    pub tick_size: f64,
}

impl Axes {
    pub fn new(x_range: [f64; 3], y_range: [f64; 3]) -> Self {
        Axes {
            x_range,
            y_range,
            x_length: (FRAME_WIDTH.round() - 2.0),
            y_length: (FRAME_HEIGHT.round() - 2.0),
            center: ORIGIN,
            color: WHITE,
            include_tips: true,
            include_numbers: false,
            tick_size: 0.1,
        }
    }

    pub fn x_length(mut self, l: f64) -> Self {
        self.x_length = l;
        self
    }
    pub fn y_length(mut self, l: f64) -> Self {
        self.y_length = l;
        self
    }
    pub fn move_to(mut self, p: Point) -> Self {
        self.center = p;
        self
    }
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }
    pub fn tips(mut self, on: bool) -> Self {
        self.include_tips = on;
        self
    }
    /// Label ticks with numbers (needs the `text` feature).
    pub fn with_numbers(mut self) -> Self {
        self.include_numbers = true;
        self
    }

    /// Graph coordinates → scene point (`coords_to_point`).
    pub fn c2p(&self, x: f64, y: f64) -> Point {
        let fx = (x - self.x_range[0]) / span(self.x_range[0], self.x_range[1]);
        let fy = (y - self.y_range[0]) / span(self.y_range[0], self.y_range[1]);
        self.center + pt((fx - 0.5) * self.x_length, (fy - 0.5) * self.y_length)
    }

    /// Scene point → graph coordinates (`point_to_coords`).
    pub fn p2c(&self, p: Point) -> (f64, f64) {
        let d = p - self.center;
        let fx = d.x / self.x_length + 0.5;
        let fy = d.y / self.y_length + 0.5;
        (
            self.x_range[0] + fx * (self.x_range[1] - self.x_range[0]),
            self.y_range[0] + fy * (self.y_range[1] - self.y_range[0]),
        )
    }

    /// Plot `y = f(x)` over the full x range.
    pub fn plot(&self, f: impl Fn(f64) -> f64) -> VMobject {
        self.plot_range(f, [self.x_range[0], self.x_range[1]])
    }

    pub fn plot_range(&self, f: impl Fn(f64) -> f64, x_range: [f64; 2]) -> VMobject {
        ParametricFunction::new(|x| self.c2p(x, f(x)), x_range)
    }

    pub fn plot_parametric(&self, f: impl Fn(f64) -> (f64, f64), t_range: [f64; 2]) -> VMobject {
        ParametricFunction::new(
            |t| {
                let (x, y) = f(t);
                self.c2p(x, y)
            },
            t_range,
        )
    }

    /// Vertical line from the x-axis to the graph point.
    pub fn v_line(&self, x: f64, y: f64) -> VMobject {
        super::geometry::DashedLine::new(self.c2p(x, 0.0), self.c2p(x, y))
    }

    fn ticks(range: [f64; 3]) -> Vec<f64> {
        grid_values(range).into_iter().filter(|v| v.abs() > 1e-9).collect()
    }

    fn axis(&self, start: Point, end: Point) -> VMobject {
        let dir = (end - start).normalize_or_zero();
        let tip_len = 0.2;
        let mut line = Line::new(start, if self.include_tips { end - dir * tip_len } else { end });
        line.stroke_width = 2.0;
        let mut g = VMobject::group(vec![line]);
        if self.include_tips {
            let perp = pt(-dir.y, dir.x) * (tip_len / 2.0);
            let base = end - dir * tip_len;
            g.push(Polygon::new(&[end, base + perp, base - perp]).set_fill(WHITE, 1.0).set_stroke_width(0.0));
        }
        g
    }

    pub fn to_mobject(&self) -> VMobject {
        let y0 = 0.0_f64.clamp(self.y_range[0], self.y_range[1]);
        let x0 = 0.0_f64.clamp(self.x_range[0], self.x_range[1]);
        let mut x_axis = self.axis(self.c2p(self.x_range[0], y0), self.c2p(self.x_range[1], y0));
        let mut y_axis = self.axis(self.c2p(x0, self.y_range[0]), self.c2p(x0, self.y_range[1]));
        let t = self.tick_size;
        for x in Self::ticks(self.x_range) {
            let p = self.c2p(x, y0);
            x_axis.push(Line::new(p + UP * t, p + DOWN * t).set_stroke_width(2.0));
        }
        for y in Self::ticks(self.y_range) {
            let p = self.c2p(x0, y);
            y_axis.push(Line::new(p + LEFT * t, p + RIGHT * t).set_stroke_width(2.0));
        }
        #[allow(unused_mut)]
        let mut g = VMobject::group(vec![x_axis, y_axis]);
        #[cfg(feature = "text")]
        if self.include_numbers {
            let fmt =
                |v: f64| if (v - v.round()).abs() < 1e-9 { format!("{}", v.round() as i64) } else { format!("{v:.1}") };
            let mut labels = Vec::new();
            for x in Self::ticks(self.x_range) {
                labels.push(crate::text::Text::sized(&fmt(x), 24.0).next_to_point_buff(
                    self.c2p(x, y0),
                    DOWN,
                    SMALL_BUFF * 1.5,
                ));
            }
            for y in Self::ticks(self.y_range) {
                labels.push(crate::text::Text::sized(&fmt(y), 24.0).next_to_point_buff(
                    self.c2p(x0, y),
                    LEFT,
                    SMALL_BUFF * 1.5,
                ));
            }
            g.push(VMobject::group(labels));
        }
        g.set_color(self.color)
    }
}

impl From<&Axes> for VMobject {
    fn from(a: &Axes) -> Self {
        a.to_mobject()
    }
}

impl From<Axes> for VMobject {
    fn from(a: Axes) -> Self {
        a.to_mobject()
    }
}

/// Background grid with axes (Manim's `NumberPlane`).
pub struct NumberPlane;
impl NumberPlane {
    pub fn new() -> VMobject {
        Self::with_ranges([-FRAME_X_RADIUS, FRAME_X_RADIUS, 1.0], [-FRAME_Y_RADIUS, FRAME_Y_RADIUS, 1.0])
    }

    pub fn with_ranges(x_range: [f64; 3], y_range: [f64; 3]) -> VMobject {
        let mut lines = Vec::new();
        let grid = grid_values;
        for x in grid(x_range) {
            if x.abs() > 1e-9 {
                lines.push(Line::new(pt(x, y_range[0]), pt(x, y_range[1])).set_stroke(BLUE_D, 2.0));
            }
        }
        for y in grid(y_range) {
            if y.abs() > 1e-9 {
                lines.push(Line::new(pt(x_range[0], y), pt(x_range[1], y)).set_stroke(BLUE_D, 2.0));
            }
        }
        let axes = VMobject::group(vec![
            Line::new(pt(x_range[0], 0.0), pt(x_range[1], 0.0)).set_stroke(WHITE, 2.0),
            Line::new(pt(0.0, y_range[0]), pt(0.0, y_range[1])).set_stroke(WHITE, 2.0),
        ]);
        VMobject::group(vec![VMobject::group(lines), axes])
    }
}

/// Most grid lines / ticks a single range may produce; guards against runaway user input.
const MAX_GRID_VALUES: usize = 10_000;

/// Multiples of `range[2]` inside `[range[0], range[1]]`. Empty for a non-positive or
/// non-finite step or bounds, and capped at [`MAX_GRID_VALUES`].
fn grid_values(range: [f64; 3]) -> Vec<f64> {
    let [lo, hi, step] = range;
    if !(step > 0.0 && step.is_finite() && lo.is_finite() && hi.is_finite()) || hi < lo {
        return Vec::new();
    }
    let start = (lo / step).ceil();
    let count = ((hi + 1e-9) / step).floor() - start + 1.0;
    if count.is_nan() || count < 1.0 {
        return Vec::new();
    }
    let count = (count as usize).min(MAX_GRID_VALUES);
    (0..count).map(|i| (start + i as f64) * step).collect()
}

/// Width of a coordinate range, never zero (a degenerate range maps as width 1).
fn span(lo: f64, hi: f64) -> f64 {
    let d = hi - lo;
    if d.abs() < 1e-12 || !d.is_finite() {
        1.0
    } else {
        d
    }
}
