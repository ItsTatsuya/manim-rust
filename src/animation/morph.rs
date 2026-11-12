//! Point-wise morphing animations: Transform and friends, fades, growth, `.animate`.

use std::cell::RefCell;
use std::rc::Rc;

use super::{impl_config, AnimConfig, Animation};
use crate::color::{Color, YELLOW};
use crate::constants::*;
use crate::mobject::{align, lerp_leaves, PathFunc, VMobject};
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

pub struct Transform;
impl Transform {
    /// Morph `id` into `target`. `id` takes `target`'s shape afterwards.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId, target: VMobject) -> Morph {
        Morph::make(id, Kind::Transform(Box::new(target)), "Transform")
    }
}

pub struct ReplacementTransform;
impl ReplacementTransform {
    /// Morph `a` into the (not yet displayed) mobject `b`; `a` is then replaced by `b` in the scene.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(a: MobjectId, b: MobjectId) -> Morph {
        Morph::make(a, Kind::Replacement(b), "ReplacementTransform")
    }
}

pub struct TransformFromCopy;
impl TransformFromCopy {
    /// A copy of `a` morphs into `b`; `a` stays put.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(a: MobjectId, b: MobjectId) -> Morph {
        Morph::make(a, Kind::FromCopy(b), "TransformFromCopy")
    }
}

pub struct FadeIn;
impl FadeIn {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Morph {
        Morph::make(id, Kind::FadeIn, "FadeIn")
    }
}

pub struct FadeOut;
impl FadeOut {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Morph {
        Morph::make(id, Kind::FadeOut, "FadeOut")
    }
}

pub struct GrowFromCenter;
impl GrowFromCenter {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Morph {
        Morph::make(id, Kind::Grow(None), "GrowFromCenter")
    }
}

pub struct GrowFromPoint;
impl GrowFromPoint {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId, point: Point) -> Morph {
        Morph::make(id, Kind::Grow(Some(point)), "GrowFromPoint")
    }
}

pub struct GrowFromEdge;
impl GrowFromEdge {
    /// Grow out of the `edge` side (`LEFT` sweeps a highlight in left to right).
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId, edge: Point) -> Morph {
        Morph::make(id, Kind::GrowEdge(edge), "GrowFromEdge")
    }
}

pub struct SpinInFromNothing;
impl SpinInFromNothing {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Morph {
        Morph::make(id, Kind::Grow(None), "SpinInFromNothing").path_arc(PI / 2.0)
    }
}

pub struct ShrinkToCenter;
impl ShrinkToCenter {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Morph {
        Morph::make(id, Kind::Shrink, "ShrinkToCenter")
    }
}

pub struct Indicate;
impl Indicate {
    /// Briefly scale up and turn yellow.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Morph {
        Self::with(id, 1.2, YELLOW)
    }
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

    fn begin(&mut self, w: &mut World) {
        let orig = w[self.id].clone();
        self.z_index = orig.z_index;
        let hidden = |m: VMobject| m.set_opacity(0.0);
        let (start, end, final_state) = match &mut self.kind {
            Kind::Transform(t) => (orig, (**t).clone(), Some((**t).clone())),
            Kind::Replacement(b) | Kind::FromCopy(b) => {
                let t = w[*b].clone();
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
        let (a, b) = align(&start, &end);
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
            Kind::Replacement(b) => {
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
