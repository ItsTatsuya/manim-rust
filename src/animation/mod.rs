//! Animations: how mobjects change over a `play` call.
//!
//! An [`Animation`] gets the alpha value with its rate function already applied
//! (0 → 1) and writes the matching state into the [`World`].

mod creation;
mod morph;
mod movement;

pub use creation::*;
pub use morph::*;
pub use movement::*;

use crate::rate_functions::{self, RateFunc};
use crate::world::World;

pub const DEFAULT_RUN_TIME: f64 = 1.0;

#[derive(Clone, Copy, Debug)]
pub struct AnimConfig {
    /// Seconds. A negative value means "work it out in `resolve`".
    pub run_time: f64,
    pub rate_func: RateFunc,
    /// Stagger between family members / grouped animations (0 = all at once, 1 = one after another).
    pub lag_ratio: f64,
}

impl Default for AnimConfig {
    fn default() -> Self {
        AnimConfig { run_time: DEFAULT_RUN_TIME, rate_func: rate_functions::smooth, lag_ratio: 0.0 }
    }
}

impl AnimConfig {
    pub fn new(run_time: f64, rate_func: RateFunc, lag_ratio: f64) -> Self {
        AnimConfig { run_time, rate_func, lag_ratio }
    }
}

/// Copy an animation before it starts (timeline seeking replays plays from copies).
/// Implemented automatically for every `Animation + Clone`.
pub trait AnimClone {
    fn box_clone(&self) -> Box<dyn Animation>;
}

impl<T: Animation + Clone + 'static> AnimClone for T {
    fn box_clone(&self) -> Box<dyn Animation> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn Animation> {
    fn clone(&self) -> Self {
        (**self).box_clone()
    }
}

/// An animation. Implementations must be `Clone` (derive it): a clone made before
/// `begin` must replay the same frames, which is how timelines seek.
pub trait Animation: AnimClone {
    fn name(&self) -> String {
        "Animation".into()
    }
    fn config(&self) -> &AnimConfig;
    fn config_mut(&mut self) -> &mut AnimConfig;
    /// Fill in config that depends on the world (e.g. `Write` timing by glyph count).
    /// Called before `begin`, possibly earlier for animations inside groups.
    fn resolve(&mut self, _w: &World) {}
    fn begin(&mut self, w: &mut World);
    /// `alpha` has the rate function applied.
    fn apply(&mut self, w: &mut World, alpha: f64);
    /// Show the frame at raw progress `t` (0 → 1, before the rate function). The default
    /// eases `t` and calls [`Animation::apply`]. Animations that stagger parts override it
    /// to ease each part on its own, as Manim CE does (`Create` on a group or text).
    fn interpolate(&mut self, w: &mut World, t: f64) {
        let f = self.config().rate_func;
        self.apply(w, f(t));
    }
    fn clean_up(&mut self, _w: &mut World) {}
}

impl Animation for Box<dyn Animation> {
    fn name(&self) -> String {
        (**self).name()
    }
    fn config(&self) -> &AnimConfig {
        (**self).config()
    }
    fn config_mut(&mut self) -> &mut AnimConfig {
        (**self).config_mut()
    }
    fn resolve(&mut self, w: &World) {
        (**self).resolve(w)
    }
    fn begin(&mut self, w: &mut World) {
        (**self).begin(w)
    }
    fn apply(&mut self, w: &mut World, alpha: f64) {
        (**self).apply(w, alpha)
    }
    fn interpolate(&mut self, w: &mut World, t: f64) {
        (**self).interpolate(w, t)
    }
    fn clean_up(&mut self, w: &mut World) {
        (**self).clean_up(w)
    }
}

/// Builder helpers available on every animation.
pub trait AnimationExt: Animation + Sized + 'static {
    fn run_time(mut self, t: f64) -> Self {
        self.config_mut().run_time = t;
        self
    }
    fn rate_func(mut self, f: RateFunc) -> Self {
        self.config_mut().rate_func = f;
        self
    }
    fn lag_ratio(mut self, l: f64) -> Self {
        self.config_mut().lag_ratio = l;
        self
    }
    fn boxed(self) -> Box<dyn Animation> {
        Box::new(self)
    }
}

impl<T: Animation + Sized + 'static> AnimationExt for T {}

macro_rules! impl_config {
    () => {
        fn config(&self) -> &$crate::animation::AnimConfig {
            &self.cfg
        }
        fn config_mut(&mut self) -> &mut $crate::animation::AnimConfig {
            &mut self.cfg
        }
    };
}
pub(crate) use impl_config;

/// Manim's lag computation: the sub-alpha of member `i` of `n` given the lag ratio.
pub fn lagged_alpha(alpha: f64, i: usize, n: usize, lag_ratio: f64) -> f64 {
    if n <= 1 {
        return alpha;
    }
    let full = (n - 1) as f64 * lag_ratio + 1.0;
    let value = alpha * full;
    (value - i as f64 * lag_ratio).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------- conversion for `play`

/// Anything `Scene::play` accepts: one animation, a tuple of them, or a `Vec<Box<dyn Animation>>`.
pub trait IntoAnimations {
    fn into_animations(self) -> Vec<Box<dyn Animation>>;
}

impl<T: Animation + 'static> IntoAnimations for T {
    fn into_animations(self) -> Vec<Box<dyn Animation>> {
        vec![Box::new(self)]
    }
}

impl IntoAnimations for Vec<Box<dyn Animation>> {
    fn into_animations(self) -> Vec<Box<dyn Animation>> {
        self
    }
}

macro_rules! tuple_into_animations {
    ($($t:ident),+) => {
        impl<$($t: Animation + 'static),+> IntoAnimations for ($($t,)+) {
            #[allow(non_snake_case)]
            fn into_animations(self) -> Vec<Box<dyn Animation>> {
                let ($($t,)+) = self;
                vec![$(Box::new($t)),+]
            }
        }
    };
}
tuple_into_animations!(A, B);
tuple_into_animations!(A, B, C);
tuple_into_animations!(A, B, C, D);
tuple_into_animations!(A, B, C, D, E);
tuple_into_animations!(A, B, C, D, E, F);
tuple_into_animations!(A, B, C, D, E, F, G);
tuple_into_animations!(A, B, C, D, E, F, G, H);

/// Build a `Vec<Box<dyn Animation>>`: `anims![Create::new(a), FadeIn::new(b)]`.
#[macro_export]
macro_rules! anims {
    ($($a:expr),* $(,)?) => { vec![$($crate::animation::AnimationExt::boxed($a)),*] };
}

// ---------------------------------------------------------------------- groups

#[derive(Clone, Copy, PartialEq, Debug)]
enum ChildState {
    Pending,
    Running,
    Done,
}

/// Run animations together, optionally staggered by `lag_ratio`
/// (0 = simultaneous, 1 = one after another).
#[derive(Clone)]
pub struct AnimationGroup {
    anims: Vec<Box<dyn Animation>>,
    timings: Vec<(f64, f64)>,
    state: Vec<ChildState>,
    max_end: f64,
    cfg: AnimConfig,
}

impl AnimationGroup {
    pub fn new(anims: impl IntoAnimations) -> Self {
        AnimationGroup {
            anims: anims.into_animations(),
            timings: Vec::new(),
            state: Vec::new(),
            max_end: 0.0,
            cfg: AnimConfig::new(-1.0, rate_functions::linear, 0.0),
        }
    }

    fn start_child(&mut self, i: usize, w: &mut World) {
        if self.state[i] == ChildState::Pending {
            self.anims[i].begin(w);
            self.state[i] = ChildState::Running;
        }
    }

    fn finish_child(&mut self, i: usize, w: &mut World) {
        self.start_child(i, w);
        if self.state[i] == ChildState::Running {
            self.anims[i].interpolate(w, 1.0);
            self.anims[i].clean_up(w);
            self.state[i] = ChildState::Done;
        }
    }
}

/// Staggered start (default lag ratio 0.05, like Manim).
pub struct LaggedStart;
impl LaggedStart {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(anims: impl IntoAnimations) -> AnimationGroup {
        AnimationGroup::new(anims).lag_ratio(0.05)
    }
}

/// One after another.
pub struct Succession;
impl Succession {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(anims: impl IntoAnimations) -> AnimationGroup {
        AnimationGroup::new(anims).lag_ratio(1.0)
    }
}

impl Animation for AnimationGroup {
    impl_config!();

    fn name(&self) -> String {
        let names: Vec<String> = self.anims.iter().map(|a| a.name()).collect();
        names.join(", ")
    }

    fn resolve(&mut self, w: &World) {
        let mut cur = 0.0f64;
        self.timings.clear();
        self.max_end = 0.0;
        for a in &mut self.anims {
            a.resolve(w);
            let start = cur;
            let end = start + a.config().run_time.max(0.0);
            self.timings.push((start, end));
            cur = start + (end - start) * self.cfg.lag_ratio;
            self.max_end = self.max_end.max(end);
        }
        if self.cfg.run_time < 0.0 {
            self.cfg.run_time = self.max_end;
        }
        self.state = vec![ChildState::Pending; self.anims.len()];
    }

    fn begin(&mut self, w: &mut World) {
        if self.state.len() != self.anims.len() {
            self.resolve(w);
        }
        for i in 0..self.anims.len() {
            if self.timings[i].0 <= 0.0 {
                self.start_child(i, w);
            }
        }
    }

    fn apply(&mut self, w: &mut World, alpha: f64) {
        let time = alpha * self.max_end;
        for i in 0..self.anims.len() {
            if self.state[i] == ChildState::Done {
                continue;
            }
            let (s, e) = self.timings[i];
            if time < s {
                continue;
            }
            let sub = if e > s { ((time - s) / (e - s)).clamp(0.0, 1.0) } else { 1.0 };
            if sub >= 1.0 {
                self.finish_child(i, w);
            } else {
                self.start_child(i, w);
                self.anims[i].interpolate(w, sub);
            }
        }
    }

    fn clean_up(&mut self, w: &mut World) {
        for i in 0..self.anims.len() {
            self.finish_child(i, w);
        }
    }
}

/// Does nothing for `run_time` seconds (useful inside `Succession`).
#[derive(Clone)]
pub struct Wait {
    cfg: AnimConfig,
}

impl Wait {
    pub fn new(seconds: f64) -> Self {
        Wait { cfg: AnimConfig::new(seconds, rate_functions::linear, 0.0) }
    }
}

impl Animation for Wait {
    impl_config!();
    fn name(&self) -> String {
        "Wait".into()
    }
    fn begin(&mut self, _w: &mut World) {}
    fn apply(&mut self, _w: &mut World, _alpha: f64) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lag_bounds() {
        assert_eq!(lagged_alpha(0.0, 3, 5, 0.5), 0.0);
        assert_eq!(lagged_alpha(1.0, 3, 5, 0.5), 1.0);
        assert_eq!(lagged_alpha(0.5, 0, 2, 1.0), 1.0);
        assert_eq!(lagged_alpha(0.5, 1, 2, 1.0), 0.0);
    }
}
