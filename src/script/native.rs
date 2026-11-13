//! The functions the JS `manim` module calls (`globalThis.__manim_native`).
//!
//! JS mobjects are handles into a registry. A handle is either a value not yet in the
//! scene, a mobject in the scene's world, or a view of a child inside another handle
//! (`group.get(2)`, members of a `VGroup`). Arguments travel as JSON strings, with
//! mobjects written as `{"$m": handle}`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use rquickjs::{Ctx, Exception, Function, Object};
use serde_json::{json, Value};

use crate::animation::*;
use crate::color::{Color, BLACK, PURE_YELLOW, YELLOW_C};
use crate::constants::*;
use crate::mobject::code::{Code, CodeBackground};
use crate::mobject::geometry::*;
use crate::mobject::graphing::{Axes, NumberPlane};
use crate::mobject::grid::{GridMobject, BRACKETS, HORIZONTAL_LINES, VERTICAL_LINES};
use crate::mobject::matrix::{self, Matrix};
use crate::mobject::table::Table;
use crate::mobject::{Subpath, VMobject};
use crate::rate_functions::{self, RateFunc};
use crate::scene::{Playback, Scene};
use crate::text::{Align, DecimalNumber, Text, TextStyle};
use crate::theme::{TextRole, Theme};
use crate::world::MobjectId;

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)] // most slots are detached values; boxing them costs more
enum Slot {
    Free,
    Detached(VMobject),
    Attached(MobjectId),
    View { root: u32, path: Vec<usize> },
}

/// A view popped out of its group for the length of a play.
struct Pop {
    handle: u32,
    root: u32,
    path: Vec<usize>,
    id: MobjectId,
}

#[derive(Default)]
pub(crate) struct State {
    pub scene: Option<Scene>,
    slots: Vec<Slot>,
    by_id: HashMap<MobjectId, u32>,
    playbacks: HashMap<u32, (Playback, Vec<Pop>)>,
    next_playback: u32,
    pub base_dir: PathBuf,
    pub sandbox: Option<PathBuf>,
}

thread_local! {
    pub(crate) static STATE: RefCell<State> = RefCell::new(State::default());
}

pub(crate) fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn scene(st: &mut State) -> Result<&mut Scene> {
    st.scene.as_mut().ok_or_else(|| anyhow!("no scene is running (create mobjects inside construct())"))
}

// ---------------------------------------------------------------------- JSON helpers

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn opt_num(o: &Value, k: &str) -> Option<f64> {
    o.get(k).and_then(num)
}

fn point(v: &Value) -> Option<Point> {
    let a = v.as_array()?;
    let x = a.first()?.as_f64()?;
    let y = a.get(1).and_then(Value::as_f64).unwrap_or(0.0);
    let z = a.get(2).and_then(Value::as_f64).unwrap_or(0.0);
    Some(Point::new(x, y, z))
}

fn opt_point(o: &Value, k: &str) -> Option<Point> {
    o.get(k).and_then(point)
}

fn color(v: &Value) -> Option<Color> {
    if let Some(s) = v.as_str() {
        return Color::hex(s);
    }
    let a = v.as_array()?;
    let c = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    Some(Color::rgb(c(0), c(1), c(2)))
}

fn opt_color(o: &Value, k: &str) -> Result<Option<Color>> {
    match o.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => color(v).map(Some).ok_or_else(|| anyhow!("{k}: invalid color {v} (use \"#RRGGBB\")")),
    }
}

fn mob_ref(v: &Value) -> Option<u32> {
    v.get("$m").and_then(Value::as_u64).map(|h| h as u32)
}

fn hex(c: Color) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", b(c.r), b(c.g), b(c.b))
}

fn pt_json(p: Point) -> Value {
    json!([p.x, p.y, p.z])
}

pub(crate) fn rate_func(name: &str) -> Result<RateFunc> {
    use rate_functions::*;
    let key = name.to_ascii_lowercase().replace(['-', '_'], "");
    Ok(match key.as_str() {
        "linear" => linear,
        "smooth" => smooth,
        "smoothstep" => smoothstep,
        "rushinto" => rush_into,
        "rushfrom" => rush_from,
        "slowinto" => slow_into,
        "doublesmooth" => double_smooth,
        "thereandback" => there_and_back,
        "thereandbackwithpause" => there_and_back_with_pause,
        "wiggle" => wiggle,
        "lingering" => lingering,
        "exponentialdecay" => exponential_decay,
        "easeinsine" => ease_in_sine,
        "easeoutsine" => ease_out_sine,
        "easeinoutsine" => ease_in_out_sine,
        "easeinquad" => ease_in_quad,
        "easeoutquad" => ease_out_quad,
        "easeinoutquad" => ease_in_out_quad,
        "easeincubic" => ease_in_cubic,
        "easeoutcubic" => ease_out_cubic,
        "easeinoutcubic" => ease_in_out_cubic,
        "easeoutback" => ease_out_back,
        "easeoutbounce" => ease_out_bounce,
        _ => bail!("unknown rate function '{name}'"),
    })
}

// ---------------------------------------------------------------------- handles

fn new_handle(st: &mut State, slot: Slot) -> u32 {
    let h = st.slots.len() as u32;
    if let Slot::Attached(id) = slot {
        st.by_id.insert(id, h);
    }
    st.slots.push(slot);
    h
}

fn slot(st: &State, h: u32) -> Result<Slot> {
    match st.slots.get(h as usize) {
        Some(Slot::Free) | None => bail!("mobject handle {h} is not valid"),
        // After a seek the world is an earlier keyframe: mobjects created later are gone.
        Some(Slot::Attached(id)) if st.scene.as_ref().is_some_and(|s| !s.world().contains(*id)) => {
            bail!("mobject handle {h} refers to a mobject that does not exist at this point of the scene")
        }
        Some(s) => Ok(s.clone()),
    }
}

/// Resolve a handle to its root (detached handle or world id) and child path.
enum Base {
    Detached(u32),
    World(MobjectId),
}

fn resolve(st: &State, h: u32) -> Result<(Base, Vec<usize>)> {
    match slot(st, h)? {
        Slot::Detached(_) => Ok((Base::Detached(h), Vec::new())),
        Slot::Attached(id) => Ok((Base::World(id), Vec::new())),
        Slot::View { root, path } => {
            let (base, mut p) = resolve(st, root)?;
            p.extend(path);
            Ok((base, p))
        }
        Slot::Free => unreachable!(),
    }
}

fn walk<'a>(mut m: &'a mut VMobject, path: &[usize]) -> Result<&'a mut VMobject> {
    for &i in path {
        let n = m.children.len();
        m = m.children.get_mut(i).ok_or_else(|| anyhow!("child {i} does not exist (the group has {n})"))?;
    }
    Ok(m)
}

fn with_mob<R>(st: &mut State, h: u32, f: impl FnOnce(&mut VMobject) -> R) -> Result<R> {
    let (base, path) = resolve(st, h)?;
    let root: &mut VMobject = match base {
        Base::Detached(r) => match &mut st.slots[r as usize] {
            Slot::Detached(m) => m,
            _ => unreachable!(),
        },
        Base::World(id) => scene(st)?.world_mut().try_get_mut(id)?,
    };
    Ok(f(walk(root, &path)?))
}

fn get_mob(st: &mut State, h: u32) -> Result<VMobject> {
    with_mob(st, h, |m| m.clone())
}

/// World id for a handle, inserting it if needed (`show` displays new ones).
/// Views are popped out of their group; the pops are returned for `push_back`.
fn ensure_in_world(st: &mut State, h: u32, show: bool, pops: &mut Vec<Pop>) -> Result<MobjectId> {
    match slot(st, h)? {
        Slot::Attached(id) => Ok(id),
        Slot::Detached(m) => {
            let s = scene(st)?;
            let id = if show { s.add(m) } else { s.insert(m) };
            st.slots[h as usize] = Slot::Attached(id);
            st.by_id.insert(id, h);
            Ok(id)
        }
        Slot::View { root, path } => {
            let sub = with_mob(st, h, |m| std::mem::replace(m, VMobject::group(vec![])))?;
            let root_shown = is_shown(st, root)?;
            let s = scene(st)?;
            let id = if root_shown || show { s.add(sub) } else { s.insert(sub) };
            st.slots[h as usize] = Slot::Attached(id);
            st.by_id.insert(id, h);
            pops.push(Pop { handle: h, root, path, id });
            Ok(id)
        }
        Slot::Free => unreachable!(),
    }
}

fn is_shown(st: &mut State, h: u32) -> Result<bool> {
    match resolve(st, h)?.0 {
        Base::Detached(_) => Ok(false),
        Base::World(id) => Ok(scene(st)?.world().is_displayed(id)),
    }
}

/// Put popped views back into their groups after a play.
fn push_back(st: &mut State, pops: Vec<Pop>) -> Result<()> {
    for p in pops.into_iter().rev() {
        let (value, shown) = {
            let s = scene(st)?;
            (s.world()[p.id].clone(), s.world().is_displayed(p.id))
        };
        scene(st)?.world_mut().remove_everywhere(p.id);
        st.by_id.remove(&p.id);
        // Only restore if the slot is still the popped one (it may have been regrouped).
        if !matches!(st.slots.get(p.handle as usize), Some(Slot::Attached(id)) if *id == p.id) {
            continue;
        }
        if shown {
            st.slots[p.handle as usize] = Slot::View { root: p.root, path: p.path.clone() };
            with_mob(st, p.handle, |m| *m = value)?;
        } else {
            // Faded out / uncreated: it leaves the group (Manim removes it from the scene).
            st.slots[p.handle as usize] = Slot::Detached(value);
        }
    }
    Ok(())
}

fn handle_for_id(st: &mut State, id: MobjectId) -> u32 {
    if let Some(h) = st.by_id.get(&id) {
        if matches!(st.slots.get(*h as usize), Some(Slot::Attached(i)) if *i == id) {
            return *h;
        }
    }
    new_handle(st, Slot::Attached(id))
}

/// Read every `{"$m": h}` in `args` so ops can use other mobjects by value.
fn collect_refs(st: &mut State, v: &Value, out: &mut HashMap<u32, VMobject>) -> Result<()> {
    if let Some(h) = mob_ref(v) {
        if let std::collections::hash_map::Entry::Vacant(e) = out.entry(h) {
            e.insert(get_mob(st, h)?);
        }
        return Ok(());
    }
    match v {
        Value::Array(a) => a.iter().try_for_each(|x| collect_refs(st, x, out)),
        Value::Object(o) => o.values().try_for_each(|x| collect_refs(st, x, out)),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------- ops

struct Refs(HashMap<u32, VMobject>);

impl Refs {
    fn mob(&self, v: &Value) -> Option<&VMobject> {
        mob_ref(v).and_then(|h| self.0.get(&h))
    }
    /// A point, or a mobject (its center, or the critical point in `dir`).
    fn point(&self, v: &Value, dir: Point) -> Option<Point> {
        match self.mob(v) {
            Some(m) => Some(m.get_critical_point(dir)),
            None => point(v),
        }
    }
}

fn arg(args: &[Value], i: usize) -> &Value {
    args.get(i).unwrap_or(&Value::Null)
}

fn arg_num(args: &[Value], i: usize, default: f64) -> f64 {
    arg(args, i).as_f64().unwrap_or(default)
}

fn arg_pt(args: &[Value], i: usize, default: Point) -> Point {
    point(arg(args, i)).unwrap_or(default)
}

fn arg_color(args: &[Value], i: usize, name: &str) -> Result<Color> {
    color(arg(args, i)).ok_or_else(|| anyhow!("{name}: expected a color, got {}", arg(args, i)))
}

/// Apply a mutating op (builder style). Unknown names are an error.
fn apply_op(m: VMobject, name: &str, args: &[Value], refs: &Refs) -> Result<VMobject> {
    Ok(match name {
        "shift" => {
            let v = args.iter().filter_map(point).fold(ORIGIN, |a, b| a + b);
            m.shift(v)
        }
        "moveTo" => {
            let edge = arg_pt(args, 1, ORIGIN);
            let p = refs.point(arg(args, 0), edge).context("moveTo needs a point or mobject")?;
            m.move_to_aligned(p, edge)
        }
        "nextTo" => {
            let dir = arg_pt(args, 1, RIGHT);
            let buff = arg_num(args, 2, DEFAULT_MOBJECT_TO_MOBJECT_BUFFER);
            let edge = arg_pt(args, 3, ORIGIN);
            match refs.mob(arg(args, 0)) {
                Some(o) => m.next_to_aligned(o, dir, buff, edge),
                None => {
                    let p = point(arg(args, 0)).context("nextTo needs a point or mobject")?;
                    let align = m.get_critical_point(edge - dir);
                    m.shift(p - align + dir * buff)
                }
            }
        }
        "toEdge" => m.to_edge_buff(arg_pt(args, 0, LEFT), arg_num(args, 1, DEFAULT_MOBJECT_TO_EDGE_BUFFER)),
        "toCorner" => m.to_edge_buff(arg_pt(args, 0, DL), arg_num(args, 1, DEFAULT_MOBJECT_TO_EDGE_BUFFER)),
        "alignTo" => {
            let dir = arg_pt(args, 1, UP);
            match refs.mob(arg(args, 0)) {
                Some(o) => m.align_to(o, dir),
                None => {
                    let p = point(arg(args, 0)).context("alignTo needs a point or mobject")?;
                    let cur = m.get_critical_point(dir);
                    let mask = pt((dir.x != 0.0) as i32 as f64, (dir.y != 0.0) as i32 as f64);
                    m.shift((p - cur) * mask)
                }
            }
        }
        "scale" => {
            let f = arg_num(args, 0, 1.0);
            match point(arg(args, 1)) {
                Some(about) => m.scale_about(f, about),
                None => m.scale(f),
            }
        }
        "stretch" => m.stretch(arg_num(args, 0, 1.0), arg_num(args, 1, 0.0) as usize),
        "rotate" => {
            let a = arg_num(args, 0, 0.0);
            match point(arg(args, 1)) {
                Some(about) => m.rotate_about(a, about),
                None => m.rotate(a),
            }
        }
        "flip" => m.flip(),
        "setWidth" | "scaleToFitWidth" => m.set_width(arg_num(args, 0, 1.0)),
        "setHeight" | "scaleToFitHeight" => m.set_height(arg_num(args, 0, 1.0)),
        "matchWidth" => {
            let w = refs.mob(arg(args, 0)).context("matchWidth needs a mobject")?.width();
            m.set_width(w)
        }
        "matchHeight" => {
            let h = refs.mob(arg(args, 0)).context("matchHeight needs a mobject")?.height();
            m.set_height(h)
        }
        "setX" => m.set_x(arg_num(args, 0, 0.0)),
        "setY" => m.set_y(arg_num(args, 0, 0.0)),
        "center" => m.center(),
        "arrange" => {
            let dir = arg_pt(args, 0, RIGHT);
            let buff = arg_num(args, 1, DEFAULT_MOBJECT_TO_MOBJECT_BUFFER);
            match point(arg(args, 2)) {
                Some(edge) => {
                    let mut m = m;
                    for i in 1..m.children.len() {
                        let prev = m.children[i - 1].clone();
                        m.children[i].apply(|c| c.next_to_aligned(&prev, dir, buff, edge));
                    }
                    m.center()
                }
                None => m.arrange_buff(dir, buff),
            }
        }
        "arrangeInGrid" => m.arrange_in_grid(arg_num(args, 0, 3.0).max(1.0) as usize, arg_num(args, 1, MED_SMALL_BUFF)),
        "setColor" => m.set_color(arg_color(args, 0, "setColor")?),
        "setFill" => {
            let c = color(arg(args, 0));
            let o = arg(args, 1).as_f64();
            let mut m = m;
            if let Some(c) = c {
                m = m.set_fill_color(c);
            }
            if let Some(o) = o {
                m = m.set_fill_opacity(o);
            }
            m
        }
        "setFillColor" => m.set_fill_color(arg_color(args, 0, "setFillColor")?),
        "setFillOpacity" => m.set_fill_opacity(arg_num(args, 0, 1.0)),
        "setStroke" => {
            let mut m = m;
            if let Some(c) = color(arg(args, 0)) {
                m = m.set_stroke_color(c);
            }
            if let Some(w) = arg(args, 1).as_f64() {
                m = m.set_stroke_width(w);
            }
            if let Some(o) = arg(args, 2).as_f64() {
                m = m.set_stroke_opacity(o);
            }
            m
        }
        "setStrokeColor" => m.set_stroke_color(arg_color(args, 0, "setStrokeColor")?),
        "setStrokeWidth" => m.set_stroke_width(arg_num(args, 0, DEFAULT_STROKE_WIDTH)),
        "setStrokeOpacity" => m.set_stroke_opacity(arg_num(args, 0, 1.0)),
        "setOpacity" => m.set_opacity(arg_num(args, 0, 1.0)),
        "fade" => m.fade(arg_num(args, 0, 0.5)),
        "setColorByGradient" => {
            let cs: Vec<Color> = args
                .iter()
                .flat_map(|a| -> Vec<Color> {
                    match a {
                        Value::Array(items) if items.first().is_some_and(|x| !x.is_number()) => {
                            items.iter().filter_map(color).collect()
                        }
                        other => color(other).into_iter().collect(),
                    }
                })
                .collect();
            m.set_color_by_gradient(&cs)
        }
        "setZIndex" => m.set_z_index(arg_num(args, 0, 0.0) as i32),
        "setColorText" => {
            let needle = arg(args, 0).as_str().context("setColorText needs a string")?;
            m.set_color_text(needle, arg_color(args, 1, "setColorText")?)
        }
        "setColorChars" => m.set_color_chars(
            arg_num(args, 0, 0.0) as usize,
            arg_num(args, 1, 0.0) as usize,
            arg_color(args, 2, "setColorChars")?,
        ),
        "fitWithin" => m.fit_within(
            arg_num(args, 0, FRAME_WIDTH),
            arg_num(args, 1, FRAME_HEIGHT),
            arg(args, 2).as_bool().unwrap_or(false),
            arg_num(args, 3, 1.5),
        ),
        "become" => {
            let o = refs.mob(arg(args, 0)).context("become needs a mobject")?.clone();
            let z = m.z_index;
            if arg(args, 1).as_bool().unwrap_or(false) {
                o.set_z_index(z)
            } else {
                o
            }
        }
        "setPoints" => {
            // [[subpath: [[x,y,z] x4 per cubic ...], closed], ...] for each family member in order.
            let data = arg(args, 0).as_array().context("setPoints needs an array")?;
            let mut m = m;
            let mut k = 0;
            let mut err = None;
            m.for_each_mut(&mut |f| {
                if f.subpaths.is_empty() {
                    return;
                }
                let Some(sps) = data.get(k).and_then(Value::as_array) else {
                    err = Some(anyhow!("setPoints: missing data for family member {k}"));
                    return;
                };
                k += 1;
                for (sp, d) in f.subpaths.iter_mut().zip(sps) {
                    let pts: Vec<Point> =
                        d.as_array().map(|a| a.iter().filter_map(point).collect()).unwrap_or_default();
                    for (c, chunk) in sp.curves.iter_mut().zip(pts.chunks(4)) {
                        if chunk.len() == 4 {
                            *c = [chunk[0], chunk[1], chunk[2], chunk[3]];
                        }
                    }
                }
                if let Some(a) = &mut f.meta.anchors {
                    let _ = a;
                }
            });
            if let Some(e) = err {
                return Err(e);
            }
            m
        }
        "insertCurves" => m.insert_curves(arg_num(args, 0, 2.0).max(1.0) as usize),
        "setKind" => m.with_kind(arg(args, 0).as_str().unwrap_or("VMobject")),
        "setLabel" => m.with_label(arg(args, 0).as_str().unwrap_or("")),
        "setRowColors" => m.set_row_colors(&colors_arg(arg(args, 0))?),
        "setColumnColors" => m.set_column_colors(&colors_arg(arg(args, 0))?),
        "addHighlightedCell" => {
            let (r, c) = pos_arg(arg(args, 0))?;
            m.add_highlighted_cell(r, c, color(arg(args, 1)).unwrap_or(PURE_YELLOW))
        }
        "addBackgroundToEntries" => m.add_background_to_entries(color(arg(args, 0)).unwrap_or(BLACK)),
        other => bail!("unknown mobject method '{other}'"),
    })
}

/// Read-only queries; `None` if `name` is not a query.
fn query(m: &VMobject, name: &str, args: &[Value]) -> Result<Option<Value>> {
    Ok(Some(match name {
        "getCenter" => pt_json(m.get_center()),
        "getTop" => pt_json(m.get_top()),
        "getBottom" => pt_json(m.get_bottom()),
        "getLeft" => pt_json(m.get_left()),
        "getRight" => pt_json(m.get_right()),
        "getCorner" | "getCriticalPoint" | "getEdgeCenter" | "getBoundaryPoint" => {
            pt_json(m.get_critical_point(arg_pt(args, 0, ORIGIN)))
        }
        "getStart" => pt_json(m.get_start()),
        "getEnd" => pt_json(m.get_end()),
        "pointFromProportion" => pt_json(m.point_from_proportion(arg_num(args, 0, 0.0))),
        "width" | "getWidth" => json!(m.width()),
        "height" | "getHeight" => json!(m.height()),
        "bbox" => match m.bounding_box() {
            Some((lo, hi)) => json!([lo.x, lo.y, hi.x, hi.y]),
            None => Value::Null,
        },
        "text" => json!(m.text()),
        "findText" => match m.find_text(arg(args, 0).as_str().unwrap_or("")) {
            Some((a, b)) => json!([a, b]),
            None => Value::Null,
        },
        "length" => json!(m.children.len()),
        "zIndex" => json!(m.z_index),
        "kind" => json!(m.meta.kind()),
        "getColor" | "getFillColor" => {
            json!(hex(m.family().iter().find(|f| !f.subpaths.is_empty()).map(|f| f.fill_color).unwrap_or(m.fill_color)))
        }
        "getStrokeColor" => json!(hex(m
            .family()
            .iter()
            .find(|f| !f.subpaths.is_empty())
            .map(|f| f.stroke_color)
            .unwrap_or(m.stroke_color))),
        "getFillOpacity" => {
            json!(m.family().iter().find(|f| !f.subpaths.is_empty()).map(|f| f.fill_opacity).unwrap_or(m.fill_opacity))
        }
        "getStrokeWidth" => {
            json!(m.family().iter().find(|f| !f.subpaths.is_empty()).map(|f| f.stroke_width).unwrap_or(m.stroke_width))
        }
        "points" => {
            let fam: Vec<Value> = m
                .family()
                .iter()
                .filter(|f| !f.subpaths.is_empty())
                .map(|f| {
                    json!(f
                        .subpaths
                        .iter()
                        .map(|sp| sp.curves.iter().flat_map(|c| c.iter().map(|p| pt_json(*p))).collect::<Vec<_>>())
                        .collect::<Vec<_>>())
                })
                .collect();
            json!(fam)
        }
        "hasText" => json!(m.has_text()),
        "childIndexOfKind" => json!(m.child_index_of_kind(arg(args, 0).as_str().unwrap_or(""))),
        "gridInfo" => match m.grid() {
            Some(g) => json!({
                "rows": g.rows, "cols": g.cols, "rowLabels": g.row_labels, "colLabels": g.col_labels,
                "entries": m.children.iter().position(|c| c.meta.grid.is_some()),
                "hlines": m.child_index_of_kind(HORIZONTAL_LINES),
                "vlines": m.child_index_of_kind(VERTICAL_LINES),
                "brackets": m.child_index_of_kind(BRACKETS),
            }),
            None => Value::Null,
        },
        _ => return Ok(None),
    }))
}

// ---------------------------------------------------------------------- construction

fn style_opts(mut m: VMobject, o: &Value) -> Result<VMobject> {
    if let Some(c) = opt_color(o, "color")? {
        m = m.set_color(c);
    }
    if let Some(c) = opt_color(o, "fillColor")? {
        m = m.set_fill_color(c);
    }
    if let Some(v) = opt_num(o, "fillOpacity") {
        m = m.set_fill_opacity(v);
    }
    if let Some(c) = opt_color(o, "strokeColor")? {
        m = m.set_stroke_color(c);
    }
    if let Some(v) = opt_num(o, "strokeWidth") {
        m = m.set_stroke_width(v);
    }
    if let Some(v) = opt_num(o, "strokeOpacity") {
        m = m.set_stroke_opacity(v);
    }
    if let Some(v) = opt_num(o, "opacity") {
        m = m.set_opacity(v);
    }
    if let Some(v) = opt_num(o, "zIndex") {
        m = m.set_z_index(v as i32);
    }
    Ok(m)
}

fn text_style(st: &mut State, o: &Value) -> Result<TextStyle> {
    let theme = st.scene.as_ref().and_then(|s| s.theme().cloned());
    let mut style = match (o.get("role").and_then(Value::as_str), &theme) {
        (Some(role), Some(t)) => t.text_style(role.parse::<TextRole>()?),
        (Some(role), None) => {
            role.parse::<TextRole>()?;
            TextStyle::default()
        }
        _ => TextStyle::default(),
    };
    if let Some(f) = o.get("font").and_then(Value::as_str) {
        style.font = f.to_string();
    }
    if let Some(v) = opt_num(o, "fontSize") {
        style.font_size = v;
    }
    if let Some(w) = o.get("weight") {
        style.weight = match w {
            Value::String(s) => match s.to_ascii_lowercase().as_str() {
                "thin" => 100,
                "light" => 300,
                "normal" | "regular" => 400,
                "medium" => 500,
                "semibold" => 600,
                "bold" => 700,
                "heavy" | "black" => 900,
                other => other.parse().map_err(|_| anyhow!("unknown weight '{s}'"))?,
            },
            v => v.as_f64().unwrap_or(400.0) as u16,
        };
    }
    if o.get("italic").and_then(Value::as_bool).unwrap_or(false)
        || o.get("slant").and_then(Value::as_str) == Some("italic")
    {
        style.italic = true;
    }
    if let Some(c) = opt_color(o, "color")? {
        style.color = c;
    }
    if let Some(v) = opt_num(o, "lineSpacing") {
        style.line_spacing = v;
    }
    if let Some(v) = opt_num(o, "maxWidth") {
        style.max_width = Some(v);
    }
    if let Some(a) = o.get("align").and_then(Value::as_str) {
        style.align = match a {
            "center" => Align::Center,
            "right" => Align::Right,
            _ => Align::Left,
        };
    }
    if let Some(fs) = o.get("fallbackFonts").and_then(Value::as_array) {
        style.fallback_fonts.extend(fs.iter().filter_map(|f| f.as_str().map(String::from)));
    }
    Ok(style)
}

fn asset_path(st: &State, p: &str) -> Result<PathBuf> {
    let path = Path::new(p);
    let full = if path.is_absolute() { path.to_path_buf() } else { st.base_dir.join(path) };
    if let Some(root) = &st.sandbox {
        let canon = super::plain(full.canonicalize().with_context(|| format!("{} does not exist", full.display()))?);
        let root = super::plain(root.canonicalize().unwrap_or_else(|_| root.clone()));
        if !canon.starts_with(&root) {
            bail!("{p} is outside the project folder");
        }
        return Ok(canon);
    }
    Ok(full)
}

fn points_arg(o: &Value, k: &str) -> Result<Vec<Point>> {
    o.get(k)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(point).collect())
        .ok_or_else(|| anyhow!("'{k}' must be a list of points"))
}

fn make(st: &mut State, kind: &str, o: &Value, refs: &Refs) -> Result<VMobject> {
    let n = |k: &str, d: f64| opt_num(o, k).unwrap_or(d);
    let p = |k: &str, d: Point| opt_point(o, k).unwrap_or(d);
    let target = |k: &str| {
        refs.mob(o.get(k).unwrap_or(&Value::Null)).cloned().ok_or_else(|| anyhow!("{kind} needs '{k}' (a mobject)"))
    };
    let m = match kind {
        "VMobject" => VMobject::new(),
        "VGroup" | "Group" => VMobject::group(vec![]),
        "Circle" => Circle::new(n("radius", 1.0)),
        "Square" => Square::new(n("sideLength", 2.0)),
        "Rectangle" => Rectangle::new(n("width", 4.0), n("height", 2.0)),
        "RoundedRectangle" => RoundedRectangle::new(n("width", 4.0), n("height", 2.0), n("cornerRadius", 0.5)),
        "Ellipse" => Ellipse::new(n("width", 2.0), n("height", 1.0)),
        "Dot" => {
            let d = Dot::at(p("point", ORIGIN));
            match opt_num(o, "radius") {
                Some(r) => d.scale(r / DEFAULT_DOT_RADIUS),
                None => d,
            }
        }
        "Line" => Line::new(p("start", LEFT), p("end", RIGHT)),
        "DashedLine" => {
            DashedLine::with_dash(p("start", LEFT), p("end", RIGHT), n("dashLength", 0.05), n("dashedRatio", 0.5))
        }
        "Arrow" => Arrow::with_buff(p("start", LEFT), p("end", RIGHT), n("buff", MED_SMALL_BUFF)),
        "DoubleArrow" => DoubleArrow::new(p("start", LEFT), p("end", RIGHT)),
        "Vector" => Vector::new(p("direction", RIGHT)),
        "CurvedArrow" => {
            CurvedArrow::with_tip_length(p("start", LEFT), p("end", RIGHT), n("angle", -PI / 4.0), n("tipLength", 0.25))
        }
        "Arc" => Arc::new(n("radius", 1.0), n("startAngle", 0.0), n("angle", PI / 2.0)),
        "ArcBetweenPoints" => ArcBetweenPoints::new(p("start", LEFT), p("end", RIGHT), n("angle", PI / 2.0)),
        "Annulus" => Annulus::new(n("innerRadius", 1.0), n("outerRadius", 2.0)),
        "Sector" => Sector::new(n("radius", 1.0), n("startAngle", 0.0), n("angle", PI / 2.0)),
        "Polygon" => Polygon::new(&points_arg(o, "points")?),
        "Polyline" => Polyline::new(&points_arg(o, "points")?),
        "RegularPolygon" => RegularPolygon::with_radius(n("n", 6.0) as usize, n("radius", 1.0)),
        "Triangle" => Triangle::new(),
        "Star" => Star::new(n("n", 5.0) as usize, n("outerRadius", 1.0), n("innerRadius", 0.4)),
        "SurroundingRectangle" => {
            let t = target("target")?;
            match opt_num(o, "cornerRadius") {
                Some(r) => SurroundingRectangle::rounded(&t, n("buff", SMALL_BUFF), r),
                None => SurroundingRectangle::with_buff(&t, n("buff", SMALL_BUFF)),
            }
        }
        "BackgroundRectangle" => BackgroundRectangle::new(&target("target")?),
        "Brace" => Brace::new(&target("target")?, p("direction", DOWN)),
        "Text" | "Paragraph" => {
            let s = o.get("text").and_then(Value::as_str).unwrap_or("");
            Text::styled(s, text_style(st, o)?)
        }
        "MarkupText" => {
            let s = o.get("text").and_then(Value::as_str).unwrap_or("");
            Text::markup(s, text_style(st, o)?)?
        }
        "DecimalNumber" => DecimalNumber::styled(n("value", 0.0), n("decimals", 2.0) as usize, text_style(st, o)?),
        "Integer" => {
            let v = n("value", 0.0).round();
            DecimalNumber::styled(v, 0, text_style(st, o)?).with_kind("Integer")
        }
        #[cfg(feature = "math")]
        "MathTex" | "Typst" => {
            let src = o.get("source").and_then(Value::as_str).unwrap_or("");
            let size = n("fontSize", DEFAULT_FONT_SIZE);
            let color = opt_color(o, "color")?.unwrap_or_else(|| crate::defaults::get().text_color);
            if kind == "MathTex" {
                crate::math::MathTex::styled(src, size, color)?
            } else {
                crate::math::Typst::styled(src, size, color)?
            }
        }
        "ImageMobject" => {
            let path = o.get("path").and_then(Value::as_str).context("ImageMobject needs a path")?;
            let m = crate::image::ImageMobject::open(asset_path(st, path)?)?;
            match opt_num(o, "height") {
                Some(h) => m.set_height(h),
                None => m,
            }
        }
        #[cfg(feature = "svg")]
        "SVGMobject" => {
            let h = n("height", 2.0);
            let m = match (o.get("path").and_then(Value::as_str), o.get("svg").and_then(Value::as_str)) {
                (Some(path), _) => {
                    let full = asset_path(st, path)?;
                    let data = std::fs::read(&full).with_context(|| format!("reading {}", full.display()))?;
                    crate::svg::SVGMobject::from_data_height(&data, h)?.with_label(path)
                }
                (None, Some(svg)) => crate::svg::SVGMobject::from_data_height(svg.as_bytes(), h)?,
                _ => bail!("SVGMobject needs a path or svg text"),
            };
            m
        }
        "SmoothCurve" => {
            // Sample runs from JS (`null` breaks the curve), smoothed through.
            let runs = o.get("runs").and_then(Value::as_array).context("SmoothCurve needs runs")?;
            let subpaths = runs
                .iter()
                .filter_map(|r| {
                    let pts: Vec<Point> = r.as_array()?.iter().filter_map(point).collect();
                    (pts.len() > 1).then(|| Subpath::new(crate::bezier::smooth_through(&pts), false))
                })
                .collect();
            VMobject { stroke_color: YELLOW_C, fill_color: YELLOW_C, ..VMobject::from_subpaths(subpaths) }
        }
        "Axes" => axes(o)?.to_mobject().with_kind("Axes"),
        "NumberPlane" => {
            let xr = o.get("xRange").and_then(range3).unwrap_or([-7.0, 7.0, 1.0]);
            let yr = o.get("yRange").and_then(range3).unwrap_or([-4.0, 4.0, 1.0]);
            NumberPlane::with_ranges(xr, yr)
        }
        "Table" | "MathTable" | "IntegerTable" | "DecimalTable" | "MobjectTable" => table(st, kind, o, refs)?,
        "Matrix" | "IntegerMatrix" | "DecimalMatrix" | "MobjectMatrix" => matrix(st, kind, o, refs)?,
        "TableCell" => {
            let (r, c) = pos_arg(o.get("pos").unwrap_or(&Value::Null))?;
            target("target")?.get_cell(r, c)
        }
        "TableHighlightedCell" => {
            let (r, c) = pos_arg(o.get("pos").unwrap_or(&Value::Null))?;
            target("target")?.get_highlighted_cell(r, c, opt_color(o, "color")?.unwrap_or(PURE_YELLOW))
        }
        "DetText" => {
            let det = o.get("determinant").filter(|v| !v.is_null()).map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            });
            matrix::get_det_text(&target("target")?, det.as_deref())?
        }
        "Code" => code(st, o)?,
        other => bail!("unknown mobject type '{other}'"),
    };
    // Table/Matrix/Code options aren't styles for the whole mobject.
    if kind.ends_with("Table") || kind.ends_with("Matrix") || kind == "Code" {
        return Ok(m);
    }
    style_opts(m, o)
}

/// `[row, col]`, 1-based.
fn pos_arg(v: &Value) -> Result<(usize, usize)> {
    let a = v.as_array().filter(|a| a.len() == 2).context("a cell position is [row, col] (1-based)")?;
    let n = |i: usize| a[i].as_f64().filter(|x| *x >= 1.0).map(|x| x as usize);
    match (n(0), n(1)) {
        (Some(r), Some(c)) => Ok((r, c)),
        _ => bail!("a cell position is [row, col], both 1 or more; got {v}"),
    }
}

/// A list of colors; a nested list counts as its first color (Manim allows gradients there).
fn colors_arg(v: &Value) -> Result<Vec<Color>> {
    let list = v.as_array().context("expected a list of colors")?;
    list.iter()
        .map(|c| {
            // `[r, g, b]` is one color; a list of colors is a gradient.
            let c = match c.as_array() {
                Some(a) if a.first().is_some_and(|f| f.is_string() || f.is_array()) => &a[0],
                _ => c,
            };
            color(c).ok_or_else(|| anyhow!("invalid color {c}"))
        })
        .collect()
}

fn sub<'a>(o: &'a Value, k: &str) -> &'a Value {
    o.get(k).unwrap_or(&Value::Null)
}

fn flag(o: &Value, k: &str, default: bool) -> bool {
    o.get(k).and_then(Value::as_bool).unwrap_or(default)
}

/// One table or matrix entry: a mobject as is, else a string or number built per `kind`.
fn grid_entry(st: &mut State, kind: &str, v: &Value, refs: &Refs, cfg: &Value) -> Result<VMobject> {
    if let Some(m) = refs.mob(v) {
        return Ok(m.clone());
    }
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    let number = || v.as_f64().or_else(|| s.trim().parse().ok()).ok_or_else(|| anyhow!("{kind}: {v} is not a number"));
    // Manim's matrices and math tables set entries smaller than `Text`; `fontSize` scales that.
    let scale = opt_num(cfg, "fontSize").map_or(1.0, |f| f / DEFAULT_FONT_SIZE);
    let color = opt_color(cfg, "color")?;
    let numbers = || {
        let st = matrix::ce_number_style();
        let size = st.font_size * scale;
        let st = st.size(size);
        match color {
            Some(c) => st.color(c),
            None => st,
        }
    };
    Ok(match kind {
        "IntegerTable" | "IntegerMatrix" => DecimalNumber::styled(number()?.round(), 0, numbers()).with_kind("Integer"),
        "DecimalTable" | "DecimalMatrix" => {
            let places = opt_num(cfg, "numDecimalPlaces").unwrap_or(1.0).max(0.0) as usize;
            DecimalNumber::styled(number()?, places, numbers())
        }
        "MobjectTable" | "MobjectMatrix" => bail!("{kind} entries must be mobjects, got {v}"),
        "MathTable" | "Matrix" => {
            #[cfg(feature = "math")]
            {
                let c = color.unwrap_or_else(|| crate::defaults::get().text_color);
                crate::math::MathTex::styled(&s, matrix::ce_math_size() * scale, c)?
            }
            #[cfg(not(feature = "math"))]
            {
                Text::styled(&s, numbers())
            }
        }
        _ => Text::styled(&s, text_style(st, cfg)?),
    })
}

fn grid_rows(st: &mut State, kind: &str, o: &Value, refs: &Refs) -> Result<Vec<Vec<VMobject>>> {
    let key = if kind.contains("Matrix") { "matrix" } else { "table" };
    let rows = o.get(key).and_then(Value::as_array).with_context(|| format!("{kind} needs '{key}', a list of rows"))?;
    let cfg = sub(o, "elementToMobjectConfig").clone();
    rows.iter()
        .map(|r| {
            let r = r.as_array().with_context(|| format!("{kind}: each row must be a list, got {r}"))?;
            r.iter().map(|v| grid_entry(st, kind, v, refs, &cfg)).collect()
        })
        .collect()
}

fn labels(st: &mut State, o: &Value, k: &str, refs: &Refs) -> Result<Vec<VMobject>> {
    let Some(list) = o.get(k).and_then(Value::as_array) else { return Ok(Vec::new()) };
    list.iter().map(|v| grid_entry(st, "Table", v, refs, &Value::Null)).collect()
}

fn table(st: &mut State, kind: &str, o: &Value, refs: &Refs) -> Result<VMobject> {
    let mut t = Table::from_mobjects(grid_rows(st, kind, o, refs)?)
        .row_labels(labels(st, o, "rowLabels", refs)?)
        .col_labels(labels(st, o, "colLabels", refs)?)
        .include_outer_lines(flag(o, "includeOuterLines", false))
        .include_inner_lines(flag(o, "includeInnerLines", true));
    if let Some(v) = o.get("topLeftEntry").filter(|v| !v.is_null()) {
        t = t.top_left_entry(grid_entry(st, "Table", v, refs, &Value::Null)?);
    }
    if let Some(b) = opt_num(o, "vBuff") {
        t = t.v_buff(b);
    }
    if let Some(b) = opt_num(o, "hBuff") {
        t = t.h_buff(b);
    }
    let lc = sub(o, "lineConfig");
    if let Some(c) = opt_color(lc, "color")?.or(opt_color(lc, "strokeColor")?) {
        t = t.line_color(c);
    }
    if let Some(w) = opt_num(lc, "strokeWidth") {
        t = t.line_width(w);
    }
    if flag(o, "addBackgroundRectanglesToEntries", false) {
        t = t.add_background_rectangles_to_entries(opt_color(o, "entriesBackgroundColor")?.unwrap_or(BLACK));
    }
    if flag(o, "includeBackgroundRectangle", false) {
        t = t.include_background_rectangle(opt_color(o, "backgroundRectangleColor")?.unwrap_or(BLACK));
    }
    t.build()
}

fn matrix(st: &mut State, kind: &str, o: &Value, refs: &Refs) -> Result<VMobject> {
    let mut m = Matrix::from_mobjects(grid_rows(st, kind, o, refs)?)
        .stretch_brackets(flag(o, "stretchBrackets", true))
        .add_background_rectangles_to_entries(flag(o, "addBackgroundRectanglesToEntries", false))
        .include_background_rectangle(flag(o, "includeBackgroundRectangle", false));
    if let Some(b) = opt_num(o, "vBuff") {
        m = m.v_buff(b);
    }
    if let Some(b) = opt_num(o, "hBuff") {
        m = m.h_buff(b);
    }
    if let Some(b) = opt_num(o, "bracketHBuff") {
        m = m.bracket_h_buff(b);
    }
    if let Some(b) = opt_num(o, "bracketVBuff") {
        m = m.bracket_v_buff(b);
    }
    if let Some(p) = opt_point(o, "elementAlignmentCorner") {
        m = m.element_alignment_corner(p);
    }
    let l = o.get("leftBracket").and_then(Value::as_str).unwrap_or("[");
    let r = o.get("rightBracket").and_then(Value::as_str).unwrap_or("]");
    m = m.brackets(l, r);
    if let Some(c) = opt_color(sub(o, "bracketConfig"), "color")? {
        m = m.bracket_color(c);
    }
    Ok(m.build())
}

fn code(st: &mut State, o: &Value) -> Result<VMobject> {
    let mut c = match (o.get("codeFile").and_then(Value::as_str), o.get("codeString").and_then(Value::as_str)) {
        (Some(path), _) => Code::from_file(asset_path(st, path)?)?,
        (None, Some(src)) => Code::new(src),
        _ => bail!("Code needs codeString or codeFile"),
    };
    if let Some(l) = o.get("language").and_then(Value::as_str) {
        c = c.language(l);
    }
    if let Some(s) = o.get("formatterStyle").and_then(Value::as_str) {
        c = c.style(s);
    }
    if let Some(n) = opt_num(o, "tabWidth") {
        c = c.tab_width(n.max(0.0) as usize);
    }
    c = c.line_numbers(flag(o, "addLineNumbers", true));
    if let Some(n) = opt_num(o, "lineNumbersFrom") {
        c = c.line_numbers_from(n.max(0.0) as usize);
    }
    c = c.background(match o.get("background").and_then(Value::as_str).unwrap_or("rectangle") {
        "rectangle" | "rect" => CodeBackground::Rectangle,
        "window" => CodeBackground::Window,
        other => bail!("unknown Code background '{other}' (rectangle or window)"),
    });
    // Paragraph settings may also be given at the top level.
    for p in [o, sub(o, "paragraphConfig")] {
        if let Some(f) = p.get("font").and_then(Value::as_str) {
            c = c.font(f);
        }
        if let Some(s) = opt_num(p, "fontSize") {
            c = c.font_size(s);
        }
        if let Some(s) = opt_num(p, "lineSpacing") {
            c = c.line_spacing(s);
        }
    }
    let bg = sub(o, "backgroundConfig");
    if let Some(b) = opt_num(bg, "buff") {
        c = c.background_buff(b);
    }
    if let Some(r) = opt_num(bg, "cornerRadius") {
        c = c.corner_radius(r);
    }
    if let Some(f) = opt_color(bg, "fillColor")?.or(opt_color(bg, "color")?) {
        c.background_fill = Some(f);
    }
    if let Some(v) = opt_num(bg, "fillOpacity") {
        c.background_fill_opacity = v;
    }
    if let Some(s) = opt_color(bg, "strokeColor")? {
        c.background_stroke_color = s;
    }
    if let Some(w) = opt_num(bg, "strokeWidth") {
        c.background_stroke_width = w;
    }
    c.build()
}

fn range3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    let x = |i: usize, d: f64| a.get(i).and_then(Value::as_f64).unwrap_or(d);
    Some([x(0, -1.0), x(1, 1.0), x(2, 1.0)])
}

pub(crate) fn axes(o: &Value) -> Result<Axes> {
    let mut a = Axes::new(
        o.get("xRange").and_then(range3).unwrap_or([-6.0, 6.0, 1.0]),
        o.get("yRange").and_then(range3).unwrap_or([-3.0, 3.0, 1.0]),
    );
    if let Some(v) = opt_num(o, "xLength") {
        a = a.x_length(v);
    }
    if let Some(v) = opt_num(o, "yLength") {
        a = a.y_length(v);
    }
    if let Some(p) = opt_point(o, "center") {
        a = a.move_to(p);
    }
    if let Some(c) = opt_color(o, "axisColor")? {
        a = a.color(c);
    }
    if let Some(t) = o.get("tips").and_then(Value::as_bool) {
        a = a.tips(t);
    }
    if o.get("numbers").and_then(Value::as_bool).unwrap_or(false) {
        a = a.with_numbers();
    }
    Ok(a)
}

// ---------------------------------------------------------------------- animations

struct Built {
    anim: Box<dyn Animation>,
}

fn configure(mut a: Box<dyn Animation>, o: &Value) -> Result<Box<dyn Animation>> {
    if let Some(t) = opt_num(o, "runTime") {
        a.config_mut().run_time = t.max(0.0);
    }
    if let Some(r) = o.get("rateFunc").and_then(Value::as_str) {
        a.config_mut().rate_func = rate_func(r)?;
    }
    if let Some(l) = opt_num(o, "lagRatio") {
        a.config_mut().lag_ratio = l;
    }
    Ok(a)
}

fn build_anim(st: &mut State, spec: &Value, pops: &mut Vec<Pop>) -> Result<Built> {
    let ty = spec.get("type").and_then(Value::as_str).context("animation spec without a type")?;
    let target_h = spec.get("target").and_then(mob_ref);
    let target = |st: &mut State, pops: &mut Vec<Pop>| -> Result<MobjectId> {
        let h = target_h.ok_or_else(|| anyhow!("{ty} needs a mobject"))?;
        ensure_in_world(st, h, false, pops)
    };
    let morph_opts = |m: Morph, _st: &mut State| -> Result<Morph> {
        let mut m = m;
        if let Some(v) = spec.get("shift") {
            m = m.shift(point(v).context("shift must be a vector")?);
        }
        if let Some(s) = opt_num(spec, "scale") {
            m = m.scale(s);
        }
        if let Some(a) = opt_num(spec, "pathArc") {
            m = m.path_arc(a);
        }
        Ok(m)
    };
    let anim: Box<dyn Animation> = match ty {
        "FadeIn" => {
            let id = target(st, pops)?;
            Box::new(morph_opts(FadeIn::new(id), st)?)
        }
        "FadeOut" => {
            let id = target(st, pops)?;
            Box::new(morph_opts(FadeOut::new(id), st)?)
        }
        "Create" => Box::new(Create::new(target(st, pops)?)),
        "Uncreate" => Box::new(Uncreate::new(target(st, pops)?)),
        "Write" => Box::new(Write::new(target(st, pops)?)),
        "Unwrite" => Box::new(Unwrite::new(target(st, pops)?)),
        "AddTextLetterByLetter" | "RemoveTextLetterByLetter" | "TypeWithCursor" | "UntypeWithCursor" => {
            let id = target(st, pops)?;
            let cursor = match ty {
                "TypeWithCursor" | "UntypeWithCursor" => {
                    let h =
                        spec.get("cursor").and_then(mob_ref).with_context(|| format!("{ty} needs a cursor mobject"))?;
                    Some(ensure_in_world(st, h, true, pops)?)
                }
                _ => None,
            };
            let mut a = match (ty, cursor) {
                ("AddTextLetterByLetter", _) => AddTextLetterByLetter::new(id),
                ("RemoveTextLetterByLetter", _) => RemoveTextLetterByLetter::new(id),
                ("TypeWithCursor", Some(c)) => TypeWithCursor::new(id, c),
                (_, Some(c)) => UntypeWithCursor::new(id, c),
                _ => unreachable!(),
            };
            if let Some(t) = opt_num(spec, "timePerChar") {
                a = a.time_per_char(t);
            }
            if let Some(b) = opt_num(spec, "buff") {
                a = a.buff(b);
            }
            if let Some(k) = spec.get("keepCursorY").and_then(Value::as_bool) {
                a = a.keep_cursor_y(k);
            }
            if let Some(k) = spec.get("leaveCursorOn").and_then(Value::as_bool) {
                a = a.leave_cursor_on(k);
            }
            Box::new(a)
        }
        "DrawBorderThenFill" => Box::new(DrawBorderThenFill::new(target(st, pops)?)),
        "ShowPassingFlash" => {
            let a = ShowPassingFlash::new(target(st, pops)?);
            Box::new(match opt_num(spec, "timeWidth") {
                Some(w) => a.time_width(w),
                None => a,
            })
        }
        "GrowFromCenter" => Box::new(morph_opts(GrowFromCenter::new(target(st, pops)?), st)?),
        "GrowFromPoint" => {
            let id = target(st, pops)?;
            let p = opt_point(spec, "point").context("GrowFromPoint needs a point")?;
            Box::new(morph_opts(GrowFromPoint::new(id, p), st)?)
        }
        "GrowFromEdge" => {
            let id = target(st, pops)?;
            Box::new(morph_opts(GrowFromEdge::new(id, opt_point(spec, "edge").unwrap_or(LEFT)), st)?)
        }
        "GrowArrow" => {
            let id = target(st, pops)?;
            let start = scene(st)?.world()[id].get_start();
            Box::new(GrowFromPoint::new(id, start))
        }
        "SpinInFromNothing" => Box::new(SpinInFromNothing::new(target(st, pops)?)),
        "ShrinkToCenter" => Box::new(ShrinkToCenter::new(target(st, pops)?)),
        "Indicate" => {
            let id = target(st, pops)?;
            let c = opt_color(spec, "color")?.unwrap_or(YELLOW_C);
            Box::new(Indicate::with(id, opt_num(spec, "scaleFactor").unwrap_or(1.2), c))
        }
        "Transform"
        | "ReplacementTransform"
        | "TransformFromCopy"
        | "FadeTransform"
        | "TransformMatchingShapes"
        | "TransformMatchingTex" => {
            let to_h = spec.get("to").and_then(mob_ref).with_context(|| format!("{ty} needs a target mobject"))?;
            let id = target(st, pops)?;
            let m = match ty {
                "Transform" => {
                    let t = get_mob(st, to_h)?;
                    Transform::new(id, t)
                }
                "TransformFromCopy" => {
                    let b = ensure_in_world(st, to_h, false, pops)?;
                    TransformFromCopy::new(id, b)
                }
                "FadeTransform" => {
                    let b = ensure_in_world(st, to_h, false, pops)?;
                    FadeTransform::new(id, b)
                }
                "TransformMatchingShapes" | "TransformMatchingTex" => {
                    let b = ensure_in_world(st, to_h, false, pops)?;
                    let m = if ty == "TransformMatchingTex" {
                        TransformMatchingTex::new(id, b)
                    } else {
                        TransformMatchingShapes::new(id, b)
                    };
                    m.transform_mismatches(spec.get("transformMismatches").and_then(Value::as_bool).unwrap_or(false))
                }
                _ => {
                    let b = ensure_in_world(st, to_h, false, pops)?;
                    ReplacementTransform::new(id, b)
                }
            };
            Box::new(morph_opts(m, st)?)
        }
        "Animate" => {
            let id = target(st, pops)?;
            let ops: Vec<(String, Vec<Value>)> = spec
                .get("ops")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|op| {
                            let arr = op.as_array()?;
                            Some((arr.first()?.as_str()?.to_string(), arr.get(1)?.as_array()?.clone()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut refs = HashMap::new();
            for (_, args) in &ops {
                for a in args {
                    collect_refs(st, a, &mut refs)?;
                }
            }
            let refs = Refs(refs);
            // Validate now so a typo fails at the call, not mid-render.
            let probe = scene(st)?.world()[id].clone();
            let mut test = probe;
            for (name, args) in &ops {
                test = apply_op(test, name, args, &refs)?;
            }
            let a = id.animate(move |m| {
                let mut m = m;
                for (name, args) in &ops {
                    m = apply_op(m.clone(), name, args, &refs).unwrap_or(m);
                }
                m
            });
            Box::new(morph_opts(a, st)?)
        }
        "Rotate" => {
            let id = target(st, pops)?;
            let a = Rotate::new(id, opt_num(spec, "angle").unwrap_or(PI));
            Box::new(match opt_point(spec, "about") {
                Some(p) => a.about(p),
                None => a,
            })
        }
        "MoveAlongPath" => {
            let id = target(st, pops)?;
            let ph = spec.get("path").and_then(mob_ref).context("MoveAlongPath needs a path mobject")?;
            let path = get_mob(st, ph)?;
            Box::new(MoveAlongPath::new(id, path))
        }
        "ChangeValue" => {
            let t = opt_num(spec, "tracker").context("ChangeValue needs a tracker")? as usize;
            let tracker = scene(st)?.tracker_id(t)?;
            Box::new(ChangeValue::new(tracker, opt_num(spec, "value").unwrap_or(0.0)))
        }
        "Wait" => Box::new(Wait::new(opt_num(spec, "runTime").unwrap_or(1.0))),
        "AnimationGroup" | "LaggedStart" | "Succession" => {
            let mut children = Vec::new();
            for c in spec.get("anims").and_then(Value::as_array).cloned().unwrap_or_default() {
                children.push(build_anim(st, &c, pops)?.anim);
            }
            let g = AnimationGroup::new(children);
            let g = match ty {
                "LaggedStart" => g.lag_ratio(0.05),
                "Succession" => g.lag_ratio(1.0),
                _ => g,
            };
            Box::new(g)
        }
        other => bail!("unknown animation '{other}'"),
    };
    Ok(Built { anim: configure(anim, spec)? })
}

// ---------------------------------------------------------------------- the JS-facing API

fn throw(ctx: &Ctx<'_>, e: anyhow::Error) -> rquickjs::Error {
    Exception::throw_message(ctx, &format!("{e:#}"))
}

fn parse(s: &str) -> Result<Value> {
    if s.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(s).context("bad JSON from script")
}

macro_rules! reg {
    // No arguments (rustfmt writes `| |` as `||`).
    ($ctx:expr, $obj:expr, $name:literal, || $body:expr) => {{
        let f = Function::new($ctx.clone(), move |ctx: Ctx<'_>| -> rquickjs::Result<_> {
            let run = || $body;
            let r: Result<_> = run();
            r.map_err(|e| throw(&ctx, e))
        })?;
        $obj.set($name, f)?;
    }};
    ($ctx:expr, $obj:expr, $name:literal, |$($a:ident : $t:ty),*| $body:expr) => {{
        let f = Function::new($ctx.clone(), move |ctx: Ctx<'_>, $($a: $t),*| -> rquickjs::Result<_> {
            let run = || $body;
            let r: Result<_> = run();
            r.map_err(|e| throw(&ctx, e))
        })?;
        $obj.set($name, f)?;
    }};
}

pub(crate) fn install(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let n = Object::new(ctx.clone())?;

    reg!(ctx, n, "mk", |kind: String, args: String| {
        let o = parse(&args)?;
        with_state(|st| {
            let mut refs = HashMap::new();
            collect_refs(st, &o, &mut refs)?;
            let m = make(st, &kind, &o, &Refs(refs))?;
            Ok(new_handle(st, Slot::Detached(m)))
        })
    });

    reg!(ctx, n, "op", |h: u32, name: String, args: String| {
        let args = parse(&args)?;
        let args = args.as_array().cloned().unwrap_or_default();
        with_state(|st| {
            let m = get_mob(st, h)?;
            if let Some(v) = query(&m, &name, &args)? {
                return Ok(v.to_string());
            }
            let mut refs = HashMap::new();
            for a in &args {
                collect_refs(st, a, &mut refs)?;
            }
            let new = apply_op(m, &name, &args, &Refs(refs))?;
            with_mob(st, h, |m| *m = new)?;
            Ok(String::new())
        })
    });

    reg!(ctx, n, "copy", |h: u32| with_state(|st| {
        let m = get_mob(st, h)?;
        Ok(new_handle(st, Slot::Detached(m)))
    }));

    reg!(ctx, n, "group", |handles: String| {
        let hs: Vec<u32> = serde_json::from_str(&handles).context("group handles")?;
        with_state(|st| {
            let mut children = Vec::new();
            let mut first_shown: Option<MobjectId> = None;
            let mut shown = Vec::new();
            for &h in &hs {
                children.push(get_mob(st, h)?);
                if let (Base::World(id), true) = (resolve(st, h)?.0, matches!(slot(st, h)?, Slot::Attached(_))) {
                    if scene(st)?.world().is_displayed(id) {
                        first_shown.get_or_insert(id);
                        shown.push(id);
                    }
                }
            }
            let g = VMobject::group(children);
            let gh = match first_shown {
                Some(first) => {
                    let s = scene(st)?;
                    let id = s.insert(g);
                    s.world_mut().replace_in_display(first, id);
                    for other in shown.into_iter().filter(|x| *x != first) {
                        s.world_mut().hide(other);
                    }
                    new_handle(st, Slot::Attached(id))
                }
                None => new_handle(st, Slot::Detached(g)),
            };
            for (i, &h) in hs.iter().enumerate() {
                st.slots[h as usize] = Slot::View { root: gh, path: vec![i] };
            }
            Ok(gh)
        })
    });

    reg!(ctx, n, "addChildren", |h: u32, handles: String| {
        let hs: Vec<u32> = serde_json::from_str(&handles).context("handles")?;
        with_state(|st| {
            let base = with_mob(st, h, |m| m.children.len())?;
            for (k, &c) in hs.iter().enumerate() {
                let m = get_mob(st, c)?;
                if let Slot::Attached(id) = slot(st, c)? {
                    scene(st)?.world_mut().remove_everywhere(id);
                }
                with_mob(st, h, |g| g.children.push(m))?;
                st.slots[c as usize] = Slot::View { root: h, path: vec![base + k] };
            }
            Ok(())
        })
    });

    reg!(ctx, n, "child", |h: u32, i: i32| with_state(|st| {
        let len = with_mob(st, h, |m| m.children.len())? as i32;
        let idx = if i < 0 { len + i } else { i };
        if idx < 0 || idx >= len {
            bail!("index {i} is out of range (the group has {len} members)");
        }
        Ok(new_handle(st, Slot::View { root: h, path: vec![idx as usize] }))
    }));

    reg!(ctx, n, "chars", |h: u32, a: i32, b: i32| with_state(|st| {
        let m = get_mob(st, h)?;
        let len = m.text().map(|t| t.chars().count()).unwrap_or(0) as i32;
        let fix = |x: i32| if x < 0 { (len + x).max(0) } else { x.min(len) } as usize;
        let part = m.chars(fix(a), fix(b));
        Ok(new_handle(st, Slot::Detached(part)))
    }));

    reg!(ctx, n, "part", |h: u32, needle: String| with_state(|st| {
        let m = get_mob(st, h)?;
        if m.find_text(&needle).is_none() {
            bail!("{needle:?} is not in the text {:?}", m.text().unwrap_or(""));
        }
        Ok(new_handle(st, Slot::Detached(m.part(&needle))))
    }));

    reg!(ctx, n, "release", |h: u32| with_state(|st| {
        // Free only detached values nobody else can reach.
        if matches!(st.slots.get(h as usize), Some(Slot::Detached(_))) {
            let referenced = st.slots.iter().any(|s| matches!(s, Slot::View { root, .. } if *root == h));
            if !referenced {
                st.slots[h as usize] = Slot::Free;
            }
        }
        Ok(())
    }));

    // ---- scene

    reg!(ctx, n, "add", |handles: String| {
        let hs: Vec<u32> = serde_json::from_str(&handles).context("handles")?;
        with_state(|st| {
            for h in hs {
                match slot(st, h)? {
                    Slot::Attached(id) => scene(st)?.world_mut().bring_to_front(id),
                    _ => {
                        let mut pops = Vec::new();
                        let id = ensure_in_world(st, h, true, &mut pops)?;
                        scene(st)?.world_mut().bring_to_front(id);
                    }
                }
            }
            Ok(())
        })
    });

    reg!(ctx, n, "remove", |handles: String| {
        let hs: Vec<u32> = serde_json::from_str(&handles).context("handles")?;
        with_state(|st| {
            for h in hs {
                match slot(st, h)? {
                    Slot::Attached(id) => scene(st)?.remove(id),
                    Slot::View { .. } => {
                        let sub = with_mob(st, h, |m| std::mem::replace(m, VMobject::group(vec![])))?;
                        st.slots[h as usize] = Slot::Detached(sub);
                    }
                    _ => {}
                }
            }
            Ok(())
        })
    });

    reg!(ctx, n, "clear", || with_state(|st| {
        scene(st)?.clear();
        Ok(())
    }));

    reg!(ctx, n, "mobjects", || with_state(|st| {
        let ids = scene(st)?.mobjects();
        let hs: Vec<u32> = ids.into_iter().map(|id| handle_for_id(st, id)).collect();
        Ok(serde_json::to_string(&hs)?)
    }));

    reg!(ctx, n, "isShown", |h: u32| with_state(|st| is_shown(st, h)));

    reg!(ctx, n, "layer", |h: u32, which: String| with_state(|st| {
        let mut pops = Vec::new();
        let id = ensure_in_world(st, h, false, &mut pops)?;
        let w = scene(st)?.world_mut();
        match which.as_str() {
            "background" => w.add_background(id),
            "overlay" => w.add_overlay(id),
            "front" => w.bring_to_front(id),
            "back" => w.bring_to_back(id),
            other => bail!("unknown layer '{other}'"),
        }
        Ok(())
    }));

    reg!(ctx, n, "time", || with_state(|st| Ok(scene(st)?.time())));
    reg!(ctx, n, "duration", || with_state(|st| Ok(scene(st)?.duration())));
    reg!(ctx, n, "frameSize", || with_state(|st| {
        let (w, h) = scene(st)?.frame_size();
        Ok(json!([w, h]).to_string())
    }));
    reg!(ctx, n, "fps", || with_state(|st| Ok(scene(st)?.config().fps)));

    reg!(ctx, n, "mark", |name: String, kind: String| with_state(|st| {
        let s = scene(st)?;
        Ok(if kind == "section" { s.section(&name) } else { s.mark(&name) })
    }));

    reg!(ctx, n, "addSound", |path: String, at: f64, gain: f64| with_state(|st| {
        let full = asset_path(st, &path)?;
        let s = scene(st)?;
        let at = if at < 0.0 { s.time() } else { at };
        s.add_sound_at(full, at, gain);
        Ok(())
    }));

    // A theme as JSON text, or the path of a theme file in the project (`.json` optional).
    reg!(ctx, n, "setTheme", |arg: String| with_state(|st| {
        let theme = if arg.trim_start().starts_with('{') {
            Theme::from_json(&arg)?
        } else {
            let mut path = asset_path(st, &arg)?;
            if !path.exists() && path.extension().is_none() {
                path.set_extension("json");
            }
            if !path.exists() {
                bail!(
                    "setTheme(\"{arg}\"): no theme file {} (pass theme JSON, a theme object, or a theme file path;                      there are no built-in named themes)",
                    path.display()
                );
            }
            Theme::load(&path)?
        };
        scene(st)?.set_theme(theme);
        Ok(())
    }));

    reg!(ctx, n, "theme", || with_state(|st| { Ok(scene(st)?.theme().map(|t| t.to_json()).unwrap_or_default()) }));

    reg!(ctx, n, "setBackgroundColor", |c: String| with_state(|st| {
        let c = Color::hex(&c).ok_or_else(|| anyhow!("invalid color {c}"))?;
        scene(st)?.set_background_color(c);
        Ok(())
    }));

    reg!(ctx, n, "trackerNew", |v: f64| with_state(|st| Ok(scene(st)?.tracker_index(v))));
    reg!(ctx, n, "trackerGet", |t: u32| with_state(|st| {
        let s = scene(st)?;
        let id = s.tracker_id(t as usize)?;
        Ok(s.value(id))
    }));
    reg!(ctx, n, "trackerSet", |t: u32, v: f64| with_state(|st| {
        let s = scene(st)?;
        let id = s.tracker_id(t as usize)?;
        s.set_value(id, v);
        Ok(())
    }));

    reg!(ctx, n, "beginPlay", |specs: String| {
        let specs = parse(&specs)?;
        with_state(|st| {
            let mut pops = Vec::new();
            let mut anims = Vec::new();
            for s in specs.as_array().cloned().unwrap_or_default() {
                anims.push(build_anim(st, &s, &mut pops)?.anim);
            }
            if anims.is_empty() {
                bail!("play() needs at least one animation");
            }
            let pb = scene(st)?.begin_play(anims);
            let id = st.next_playback;
            st.next_playback += 1;
            st.playbacks.insert(id, (pb, pops));
            Ok(id)
        })
    });

    reg!(ctx, n, "beginWait", |seconds: f64| with_state(|st| {
        let pb = scene(st)?.begin_wait(seconds);
        let id = st.next_playback;
        st.next_playback += 1;
        st.playbacks.insert(id, (pb, Vec::new()));
        Ok(id)
    }));

    reg!(ctx, n, "seekBegin", |t: f64| with_state(|st| {
        let pb = scene(st)?.seek_begin(t)?;
        let id = st.next_playback;
        st.next_playback += 1;
        st.playbacks.insert(id, (pb, Vec::new()));
        Ok(id)
    }));

    // The recorded segment a playback belongs to (its play/wait index).
    reg!(ctx, n, "playbackSegment", |pid: u32| with_state(|st| {
        Ok(st.playbacks.get(&pid).context("unknown playback")?.0.segment() as u32)
    }));

    // Returns dt, or -1 once finished.
    reg!(ctx, n, "step", |pid: u32| with_state(|st| {
        let (mut pb, pops) = st.playbacks.remove(&pid).context("unknown playback")?;
        let r = scene(st)?.step(&mut pb);
        match r {
            Some(dt) => {
                st.playbacks.insert(pid, (pb, pops));
                Ok(dt)
            }
            None => {
                if !pb.is_replay() {
                    push_back(st, pops)?;
                }
                Ok(-1.0)
            }
        }
    }));

    reg!(ctx, n, "endFrame", |pid: u32, dt: f64| with_state(|st| {
        let (pb, pops) = st.playbacks.remove(&pid).context("unknown playback")?;
        scene(st)?.end_frame(&pb, dt);
        st.playbacks.insert(pid, (pb, pops));
        Ok(())
    }));

    // Run a playback to the end without JS updaters.
    reg!(ctx, n, "playAll", |pid: u32| with_state(|st| {
        let (mut pb, pops) = st.playbacks.remove(&pid).context("unknown playback")?;
        let s = scene(st)?;
        while let Some(dt) = s.step(&mut pb) {
            s.end_frame(&pb, dt);
        }
        if !pb.is_replay() {
            push_back(st, pops)?;
        }
        Ok(())
    }));

    reg!(ctx, n, "inspect", || with_state(|st| Ok(serde_json::to_string(&scene(st)?.inspect())?)));

    reg!(ctx, n, "axesInfo", |args: String| {
        let a = axes(&parse(&args)?)?;
        let o = a.c2p(0.0, 0.0);
        let x1 = a.c2p(1.0, 0.0) - o;
        let y1 = a.c2p(0.0, 1.0) - o;
        Ok(json!({ "origin": pt_json(o), "xUnit": x1.x, "yUnit": y1.y,
                   "xRange": a.x_range, "yRange": a.y_range })
        .to_string())
    });

    reg!(ctx, n, "registerFonts", |dir: String| with_state(|st| {
        let full = asset_path(st, &dir)?;
        Ok(crate::text::register_font_dir(full)? as u32)
    }));

    reg!(ctx, n, "registerFont", |file: String| with_state(|st| {
        let full = asset_path(st, &file)?;
        Ok(crate::text::register_font_file(full)?.join(", "))
    }));

    reg!(ctx, n, "hasFont", |name: String| Ok(crate::text::has_font(&name)));
    reg!(ctx, n, "codeStyles", || Ok(json!(Code::styles()).to_string()));
    reg!(ctx, n, "codeLanguages", || Ok(json!(Code::languages()).to_string()));

    reg!(ctx, n, "log", |msg: String| {
        eprintln!("{msg}");
        Ok(())
    });

    ctx.globals().set("__manim_native", n)?;
    Ok(())
}
