//! Drawing animations: Create, Uncreate, DrawBorderThenFill, Write, ShowPassingFlash.

use super::{impl_config, lagged_alpha, AnimConfig, Animation};
use crate::mobject::{lerp_leaf, PathFunc, VMobject};
use crate::rate_functions;
use crate::world::{MobjectId, World};

/// Draw the outline progressively.
#[derive(Clone)]
pub struct Create {
    id: MobjectId,
    cfg: AnimConfig,
    reverse: bool,
    remover: bool,
    original: VMobject,
    leaves: Vec<VMobject>,
}

impl Create {
    pub fn new(id: MobjectId) -> Self {
        Create {
            id,
            cfg: AnimConfig::new(1.0, rate_functions::smooth, 1.0),
            reverse: false,
            remover: false,
            original: VMobject::default(),
            leaves: Vec::new(),
        }
    }
}

/// Reverse of `Create`; removes the mobject at the end.
pub struct Uncreate;
impl Uncreate {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> Create {
        Create { reverse: true, remover: true, ..Create::new(id) }
    }
}

impl Animation for Create {
    impl_config!();
    fn name(&self) -> String {
        if self.reverse { "Uncreate" } else { "Create" }.into()
    }
    fn begin(&mut self, w: &mut World) {
        self.original = w[self.id].clone();
        self.leaves = self.original.leaves();
        w.show(self.id);
    }
    fn apply(&mut self, w: &mut World, alpha: f64) {
        let a = if self.reverse { 1.0 - alpha } else { alpha };
        let n = self.leaves.len();
        let lag = self.cfg.lag_ratio;
        let children = self.leaves.iter().enumerate().map(|(i, l)| l.partial(lagged_alpha(a, i, n, lag))).collect();
        w[self.id] = VMobject { z_index: self.original.z_index, ..VMobject::group(children) };
    }
    fn clean_up(&mut self, w: &mut World) {
        w[self.id] = self.original.clone();
        if self.remover {
            w.hide(self.id);
        }
    }
}

/// Trace the outline, then fade the fill in.
#[derive(Clone)]
pub struct DrawBorderThenFill {
    id: MobjectId,
    cfg: AnimConfig,
    name: &'static str,
    original: VMobject,
    leaves: Vec<VMobject>,
    outlines: Vec<VMobject>,
}

impl DrawBorderThenFill {
    pub fn new(id: MobjectId) -> Self {
        DrawBorderThenFill {
            id,
            cfg: AnimConfig::new(2.0, rate_functions::double_smooth, 0.0),
            name: "DrawBorderThenFill",
            original: VMobject::default(),
            leaves: Vec::new(),
            outlines: Vec::new(),
        }
    }
}

/// Handwriting-style reveal; timing scales with the number of glyphs.
pub struct Write;
impl Write {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> DrawBorderThenFill {
        DrawBorderThenFill {
            cfg: AnimConfig::new(-1.0, rate_functions::linear, -1.0),
            name: "Write",
            ..DrawBorderThenFill::new(id)
        }
    }
}

impl Animation for DrawBorderThenFill {
    impl_config!();
    fn name(&self) -> String {
        self.name.into()
    }
    fn resolve(&mut self, w: &World) {
        let n = w[self.id].leaves().len().max(1) as f64;
        if self.cfg.run_time < 0.0 {
            self.cfg.run_time = if n < 15.0 { 1.0 } else { 2.0 };
        }
        if self.cfg.lag_ratio < 0.0 {
            self.cfg.lag_ratio = (4.0 / n).min(0.2);
        }
    }
    fn begin(&mut self, w: &mut World) {
        self.resolve(w);
        self.original = w[self.id].clone();
        self.leaves = self.original.leaves();
        self.outlines = self
            .leaves
            .iter()
            .map(|l| {
                let mut o = l.clone();
                o.fill_opacity = 0.0;
                if o.stroke_width <= 0.0 || o.stroke_opacity <= 0.0 {
                    o.stroke_color = l.fill_color;
                }
                o.stroke_width = 2.0;
                o.stroke_opacity = 1.0;
                o
            })
            .collect();
        w.show(self.id);
    }
    fn apply(&mut self, w: &mut World, alpha: f64) {
        let n = self.leaves.len();
        let lag = self.cfg.lag_ratio;
        let children = (0..n)
            .map(|i| {
                let s = lagged_alpha(alpha, i, n, lag);
                if s < 0.5 {
                    self.outlines[i].partial(2.0 * s)
                } else {
                    lerp_leaf(&self.outlines[i], &self.leaves[i], 2.0 * s - 1.0, PathFunc::Straight)
                }
            })
            .collect();
        w[self.id] = VMobject { z_index: self.original.z_index, ..VMobject::group(children) };
    }
    fn clean_up(&mut self, w: &mut World) {
        w[self.id] = self.original.clone();
    }
}

/// A short segment of the outline travels along the path, then disappears.
#[derive(Clone)]
pub struct ShowPassingFlash {
    id: MobjectId,
    cfg: AnimConfig,
    time_width: f64,
    original: VMobject,
    leaves: Vec<VMobject>,
}

impl ShowPassingFlash {
    pub fn new(id: MobjectId) -> Self {
        ShowPassingFlash {
            id,
            cfg: AnimConfig::new(1.0, rate_functions::smooth, 0.0),
            time_width: 0.1,
            original: VMobject::default(),
            leaves: Vec::new(),
        }
    }
    pub fn time_width(mut self, w: f64) -> Self {
        self.time_width = w;
        self
    }
}

impl Animation for ShowPassingFlash {
    impl_config!();
    fn name(&self) -> String {
        "ShowPassingFlash".into()
    }
    fn begin(&mut self, w: &mut World) {
        self.original = w[self.id].clone();
        self.leaves = self.original.leaves();
        w.show(self.id);
    }
    fn apply(&mut self, w: &mut World, alpha: f64) {
        let upper = alpha * (1.0 + self.time_width);
        let lower = upper - self.time_width;
        let children = self.leaves.iter().map(|l| l.partial_range(lower.max(0.0), upper.min(1.0))).collect();
        w[self.id] = VMobject { z_index: self.original.z_index, ..VMobject::group(children) };
    }
    fn clean_up(&mut self, w: &mut World) {
        w[self.id] = self.original.clone();
        w.hide(self.id);
    }
}
