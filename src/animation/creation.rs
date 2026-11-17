//! Drawing animations: Create, Uncreate, DrawBorderThenFill, Write, Unwrite,
//! ShowPassingFlash, and the typewriter reveals (AddTextLetterByLetter, TypeWithCursor).

use super::{impl_config, lagged_alpha, AnimConfig, Animation};
use crate::constants::*;
use crate::mobject::{lerp_leaf, match_invisible_style, PathFunc, VMobject};
use crate::rate_functions::{self, RateFunc};
use crate::world::{MobjectId, World};

/// Progress of part `i` of `n` at raw progress `t`: staggered by `lag`, then eased by
/// `rate` on its own (Manim CE's `get_sub_alpha`). `reverse` runs the parts last-first
/// and backwards, for the removing variants.
fn part_alpha(t: f64, i: usize, n: usize, lag: f64, rate: RateFunc, reverse: bool) -> f64 {
    if reverse {
        1.0 - rate(lagged_alpha(t, n - 1 - i, n, lag))
    } else {
        rate(lagged_alpha(t, i, n, lag))
    }
}

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
    /// Draw the mobject's outline progressively (Manim's `Create`).
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

    fn draw(&self, w: &mut World, alpha_of: impl Fn(usize, usize) -> f64) {
        let n = self.leaves.len();
        let children = self.leaves.iter().enumerate().map(|(i, l)| l.partial(alpha_of(i, n))).collect();
        w[self.id] = VMobject { z_index: self.original.z_index, ..VMobject::group(children) };
    }
}

/// Reverse of `Create`; removes the mobject at the end.
pub struct Uncreate;
impl Uncreate {
    /// Run `Create` backwards, then remove the mobject.
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
        let lag = self.cfg.lag_ratio;
        let rev = self.reverse;
        self.draw(w, |i, n| part_alpha(alpha, i, n, lag, rate_functions::linear, rev));
    }
    /// Each part eases on its own, so with the default lag of 1 every curve of a group
    /// is drawn smoothly instead of the whole group crawling at the ends and racing between.
    fn interpolate(&mut self, w: &mut World, t: f64) {
        let (lag, rate, rev) = (self.cfg.lag_ratio, self.cfg.rate_func, self.reverse);
        self.draw(w, |i, n| part_alpha(t, i, n, lag, rate, rev));
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
    /// Erase instead (fill fades to the outline, which then retracts), last part first.
    reverse: bool,
    remover: bool,
    original: VMobject,
    leaves: Vec<VMobject>,
    outlines: Vec<VMobject>,
}

impl DrawBorderThenFill {
    /// Draw the outline, then fade the fill in.
    pub fn new(id: MobjectId) -> Self {
        DrawBorderThenFill {
            id,
            cfg: AnimConfig::new(2.0, rate_functions::double_smooth, 0.0),
            name: "DrawBorderThenFill",
            reverse: false,
            remover: false,
            original: VMobject::default(),
            leaves: Vec::new(),
            outlines: Vec::new(),
        }
    }

    fn draw(&self, w: &mut World, alpha_of: impl Fn(usize, usize) -> f64) {
        let n = self.leaves.len();
        let children = (0..n)
            .map(|i| {
                let s = alpha_of(i, n);
                if s < 0.5 {
                    self.outlines[i].partial(2.0 * s)
                } else {
                    lerp_leaf(&self.outlines[i], &self.leaves[i], 2.0 * s - 1.0, PathFunc::Straight)
                }
            })
            .collect();
        w[self.id] = VMobject { z_index: self.original.z_index, ..VMobject::group(children) };
    }
}

/// Handwriting-style reveal; timing scales with the number of glyphs (Manim CE's
/// defaults: 1 s under 15 glyphs, else 2 s; lag ratio `min(4 / glyphs, 0.2)`). The
/// lag is spread inside the run time: a play lasts exactly its `runTime`.
pub struct Write;
impl Write {
    /// Handwriting-style reveal of text or shapes; run time scales with the number of glyphs.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> DrawBorderThenFill {
        DrawBorderThenFill {
            cfg: AnimConfig::new(-1.0, rate_functions::linear, -1.0),
            name: "Write",
            ..DrawBorderThenFill::new(id)
        }
    }
}

/// `Write` backwards, erasing from the last glyph; removes the mobject at the end.
pub struct Unwrite;
impl Unwrite {
    /// Erase with [`Write`] reversed, then remove the mobject.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> DrawBorderThenFill {
        DrawBorderThenFill { name: "Unwrite", reverse: true, remover: true, ..Write::new(id) }
    }
}

impl Animation for DrawBorderThenFill {
    impl_config!();
    fn name(&self) -> String {
        self.name.into()
    }
    fn resolve(&mut self, w: &World) {
        let n = w.try_get(self.id).map_or(1, |m| m.leaves().len().max(1)) as f64;
        if self.cfg.run_time < 0.0 {
            self.cfg.run_time = if n < 15.0 { 1.0 } else { 2.0 };
        }
        if self.cfg.lag_ratio < 0.0 {
            self.cfg.lag_ratio = (4.0 / n).min(0.2);
        }
    }
    fn validate(&self, w: &World) -> anyhow::Result<()> {
        w.try_get(self.id).map(|_| ())
    }
    fn begin(&mut self, w: &mut World) {
        self.resolve(w);
        self.original = w[self.id].clone();
        let mut leaves = self.original.leaves();
        self.outlines = leaves
            .iter_mut()
            .map(|l| {
                let mut o = l.clone();
                o.fill_opacity = 0.0;
                let stroked = l.stroke_width > 0.0 && l.stroke_opacity > 0.0;
                if l.fill_opacity <= 0.0 && !stroked {
                    // Invisible parts stay invisible: no outline that blinks out at the end.
                    o.stroke_opacity = 0.0;
                    return o;
                }
                if !stroked {
                    o.stroke_color = l.fill_color;
                }
                o.stroke_width = 2.0;
                o.stroke_opacity = 1.0;
                // The outline fades into the fill in its own color, not toward the
                // glyph's (invisible) stroke color.
                match_invisible_style(&mut o, l);
                o
            })
            .collect();
        self.leaves = leaves;
        w.show(self.id);
    }
    fn apply(&mut self, w: &mut World, alpha: f64) {
        let (lag, rev) = (self.cfg.lag_ratio, self.reverse);
        self.draw(w, |i, n| part_alpha(alpha, i, n, lag, rate_functions::linear, rev));
    }
    fn interpolate(&mut self, w: &mut World, t: f64) {
        let (lag, rate, rev) = (self.cfg.lag_ratio, self.cfg.rate_func, self.reverse);
        self.draw(w, |i, n| part_alpha(t, i, n, lag, rate, rev));
    }
    fn clean_up(&mut self, w: &mut World) {
        w[self.id] = self.original.clone();
        if self.remover {
            w.hide(self.id);
        }
    }
}

/// Letters appear (or disappear) one at a time, typewriter style; optionally a cursor
/// rides after the last letter shown. Built by [`AddTextLetterByLetter`],
/// [`RemoveTextLetterByLetter`], [`TypeWithCursor`] and [`UntypeWithCursor`].
#[derive(Clone)]
pub struct ShowLetters {
    id: MobjectId,
    cfg: AnimConfig,
    name: &'static str,
    remove: bool,
    time_per_char: f64,
    cursor: Option<MobjectId>,
    buff: f64,
    keep_cursor_y: bool,
    leave_cursor_on: bool,
    original: VMobject,
    leaves: Vec<VMobject>,
    cursor_start: Option<VMobject>,
}

impl ShowLetters {
    fn make(id: MobjectId, name: &'static str, remove: bool, cursor: Option<MobjectId>) -> Self {
        ShowLetters {
            id,
            cfg: AnimConfig::new(-1.0, rate_functions::linear, 0.0),
            name,
            remove,
            time_per_char: 0.1,
            cursor,
            buff: 0.1,
            keep_cursor_y: true,
            leave_cursor_on: true,
            original: VMobject::default(),
            leaves: Vec::new(),
            cursor_start: None,
        }
    }

    /// Seconds per letter when no run time is given (default 0.1, like Manim CE).
    pub fn time_per_char(mut self, s: f64) -> Self {
        self.time_per_char = s.max(0.0);
        self
    }

    /// Gap between the last letter and the cursor (default 0.1).
    pub fn buff(mut self, b: f64) -> Self {
        self.buff = b;
        self
    }

    /// Keep the cursor at its own height (default) or center it on each letter.
    pub fn keep_cursor_y(mut self, keep: bool) -> Self {
        self.keep_cursor_y = keep;
        self
    }

    /// Leave the cursor on screen afterwards (default true).
    pub fn leave_cursor_on(mut self, on: bool) -> Self {
        self.leave_cursor_on = on;
        self
    }

    fn shown(&self, alpha: f64) -> usize {
        let n = self.leaves.len();
        // Adding shows a letter from the very first frame; removing hides one at once too.
        let k = if self.remove { ((1.0 - alpha) * n as f64 + 1e-9).floor() } else { (alpha * n as f64 - 1e-9).ceil() };
        (k.max(0.0) as usize).min(n)
    }
}

/// Namespace for the letter-by-letter reveal of text; the constructor returns a `ShowLetters` animation.
pub struct AddTextLetterByLetter;
impl AddTextLetterByLetter {
    /// Reveal the letters one at a time (0.1 s per letter unless a run time is set).
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> ShowLetters {
        ShowLetters::make(id, "AddTextLetterByLetter", false, None)
    }
}

/// Namespace for the letter-by-letter removal of text, last letter first.
pub struct RemoveTextLetterByLetter;
impl RemoveTextLetterByLetter {
    /// Remove the letters one at a time, last first; removes the mobject at the end.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId) -> ShowLetters {
        ShowLetters::make(id, "RemoveTextLetterByLetter", true, None)
    }
}

/// Namespace for typing text with a cursor mobject following the last letter.
pub struct TypeWithCursor;
impl TypeWithCursor {
    /// Type the text letter by letter with `cursor` following the last letter.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId, cursor: MobjectId) -> ShowLetters {
        ShowLetters::make(id, "TypeWithCursor", false, Some(cursor))
    }
}

/// Namespace for deleting text with a cursor mobject following, last letter first.
pub struct UntypeWithCursor;
impl UntypeWithCursor {
    /// Delete the text letter by letter, last first, with `cursor` following.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: MobjectId, cursor: MobjectId) -> ShowLetters {
        ShowLetters::make(id, "UntypeWithCursor", true, Some(cursor))
    }
}

impl Animation for ShowLetters {
    impl_config!();
    fn name(&self) -> String {
        self.name.into()
    }
    fn resolve(&mut self, w: &World) {
        if self.cfg.run_time < 0.0 {
            self.cfg.run_time = self.time_per_char * w.try_get(self.id).map_or(1, |m| m.leaves().len().max(1)) as f64;
        }
    }
    fn validate(&self, w: &World) -> anyhow::Result<()> {
        w.try_get(self.id).map(|_| ())
    }
    fn begin(&mut self, w: &mut World) {
        self.resolve(w);
        self.original = w[self.id].clone();
        self.leaves = self.original.leaves();
        w.show(self.id);
        if let Some(c) = self.cursor {
            self.cursor_start = Some(w[c].clone());
            w.show(c);
        }
    }
    fn apply(&mut self, w: &mut World, alpha: f64) {
        let k = self.shown(alpha);
        w[self.id] = VMobject { z_index: self.original.z_index, ..VMobject::group(self.leaves[..k].to_vec()) };
        if let (Some(c), Some(start)) = (self.cursor, &self.cursor_start) {
            let Some(first) = self.leaves.first() else { return };
            let half = start.width() / 2.0;
            let (x, y) = match k {
                0 => (first.get_critical_point(LEFT).x + half, first.get_center().y),
                _ => (
                    self.leaves[k - 1].get_critical_point(RIGHT).x + self.buff + half,
                    self.leaves[k - 1].get_center().y,
                ),
            };
            let from = start.get_center();
            let y = if self.keep_cursor_y { from.y } else { y };
            w[c] = start.clone().shift(Point::new(x - from.x, y - from.y, 0.0));
        }
    }
    fn clean_up(&mut self, w: &mut World) {
        w[self.id] = self.original.clone();
        if self.remove {
            w.hide(self.id);
        }
        if let (Some(c), false) = (self.cursor, self.leave_cursor_on) {
            w.hide(c);
        }
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
    /// Flash along the outline of mobject `id`.
    pub fn new(id: MobjectId) -> Self {
        ShowPassingFlash {
            id,
            cfg: AnimConfig::new(1.0, rate_functions::smooth, 0.0),
            time_width: 0.1,
            original: VMobject::default(),
            leaves: Vec::new(),
        }
    }
    /// Fraction of the outline (default 0.1) visible at once.
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
