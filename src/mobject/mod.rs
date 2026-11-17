//! Vectorized mobjects: Bézier paths with fill/stroke style and a child hierarchy.
//!
//! Builder methods consume and return `Self`, so construction reads like Manim:
//! `Circle::new(1.0).set_fill(PINK, 0.5).shift(LEFT)`.
//! To mutate a mobject that already lives in a scene use
//! `scene.modify(id, |m| m.shift(RIGHT))`.

mod align;
#[cfg(feature = "text")]
pub mod code;
pub mod geometry;
pub mod graphing;
pub mod grid;
pub mod highlight;
#[cfg(feature = "text")]
pub mod matrix;
#[cfg(feature = "text")]
pub mod table;

use std::sync::Arc;

use crate::bezier::{self, Cubic};
use crate::color::{Color, WHITE};
use crate::constants::*;

/// Decoded raster image, stored premultiplied for the rasterizer.
#[derive(Debug)]
pub struct RasterImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Premultiplied RGBA8, `width * height * 4` bytes.
    pub premultiplied: Vec<u8>,
}

impl RasterImage {
    /// Build from straight (non-premultiplied) RGBA8 pixels. Fails when `rgba` is not
    /// exactly `width * height * 4` bytes.
    pub fn from_rgba(width: u32, height: u32, mut rgba: Vec<u8>) -> anyhow::Result<Self> {
        let expected = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4));
        if expected != Some(rgba.len()) {
            anyhow::bail!(
                "RGBA buffer holds {} bytes, but {width}x{height} pixels need {}",
                rgba.len(),
                width as u64 * height as u64 * 4
            );
        }
        for px in rgba.as_chunks_mut::<4>().0 {
            let a = px[3] as u16;
            if a < 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u16 * a + 127) / 255) as u8;
                }
            }
        }
        Ok(RasterImage { width, height, premultiplied: rgba })
    }
}

/// Shared image pixels; equal only when they are the same allocation (cheap to compare).
#[derive(Clone, Debug)]
pub struct ImageRef(pub Arc<RasterImage>);

impl PartialEq for ImageRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Facts about a mobject that tools (layout checks, slicing, inspectors) can read.
/// Transforms keep them; they never affect drawing, except `image`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    /// What built it: `"Text"`, `"Math"`, `"Image"`, `"SVG"`, ...
    pub kind: Option<Arc<str>>,
    /// Human-readable content, e.g. the source string of a `Text`.
    pub label: Option<Arc<str>>,
    /// For glyphs: index (in chars) of the character this glyph draws.
    pub char_index: Option<u32>,
    /// A raster image drawn instead of the paths.
    pub image: Option<ImageRef>,
    /// Where the image's top-left, top-right and bottom-left corners are.
    pub anchors: Option<[Point; 3]>,
    /// Fill with the non-zero winding rule instead of even-odd. Font glyphs and SVG paths
    /// need it: their contours may overlap (variable fonts) without meaning a hole.
    pub nonzero: bool,
    /// On the entries group of a `Table` or `Matrix`: the grid's shape and spacing.
    pub grid: Option<Grid>,
}

/// Shape and spacing of a table's or matrix's entry grid. The entries group holds the
/// entries row by row; transforms scale the buffers along with the points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// Number of rows (labels count).
    pub rows: usize,
    /// Number of columns (labels count).
    pub cols: usize,
    /// Horizontal gap between columns (Manim's `h_buff`).
    pub h_buff: f64,
    /// Vertical gap between rows (Manim's `v_buff`).
    pub v_buff: f64,
    /// Table only: the first column holds row labels.
    pub row_labels: bool,
    /// Table only: the first row holds column labels.
    pub col_labels: bool,
}

impl Meta {
    /// The mobject's kind tag (`"Text"`, `"Math"`, `"Table"`, ...), empty if untagged.
    pub fn kind(&self) -> &str {
        self.kind.as_deref().unwrap_or("")
    }
}

/// One continuous run of cubic curves.
#[derive(Clone, Debug, PartialEq)]
pub struct Subpath {
    /// The cubic BÃ©ziers in order; each starts where the previous one ends.
    pub curves: Vec<Cubic>,
    /// Whether the last curve joins back to the first.
    pub closed: bool,
}

impl Subpath {
    /// A subpath from cubics and a closed flag.
    pub fn new(curves: Vec<Cubic>, closed: bool) -> Self {
        Subpath { curves, closed }
    }

    /// Straight segments through `points`.
    pub fn polyline(points: &[Point], closed: bool) -> Self {
        let mut curves: Vec<Cubic> = points.windows(2).map(|w| bezier::line(w[0], w[1])).collect();
        if closed && points.len() > 2 {
            curves.push(bezier::line(points[points.len() - 1], points[0]));
        }
        Subpath { curves, closed }
    }

    /// A zero-size path sitting at `p` (used to pad alignments).
    pub fn degenerate(p: Point) -> Self {
        Subpath { curves: vec![[p; 4]], closed: false }
    }

    /// First point, if there are curves.
    pub fn start(&self) -> Option<Point> {
        self.curves.first().map(|c| c[0])
    }

    /// Last point, if there are curves.
    pub fn end(&self) -> Option<Point> {
        self.curves.last().map(|c| c[3])
    }
}

/// Deepest group nesting accepted from scripts, `add` / `push` and [`Scene::add`](crate::Scene::add).
pub const MAX_NESTING: usize = 256;

/// A vectorized mobject. Groups are VMobjects with no paths and some children.
#[derive(Clone, Debug, PartialEq)]
pub struct VMobject {
    /// The mobject's own outlines (empty for pure groups).
    pub subpaths: Vec<Subpath>,
    /// Fill color.
    pub fill_color: Color,
    /// Fill opacity in `[0, 1]` (0 = no fill).
    pub fill_opacity: f64,
    /// Stroke color.
    pub stroke_color: Color,
    /// Stroke opacity in `[0, 1]`.
    pub stroke_opacity: f64,
    /// Stroke width in Manim's units (1 = 0.01 scene units).
    pub stroke_width: f64,
    /// Draw order within the scene: higher draws on top.
    pub z_index: i32,
    /// Sub-mobjects drawn with this one.
    pub children: Vec<VMobject>,
    /// Non-geometric data: kind tag, label, text and image metadata.
    pub meta: Meta,
}

impl Default for VMobject {
    fn default() -> Self {
        VMobject {
            subpaths: Vec::new(),
            fill_color: crate::defaults::color(),
            fill_opacity: 0.0,
            stroke_color: crate::defaults::color(),
            stroke_opacity: 1.0,
            stroke_width: DEFAULT_STROKE_WIDTH,
            z_index: 0,
            children: Vec::new(),
            meta: Meta::default(),
        }
    }
}

/// Shorthand: `VGroup::new(vec![a, b])` or `vgroup![a, b]`.
pub struct VGroup;

impl VGroup {
    /// A group of `children`.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(children: Vec<VMobject>) -> VMobject {
        VMobject::group(children)
    }
}

/// Build a group from mobjects (each converted with `.into()`): `vgroup![a, b]`.
#[macro_export]
macro_rules! vgroup {
    ($($m:expr),* $(,)?) => { $crate::mobject::VMobject::group(vec![$($m.into()),*]) };
}

/// Pre-order walk over a mobject and its descendants (see [`VMobject::family_iter`]).
pub struct FamilyIter<'a> {
    next: Option<&'a VMobject>,
    stack: Vec<&'a VMobject>,
}

impl<'a> Iterator for FamilyIter<'a> {
    type Item = &'a VMobject;

    fn next(&mut self) -> Option<&'a VMobject> {
        let m = self.next.take().or_else(|| self.stack.pop())?;
        self.stack.extend(m.children.iter().rev());
        Some(m)
    }
}

impl VMobject {
    /// An empty mobject with the current default style.
    pub fn new() -> Self {
        Self::default()
    }

    /// A mobject made of `subpaths`.
    pub fn from_subpaths(subpaths: Vec<Subpath>) -> Self {
        VMobject { subpaths, ..Default::default() }
    }

    /// A group holding `children`.
    pub fn group(children: Vec<VMobject>) -> Self {
        VMobject { children, ..Default::default() }
    }

    // ------------------------------------------------------------------ family

    /// Pre-order traversal of this mobject and all descendants.
    /// Iterative, so the depth of the tree never costs call-stack.
    pub fn family(&self) -> Vec<&VMobject> {
        self.family_iter().collect()
    }

    /// [`family`](Self::family) as an iterator, in the same order: nothing is allocated for
    /// a mobject without children (every shape), so hot paths use this.
    pub fn family_iter(&self) -> FamilyIter<'_> {
        FamilyIter { next: Some(self), stack: Vec::new() }
    }

    /// Call `f` on this mobject and all descendants, parents first.
    pub fn for_each_mut(&mut self, f: &mut impl FnMut(&mut VMobject)) {
        let mut stack = vec![self];
        while let Some(m) = stack.pop() {
            f(m);
            stack.extend(m.children.iter_mut().rev());
        }
    }

    /// Levels in this mobject's tree: 1 for a mobject without children, 2 for a group of such.
    pub fn depth(&self) -> usize {
        let mut deepest = 0;
        let mut stack = vec![(self, 1usize)];
        while let Some((m, d)) = stack.pop() {
            deepest = deepest.max(d);
            stack.extend(m.children.iter().map(|c| (c, d + 1)));
        }
        deepest
    }

    /// Every family member that has paths, flattened into childless copies.
    /// Subpaths with no curves are dropped, so every leaf has drawable geometry.
    pub fn leaves(&self) -> Vec<VMobject> {
        self.family()
            .into_iter()
            .filter(|m| m.subpaths.iter().any(|s| !s.curves.is_empty()))
            .map(|m| VMobject {
                subpaths: m.subpaths.iter().filter(|s| !s.curves.is_empty()).cloned().collect(),
                children: Vec::new(),
                ..m.clone()
            })
            .collect()
    }

    /// Append a child (Manim's `add`; builder style). A child that would take the tree past
    /// [`MAX_NESTING`] levels is left out; use [`try_push`](Self::try_push) to get the error.
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, child: impl Into<VMobject>) -> Self {
        let _ = self.try_push(child);
        self
    }

    /// Like [`add`](Self::add), in place.
    pub fn push(&mut self, child: impl Into<VMobject>) {
        let _ = self.try_push(child);
    }

    /// Append a child unless the tree would nest deeper than [`MAX_NESTING`] levels (deep
    /// trees are walked recursively by cloning, comparing and dropping).
    pub fn try_push(&mut self, child: impl Into<VMobject>) -> anyhow::Result<()> {
        let child = child.into();
        if child.depth() >= MAX_NESTING {
            anyhow::bail!("groups nested more than {MAX_NESTING} levels deep are not supported");
        }
        self.children.push(child);
        Ok(())
    }

    /// Index of the first direct child whose `meta.kind` is `kind`.
    pub fn child_index_of_kind(&self, kind: &str) -> Option<usize> {
        self.children.iter().position(|c| c.meta.kind() == kind)
    }

    /// The first direct child whose `meta.kind` is `kind` (parts of `Code`, `Table`, ...).
    pub fn child_of_kind(&self, kind: &str) -> Option<&VMobject> {
        self.children.iter().find(|c| c.meta.kind() == kind)
    }

    /// Mutable version of [`VMobject::child_of_kind`].
    pub fn child_of_kind_mut(&mut self, kind: &str) -> Option<&mut VMobject> {
        self.children.iter_mut().find(|c| c.meta.kind() == kind)
    }

    /// Total number of cubic curves in this mobject and its descendants.
    pub fn num_curves(&self) -> usize {
        self.family_iter().flat_map(|m| &m.subpaths).map(|s| s.curves.len()).sum()
    }

    /// Run `f` on an owned copy and store the result (lets `&mut` holders use builder methods).
    pub fn apply(&mut self, f: impl FnOnce(VMobject) -> VMobject) {
        let m = std::mem::take(self);
        *self = f(m);
    }

    /// Apply a point-wise function (Manim's `apply_function`). Pair with `.animate`
    /// to warp grids: `plane.animate(|m| m.apply_function(|p| ...))`.
    pub fn apply_function(self, f: impl Fn(Point) -> Point) -> Self {
        self.map_points(f)
    }

    /// Subdivide every curve into `n` pieces (smoother non-linear warps).
    pub fn insert_curves(mut self, n: usize) -> Self {
        self.for_each_mut(&mut |m| {
            for sp in &mut m.subpaths {
                sp.curves = sp.curves.iter().flat_map(|c| bezier::subdivide(c, n)).collect();
            }
        });
        self
    }

    fn map_points(mut self, f: impl Fn(Point) -> Point) -> Self {
        self.for_each_mut(&mut |m| {
            if let Some(g) = &mut m.meta.grid {
                // Exact for the affine maps (shift, scale, stretch, rotate) layout uses.
                let o = f(ORIGIN);
                g.h_buff = (f(pt(g.h_buff, 0.0)) - o).length();
                g.v_buff = (f(pt(0.0, g.v_buff)) - o).length();
            }
            if let Some(a) = &mut m.meta.anchors {
                for p in a.iter_mut() {
                    *p = f(*p);
                }
            }
            for sp in &mut m.subpaths {
                for c in &mut sp.curves {
                    for p in c.iter_mut() {
                        *p = f(*p);
                    }
                }
            }
        });
        self
    }

    // ------------------------------------------------------------------ geometry queries

    /// Axis-aligned bounding box `(min, max)` over all control points.
    pub fn bounding_box(&self) -> Option<(Point, Point)> {
        let mut it = self
            .family_iter()
            .flat_map(|m| m.subpaths.iter())
            .flat_map(|s| s.curves.iter())
            .flat_map(|c| c.iter().copied());
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// Center of the bounding box of the whole family.
    pub fn get_center(&self) -> Point {
        self.bounding_box().map(|(lo, hi)| (lo + hi) / 2.0).unwrap_or(ORIGIN)
    }

    /// Manim's `get_critical_point`: for each axis, max if dir>0, min if dir<0, center if 0.
    pub fn get_critical_point(&self, dir: Point) -> Point {
        let Some((lo, hi)) = self.bounding_box() else { return ORIGIN };
        let pick = |d: f64, l: f64, h: f64| {
            if d > 0.0 {
                h
            } else if d < 0.0 {
                l
            } else {
                (l + h) / 2.0
            }
        };
        Point::new(pick(dir.x, lo.x, hi.x), pick(dir.y, lo.y, hi.y), pick(dir.z, lo.z, hi.z))
    }

    /// The bounding-box point in direction `dir` (a corner or edge midpoint, see [`get_critical_point`](VMobject::get_critical_point)).
    pub fn get_corner(&self, dir: Point) -> Point {
        self.get_critical_point(dir)
    }
    /// Midpoint of the top edge.
    pub fn get_top(&self) -> Point {
        self.get_critical_point(UP)
    }
    /// Midpoint of the bottom edge.
    pub fn get_bottom(&self) -> Point {
        self.get_critical_point(DOWN)
    }
    /// Midpoint of the left edge.
    pub fn get_left(&self) -> Point {
        self.get_critical_point(LEFT)
    }
    /// Midpoint of the right edge.
    pub fn get_right(&self) -> Point {
        self.get_critical_point(RIGHT)
    }

    /// Bounding-box width in scene units.
    pub fn width(&self) -> f64 {
        self.bounding_box().map(|(lo, hi)| hi.x - lo.x).unwrap_or(0.0)
    }

    /// Bounding-box height in scene units.
    pub fn height(&self) -> f64 {
        self.bounding_box().map(|(lo, hi)| hi.y - lo.y).unwrap_or(0.0)
    }

    /// First point of the first path, or the origin if there is none.
    pub fn get_start(&self) -> Point {
        self.family().iter().find_map(|m| m.subpaths.first().and_then(|s| s.start())).unwrap_or(ORIGIN)
    }

    /// Last point of the last path, or the origin if there is none.
    pub fn get_end(&self) -> Point {
        self.family().iter().rev().find_map(|m| m.subpaths.last().and_then(|s| s.end())).unwrap_or(ORIGIN)
    }

    /// Point at fraction `alpha` of the total arc length.
    pub fn point_from_proportion(&self, alpha: f64) -> Point {
        let curves: Vec<&Cubic> =
            self.family().into_iter().flat_map(|m| m.subpaths.iter()).flat_map(|s| s.curves.iter()).collect();
        if curves.is_empty() {
            return self.get_center();
        }
        let lengths: Vec<f64> = curves.iter().map(|c| bezier::length(c)).collect();
        let total: f64 = lengths.iter().sum();
        if total <= 0.0 {
            return curves[0][0];
        }
        let mut target = alpha.clamp(0.0, 1.0) * total;
        for (c, l) in curves.iter().zip(&lengths) {
            if target <= *l || std::ptr::eq(*c, *curves.last().unwrap()) {
                let t = if *l > 0.0 { (target / l).clamp(0.0, 1.0) } else { 0.0 };
                return bezier::point_at(c, t);
            }
            target -= l;
        }
        curves.last().unwrap()[3]
    }

    // ------------------------------------------------------------------ transforms

    /// Translate by `v`.
    pub fn shift(self, v: Point) -> Self {
        self.map_points(|p| p + v)
    }

    /// Move so the center is at `p`.
    pub fn move_to(self, p: Point) -> Self {
        let c = self.get_center();
        self.shift(p - c)
    }

    /// Move so that the critical point in `aligned_edge` lands on `p`.
    pub fn move_to_aligned(self, p: Point, aligned_edge: Point) -> Self {
        let c = self.get_critical_point(aligned_edge);
        self.shift(p - c)
    }

    /// Move horizontally so the center's x is `x`.
    pub fn set_x(self, x: f64) -> Self {
        let c = self.get_center();
        self.shift(pt(x - c.x, 0.0))
    }

    /// Move vertically so the center's y is `y`.
    pub fn set_y(self, y: f64) -> Self {
        let c = self.get_center();
        self.shift(pt(0.0, y - c.y))
    }

    /// Move so the center is at the origin.
    pub fn center(self) -> Self {
        self.move_to(ORIGIN)
    }

    /// Scale about the center by `factor`.
    pub fn scale(self, factor: f64) -> Self {
        let c = self.get_center();
        self.scale_about(factor, c)
    }

    /// Scale about the point `about` by `factor`.
    pub fn scale_about(self, factor: f64, about: Point) -> Self {
        self.map_points(|p| about + (p - about) * factor)
    }

    /// Scale independently along one axis (0 = x, 1 = y).
    pub fn stretch(self, factor: f64, dim: usize) -> Self {
        if dim > 2 {
            return self;
        }
        let c = self.get_center();
        self.map_points(|mut p| {
            p[dim] = c[dim] + (p[dim] - c[dim]) * factor;
            p
        })
    }

    /// Scale uniformly to width `w` (no-op for zero width).
    pub fn set_width(self, w: f64) -> Self {
        let cur = self.width();
        if cur <= 0.0 {
            return self;
        }
        self.scale(w / cur)
    }

    /// Scale uniformly to height `h` (no-op for zero height).
    pub fn set_height(self, h: f64) -> Self {
        let cur = self.height();
        if cur <= 0.0 {
            return self;
        }
        self.scale(h / cur)
    }

    /// Rotate counter-clockwise about the mobject's center.
    pub fn rotate(self, angle: f64) -> Self {
        let c = self.get_center();
        self.rotate_about(angle, c)
    }

    /// Rotate counter-clockwise by `angle` radians about `about`.
    pub fn rotate_about(self, angle: f64, about: Point) -> Self {
        let (s, c) = angle.sin_cos();
        self.map_points(|p| {
            let d = p - about;
            about + Point::new(d.x * c - d.y * s, d.x * s + d.y * c, d.z)
        })
    }

    /// Mirror horizontally about the center.
    pub fn flip(self) -> Self {
        self.stretch(-1.0, 0)
    }

    /// Place next to `other` on side `dir`, with the default gap.
    pub fn next_to(self, other: &VMobject, dir: Point) -> Self {
        self.next_to_buff(other, dir, DEFAULT_MOBJECT_TO_MOBJECT_BUFFER)
    }

    /// Place next to `other` on side `dir`, leaving `buff` scene units.
    pub fn next_to_buff(self, other: &VMobject, dir: Point, buff: f64) -> Self {
        let target = other.get_critical_point(dir);
        self.next_to_point_buff(target, dir, buff)
    }

    /// Manim's `next_to(other, dir, buff, aligned_edge)`: also line up the
    /// `aligned_edge` sides, e.g. `next_to_aligned(&title, DOWN, 0.3, LEFT)` puts self
    /// under `title`, left edges flush.
    pub fn next_to_aligned(self, other: &VMobject, dir: Point, buff: f64, aligned_edge: Point) -> Self {
        let target = other.get_critical_point(aligned_edge + dir);
        let align = self.get_critical_point(aligned_edge - dir);
        self.shift(target - align + dir * buff)
    }

    /// Shrink (and with `grow`, enlarge up to `max_grow`×) to fit inside `width` × `height`,
    /// keeping the aspect ratio. The center stays put.
    pub fn fit_within(self, width: f64, height: f64, grow: bool, max_grow: f64) -> Self {
        let (w, h) = (self.width(), self.height());
        let mut factor = f64::INFINITY;
        if w > 0.0 {
            factor = factor.min(width / w);
        }
        if h > 0.0 {
            factor = factor.min(height / h);
        }
        if !factor.is_finite() {
            return self;
        }
        if factor < 1.0 || (grow && factor > 1.0) {
            let f = if grow { factor.min(max_grow.max(1.0)) } else { factor };
            self.scale(f)
        } else {
            self
        }
    }

    /// Tag what built this mobject (used by layout checks and inspectors).
    pub fn with_kind(mut self, kind: &str) -> Self {
        self.meta.kind = Some(kind.into());
        self
    }

    /// Attach human-readable content (e.g. a text's source string).
    pub fn with_label(mut self, label: &str) -> Self {
        self.meta.label = Some(label.into());
        self
    }

    /// Whether this mobject (or a descendant) draws text glyphs.
    pub fn has_text(&self) -> bool {
        self.family().iter().any(|m| m.meta.char_index.is_some() || matches!(m.meta.kind(), "Text" | "Math"))
    }

    /// The first `kind`/`label` found in this family (for reports).
    pub fn describe(&self) -> String {
        for m in self.family() {
            if let Some(l) = &m.meta.label {
                return l.to_string();
            }
        }
        self.family()
            .iter()
            .find_map(|m| m.meta.kind.as_ref().map(|k| k.to_string()))
            .unwrap_or_else(|| "VMobject".into())
    }

    /// Place next to the point `p` on side `dir`, with the default gap.
    pub fn next_to_point(self, p: Point, dir: Point) -> Self {
        self.next_to_point_buff(p, dir, DEFAULT_MOBJECT_TO_MOBJECT_BUFFER)
    }

    /// Place next to the point `p` on side `dir`, leaving `buff` scene units.
    pub fn next_to_point_buff(self, p: Point, dir: Point, buff: f64) -> Self {
        let align = self.get_critical_point(-dir);
        self.shift(p - align + dir * buff)
    }

    /// Move to the frame edge or corner in direction `edge`, keeping the default gap.
    pub fn to_edge(self, edge: Point) -> Self {
        self.to_edge_buff(edge, DEFAULT_MOBJECT_TO_EDGE_BUFFER)
    }

    /// Move to the frame edge or corner in direction `edge`, keeping `buff` scene units.
    pub fn to_edge_buff(self, edge: Point, buff: f64) -> Self {
        let sgn = |v: f64| {
            if v > 0.0 {
                1.0
            } else if v < 0.0 {
                -1.0
            } else {
                0.0
            }
        };
        let sign = Point::new(sgn(edge.x), sgn(edge.y), 0.0);
        let target = Point::new(sign.x * FRAME_X_RADIUS, sign.y * FRAME_Y_RADIUS, 0.0);
        let align = self.get_critical_point(edge);
        let shift = (target - align - edge * buff) * sign.abs();
        self.shift(shift)
    }

    /// Move to the frame corner in direction `corner` (same as [`to_edge`](VMobject::to_edge)).
    pub fn to_corner(self, corner: Point) -> Self {
        self.to_edge(corner)
    }

    /// Align the `dir` edge of self with the `dir` edge of `other`.
    pub fn align_to(self, other: &VMobject, dir: Point) -> Self {
        let target = other.get_critical_point(dir);
        let cur = self.get_critical_point(dir);
        let mask = Point::new((dir.x != 0.0) as i32 as f64, (dir.y != 0.0) as i32 as f64, 0.0);
        self.shift((target - cur) * mask)
    }

    /// Lay children out in a row/column, then center the group at the origin.
    pub fn arrange(self, dir: Point) -> Self {
        self.arrange_buff(dir, DEFAULT_MOBJECT_TO_MOBJECT_BUFFER)
    }

    /// Lay children out along `dir` with `buff` between them, then center the group at the origin.
    pub fn arrange_buff(mut self, dir: Point, buff: f64) -> Self {
        for i in 1..self.children.len() {
            let prev = self.children[i - 1].clone();
            self.children[i].apply(|c| c.next_to_buff(&prev, dir, buff));
        }
        self.center()
    }

    /// Grid layout with `cols` columns.
    pub fn arrange_in_grid(mut self, cols: usize, buff: f64) -> Self {
        let cols = cols.max(1);
        let cell_w = self.children.iter().map(|c| c.width()).fold(0.0, f64::max) + buff;
        let cell_h = self.children.iter().map(|c| c.height()).fold(0.0, f64::max) + buff;
        for (i, c) in self.children.iter_mut().enumerate() {
            let (r, k) = (i / cols, i % cols);
            c.apply(|m| m.move_to(pt(k as f64 * cell_w, -(r as f64) * cell_h)));
        }
        self.center()
    }

    // ------------------------------------------------------------------ style

    fn map_style(mut self, mut f: impl FnMut(&mut VMobject)) -> Self {
        self.for_each_mut(&mut |m| f(m));
        self
    }

    /// Set fill and stroke color of the whole family.
    pub fn set_color(self, c: Color) -> Self {
        self.map_style(|m| {
            m.fill_color = c;
            m.stroke_color = c;
        })
    }

    /// Set fill color and opacity of the whole family.
    pub fn set_fill(self, c: Color, opacity: f64) -> Self {
        self.map_style(|m| {
            m.fill_color = c;
            m.fill_opacity = opacity;
        })
    }

    /// Set fill color of the whole family.
    pub fn set_fill_color(self, c: Color) -> Self {
        self.map_style(|m| m.fill_color = c)
    }

    /// Set fill opacity of the whole family.
    pub fn set_fill_opacity(self, o: f64) -> Self {
        self.map_style(|m| m.fill_opacity = o)
    }

    /// Set stroke color and width of the whole family.
    pub fn set_stroke(self, c: Color, width: f64) -> Self {
        self.map_style(|m| {
            m.stroke_color = c;
            m.stroke_width = width;
        })
    }

    /// Set stroke color of the whole family.
    pub fn set_stroke_color(self, c: Color) -> Self {
        self.map_style(|m| m.stroke_color = c)
    }

    /// Set stroke width of the whole family.
    pub fn set_stroke_width(self, w: f64) -> Self {
        self.map_style(|m| m.stroke_width = w)
    }

    /// Set stroke opacity of the whole family.
    pub fn set_stroke_opacity(self, o: f64) -> Self {
        self.map_style(|m| m.stroke_opacity = o)
    }

    /// Set fill and stroke opacity of the whole family.
    pub fn set_opacity(self, o: f64) -> Self {
        self.map_style(|m| {
            m.fill_opacity = o;
            m.stroke_opacity = o;
        })
    }

    /// Multiply all opacities by `(1 - darkness)`.
    pub fn fade(self, darkness: f64) -> Self {
        let k = 1.0 - darkness.clamp(0.0, 1.0);
        self.map_style(|m| {
            m.fill_opacity *= k;
            m.stroke_opacity *= k;
        })
    }

    /// Color children along a gradient.
    pub fn set_color_by_gradient(mut self, colors: &[Color]) -> Self {
        let n = self.children.len().max(1);
        let grad = crate::color::color_gradient(colors, n);
        if self.children.is_empty() {
            return self.set_color(colors.first().copied().unwrap_or(WHITE));
        }
        for (c, col) in self.children.iter_mut().zip(grad) {
            c.apply(|m| m.set_color(col));
        }
        self
    }

    /// Set the draw order (higher draws on top).
    pub fn set_z_index(mut self, z: i32) -> Self {
        self.z_index = z;
        self
    }

    // ------------------------------------------------------------------ text selection

    /// The source string of a text mobject (`Text`, `DecimalNumber`, math), if any.
    pub fn text(&self) -> Option<&str> {
        self.meta.label.as_deref()
    }

    /// Glyphs drawing characters `start..end` of the text (char indices, spaces count),
    /// as a group of copies. Use it to measure or frame part of a sentence:
    /// `SurroundingRectangle::new(&sentence.chars(6, 11))`.
    pub fn chars(&self, start: usize, end: usize) -> VMobject {
        let picked = self
            .family()
            .into_iter()
            .filter(|m| m.meta.char_index.is_some_and(|i| (i as usize) >= start && (i as usize) < end))
            .map(|m| VMobject { children: Vec::new(), ..m.clone() })
            .collect();
        VMobject::group(picked)
    }

    /// Char range of the first occurrence of `needle` in this text.
    pub fn find_text(&self, needle: &str) -> Option<(usize, usize)> {
        let hay = self.text()?;
        let byte = hay.find(needle)?;
        let start = hay[..byte].chars().count();
        Some((start, start + needle.chars().count()))
    }

    /// Glyphs of the first occurrence of `needle` (an empty group if absent).
    pub fn part(&self, needle: &str) -> VMobject {
        match self.find_text(needle) {
            Some((a, b)) => self.chars(a, b),
            None => VMobject::group(vec![]),
        }
    }

    /// Recolor the glyphs of characters `start..end`.
    pub fn set_color_chars(self, start: usize, end: usize, c: Color) -> Self {
        self.map_style(|m| {
            if m.meta.char_index.is_some_and(|i| (i as usize) >= start && (i as usize) < end) {
                m.fill_color = c;
                m.stroke_color = c;
            }
        })
    }

    /// Recolor every occurrence of `needle`.
    pub fn set_color_text(self, needle: &str, c: Color) -> Self {
        let Some(hay) = self.text().map(str::to_string) else { return self };
        if needle.is_empty() {
            return self;
        }
        let mut out = self;
        for (byte, _) in hay.match_indices(needle) {
            let a = hay[..byte].chars().count();
            out = out.set_color_chars(a, a + needle.chars().count(), c);
        }
        out
    }

    // ------------------------------------------------------------------ animation helpers

    /// Keep only the first `alpha` fraction of the curves (Manim's `pointwise_become_partial`).
    pub fn partial(&self, alpha: f64) -> VMobject {
        self.partial_range(0.0, alpha)
    }

    /// Keep the curves between fractions `a` and `b` of the total.
    pub fn partial_range(&self, a: f64, b: f64) -> VMobject {
        let mut out = VMobject { subpaths: Vec::new(), children: Vec::new(), ..self.clone() };
        let total: usize = self.subpaths.iter().map(|s| s.curves.len()).sum();
        if total == 0 || b <= a {
            return out;
        }
        let (lo, hi) = (a.clamp(0.0, 1.0) * total as f64, b.clamp(0.0, 1.0) * total as f64);
        let mut idx = 0usize;
        for sp in &self.subpaths {
            let mut curves = Vec::new();
            let mut whole = true;
            for c in &sp.curves {
                let (c0, c1) = (idx as f64, idx as f64 + 1.0);
                idx += 1;
                if c1 <= lo || c0 >= hi {
                    whole = false;
                    continue;
                }
                let s = (lo - c0).max(0.0);
                let e = (hi - c0).min(1.0);
                whole &= s <= 0.0 && e >= 1.0;
                curves.push(bezier::partial(c, s, e));
            }
            if !curves.is_empty() {
                out.subpaths.push(Subpath { curves, closed: sp.closed && whole });
            }
        }
        out
    }
}

// ---------------------------------------------------------------------- alignment & interpolation

fn collapsed_copies(leaves: &[VMobject], at: Point) -> Vec<VMobject> {
    leaves
        .iter()
        .map(|l| {
            let mut c = l.clone();
            for sp in &mut c.subpaths {
                sp.curves.fill([at; 4]);
            }
            c
        })
        .collect()
}

/// Make two mobjects structurally compatible: equal leaf counts, subpath counts and curve counts.
/// Returns the flattened, aligned leaves of each.
///
/// Unlike Manim CE (pairing by index), leaves pair up in reading order by position and
/// shape, subpaths by size and position, and closed paths start where they travel least;
/// see the `align` module.
pub fn align(a: &VMobject, b: &VMobject) -> (Vec<VMobject>, Vec<VMobject>) {
    let mut la = a.leaves();
    let mut lb = b.leaves();
    if la.is_empty() && lb.is_empty() {
        return (la, lb);
    }
    if la.is_empty() {
        la = collapsed_copies(&lb, a.get_center());
    }
    if lb.is_empty() {
        lb = collapsed_copies(&la, b.get_center());
        for l in &mut lb {
            l.fill_opacity = 0.0;
            l.stroke_opacity = 0.0;
        }
    }
    align::align_leaves(la, lb)
}

pub(crate) use align::{match_invisible_style, same_shape};

/// How points travel during an interpolation.
#[derive(Clone, Copy, Debug)]
pub enum PathFunc {
    /// Points move in straight lines.
    Straight,
    /// Travel along circular arcs subtending this angle (Manim's `path_arc`).
    Arc(f64),
}

impl PathFunc {
    /// The point `t` of the way from `a` to `b` along this path.
    pub fn apply(self, a: Point, b: Point, t: f64) -> Point {
        match self {
            PathFunc::Straight => a.lerp(b, t),
            PathFunc::Arc(angle) if angle.abs() < 1e-6 => a.lerp(b, t),
            PathFunc::Arc(angle) => {
                let half_tan = (angle / 2.0).tan();
                // A whole number of turns has no finite arc center: travel straight.
                if half_tan.abs() < 1e-6 || !half_tan.is_finite() {
                    return a.lerp(b, t);
                }
                let v = b - a;
                let center = a + v * 0.5 + OUT.cross(v * 0.5) / half_tan;
                let (s, c) = (angle * t).sin_cos();
                let d = a - center;
                center + Point::new(d.x * c - d.y * s, d.x * s + d.y * c, d.z)
            }
        }
    }
}

/// Interpolate two structurally aligned leaves.
pub fn lerp_leaf(a: &VMobject, b: &VMobject, t: f64, path: PathFunc) -> VMobject {
    let subpaths = a
        .subpaths
        .iter()
        .zip(&b.subpaths)
        .map(|(sa, sb)| Subpath {
            curves: sa
                .curves
                .iter()
                .zip(&sb.curves)
                .map(|(ca, cb)| std::array::from_fn(|i| path.apply(ca[i], cb[i], t)))
                .collect(),
            closed: if t >= 1.0 { sb.closed } else { sa.closed && sb.closed },
        })
        .collect();
    let l = |x: f64, y: f64| x + (y - x) * t;
    VMobject {
        subpaths,
        fill_color: a.fill_color.lerp(b.fill_color, t),
        fill_opacity: l(a.fill_opacity, b.fill_opacity),
        stroke_color: a.stroke_color.lerp(b.stroke_color, t),
        stroke_opacity: l(a.stroke_opacity, b.stroke_opacity),
        stroke_width: l(a.stroke_width, b.stroke_width),
        z_index: if t >= 1.0 { b.z_index } else { a.z_index },
        children: Vec::new(),
        meta: {
            let mut meta = if t >= 1.0 { b.meta.clone() } else { a.meta.clone() };
            if let (Some(x), Some(y)) = (a.meta.anchors, b.meta.anchors) {
                meta.anchors = Some(std::array::from_fn(|i| path.apply(x[i], y[i], t)));
            }
            meta
        },
    }
}

/// Interpolate two lists of aligned leaves into a group.
pub fn lerp_leaves(a: &[VMobject], b: &[VMobject], t: f64, path: PathFunc, z_index: i32) -> VMobject {
    let children = a.iter().zip(b).map(|(x, y)| lerp_leaf(x, y, t, path)).collect();
    VMobject { z_index, ..VMobject::group(children) }
}

#[cfg(test)]
mod tests {
    use super::geometry::*;
    use super::*;

    #[test]
    fn align_square_and_circle() {
        let (a, b) = align(&Square::new(2.0), &Circle::new(1.0));
        assert_eq!(a.len(), b.len());
        assert_eq!(a[0].subpaths[0].curves.len(), b[0].subpaths[0].curves.len());
    }

    #[test]
    fn align_group_with_single() {
        let g = vgroup![Square::new(1.0), Circle::new(1.0), Triangle::new()];
        let (a, b) = align(&Circle::new(1.0), &g);
        assert_eq!(a.len(), 3);
        assert_eq!(b.len(), 3);
    }

    #[test]
    fn partial_halves_curves() {
        let sq = Square::new(2.0);
        assert_eq!(sq.partial(0.5).num_curves(), 2);
        assert_eq!(sq.partial(0.0).num_curves(), 0);
        assert!(sq.partial(1.0).subpaths[0].closed);
    }

    #[test]
    fn next_to_places_right() {
        let a = Square::new(2.0);
        let b = Square::new(2.0).next_to_buff(&a, RIGHT, 0.5);
        assert!((b.get_left().x - 1.5).abs() < 1e-9);
        assert!(b.get_center().y.abs() < 1e-9);
    }

    #[test]
    fn to_edge_respects_buffer() {
        let s = Square::new(1.0).to_edge(LEFT);
        assert!((s.get_left().x - (-FRAME_X_RADIUS + DEFAULT_MOBJECT_TO_EDGE_BUFFER)).abs() < 1e-9);
    }
}
