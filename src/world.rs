//! Arena that owns every mobject and value tracker in a scene.
//!
//! Python Manim relies on shared mutable references; here the scene owns all state and
//! hands out copyable handles ([`MobjectId`], [`TrackerId`]) that animations and
//! updaters use to reach it.

use std::ops::{Index, IndexMut};
use std::sync::Arc;

use crate::mobject::VMobject;

/// Handle to a mobject stored in a [`World`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MobjectId(pub(crate) usize);

/// Handle to a value tracker stored in a [`World`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackerId(pub(crate) usize);

/// The live camera frame, relative to the scene's base frame (the frame the [`Camera`](crate::camera::Camera)
/// was configured with). Lives in the world so animations can move it and seeking restores it with everything else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameState {
    /// Zoom factor: `2.0` shows half the base frame's width and height. `1.0` is the base frame.
    pub zoom: f64,
    /// Scene point shown at the centre of the frame; `None` keeps the base frame's centre.
    pub center: Option<crate::constants::Point>,
    /// Centre of the base frame, kept in sync by the scene so animations can start from it.
    pub base_center: crate::constants::Point,
}

impl Default for FrameState {
    fn default() -> Self {
        FrameState { zoom: 1.0, center: None, base_center: crate::constants::ORIGIN }
    }
}

impl FrameState {
    /// The centre actually shown.
    pub fn center_now(&self) -> crate::constants::Point {
        self.center.unwrap_or(self.base_center)
    }
}

/// Cloning a world is cheap: mobjects are shared copy-on-write, so a snapshot only
/// copies the ones that change afterwards (timeline keyframes rely on this).
#[derive(Default, Clone)]
pub struct World {
    mobjects: Vec<Arc<VMobject>>,
    /// Displayed mobjects, in draw order (before z-index sorting).
    display: Vec<MobjectId>,
    /// Drawn below everything and kept by `clear` (grid paper, backdrops).
    background: Vec<MobjectId>,
    /// Drawn above everything and kept by `clear` (watermarks, captions).
    overlay: Vec<MobjectId>,
    trackers: Vec<f64>,
    frame: FrameState,
}

impl World {
    /// The live camera frame (zoom and centre relative to the base frame).
    pub fn frame(&self) -> FrameState {
        self.frame
    }

    /// Set the live camera frame. Non-finite or non-positive zoom is ignored.
    pub fn set_frame(&mut self, zoom: f64, center: Option<crate::constants::Point>) {
        if zoom.is_finite() && zoom > 0.0 {
            self.frame.zoom = zoom.clamp(1e-3, 1e3);
        }
        self.frame.center = center.filter(|c| c.is_finite());
    }

    /// Tell the world where the base frame is centred (the scene does this before every play).
    pub fn set_base_center(&mut self, c: crate::constants::Point) {
        self.frame.base_center = c;
    }

    /// An empty world.
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a mobject without displaying it.
    pub fn insert(&mut self, m: VMobject) -> MobjectId {
        self.mobjects.push(Arc::new(m));
        MobjectId(self.mobjects.len() - 1)
    }

    /// The mobject `id`. Panics if it doesn't exist (see [`World::try_get`]).
    pub fn get(&self, id: MobjectId) -> &VMobject {
        self.try_get(id).unwrap_or_else(|e| panic!("{e}"))
    }

    /// Panics if `id` doesn't exist (see [`World::try_get_mut`]).
    pub fn get_mut(&mut self, id: MobjectId) -> &mut VMobject {
        self.try_get_mut(id).unwrap_or_else(|e| panic!("{e}"))
    }

    /// The mobject `id`, or an error if this world has no such mobject (e.g. an id
    /// created later in the scene, used on a world restored from an earlier keyframe).
    pub fn try_get(&self, id: MobjectId) -> anyhow::Result<&VMobject> {
        let n = self.mobjects.len();
        self.mobjects.get(id.0).map(|m| &**m).ok_or_else(|| missing(id, n))
    }

    /// Mutable access to mobject `id` (copy-on-write), or an error if it does not exist.
    pub fn try_get_mut(&mut self, id: MobjectId) -> anyhow::Result<&mut VMobject> {
        let n = self.mobjects.len();
        self.mobjects.get_mut(id.0).map(Arc::make_mut).ok_or_else(|| missing(id, n))
    }

    /// Replace mobject `id`. Panics if it doesn't exist (see [`World::try_set`]).
    pub fn set(&mut self, id: MobjectId, m: VMobject) {
        self.try_set(id, m).unwrap_or_else(|e| panic!("{e}"));
    }

    /// Replace mobject `id`, or fail if this world has no such mobject.
    pub fn try_set(&mut self, id: MobjectId, m: VMobject) -> anyhow::Result<()> {
        let n = self.mobjects.len();
        *self.mobjects.get_mut(id.0).ok_or_else(|| missing(id, n))? = Arc::new(m);
        Ok(())
    }

    /// Apply builder-style methods in place: `w.modify(id, |m| m.shift(RIGHT))`.
    pub fn modify(&mut self, id: MobjectId, f: impl FnOnce(VMobject) -> VMobject) {
        self.get_mut(id).apply(f);
    }

    /// Number of mobjects ever stored (ids are `0..len`).
    pub fn len(&self) -> usize {
        self.mobjects.len()
    }

    /// Whether the world stores no mobjects.
    pub fn is_empty(&self) -> bool {
        self.mobjects.is_empty()
    }

    /// Whether `id` refers to a stored mobject.
    pub fn contains(&self, id: MobjectId) -> bool {
        id.0 < self.mobjects.len()
    }

    /// Add `id` to the scene layer (on top) if it is not displayed already.
    pub fn show(&mut self, id: MobjectId) {
        if !self.display.contains(&id) {
            self.display.push(id);
        }
    }

    /// Remove `id` from the scene layer.
    pub fn hide(&mut self, id: MobjectId) {
        self.display.retain(|d| *d != id);
    }

    /// Move to the top of the draw order.
    pub fn bring_to_front(&mut self, id: MobjectId) {
        self.hide(id);
        self.display.push(id);
    }

    /// Move to the bottom of the draw order.
    pub fn bring_to_back(&mut self, id: MobjectId) {
        self.hide(id);
        self.display.insert(0, id);
    }

    /// Swap `old` for `new` at the same position in the draw order.
    pub fn replace_in_display(&mut self, old: MobjectId, new: MobjectId) {
        self.display.retain(|d| *d != new);
        match self.display.iter().position(|d| *d == old) {
            Some(i) => self.display[i] = new,
            None => self.display.push(new),
        }
    }

    /// Whether `id` is in the scene layer.
    pub fn is_displayed(&self, id: MobjectId) -> bool {
        self.display.contains(&id)
    }

    /// Mobjects of the scene layer, in draw order (before z-index sorting).
    pub fn displayed(&self) -> &[MobjectId] {
        &self.display
    }

    /// Remove everything from the scene layer (background and overlay stay).
    pub fn clear(&mut self) {
        self.display.clear();
    }

    // ------------------------------------------------------------------ layers

    /// Draw `id` below the scene, unaffected by `clear` and ignored by layout checks.
    pub fn add_background(&mut self, id: MobjectId) {
        self.display.retain(|d| *d != id);
        if !self.background.contains(&id) {
            self.background.push(id);
        }
    }

    /// Draw `id` above the scene, unaffected by `clear` and ignored by layout checks.
    pub fn add_overlay(&mut self, id: MobjectId) {
        self.display.retain(|d| *d != id);
        if !self.overlay.contains(&id) {
            self.overlay.push(id);
        }
    }

    /// Mobjects drawn below everything, in order.
    pub fn background(&self) -> &[MobjectId] {
        &self.background
    }

    /// Mobjects drawn above everything, in order.
    pub fn overlay(&self) -> &[MobjectId] {
        &self.overlay
    }

    /// Empty the background layer.
    pub fn clear_background(&mut self) {
        self.background.clear();
    }

    /// Empty the overlay layer.
    pub fn clear_overlay(&mut self) {
        self.overlay.clear();
    }

    /// Take `id` out of every layer.
    pub fn remove_everywhere(&mut self, id: MobjectId) {
        self.display.retain(|d| *d != id);
        self.background.retain(|d| *d != id);
        self.overlay.retain(|d| *d != id);
    }

    /// Current values of every tracker, indexed by creation order.
    pub fn trackers(&self) -> &[f64] {
        &self.trackers
    }

    /// Create a tracker holding `v`.
    pub fn tracker(&mut self, v: f64) -> TrackerId {
        self.trackers.push(v);
        TrackerId(self.trackers.len() - 1)
    }

    /// The tracker's value. Panics if `t` doesn't exist (see [`World::try_value`]).
    pub fn value(&self, t: TrackerId) -> f64 {
        self.try_value(t).unwrap_or_else(|e| panic!("{e}"))
    }

    /// Panics if `t` doesn't exist (see [`World::try_set_value`]).
    pub fn set_value(&mut self, t: TrackerId, v: f64) {
        self.try_set_value(t, v).unwrap_or_else(|e| panic!("{e}"));
    }

    /// Value of tracker `t`, or an error if it does not exist.
    pub fn try_value(&self, t: TrackerId) -> anyhow::Result<f64> {
        self.trackers.get(t.0).copied().ok_or_else(|| missing_tracker(t, self.trackers.len()))
    }

    /// Set the value of tracker `t`, or error if it does not exist.
    pub fn try_set_value(&mut self, t: TrackerId, v: f64) -> anyhow::Result<()> {
        let n = self.trackers.len();
        *self.trackers.get_mut(t.0).ok_or_else(|| missing_tracker(t, n))? = v;
        Ok(())
    }
}

fn missing(id: MobjectId, n: usize) -> anyhow::Error {
    anyhow::anyhow!("mobject {} does not exist at this point of the scene (the world has {n})", id.0)
}

fn missing_tracker(t: TrackerId, n: usize) -> anyhow::Error {
    anyhow::anyhow!("value tracker {} does not exist at this point of the scene (the world has {n})", t.0)
}

impl Index<MobjectId> for World {
    type Output = VMobject;
    fn index(&self, id: MobjectId) -> &VMobject {
        self.get(id)
    }
}

impl IndexMut<MobjectId> for World {
    fn index_mut(&mut self, id: MobjectId) -> &mut VMobject {
        self.get_mut(id)
    }
}
