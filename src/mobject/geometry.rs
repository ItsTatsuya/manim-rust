//! Basic shapes: circles, polygons, lines, arrows...
//!
//! Each type is a constructor namespace returning a [`VMobject`] with Manim's default style.

#![allow(clippy::new_ret_no_self)]

use super::{Subpath, VMobject};
use crate::bezier::{self, Cubic};
use crate::color::*;
use crate::constants::*;

/// Cubic approximation of a circular arc.
pub fn arc_curves(center: Point, radius: f64, start_angle: f64, angle: f64) -> Vec<Cubic> {
    let n = ((angle.abs() / (TAU / 8.0)).ceil() as usize).max(1);
    let seg = angle / n as f64;
    let k = 4.0 / 3.0 * (seg / 4.0).tan() * radius;
    (0..n)
        .map(|i| {
            let a0 = start_angle + seg * i as f64;
            let a1 = a0 + seg;
            let (s0, c0) = a0.sin_cos();
            let (s1, c1) = a1.sin_cos();
            let p0 = center + pt(c0, s0) * radius;
            let p3 = center + pt(c1, s1) * radius;
            [p0, p0 + pt(-s0, c0) * k, p3 - pt(-s1, c1) * k, p3]
        })
        .collect()
}

fn stroked(subpaths: Vec<Subpath>, color: Color) -> VMobject {
    VMobject { stroke_color: color, fill_color: color, ..VMobject::from_subpaths(subpaths) }
}

pub struct Arc;
impl Arc {
    pub fn new(radius: f64, start_angle: f64, angle: f64) -> VMobject {
        stroked(vec![Subpath::new(arc_curves(ORIGIN, radius, start_angle, angle), false)], crate::defaults::color())
    }
}

/// Arc from `a` to `b` bulging by `angle`.
pub struct ArcBetweenPoints;
impl ArcBetweenPoints {
    pub fn new(a: Point, b: Point, angle: f64) -> VMobject {
        let chord = b - a;
        let len = chord.length();
        if len < 1e-9 || angle.abs() < 1e-6 {
            return Line::new(a, b);
        }
        // Travelling counter-clockwise (angle > 0) from a to b, the center is left of the chord.
        let radius = len / 2.0 / (angle / 2.0).sin().abs();
        let mid = (a + b) / 2.0;
        let left = pt(-chord.y, chord.x) / len;
        let center = mid + left * (len / 2.0 / (angle / 2.0).tan());
        let start = (a - center).y.atan2((a - center).x);
        stroked(vec![Subpath::new(arc_curves(center, radius, start, angle), false)], crate::defaults::color())
    }
}

pub struct Circle;
impl Circle {
    pub fn new(radius: f64) -> VMobject {
        stroked(vec![Subpath::new(arc_curves(ORIGIN, radius, 0.0, TAU), true)], RED)
    }
}

pub struct Ellipse;
impl Ellipse {
    pub fn new(width: f64, height: f64) -> VMobject {
        Circle::new(1.0).stretch(width / 2.0, 0).stretch(height / 2.0, 1).set_color(crate::defaults::color())
    }
}

pub struct Dot;
impl Dot {
    pub fn new() -> VMobject {
        Self::at(ORIGIN)
    }
    pub fn at(p: Point) -> VMobject {
        Circle::new(DEFAULT_DOT_RADIUS).set_fill(crate::defaults::color(), 1.0).set_stroke_width(0.0).move_to(p)
    }
}

/// Ring between two radii (filled with even-odd).
pub struct Annulus;
impl Annulus {
    pub fn new(inner: f64, outer: f64) -> VMobject {
        let mut m = stroked(
            vec![
                Subpath::new(arc_curves(ORIGIN, outer, 0.0, TAU), true),
                Subpath::new(arc_curves(ORIGIN, inner, 0.0, -TAU), true),
            ],
            crate::defaults::color(),
        );
        m.fill_opacity = 1.0;
        m.stroke_width = 0.0;
        m
    }
}

pub struct Sector;
impl Sector {
    pub fn new(radius: f64, start_angle: f64, angle: f64) -> VMobject {
        let mut curves = arc_curves(ORIGIN, radius, start_angle, angle);
        let end = curves.last().unwrap()[3];
        let start = curves[0][0];
        curves.push(bezier::line(end, ORIGIN));
        curves.push(bezier::line(ORIGIN, start));
        let mut m = stroked(vec![Subpath::new(curves, true)], crate::defaults::color());
        m.fill_opacity = 1.0;
        m.stroke_width = 0.0;
        m
    }
}

pub struct Polygon;
impl Polygon {
    pub fn new(vertices: &[Point]) -> VMobject {
        stroked(vec![Subpath::polyline(vertices, true)], BLUE)
    }
}

pub struct Polyline;
impl Polyline {
    pub fn new(points: &[Point]) -> VMobject {
        stroked(vec![Subpath::polyline(points, false)], crate::defaults::color())
    }
}

pub struct RegularPolygon;
impl RegularPolygon {
    pub fn new(n: usize) -> VMobject {
        Self::with_radius(n, 1.0)
    }
    pub fn with_radius(n: usize, radius: f64) -> VMobject {
        let n = n.max(3);
        let start = if n.is_multiple_of(2) { 0.0 } else { 90.0 * DEGREES };
        let verts: Vec<Point> = (0..n)
            .map(|i| {
                let a = start + TAU * i as f64 / n as f64;
                pt(a.cos(), a.sin()) * radius
            })
            .collect();
        Polygon::new(&verts)
    }
}

pub struct Triangle;
impl Triangle {
    pub fn new() -> VMobject {
        RegularPolygon::new(3)
    }
}

pub struct Star;
impl Star {
    pub fn new(points: usize, outer: f64, inner: f64) -> VMobject {
        let n = points.max(2);
        let verts: Vec<Point> = (0..2 * n)
            .map(|i| {
                let a = 90.0 * DEGREES + PI * i as f64 / n as f64;
                let r = if i % 2 == 0 { outer } else { inner };
                pt(a.cos(), a.sin()) * r
            })
            .collect();
        Polygon::new(&verts)
    }
}

pub struct Rectangle;
impl Rectangle {
    pub fn new(width: f64, height: f64) -> VMobject {
        let (w, h) = (width / 2.0, height / 2.0);
        Polygon::new(&[pt(w, h), pt(-w, h), pt(-w, -h), pt(w, -h)]).set_color(crate::defaults::color())
    }
}

pub struct Square;
impl Square {
    pub fn new(side: f64) -> VMobject {
        Rectangle::new(side, side)
    }
}

pub struct RoundedRectangle;
impl RoundedRectangle {
    pub fn new(width: f64, height: f64, corner_radius: f64) -> VMobject {
        let (w, h) = (width / 2.0, height / 2.0);
        let r = corner_radius.min(w).min(h).max(0.0);
        let mut c: Vec<Cubic> = Vec::new();
        c.extend(arc_curves(pt(w - r, h - r), r, 0.0, PI / 2.0));
        c.push(bezier::line(pt(w - r, h), pt(-w + r, h)));
        c.extend(arc_curves(pt(-w + r, h - r), r, PI / 2.0, PI / 2.0));
        c.push(bezier::line(pt(-w, h - r), pt(-w, -h + r)));
        c.extend(arc_curves(pt(-w + r, -h + r), r, PI, PI / 2.0));
        c.push(bezier::line(pt(-w + r, -h), pt(w - r, -h)));
        c.extend(arc_curves(pt(w - r, -h + r), r, 1.5 * PI, PI / 2.0));
        c.push(bezier::line(pt(w, -h + r), pt(w, h - r)));
        stroked(vec![Subpath::new(c, true)], crate::defaults::color())
    }
}

pub struct Line;
impl Line {
    pub fn new(start: Point, end: Point) -> VMobject {
        stroked(vec![Subpath::new(vec![bezier::line(start, end)], false)], crate::defaults::color())
    }
}

pub struct DashedLine;
impl DashedLine {
    pub fn new(start: Point, end: Point) -> VMobject {
        Self::with_dash(start, end, 0.05, 0.5)
    }
    pub fn with_dash(start: Point, end: Point, dash_length: f64, dashed_ratio: f64) -> VMobject {
        let len = start.distance(end);
        let period = dash_length / dashed_ratio.clamp(0.01, 1.0);
        let n = ((len / period).ceil() as usize).max(1);
        let subpaths = (0..n)
            .map(|i| {
                let a = i as f64 / n as f64;
                let b = a + dashed_ratio / n as f64;
                Subpath::new(vec![bezier::line(start.lerp(end, a), start.lerp(end, b.min(1.0)))], false)
            })
            .collect();
        stroked(subpaths, crate::defaults::color())
    }
}

fn arrow_tip(base_dir: Point, end: Point, length: f64, color: Color) -> VMobject {
    let perp = pt(-base_dir.y, base_dir.x) * (length / 2.0);
    let base = end - base_dir * length;
    let mut tip = Polygon::new(&[end, base + perp, base - perp]);
    tip.fill_color = color;
    tip.stroke_color = color;
    tip.fill_opacity = 1.0;
    tip.stroke_width = 0.0;
    tip
}

pub struct Arrow;
impl Arrow {
    pub fn new(start: Point, end: Point) -> VMobject {
        Self::with_buff(start, end, MED_SMALL_BUFF)
    }
    pub fn with_buff(start: Point, end: Point, buff: f64) -> VMobject {
        let v = end - start;
        let len = v.length();
        if len < 1e-9 {
            return VMobject::group(vec![]);
        }
        let dir = v / len;
        // Never let the buffers eat more than half the arrow (it would flip direction).
        let buff = buff.clamp(0.0, len * 0.25);
        let (s, e) = (start + dir * buff, end - dir * buff);
        let tip_len = 0.35_f64.min(0.25 * (len - 2.0 * buff));
        let mut line = Line::new(s, e - dir * tip_len);
        line.stroke_width = 6.0;
        VMobject::group(vec![line, arrow_tip(dir, e, tip_len, crate::defaults::color())])
    }
}

/// Arrow from the origin.
pub struct Vector;
impl Vector {
    pub fn new(direction: Point) -> VMobject {
        Arrow::with_buff(ORIGIN, direction, 0.0).set_color(YELLOW)
    }
}

pub struct DoubleArrow;
impl DoubleArrow {
    pub fn new(start: Point, end: Point) -> VMobject {
        let v = end - start;
        let len = v.length();
        if len < 1e-9 {
            return VMobject::group(vec![]);
        }
        let dir = v / len;
        let buff = MED_SMALL_BUFF.min(len * 0.25);
        let (s, e) = (start + dir * buff, end - dir * buff);
        let tip_len = 0.35_f64.min(0.2 * (len - 2.0 * buff));
        let mut line = Line::new(s + dir * tip_len, e - dir * tip_len);
        line.stroke_width = 6.0;
        VMobject::group(vec![
            line,
            arrow_tip(dir, e, tip_len, crate::defaults::color()),
            arrow_tip(-dir, s, tip_len, crate::defaults::color()),
        ])
    }
}

/// Rectangle around a mobject.
pub struct SurroundingRectangle;
impl SurroundingRectangle {
    pub fn new(m: &VMobject) -> VMobject {
        Self::with_buff(m, SMALL_BUFF)
    }
    pub fn with_buff(m: &VMobject, buff: f64) -> VMobject {
        Rectangle::new(m.width() + 2.0 * buff, m.height() + 2.0 * buff).move_to(m.get_center()).set_color(YELLOW)
    }
}

/// Opaque rectangle behind a mobject.
pub struct BackgroundRectangle;
impl BackgroundRectangle {
    pub fn new(m: &VMobject) -> VMobject {
        SurroundingRectangle::with_buff(m, SMALL_BUFF)
            .set_fill(BLACK, 0.75)
            .set_stroke_width(0.0)
            .set_z_index(m.z_index - 1)
    }
}

impl SurroundingRectangle {
    /// A rounded rectangle `buff` away from `m` on every side.
    pub fn rounded(m: &VMobject, buff: f64, corner_radius: f64) -> VMobject {
        RoundedRectangle::new(m.width() + 2.0 * buff, m.height() + 2.0 * buff, corner_radius)
            .move_to(m.get_center())
            .set_color(YELLOW)
    }
}

/// An arrow along a circular arc from `start` to `end` (Manim's `CurvedArrow`).
pub struct CurvedArrow;
impl CurvedArrow {
    /// `angle` is the arc's bend in radians (positive bends counter-clockwise).
    pub fn new(start: Point, end: Point, angle: f64) -> VMobject {
        Self::with_tip_length(start, end, angle, 0.25)
    }

    pub fn with_tip_length(start: Point, end: Point, angle: f64, tip_length: f64) -> VMobject {
        let arc = ArcBetweenPoints::new(start, end, angle);
        let total: f64 = arc.subpaths.iter().flat_map(|s| &s.curves).map(bezier::length).sum();
        if total < 1e-9 {
            return VMobject::group(vec![]);
        }
        let tip_length = tip_length.min(total * 0.4);
        // Direction of travel where the shaft meets the tip.
        let keep = 1.0 - tip_length / total;
        let base = arc.point_from_proportion(keep);
        let dir = (end - base).normalize_or_zero();
        let color = arc.stroke_color;
        let shaft = arc.partial(keep);
        VMobject::group(vec![shaft, arrow_tip(dir, end, tip_length, color)])
    }
}

/// A curly brace under (or beside) a mobject (Manim's `Brace`).
pub struct Brace;
impl Brace {
    /// Brace on the `direction` side of `m` (`DOWN`, `UP`, `LEFT` or `RIGHT`).
    pub fn new(m: &VMobject, direction: Point) -> VMobject {
        let horizontal = direction.y.abs() >= direction.x.abs();
        let length = if horizontal { m.width() } else { m.height() };
        let brace = Self::shape(length.max(0.2), 0.18);
        // `shape` points down; rotate to face `direction`.
        let angle = direction.y.atan2(direction.x) + PI / 2.0;
        let brace = brace.rotate_about(angle, ORIGIN);
        brace.next_to_buff(m, direction, SMALL_BUFF)
    }

    /// A filled brace `width` wide, `height` tall, its point facing down, centered at the origin.
    pub fn shape(width: f64, height: f64) -> VMobject {
        let (w, h) = (width / 2.0, height);
        let t = 0.035; // stroke thickness of the brace body
        let q = (w * 0.5).min(h * 1.6);
        // Upper edge (left to right), then lower edge back.
        let upper: Vec<Cubic> = vec![
            [pt(-w, h), pt(-w, h * 0.5), pt(-w + q * 0.3, h * 0.5), pt(-w + q, h * 0.5)],
            [pt(-w + q, h * 0.5), pt(-q * 0.6, h * 0.5), pt(-t, h * 0.4), pt(0.0, 0.0)],
            [pt(0.0, 0.0), pt(t, h * 0.4), pt(q * 0.6, h * 0.5), pt(w - q, h * 0.5)],
            [pt(w - q, h * 0.5), pt(w - q * 0.3, h * 0.5), pt(w, h * 0.5), pt(w, h)],
        ];
        let lower: Vec<Cubic> = vec![
            [pt(w, h), pt(w - t, h), pt(w - t, h * 0.5 + t), pt(w - q, h * 0.5 + t)],
            [pt(w - q, h * 0.5 + t), pt(q * 0.6, h * 0.5 + t), pt(t * 0.5, h * 0.5), pt(0.0, t * 1.5)],
            [pt(0.0, t * 1.5), pt(-t * 0.5, h * 0.5), pt(-q * 0.6, h * 0.5 + t), pt(-w + q, h * 0.5 + t)],
            [pt(-w + q, h * 0.5 + t), pt(-w + t, h * 0.5 + t), pt(-w + t, h), pt(-w, h)],
        ];
        let curves: Vec<Cubic> = upper.into_iter().chain(lower).collect();
        let mut m = stroked(vec![Subpath::new(curves, true)], crate::defaults::color());
        m.fill_opacity = 1.0;
        m.stroke_width = 0.0;
        m.shift(pt(0.0, -h / 2.0))
    }
}
