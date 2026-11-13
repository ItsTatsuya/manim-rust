//! Layout inspection: what is on screen, and problems an explainer video should not have.
//!
//! [`inspect`] lists the displayed mobjects with their bounding boxes. A [`Checker`]
//! (enabled with `--check`) watches every `play` and reports:
//!
//! - `overlap`: two texts overlapping once an animation has finished,
//! - `offscreen`: text partly outside the frame once an animation has finished,
//! - `ghosting`: two texts overlapping while both are half-transparent mid-animation
//!   (a crossfade that reads as a smudge; fade one out before the other comes in).
//!
//! The background and overlay layers are ignored.

use std::collections::HashSet;

use serde::Serialize;

use crate::mobject::VMobject;
use crate::world::{MobjectId, World};

/// One displayed mobject.
#[derive(Clone, Debug, Serialize)]
pub struct Item {
    /// The scene id of the top-level mobject.
    pub id: usize,
    /// `kind` of the mobject (`Text`, `Math`, `Image`, ...), or `VMobject`.
    pub kind: String,
    /// Its text, or a short description.
    pub label: String,
    /// `[left, bottom, right, top]` in scene units.
    pub bbox: [f64; 4],
    /// Largest fill or stroke opacity in the family.
    pub opacity: f64,
    pub z_index: i32,
    /// Whether it draws text.
    pub text: bool,
}

fn bbox(m: &VMobject) -> Option<[f64; 4]> {
    let (lo, hi) = m.bounding_box()?;
    Some([lo.x, lo.y, hi.x, hi.y])
}

fn opacity(m: &VMobject) -> f64 {
    m.family()
        .iter()
        .filter(|f| !f.subpaths.is_empty() || f.meta.image.is_some())
        .map(|f| {
            let stroke = if f.stroke_width > 0.0 { f.stroke_opacity } else { 0.0 };
            f.fill_opacity.max(stroke)
        })
        .fold(0.0, f64::max)
}

/// Everything in the scene layer, in draw order.
pub fn inspect(world: &World) -> Vec<Item> {
    let mut out = Vec::new();
    for &id in world.displayed() {
        let m = &world[id];
        let Some(b) = bbox(m) else { continue };
        out.push(Item {
            id: id.0,
            kind: m.meta.kind.as_deref().unwrap_or("VMobject").to_string(),
            label: m.describe(),
            bbox: b,
            opacity: opacity(m),
            z_index: m.z_index,
            text: m.has_text(),
        });
    }
    out
}

/// A text block on screen: a `Text`/math mobject, or (mid-animation, when hierarchies
/// are flattened) all the glyphs of one top-level mobject.
#[derive(Clone, Debug)]
struct TextBlock {
    id: MobjectId,
    label: String,
    bbox: [f64; 4],
    opacity: f64,
}

fn text_blocks(world: &World) -> Vec<TextBlock> {
    let mut out = Vec::new();
    for &id in world.displayed() {
        let top = &world[id];
        let mut found = false;
        // Prefer tagged text units (keeps sentences inside one group apart).
        fn walk(m: &VMobject, id: MobjectId, out: &mut Vec<TextBlock>, found: &mut bool) {
            if matches!(m.meta.kind(), "Text" | "Math" | "DecimalNumber" | "Integer") {
                if let Some(b) = bbox(m) {
                    out.push(TextBlock { id, label: m.describe(), bbox: b, opacity: opacity(m) });
                    *found = true;
                }
                return;
            }
            for c in &m.children {
                walk(c, id, out, found);
            }
        }
        walk(top, id, &mut out, &mut found);
        if !found {
            // Flattened during an animation: gather glyph leaves.
            let glyphs: Vec<&VMobject> = top.family().into_iter().filter(|m| m.meta.char_index.is_some()).collect();
            if glyphs.is_empty() {
                continue;
            }
            let g = VMobject::group(glyphs.iter().map(|m| VMobject { children: vec![], ..(*m).clone() }).collect());
            if let Some(b) = bbox(&g) {
                out.push(TextBlock { id, label: top.describe(), bbox: b, opacity: opacity(&g) });
            }
        }
    }
    out
}

fn overlap_area(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let w = a[2].min(b[2]) - a[0].max(b[0]);
    let h = a[3].min(b[3]) - a[1].max(b[1]);
    if w <= 0.0 || h <= 0.0 {
        0.0
    } else {
        w * h
    }
}

fn area(a: &[f64; 4]) -> f64 {
    ((a[2] - a[0]) * (a[3] - a[1])).max(1e-9)
}

#[derive(Clone, Debug, Serialize)]
pub struct Problem {
    /// `overlap`, `offscreen` or `ghosting`.
    pub kind: String,
    /// Scene time in seconds.
    pub time: f64,
    /// Index of the `play` it happened in.
    pub play: Option<usize>,
    pub a: String,
    pub b: Option<String>,
    pub message: String,
}

/// Watches plays for layout problems.
pub struct Checker {
    frame_width: f64,
    frame_height: f64,
    frame_center: [f64; 2],
    /// How much of the smaller box must be covered to count as overlapping.
    pub min_overlap: f64,
    /// How far (scene units) text may poke past the frame edge.
    pub edge_tolerance: f64,
    problems: Vec<Problem>,
    seen: HashSet<(String, String, Option<String>)>,
}

impl Checker {
    pub fn new(frame_width: f64, frame_height: f64) -> Self {
        Checker {
            frame_width,
            frame_height,
            frame_center: [0.0, 0.0],
            min_overlap: 0.08,
            edge_tolerance: 0.02,
            problems: Vec::new(),
            seen: HashSet::new(),
        }
    }

    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    fn report(&mut self, p: Problem) -> Option<&Problem> {
        let key = (p.kind.clone(), p.a.clone(), p.b.clone());
        if self.seen.insert(key) {
            self.problems.push(p);
            self.problems.last()
        } else {
            None
        }
    }

    /// Follow the camera: offscreen checks use this frame (scene units).
    pub fn set_frame(&mut self, width: f64, height: f64, center: [f64; 2]) {
        self.frame_width = width;
        self.frame_height = height;
        self.frame_center = center;
    }

    /// State after a play finished. Returns the new problems.
    pub fn settled(&mut self, world: &World, time: f64, play: Option<usize>) -> Vec<Problem> {
        let blocks: Vec<TextBlock> = text_blocks(world).into_iter().filter(|b| b.opacity > 0.2).collect();
        let mut new = Vec::new();
        for (i, a) in blocks.iter().enumerate() {
            for b in &blocks[i + 1..] {
                let o = overlap_area(&a.bbox, &b.bbox);
                if o > self.min_overlap * area(&a.bbox).min(area(&b.bbox)) {
                    let p = Problem {
                        kind: "overlap".into(),
                        time,
                        play,
                        a: a.label.clone(),
                        b: Some(b.label.clone()),
                        message: format!("text {:?} overlaps {:?}", a.label, b.label),
                    };
                    new.extend(self.report(p).cloned());
                }
            }
            let (hw, hh) =
                (self.frame_width / 2.0 + self.edge_tolerance, self.frame_height / 2.0 + self.edge_tolerance);
            let [l, bo, r, t] = a.bbox;
            let [cx, cy] = self.frame_center;
            let (l, r, bo, t) = (l - cx, r - cx, bo - cy, t - cy);
            if l < -hw || r > hw || bo < -hh || t > hh {
                let p = Problem {
                    kind: "offscreen".into(),
                    time,
                    play,
                    a: a.label.clone(),
                    b: None,
                    message: format!("text {:?} runs off the frame", a.label),
                };
                new.extend(self.report(p).cloned());
            }
        }
        new
    }

    /// State in the middle of a play. Returns the new problems.
    pub fn midway(&mut self, world: &World, time: f64, play: Option<usize>) -> Vec<Problem> {
        let blocks = text_blocks(world);
        let fading = |b: &TextBlock| b.opacity > 0.08 && b.opacity < 0.92;
        let mut new = Vec::new();
        for (i, a) in blocks.iter().enumerate() {
            for b in &blocks[i + 1..] {
                if a.id == b.id || !(fading(a) && fading(b)) {
                    continue;
                }
                let o = overlap_area(&a.bbox, &b.bbox);
                if o > self.min_overlap * area(&a.bbox).min(area(&b.bbox)) {
                    let p = Problem {
                        kind: "ghosting".into(),
                        time,
                        play,
                        a: a.label.clone(),
                        b: Some(b.label.clone()),
                        message: format!("{:?} and {:?} crossfade on top of each other", a.label, b.label),
                    };
                    new.extend(self.report(p).cloned());
                }
            }
        }
        new
    }
}
