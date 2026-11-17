//! Motion and value animations: Rotate, MoveAlongPath, ChangeValue, UpdateFromAlpha.

use super::{impl_config, AnimConfig, Animation};
use crate::constants::*;
use crate::mobject::VMobject;
use crate::rate_functions;
use crate::world::{MobjectId, TrackerId, World};

/// True rotation (points travel on circles, unlike `animate(|m| m.rotate(..))`).
#[derive(Clone)]
pub struct Rotate {
    id: MobjectId,
    cfg: AnimConfig,
    angle: f64,
    about: Option<Point>,
    original: VMobject,
}

impl Rotate {
    /// Rotate `id` by `angle` radians about its center.
    pub fn new(id: MobjectId, angle: f64) -> Self {
        Rotate {
            id,
            cfg: AnimConfig::new(1.0, rate_functions::linear, 0.0),
            angle,
            about: None,
            original: VMobject::default(),
        }
    }
    /// Rotate about `p` instead of the center.
    pub fn about(mut self, p: Point) -> Self {
        self.about = Some(p);
        self
    }
}

impl Animation for Rotate {
    impl_config!();
    fn name(&self) -> String {
        "Rotate".into()
    }
    fn begin(&mut self, w: &mut World) {
        self.original = w[self.id].clone();
        w.show(self.id);
    }
    fn apply(&mut self, w: &mut World, alpha: f64) {
        let about = self.about.unwrap_or_else(|| self.original.get_center());
        w[self.id] = self.original.clone().rotate_about(self.angle * alpha, about);
    }
}

/// Move the mobject's center along a path.
#[derive(Clone)]
pub struct MoveAlongPath {
    id: MobjectId,
    cfg: AnimConfig,
    path: VMobject,
    original: VMobject,
}

impl MoveAlongPath {
    /// Move `id`'s center along `path`.
    pub fn new(id: MobjectId, path: VMobject) -> Self {
        MoveAlongPath {
            id,
            cfg: AnimConfig::new(1.0, rate_functions::linear, 0.0),
            path,
            original: VMobject::default(),
        }
    }
}

impl Animation for MoveAlongPath {
    impl_config!();
    fn name(&self) -> String {
        "MoveAlongPath".into()
    }
    fn begin(&mut self, w: &mut World) {
        self.original = w[self.id].clone();
        w.show(self.id);
    }
    fn apply(&mut self, w: &mut World, alpha: f64) {
        w[self.id] = self.original.clone().move_to(self.path.point_from_proportion(alpha));
    }
}

/// Animate a value tracker to a new value.
#[derive(Clone)]
pub struct ChangeValue {
    t: TrackerId,
    cfg: AnimConfig,
    target: f64,
    start: f64,
}

impl ChangeValue {
    /// Animate tracker `t` to `target`.
    pub fn new(t: TrackerId, target: f64) -> Self {
        ChangeValue { t, cfg: AnimConfig::default(), target, start: 0.0 }
    }
}

impl Animation for ChangeValue {
    impl_config!();
    fn name(&self) -> String {
        "ChangeValue".into()
    }
    fn begin(&mut self, w: &mut World) {
        self.start = w.value(self.t);
    }
    fn apply(&mut self, w: &mut World, alpha: f64) {
        w.set_value(self.t, self.start + (self.target - self.start) * alpha);
    }
}

/// Escape hatch: run any closure with the rated alpha each frame. Clones share the
/// closure, so for seeking it should depend only on `alpha` and the world.
type AlphaFn = std::rc::Rc<std::cell::RefCell<dyn FnMut(&mut World, f64)>>;

/// Run a closure each frame with the eased progress; see the note on seeking above.
#[derive(Clone)]
pub struct UpdateFromAlpha {
    cfg: AnimConfig,
    f: AlphaFn,
}

impl UpdateFromAlpha {
    /// Call `f(world, alpha)` every frame.
    pub fn new(f: impl FnMut(&mut World, f64) + 'static) -> Self {
        UpdateFromAlpha { cfg: AnimConfig::default(), f: std::rc::Rc::new(std::cell::RefCell::new(f)) }
    }
}

impl Animation for UpdateFromAlpha {
    impl_config!();
    fn name(&self) -> String {
        "UpdateFromAlpha".into()
    }
    fn begin(&mut self, w: &mut World) {
        (self.f.borrow_mut())(w, 0.0);
    }
    fn apply(&mut self, w: &mut World, alpha: f64) {
        (self.f.borrow_mut())(w, alpha);
    }
}
