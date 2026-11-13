//! Arena that owns every mobject and value tracker in a scene.
//!
//! Python Manim relies on shared mutable references; here the scene owns all state and
//! hands out copyable handles ([`MobjectId`], [`TrackerId`]) that animations and
//! updaters use to reach it.

use std::ops::{Index, IndexMut};
use std::sync::Arc;

use crate::mobject::VMobject;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MobjectId(pub(crate) usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackerId(pub(crate) usize);

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
}

impl World {
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

    pub fn try_get_mut(&mut self, id: MobjectId) -> anyhow::Result<&mut VMobject> {
        let n = self.mobjects.len();
        self.mobjects.get_mut(id.0).map(Arc::make_mut).ok_or_else(|| missing(id, n))
    }

    pub fn set(&mut self, id: MobjectId, m: VMobject) {
        self.mobjects[id.0] = Arc::new(m);
    }

    /// Apply builder-style methods in place: `w.modify(id, |m| m.shift(RIGHT))`.
    pub fn modify(&mut self, id: MobjectId, f: impl FnOnce(VMobject) -> VMobject) {
        self.get_mut(id).apply(f);
    }

    /// Number of mobjects ever stored (ids are `0..len`).
    pub fn len(&self) -> usize {
        self.mobjects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mobjects.is_empty()
    }

    /// Whether `id` refers to a stored mobject.
    pub fn contains(&self, id: MobjectId) -> bool {
        id.0 < self.mobjects.len()
    }

    pub fn show(&mut self, id: MobjectId) {
        if !self.display.contains(&id) {
            self.display.push(id);
        }
    }

    pub fn hide(&mut self, id: MobjectId) {
        self.display.retain(|d| *d != id);
    }

    /// Move to the top of the draw order.
    pub fn bring_to_front(&mut self, id: MobjectId) {
        self.hide(id);
        self.display.push(id);
    }

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

    pub fn is_displayed(&self, id: MobjectId) -> bool {
        self.display.contains(&id)
    }

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

    pub fn background(&self) -> &[MobjectId] {
        &self.background
    }

    pub fn overlay(&self) -> &[MobjectId] {
        &self.overlay
    }

    pub fn clear_background(&mut self) {
        self.background.clear();
    }

    pub fn clear_overlay(&mut self) {
        self.overlay.clear();
    }

    /// Take `id` out of every layer.
    pub fn remove_everywhere(&mut self, id: MobjectId) {
        self.display.retain(|d| *d != id);
        self.background.retain(|d| *d != id);
        self.overlay.retain(|d| *d != id);
    }

    pub fn trackers(&self) -> &[f64] {
        &self.trackers
    }

    pub fn tracker(&mut self, v: f64) -> TrackerId {
        self.trackers.push(v);
        TrackerId(self.trackers.len() - 1)
    }

    pub fn value(&self, t: TrackerId) -> f64 {
        self.trackers[t.0]
    }

    pub fn set_value(&mut self, t: TrackerId, v: f64) {
        self.trackers[t.0] = v;
    }
}

fn missing(id: MobjectId, n: usize) -> anyhow::Error {
    anyhow::anyhow!("mobject {} does not exist at this point of the scene (the world has {n})", id.0)
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
