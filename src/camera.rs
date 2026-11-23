//! Camera: turns the world into a renderer-agnostic display list in pixel space.

use std::sync::{Arc, Mutex};

use crate::bezier;
use crate::color::{Color, BLACK};
use crate::constants::*;
use crate::mobject::VMobject;
use crate::renderer::{DisplayList, DrawImage, DrawPath, Layer, PxGradient, PxShadow, PxSubpath};
use crate::world::World;

/// Matches Cairo Manim: 1 stroke-width unit = 0.01 scene units.
const STROKE_WIDTH_UNIT: f64 = 0.01;

/// Maps scene coordinates to pixels and turns a [`World`] into a [`DisplayList`].
///
/// `frame_height` and `frame_center` describe the *base* frame. A scene can zoom and move the
/// view away from it with a camera animation; that live frame is kept in the [`World`]
/// (see [`World::frame`]) and applied by [`Camera::display_list`].
#[derive(Clone, Debug)]
pub struct Camera {
    /// Output width in pixels.
    pub pixel_width: u32,
    /// Output height in pixels.
    pub pixel_height: u32,
    /// Height of the visible area in scene units; the width follows from the pixel aspect ratio.
    pub frame_height: f64,
    /// Scene point shown at the center of the frame.
    pub frame_center: Point,
    /// Frame background; its alpha is ignored (frames are always opaque).
    pub background: Color,
    layer_cache: Arc<Mutex<Option<Layer>>>,
}

impl Camera {
    /// A camera with the default frame (height 8 units, centered on the origin) and a black background.
    pub fn new(pixel_width: u32, pixel_height: u32) -> Self {
        Camera {
            pixel_width,
            pixel_height,
            frame_height: FRAME_HEIGHT,
            frame_center: ORIGIN,
            background: BLACK,
            layer_cache: Default::default(),
        }
    }

    /// Width of the visible area in scene units.
    pub fn frame_width(&self) -> f64 {
        self.frame_height * self.pixel_width as f64 / self.pixel_height as f64
    }

    /// The frame shown now (centre and height in scene units): the base frame zoomed and moved
    /// by the world's [`FrameState`](crate::world::FrameState).
    pub fn live_frame(&self, world: &World) -> (Point, f64) {
        let f = world.frame();
        (f.center.unwrap_or(self.frame_center), self.frame_height / f.zoom)
    }

    /// Pixel position (x right, y down, origin top-left) of a scene point in the base frame.
    pub fn to_pixel(&self, p: Point) -> [f32; 2] {
        self.view(self.frame_center, self.frame_height).to_pixel(p)
    }

    fn view(&self, center: Point, height: f64) -> View {
        View { center, height, px_w: self.pixel_width as f64, px_h: self.pixel_height as f64 }
    }

    /// Build the display list for the current state of `world`: displayed mobjects sorted by z-index, then the overlay. The background layer is reused between frames when unchanged.
    ///
    /// Displayed mobjects are drawn through the world's live frame; the background layer, the
    /// overlay and mobjects marked [`VMobject::pin`] use the base frame.
    pub fn display_list(&self, world: &World) -> DisplayList {
        let base = self.view(self.frame_center, self.frame_height);
        let (center, height) = self.live_frame(world);
        let live = self.view(center, height);
        let mut ids: Vec<_> = world.displayed().to_vec();
        ids.sort_by_key(|id| world[*id].z_index); // stable: keeps add order within a z level
        let mut items = Vec::new();
        for id in ids {
            self.push_items(&world[id], &live, &base, &mut items);
        }
        for id in world.overlay() {
            self.push_items(&world[*id], &base, &base, &mut items);
        }
        let background_layer = if world.background().is_empty() {
            None
        } else {
            let mut layer = Vec::new();
            for id in world.background() {
                self.push_items(&world[*id], &base, &base, &mut layer);
            }
            // Reuse the previous frame's Arc when nothing changed, so renderers can
            // cache the rasterized layer and frame dedup compares pointers.
            let prev = self.layer_cache.lock().unwrap_or_else(|e| e.into_inner()).clone();
            match prev {
                Some(prev) if *prev.0 == layer => Some(prev),
                _ => {
                    let l = Layer(Arc::new(layer));
                    *self.layer_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(l.clone());
                    Some(l)
                }
            }
        };
        DisplayList {
            width: self.pixel_width,
            height: self.pixel_height,
            // The frame is always opaque (premultiplied and straight pixels coincide, and the
            // PNG / video writers expect that): a translucent background color is made solid.
            background: {
                let b = self.background;
                let mut rgba = Color::rgb(b.r, b.g, b.b).to_array(1.0);
                rgba[3] = 1.0;
                rgba
            },
            background_layer,
            items,
        }
    }

    fn push_items(&self, mob: &VMobject, live: &View, base: &View, items: &mut Vec<DrawPath>) {
        for m in mob.family_iter() {
            let view = if m.effects.pinned { base } else { live };
            let px_per_unit = view.px_w / view.width();
            let clip = m.effects.clip.and_then(|[l, b, r, t]| {
                let (a, c) = (base.to_pixel(Point::new(l, t, 0.0)), base.to_pixel(Point::new(r, b, 0.0)));
                // Whole pixels, so the clip edge is the same on every rasterizer.
                let rect = [a[0].round(), a[1].round(), c[0].round(), c[1].round()];
                rect.iter().all(|v| v.is_finite()).then_some(rect)
            });
            if let (Some(image), Some([ul, ur, dl])) = (&m.meta.image, m.meta.anchors) {
                let opacity = m.fill_opacity as f32;
                if opacity <= 0.0 {
                    continue;
                }
                let (w, h) = (image.0.width.max(1) as f32, image.0.height.max(1) as f32);
                let (o, x, y) = (view.to_pixel(ul), view.to_pixel(ur), view.to_pixel(dl));
                let transform =
                    [(x[0] - o[0]) / w, (x[1] - o[1]) / w, (y[0] - o[0]) / h, (y[1] - o[1]) / h, o[0], o[1]];
                if transform.iter().all(|v| v.is_finite()) {
                    items.push(DrawPath {
                        image: Some(DrawImage { image: image.clone(), transform, opacity }),
                        clip,
                        ..Default::default()
                    });
                }
                continue;
            }
            if m.subpaths.is_empty() {
                continue;
            }
            let fx = &m.effects;
            let fill = (m.fill_opacity > 0.0).then(|| m.fill_color.to_array(m.fill_opacity)).filter(|c| c[3] > 0.0);
            let stroke_px = m.stroke_width * STROKE_WIDTH_UNIT * px_per_unit;
            let stroke = (stroke_px > 0.0 && m.stroke_opacity > 0.0)
                .then(|| (m.stroke_color.to_array(m.stroke_opacity), stroke_px as f32))
                .filter(|(c, _)| c[3] > 0.0);
            let blur = (fx.backdrop_blur * px_per_unit) as f32;
            let blur = if blur.is_finite() && blur > 0.0 { blur } else { 0.0 };
            let gradient =
                if fill.is_some() { fx.gradient.as_ref().and_then(|g| self.gradient(m, g, view)) } else { None };
            // A gradient still counts as "filled"; the solid colour is only a stand-in for renderers that ignore it.
            let fill = match &gradient {
                Some(g) => Some(g.stops[0].1),
                None => fill,
            };
            let shadow = fill.and(fx.shadow.as_ref()).and_then(|s| {
                let color = s.color.to_array(m.fill_opacity);
                let sigma = (s.blur * px_per_unit) as f32;
                let offset = [(s.dx * px_per_unit) as f32, (-s.dy * px_per_unit) as f32];
                (color[3] > 0.0 && sigma.is_finite() && offset.iter().all(|v| v.is_finite())).then_some(PxShadow {
                    color,
                    offset,
                    sigma,
                })
            });
            if fill.is_none() && stroke.is_none() && blur == 0.0 {
                continue;
            }
            let subpaths: Vec<PxSubpath> = m
                .subpaths
                .iter()
                .filter(|s| !s.curves.is_empty())
                .map(|s| PxSubpath {
                    start: view.to_pixel(s.curves[0][0]),
                    curves: s
                        .curves
                        .iter()
                        .map(|c| [view.to_pixel(c[1]), view.to_pixel(c[2]), view.to_pixel(c[3])])
                        .collect(),
                    closed: s.closed,
                })
                // Drop non-finite subpaths (e.g. NaN from a user's `apply_function`, or
                // coordinates beyond f32 range) instead of handing them to the rasterizers.
                .filter(|s| s.start.iter().chain(s.curves.iter().flatten().flatten()).all(|v| v.is_finite()))
                .collect();
            if subpaths.is_empty() {
                continue;
            }
            let nonzero = m.meta.nonzero;
            match fx.dash.filter(|_| stroke.is_some()) {
                Some(d) => {
                    // The fill (and shadow, blur) keep the whole outline; the stroke becomes dashes.
                    if fill.is_some() || blur > 0.0 {
                        items.push(DrawPath {
                            subpaths: subpaths.clone(),
                            fill,
                            nonzero,
                            gradient,
                            shadow,
                            backdrop_blur: blur,
                            clip,
                            ..Default::default()
                        });
                    }
                    let dashes =
                        dash_subpaths(&subpaths, d.length * px_per_unit, d.gap * px_per_unit, d.phase * px_per_unit);
                    if !dashes.is_empty() {
                        items.push(DrawPath { subpaths: dashes, stroke, nonzero, clip, ..Default::default() });
                    }
                }
                None => items.push(DrawPath {
                    subpaths,
                    fill,
                    stroke,
                    nonzero,
                    gradient,
                    shadow,
                    backdrop_blur: blur,
                    clip,
                    ..Default::default()
                }),
            }
        }
    }

    /// Resolve a mobject's gradient over its bounding box into pixels.
    fn gradient(&self, m: &VMobject, g: &crate::effects::Gradient, view: &View) -> Option<PxGradient> {
        let (lo, hi) = m.bounding_box()?;
        let (c, size) = ((lo + hi) / 2.0, hi - lo);
        let d = Point::new(g.angle.cos(), g.angle.sin(), 0.0);
        // The gradient line is as long as the box's extent along its direction, so the corners get the end stops.
        let half = (size.x * d.x.abs() + size.y * d.y.abs()) / 2.0;
        let stops: Vec<_> =
            g.stops.iter().map(|s| (s.at.clamp(0.0, 1.0) as f32, s.color.to_array(m.fill_opacity))).collect();
        let (start, end) = (view.to_pixel(c - d * half), view.to_pixel(c + d * half));
        let ok = start.iter().chain(&end).all(|v| v.is_finite()) && stops.len() >= 2;
        ok.then_some(PxGradient { start, end, stops })
    }
}

/// A frame: which scene rectangle is visible, and how it maps to pixels.
struct View {
    center: Point,
    height: f64,
    px_w: f64,
    px_h: f64,
}

impl View {
    fn width(&self) -> f64 {
        self.height * self.px_w / self.px_h
    }

    fn to_pixel(&self, p: Point) -> [f32; 2] {
        let d = p - self.center;
        [((d.x / self.width() + 0.5) * self.px_w) as f32, ((0.5 - d.y / self.height) * self.px_h) as f32]
    }
}

/// Cut subpaths into dashes of `len` pixels with `gap` between them, the pattern shifted by
/// `phase`. A closed subpath is walked all the way round, including its closing edge.
fn dash_subpaths(subpaths: &[PxSubpath], len: f64, gap: f64, phase: f64) -> Vec<PxSubpath> {
    const MAX_DASHES: usize = 200_000;
    let period = len + gap;
    if !(len.is_finite() && gap.is_finite() && phase.is_finite()) || len <= 1e-3 || period <= 1e-3 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for sp in subpaths {
        let mut cubics: Vec<bezier::Cubic> = Vec::with_capacity(sp.curves.len() + 1);
        let mut at = pt2(sp.start);
        for c in &sp.curves {
            let cubic = [at, pt2(c[0]), pt2(c[1]), pt2(c[2])];
            cubics.push(cubic);
            at = cubic[3];
        }
        if sp.closed && at.distance(pt2(sp.start)) > 1e-3 {
            cubics.push(bezier::line(at, pt2(sp.start)));
        }
        // `pos` is where we are inside the pattern (dash first, then gap); the pattern starts `phase` into itself.
        let mut pos = phase.rem_euclid(period);
        let mut current: Vec<bezier::Cubic> = Vec::new();
        for cubic in &cubics {
            let total = bezier::length(cubic);
            if total <= 1e-9 {
                continue;
            }
            let table = length_table(cubic);
            let mut s = 0.0;
            while s < total - 1e-9 {
                let in_dash = pos < len;
                let to_edge = if in_dash { len - pos } else { period - pos };
                let step = to_edge.min(total - s);
                if in_dash {
                    current.push(bezier::partial(cubic, t_of(&table, s, total), t_of(&table, s + step, total)));
                }
                s += step;
                pos += step;
                if step >= to_edge - 1e-9 {
                    // Reached the end of the dash or of the gap.
                    if in_dash {
                        flush(&mut current, &mut out);
                        if out.len() > MAX_DASHES {
                            return out;
                        }
                        pos = len;
                    } else {
                        pos = 0.0;
                    }
                }
            }
        }
        flush(&mut current, &mut out);
    }
    out
}

fn pt2(p: [f32; 2]) -> Point {
    Point::new(p[0] as f64, p[1] as f64, 0.0)
}

fn flush(current: &mut Vec<bezier::Cubic>, out: &mut Vec<PxSubpath>) {
    if let Some(first) = current.first() {
        let f = |p: Point| [p.x as f32, p.y as f32];
        out.push(PxSubpath {
            start: f(first[0]),
            curves: current.iter().map(|c| [f(c[1]), f(c[2]), f(c[3])]).collect(),
            closed: false,
        });
    }
    current.clear();
}

/// Cumulative arc length at 32 evenly spaced parameters of a cubic.
fn length_table(c: &bezier::Cubic) -> [f64; 33] {
    let mut t = [0.0; 33];
    let mut prev = c[0];
    for (i, slot) in t.iter_mut().enumerate().skip(1) {
        let p = bezier::point_at(c, i as f64 / 32.0);
        *slot = prev.distance(p);
        prev = p;
    }
    for i in 1..33 {
        t[i] += t[i - 1];
    }
    t
}

/// The curve parameter at arc length `s` (of `total`), by interpolating the table.
fn t_of(table: &[f64; 33], s: f64, total: f64) -> f64 {
    let end = table[32].max(1e-12);
    let target = (s / total * end).clamp(0.0, end);
    let i = table.partition_point(|&v| v < target).clamp(1, 32);
    let (lo, hi) = (table[i - 1], table[i]);
    let f = if hi > lo { (target - lo) / (hi - lo) } else { 0.0 };
    ((i - 1) as f64 + f) / 32.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x0: f32, x1: f32) -> PxSubpath {
        PxSubpath { start: [x0, 0.0], curves: vec![[[x0, 0.0], [x1, 0.0], [x1, 0.0]]], closed: false }
    }

    fn spans(d: &[PxSubpath]) -> Vec<(f32, f32)> {
        d.iter().map(|s| (s.start[0], s.curves.last().unwrap()[2][0])).collect()
    }

    #[test]
    fn dashes_cover_the_pattern() {
        let d = dash_subpaths(&[line(0.0, 100.0)], 10.0, 5.0, 0.0);
        let s = spans(&d);
        assert_eq!(s.len(), 7);
        for (i, (a, b)) in s.iter().enumerate() {
            assert!((a - 15.0 * i as f32).abs() < 0.2, "dash {i} starts at {a}");
            assert!((b - a - 10.0).abs() < 0.2, "dash {i} is {} long", b - a);
        }
    }

    #[test]
    fn phase_shifts_the_pattern() {
        // A phase of 5 starts 5 px into the first dash: it is 5 px long, then a gap of 5.
        let s = spans(&dash_subpaths(&[line(0.0, 100.0)], 10.0, 5.0, 5.0));
        assert!((s[0].0).abs() < 0.2 && (s[0].1 - 5.0).abs() < 0.2, "{:?}", s[0]);
        assert!((s[1].0 - 10.0).abs() < 0.2, "{:?}", s[1]);
        // A phase of a whole period is no shift at all.
        assert_eq!(dash_subpaths(&[line(0.0, 100.0)], 10.0, 5.0, 15.0).len(), 7);
    }

    #[test]
    fn dashes_continue_across_curves() {
        let two = PxSubpath {
            start: [0.0, 0.0],
            curves: vec![[[0.0, 0.0], [6.0, 0.0], [6.0, 0.0]], [[6.0, 0.0], [20.0, 0.0], [20.0, 0.0]]],
            closed: false,
        };
        let s = spans(&dash_subpaths(&[two], 10.0, 5.0, 0.0));
        // One dash [0, 10] spans the join at x = 6 and is a single subpath of two curves.
        assert_eq!(s.len(), 2, "{s:?}");
        assert!((s[0].1 - 10.0).abs() < 0.2 && (s[1].0 - 15.0).abs() < 0.2, "{s:?}");
    }

    #[test]
    fn degenerate_patterns_draw_nothing() {
        assert!(dash_subpaths(&[line(0.0, 10.0)], 0.0, 5.0, 0.0).is_empty());
        assert!(dash_subpaths(&[line(0.0, 10.0)], f64::NAN, 5.0, 0.0).is_empty());
    }
}
