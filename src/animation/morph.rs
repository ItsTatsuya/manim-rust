//! Point-wise morphing animations: Transform and friends, fades, growth, `.animate`.

use std::cell::RefCell;
use std::rc::Rc;

use super::{impl_config, AnimConfig, Animation};
use crate::color::{Color, YELLOW};
use crate::constants::*;
use crate::mobject::{align, lerp_leaves, same_shape, PathFunc, VMobject};
use crate::rate_functions;
use crate::world::{MobjectId, World};

type Builder = Box<dyn FnOnce(VMobject) -> VMobject>;

/// `.animate`'s closure runs once; copies of the animation (timeline replays) reuse its
/// result, which is the same because they start from the same world.
struct AnimateCell {
    f: RefCell<Option<Builder>>,
    out: RefCell<Option<VMobject>>,
}

#[derive(Clone)]
enum Kind {
    Transform(Box<VMobject>),
    Replacement(MobjectId),
    FromCopy(MobjectId),
    /// Cross-fade: each side stretches to the other's box while fading.
    FadeTransform(MobjectId),
    /// Matching pieces morph, the rest fade (`transform_mismatches`: morph them too).
    Matching {
        to: MobjectId,
        transform_mismatches: bool,
    },
    Animate(Rc<AnimateCell>),
    FadeIn,
    FadeOut,
    Grow(Option<Point>),
    GrowEdge(Point),
    Shrink,
    Indicate(f64, Color),
}

/// Interpolates a mobject from a start state to an end state. Built by the
/// constructors [`Transform`], [`FadeIn`], [`GrowFromCenter`], `id.animate(...)`, etc.
#[derive(Clone)]
pub struct Morph {
    id: MobjectId,
    cfg: AnimConfig,
    kind: Kind,
    name: &'static str,
    path: PathFunc,
    shift: Point,
    scale: f64,
    /// The mobject actually being interpolated (differs from `id` for `TransformFromCopy`).
    work: MobjectId,
    start: Vec<VMobject>,
    end: Vec<VMobject>,
    final_state: Option<VMobject>,
    z_index: i32,
}

impl Morph {
    fn make(id: MobjectId, kind: Kind, name: &'static str) -> Self {
        Morph {
            id,
            cfg: AnimConfig::default(),
            kind,
            name,
            path: PathFunc::Straight,
            shift: ORIGIN,
            scale: 1.0,
            work: id,
            start: Vec::new(),
            end: Vec::new(),
            final_state: None,
            z_index: 0,
        }
    }

    /// Move points along arcs of this angle instead of straight lines.
    pub fn path_arc(mut self, angle: f64) -> Self {
        self.path = PathFunc::Arc(angle);
        self
    }

    /// For matching transforms: morph unmatched pieces into each other instead of fading.
    pub fn transform_mismatches(mut self, on: bool) -> Self {
        if let Kind::Matching { transform_mismatches, .. } = &mut self.kind {
            *transform_mismatches = on;
        }
        self
    }

    /// For fades: travel by this vector while fading.
    pub fn shift(mut self, v: Point) -> Self {
        self.shift = v;
        self
    }

    /// For fades: scale from/to this factor while fading.
    pub fn scale(mut self, f: f64) -> Self {
        self.scale = f;
        self
    }
}

/// Namespace for [`Transform`]-style morphs: change a mobject into a target shape.
pub struct Transform;
impl Transform {
    /// Morph `id` into `target`. `id` takes `target`'s shape afterwards.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId, target: VMobject) -> Morph {
        Morph::make(id, Kind::Transform(Box::new(target)), "Transform")
    }
}

/// Namespace for `ReplacementTransform`: morph one mobject into another, which then takes its place.
pub struct ReplacementTransform;
impl ReplacementTransform {
    /// Morph `a` into the (not yet displayed) mobject `b`; `a` is then replaced by `b` in the scene.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(a: MobjectId, b: MobjectId) -> Morph {
        Morph::make(a, Kind::Replacement(b), "ReplacementTransform")
    }
}

/// Namespace for `TransformFromCopy`: morph a copy, leaving the original in place.
pub struct TransformFromCopy;
impl TransformFromCopy {
    /// A copy of `a` morphs into `b`; `a` stays put.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(a: MobjectId, b: MobjectId) -> Morph {
        Morph::make(a, Kind::FromCopy(b), "TransformFromCopy")
    }
}

/// Namespace for `FadeTransform`: cross-fade between two mobjects.
pub struct FadeTransform;
impl FadeTransform {
    /// Cross-fade `a` into `b`: `a` stretches onto `b`'s box while fading out, `b` grows
    /// out of `a`'s box while fading in; `b` then replaces `a` in the scene (Manim CE's
    /// `FadeTransform` with `stretch=True`).
    #[allow(clippy::new_ret_no_self)]
    pub fn new(a: MobjectId, b: MobjectId) -> Morph {
        Morph::make(a, Kind::FadeTransform(b), "FadeTransform")
    }
}

/// Namespace for `TransformMatchingShapes`: match same-shaped pieces and morph them.
pub struct TransformMatchingShapes;
impl TransformMatchingShapes {
    /// Pieces of `a` with the same shape as pieces of `b` (the same glyph, at any size)
    /// move onto them; the others fade out and in. `b` then replaces `a` in the scene.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(a: MobjectId, b: MobjectId) -> Morph {
        Morph::make(a, Kind::Matching { to: b, transform_mismatches: false }, "TransformMatchingShapes")
    }
}

/// Namespace for `TransformMatchingTex`: [`TransformMatchingShapes`] for equations.
pub struct TransformMatchingTex;
impl TransformMatchingTex {
    /// [`TransformMatchingShapes`] for equations. Manim CE matches by TeX substring; here
    /// an equation is one typeset piece, so glyphs are matched by shape, which also pairs
    /// up symbols that moved inside a substring.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(a: MobjectId, b: MobjectId) -> Morph {
        Morph::make(a, Kind::Matching { to: b, transform_mismatches: false }, "TransformMatchingTex")
    }
}

/// Namespace for fading a mobject in.
pub struct FadeIn;
impl FadeIn {
    /// Fade `id` in (also scale or shift with the returned [`Morph`]'s builders).
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Morph {
        Morph::make(id, Kind::FadeIn, "FadeIn")
    }
}

/// Namespace for fading a mobject out and removing it.
pub struct FadeOut;
impl FadeOut {
    /// Fade `id` out, then take it off the scene.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Morph {
        Morph::make(id, Kind::FadeOut, "FadeOut")
    }
}

/// Namespace for growing a mobject from its center.
pub struct GrowFromCenter;
impl GrowFromCenter {
    /// Grow `id` from its center.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Morph {
        Morph::make(id, Kind::Grow(None), "GrowFromCenter")
    }
}

/// Namespace for growing a mobject from a point.
pub struct GrowFromPoint;
impl GrowFromPoint {
    /// Grow `id` from `point`.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId, point: Point) -> Morph {
        Morph::make(id, Kind::Grow(Some(point)), "GrowFromPoint")
    }
}

/// Namespace for growing a mobject from one of its edges.
pub struct GrowFromEdge;
impl GrowFromEdge {
    /// Grow out of the `edge` side (`LEFT` sweeps a highlight in left to right).
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId, edge: Point) -> Morph {
        Morph::make(id, Kind::GrowEdge(edge), "GrowFromEdge")
    }
}

/// Namespace for growing a mobject from its center while spinning.
pub struct SpinInFromNothing;
impl SpinInFromNothing {
    /// Grow `id` from its center along a quarter-turn arc.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Morph {
        Morph::make(id, Kind::Grow(None), "SpinInFromNothing").path_arc(PI / 2.0)
    }
}

/// Namespace for shrinking a mobject to its center.
pub struct ShrinkToCenter;
impl ShrinkToCenter {
    /// Shrink `id` to its center, then take it off the scene.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Morph {
        Morph::make(id, Kind::Shrink, "ShrinkToCenter")
    }
}

/// Namespace for briefly highlighting a mobject.
pub struct Indicate;
impl Indicate {
    /// Briefly scale up and turn yellow.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Morph {
        Self::with(id, 1.2, YELLOW)
    }
    /// Scale `id` by `scale` and tint it `color`, then return to normal.
    pub fn with(id: MobjectId, scale: f64, color: Color) -> Morph {
        let mut m = Morph::make(id, Kind::Indicate(scale, color), "Indicate");
        m.cfg.rate_func = rate_functions::there_and_back;
        m
    }
}

impl MobjectId {
    /// Manim's `.animate`: `s.play(sq.animate(|m| m.shift(RIGHT).rotate(PI / 4.0)))`.
    pub fn animate(self, f: impl FnOnce(VMobject) -> VMobject + 'static) -> Morph {
        let cell = AnimateCell { f: RefCell::new(Some(Box::new(f))), out: RefCell::new(None) };
        Morph::make(self, Kind::Animate(Rc::new(cell)), "Animate")
    }
}

impl Animation for Morph {
    impl_config!();

    fn name(&self) -> String {
        self.name.into()
    }

    fn validate(&self, w: &World) -> anyhow::Result<()> {
        w.try_get(self.id)?;
        match &self.kind {
            Kind::Replacement(b) | Kind::FromCopy(b) | Kind::FadeTransform(b) => w.try_get(*b)?,
            Kind::Matching { to, .. } => w.try_get(*to)?,
            _ => return Ok(()),
        };
        Ok(())
    }

    fn begin(&mut self, w: &mut World) {
        let orig = w[self.id].clone();
        self.z_index = orig.z_index;
        let hidden = |m: VMobject| m.set_opacity(0.0);
        let mut aligned = None;
        let (start, end, final_state) = match &mut self.kind {
            Kind::Transform(t) => (orig, (**t).clone(), Some((**t).clone())),
            Kind::Replacement(b) | Kind::FromCopy(b) => {
                let t = w[*b].clone();
                (orig, t, None)
            }
            Kind::FadeTransform(b) => {
                let t = w[*b].clone();
                aligned = Some(fade_transform(&orig, &t));
                (orig, t, None)
            }
            Kind::Matching { to, transform_mismatches } => {
                let t = w[*to].clone();
                aligned = Some(matching(&orig, &t, *transform_mismatches));
                (orig, t, None)
            }
            Kind::Animate(cell) => {
                let cached = cell.out.borrow().clone();
                let t = match cached {
                    Some(t) => t,
                    None => {
                        let f = cell.f.borrow_mut().take().expect("animation closure already consumed");
                        let t = f(orig.clone());
                        *cell.out.borrow_mut() = Some(t.clone());
                        t
                    }
                };
                (orig, t.clone(), Some(t))
            }
            Kind::FadeIn => {
                let start = hidden(orig.clone().scale(self.scale).shift(-self.shift));
                (start, orig.clone(), Some(orig))
            }
            Kind::FadeOut => {
                let end = hidden(orig.clone().scale(self.scale).shift(self.shift));
                (orig.clone(), end, Some(orig))
            }
            Kind::Grow(p) => {
                let about = p.unwrap_or_else(|| orig.get_center());
                (orig.clone().scale_about(0.0, about), orig.clone(), Some(orig))
            }
            Kind::GrowEdge(edge) => {
                let about = orig.get_critical_point(*edge);
                // Collapse only along the edge's axis so the growth is a sweep, not a zoom.
                let (sx, sy) = (if edge.x != 0.0 { 0.0 } else { 1.0 }, if edge.y != 0.0 { 0.0 } else { 1.0 });
                let start = orig.clone().apply_function(move |p| {
                    Point::new(about.x + (p.x - about.x) * sx, about.y + (p.y - about.y) * sy, p.z)
                });
                (start, orig.clone(), Some(orig))
            }
            Kind::Shrink => {
                let c = orig.get_center();
                (orig.clone(), orig.clone().scale_about(0.0, c), Some(orig))
            }
            Kind::Indicate(s, c) => {
                let end = orig.clone().scale(*s).set_color(*c);
                (orig.clone(), end, Some(orig))
            }
        };
        if let Kind::FromCopy(_) = self.kind {
            self.work = w.insert(start.clone());
        }
        let (a, b) = aligned.unwrap_or_else(|| align(&start, &end));
        self.start = a;
        self.end = b;
        self.final_state = final_state;
        w.show(self.work);
    }

    fn apply(&mut self, w: &mut World, alpha: f64) {
        w[self.work] = lerp_leaves(&self.start, &self.end, alpha, self.path, self.z_index);
    }

    fn clean_up(&mut self, w: &mut World) {
        match self.kind {
            Kind::Replacement(b) | Kind::FadeTransform(b) | Kind::Matching { to: b, .. } => {
                w.replace_in_display(self.id, b);
            }
            Kind::FromCopy(b) => {
                w.replace_in_display(self.work, b);
            }
            _ => {
                if let Some(f) = self.final_state.take() {
                    w[self.id] = f;
                }
                if matches!(self.kind, Kind::FadeOut | Kind::Shrink) {
                    w.hide(self.id);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------- cross-fades and matching

/// Stretch `m` so its box becomes `to`'s (a flat side keeps its size and is only moved).
fn stretch_onto(m: &VMobject, to: &VMobject) -> VMobject {
    let (Some((a0, a1)), Some((b0, b1))) = (m.bounding_box(), to.bounding_box()) else { return m.clone() };
    let axis = |v: f64, a0: f64, a1: f64, b0: f64, b1: f64| {
        if (a1 - a0).abs() < 1e-9 {
            v - (a0 + a1) / 2.0 + (b0 + b1) / 2.0
        } else {
            b0 + (v - a0) / (a1 - a0) * (b1 - b0)
        }
    };
    m.clone()
        .apply_function(move |p| Point::new(axis(p.x, a0.x, a1.x, b0.x, b1.x), axis(p.y, a0.y, a1.y, b0.y, b1.y), p.z))
}

fn fade_transform(a: &VMobject, b: &VMobject) -> (Vec<VMobject>, Vec<VMobject>) {
    let mut start = a.leaves();
    start.extend(stretch_onto(b, a).set_opacity(0.0).leaves());
    let mut end = stretch_onto(a, b).set_opacity(0.0).leaves();
    end.extend(b.leaves());
    (start, end)
}

/// Sort leaves into classes of the same shape; per class, the indices into `la` and `lb`.
fn shape_classes(la: &[VMobject], lb: &[VMobject]) -> Vec<(Vec<usize>, Vec<usize>)> {
    let mut reps: Vec<&VMobject> = Vec::new();
    let mut classes: Vec<(Vec<usize>, Vec<usize>)> = Vec::new();
    for (side, leaves) in [la, lb].into_iter().enumerate() {
        for (i, leaf) in leaves.iter().enumerate() {
            let k = match reps.iter().position(|r| same_shape(r, leaf)) {
                Some(k) => k,
                None => {
                    reps.push(leaf);
                    classes.push((Vec::new(), Vec::new()));
                    reps.len() - 1
                }
            };
            if side == 0 {
                classes[k].0.push(i);
            } else {
                classes[k].1.push(i);
            }
        }
    }
    classes
}

fn matching(a: &VMobject, b: &VMobject, transform_mismatches: bool) -> (Vec<VMobject>, Vec<VMobject>) {
    let (la, lb) = (a.leaves(), b.leaves());
    let pick = |v: &[VMobject], idx: &[usize]| VMobject::group(idx.iter().map(|&i| v[i].clone()).collect());
    let (mut start, mut end) = (Vec::new(), Vec::new());
    let (mut lone_a, mut lone_b) = (Vec::new(), Vec::new());
    for (ia, ib) in shape_classes(&la, &lb) {
        if ia.is_empty() || ib.is_empty() {
            lone_a.extend(ia);
            lone_b.extend(ib);
            continue;
        }
        let (s, e) = align(&pick(&la, &ia), &pick(&lb, &ib));
        start.extend(s);
        end.extend(e);
    }
    lone_a.sort_unstable();
    lone_b.sort_unstable();
    let (ga, gb) = (pick(&la, &lone_a), pick(&lb, &lone_b));
    if transform_mismatches && !lone_a.is_empty() && !lone_b.is_empty() {
        let (s, e) = align(&ga, &gb);
        start.extend(s);
        end.extend(e);
    } else {
        // As in Manim CE: leftovers of `a` fade out drifting toward `b`'s leftovers, and
        // `b`'s leftovers fade in where they stand.
        let drift = if lone_a.is_empty() || lone_b.is_empty() { ORIGIN } else { gb.get_center() - ga.get_center() };
        for &i in &lone_a {
            start.push(la[i].clone());
            end.push(la[i].clone().shift(drift).set_opacity(0.0));
        }
        for &j in &lone_b {
            start.push(lb[j].clone().set_opacity(0.0));
            end.push(lb[j].clone());
        }
    }
    (start, end)
}
