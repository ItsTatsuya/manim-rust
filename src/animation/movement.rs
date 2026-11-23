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

/// Move and zoom the camera frame (Manim's `MovingCameraScene` frame animation).
///
/// Two ways to aim it:
/// - [`MoveCamera::to`]: end with `zoom` (2.0 shows half the base frame's width) centred on a scene point.
/// - [`MoveCamera::zoom_on`]: zoom in on a scene point while it glides to another place on screen.
///   The point stays under the same screen position for the whole move, so a push-in on a graph's
///   crossing lands it in the middle of the graph's panel (and zooming back with the
///   original position as the anchor undoes it exactly).
///
/// Mobjects marked [`VMobject::pin`](crate::mobject::VMobject::pin), the overlay and the background do not follow the camera.
#[derive(Clone)]
pub struct MoveCamera {
    cfg: AnimConfig,
    zoom: Option<f64>,
    center: Option<Point>,
    focus: Option<(Point, Point)>,
    start_zoom: f64,
    start_center: Point,
    start_screen: Point,
}

impl MoveCamera {
    fn blank() -> Self {
        MoveCamera {
            cfg: AnimConfig::default(),
            zoom: None,
            center: None,
            focus: None,
            start_zoom: 1.0,
            start_center: ORIGIN,
            start_screen: ORIGIN,
        }
    }

    /// End zoomed by `zoom` (`None` keeps the current zoom) and centred on `center` (`None` is the base frame's centre).
    pub fn to(zoom: Option<f64>, center: Option<Point>) -> Self {
        MoveCamera { zoom, center, ..Self::blank() }
    }

    /// Back to the base frame: no zoom, the base centre.
    pub fn reset() -> Self {
        MoveCamera { zoom: Some(1.0), ..Self::blank() }
    }

    /// End at `zoom` with scene point `point` shown at `anchor`, a position given in base-frame
    /// scene coordinates (the point's place on screen before any camera move).
    pub fn zoom_on(point: Point, zoom: f64, anchor: Point) -> Self {
        MoveCamera { zoom: Some(zoom), focus: Some((point, anchor)), ..Self::blank() }
    }

    fn screen_of(p: Point, zoom: f64, center: Point, origin: Point) -> Point {
        (p - center) * zoom + origin
    }
}

impl Animation for MoveCamera {
    impl_config!();
    fn name(&self) -> String {
        "MoveCamera".into()
    }
    fn begin(&mut self, w: &mut World) {
        let f = w.frame();
        self.start_zoom = f.zoom;
        self.start_center = f.center_now();
        if let Some((p, _)) = self.focus {
            self.start_screen = Self::screen_of(p, f.zoom, f.center_now(), f.base_center);
        }
    }
    fn apply(&mut self, w: &mut World, alpha: f64) {
        let base = w.frame().base_center;
        let z1 = self.zoom.unwrap_or(self.start_zoom);
        let zoom = self.start_zoom + (z1 - self.start_zoom) * alpha;
        if let Some((p, anchor)) = self.focus {
            let screen = self.start_screen + (anchor - self.start_screen) * alpha;
            // screen = (p - centre) * zoom + base  =>  centre = p - (screen - base) / zoom
            w.set_frame(zoom, Some(p - (screen - base) / zoom));
        } else {
            let c1 = self.center.unwrap_or(base);
            let center = self.start_center + (c1 - self.start_center) * alpha;
            w.set_frame(zoom, Some(center));
        }
    }
    fn clean_up(&mut self, w: &mut World) {
        // Back at the base frame exactly: no float drift, and later changes to the base centre apply.
        let f = w.frame();
        if (f.zoom - 1.0).abs() < 1e-12 && f.center.is_some_and(|c| c.distance(f.base_center) < 1e-9) {
            w.set_frame(1.0, None);
        }
    }
}
