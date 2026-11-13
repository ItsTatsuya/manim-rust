//! Point correspondence for morphs: which leaf becomes which, which subpath of a leaf
//! becomes which, and where each closed path starts.
//!
//! Manim CE pairs leaves and subpaths by index, pads missing subpaths with a point at
//! the end of the last one, splits curves evenly by index and never looks at where a
//! closed path starts. That makes glyphs shred and shapes twist mid-morph. Here:
//!
//! - leaves are paired by a monotone (reading-order) alignment on position and shape
//!   when the counts differ, so `A = π r · r` → `A = π r²` keeps `A`, `=`, `π`, `r` in place;
//! - subpaths are paired by size and position, and a subpath with no partner shrinks
//!   to (or grows from) its own center instead of streaking to some other contour;
//! - closed subpaths get matching orientation and the start point that minimizes travel;
//! - new curves go where the path is longest, so points stay evenly spread;
//! - a fill or stroke that is invisible on one side takes the other side's color,
//!   so nothing fades through a color that was never shown.

use super::{Subpath, VMobject};
use crate::bezier;
use crate::constants::*;

// ---------------------------------------------------------------------- subpath geometry

fn bbox<'a>(pts: impl IntoIterator<Item = &'a Point>) -> Option<(Point, Point)> {
    let mut it = pts.into_iter();
    let first = *it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))))
}

fn sp_points(sp: &Subpath) -> impl Iterator<Item = &Point> {
    sp.curves.iter().flat_map(|c| c.iter())
}

fn leaf_bbox(m: &VMobject) -> (Point, Point) {
    bbox(m.subpaths.iter().flat_map(sp_points)).unwrap_or((ORIGIN, ORIGIN))
}

/// Signed area (counter-clockwise positive) of the region a subpath encloses, closing it
/// with a straight line. Sampled, which is plenty for orientation and size.
fn signed_area(sp: &Subpath) -> f64 {
    let mut pts = Vec::with_capacity(sp.curves.len() * 8 + 1);
    for c in &sp.curves {
        for k in 0..8 {
            pts.push(bezier::point_at(c, k as f64 / 8.0));
        }
    }
    if let Some(e) = sp.end() {
        pts.push(e);
    }
    let n = pts.len();
    (0..n).map(|i| pts[i].x * pts[(i + 1) % n].y - pts[(i + 1) % n].x * pts[i].y).sum::<f64>() / 2.0
}

fn is_loop(sp: &Subpath) -> bool {
    match (sp.start(), sp.end()) {
        (Some(s), Some(e)) => sp.closed || s.distance(e) < 1e-6,
        _ => false,
    }
}

/// A closed subpath whose curves don't return to the start gets the closing segment as
/// a real curve, so the loop can be rotated and the gap is drawn while it's open mid-morph.
fn close_loop(sp: &mut Subpath) {
    if !sp.closed {
        return;
    }
    if let (Some(s), Some(e)) = (sp.start(), sp.end()) {
        if s.distance(e) > 1e-9 {
            sp.curves.push(bezier::line(e, s));
        }
    }
}

fn reversed(sp: &Subpath) -> Subpath {
    Subpath { curves: sp.curves.iter().rev().map(|c| [c[3], c[2], c[1], c[0]]).collect(), closed: sp.closed }
}

/// Split curves until there are `n`, giving each new piece to the curve whose pieces are
/// currently longest (Manim splits evenly by index, which crowds points onto short curves).
fn resample(sp: &Subpath, n: usize) -> Subpath {
    let m = sp.curves.len();
    if m >= n || m == 0 {
        return sp.clone();
    }
    let lens: Vec<f64> = sp.curves.iter().map(bezier::length).collect();
    let mut pieces = vec![1usize; m];
    if lens.iter().sum::<f64>() <= 1e-12 {
        for i in 0..n - m {
            pieces[i * m / (n - m)] += 1;
        }
    } else {
        for _ in 0..n - m {
            let (best, _) = lens
                .iter()
                .zip(&pieces)
                .map(|(l, k)| l / *k as f64)
                .enumerate()
                .fold((0, f64::MIN), |acc, (i, v)| if v > acc.1 { (i, v) } else { acc });
            pieces[best] += 1;
        }
    }
    let curves = sp.curves.iter().zip(pieces).flat_map(|(c, k)| bezier::subdivide(c, k)).collect();
    Subpath { curves, closed: sp.closed }
}

/// Curve start points, centered and scaled to unit RMS radius (so the best rotation
/// depends on shape, not on where or how big the two paths are).
fn normalized_anchors(sp: &Subpath) -> Vec<Point> {
    let pts: Vec<Point> = sp.curves.iter().map(|c| c[0]).collect();
    let n = pts.len().max(1) as f64;
    let mean = pts.iter().fold(ORIGIN, |a, p| a + *p) / n;
    let rms = (pts.iter().map(|p| (*p - mean).length_squared()).sum::<f64>() / n).sqrt().max(1e-9);
    pts.into_iter().map(|p| (p - mean) / rms).collect()
}

/// Cyclic shift `s` of `b`'s curves (curve `i` of `a` ↔ curve `i + s` of `b`) with the least travel.
fn best_shift(a: &Subpath, b: &Subpath) -> (usize, f64) {
    let (pa, pb) = (normalized_anchors(a), normalized_anchors(b));
    let k = pa.len().min(pb.len());
    let mut best = (0, f64::INFINITY);
    for s in 0..k {
        let cost: f64 = (0..k).map(|i| pa[i].distance_squared(pb[(i + s) % k])).sum();
        if cost < best.1 - 1e-12 {
            best = (s, cost);
        }
    }
    best
}

fn rotated(sp: &Subpath, s: usize) -> Subpath {
    let k = sp.curves.len();
    Subpath { curves: (0..k).map(|i| sp.curves[(i + s) % k]).collect(), closed: sp.closed }
}

/// All points of `sp` at `p`: a subpath that has shrunk to nothing, with the same curve count.
fn collapsed(sp: &Subpath, p: Point) -> Subpath {
    Subpath { curves: vec![[p; 4]; sp.curves.len().max(1)], closed: sp.closed }
}

/// Maps points from one box onto another (stretching), for placing collapsed partners.
fn box_map(from: (Point, Point), to: (Point, Point)) -> impl Fn(Point) -> Point {
    move |p| {
        let f = |v: f64, a0: f64, a1: f64, b0: f64, b1: f64| {
            if (a1 - a0).abs() < 1e-9 {
                (b0 + b1) / 2.0
            } else {
                b0 + (v - a0) / (a1 - a0) * (b1 - b0)
            }
        };
        Point::new(f(p.x, from.0.x, from.1.x, to.0.x, to.1.x), f(p.y, from.0.y, from.1.y, to.0.y, to.1.y), p.z)
    }
}

// ---------------------------------------------------------------------- one leaf with another

struct SpInfo {
    area: f64,
    /// Center relative to the leaf's box (0..1 on each axis).
    rel: Point,
    center: Point,
}

fn infos(m: &VMobject) -> Vec<SpInfo> {
    let (lo, hi) = leaf_bbox(m);
    let size = (hi - lo).max(Point::splat(1e-9));
    let leaf_area = (size.x * size.y).max(1e-12);
    m.subpaths
        .iter()
        .map(|sp| {
            let (a, b) = bbox(sp_points(sp)).unwrap_or((ORIGIN, ORIGIN));
            let center = (a + b) / 2.0;
            let mut rel = (center - lo) / size;
            rel.z = 0.0;
            SpInfo { area: signed_area(sp).abs() / leaf_area, rel, center }
        })
        .collect()
}

/// The orientation of a leaf: the sign of its largest closed subpath's area.
fn orientation(m: &VMobject) -> Option<f64> {
    m.subpaths
        .iter()
        .filter(|s| is_loop(s))
        .map(signed_area)
        .filter(|a| a.abs() > 1e-12)
        .max_by(|a, b| a.abs().total_cmp(&b.abs()))
        .map(f64::signum)
}

/// Make two leaves structurally identical (same subpath count, same curve count per
/// subpath) with a correspondence that morphs without twisting.
pub(crate) fn align_pair(a: &mut VMobject, b: &mut VMobject) {
    for sp in a.subpaths.iter_mut().chain(b.subpaths.iter_mut()) {
        close_loop(sp);
    }
    // Same winding direction. Reversing every subpath of a leaf keeps its fill under
    // both fill rules (the relative orientation of outer contours and holes is kept).
    match (orientation(a), orientation(b)) {
        (Some(x), Some(y)) if x != y => b.subpaths = b.subpaths.iter().map(reversed).collect(),
        (None, None) if a.subpaths.len() == 1 && b.subpaths.len() == 1 => {
            // Two open strokes: run them the same way.
            let (sa, sb) = (&a.subpaths[0], &b.subpaths[0]);
            if let (Some(a0), Some(a1), Some(b0), Some(b1)) = (sa.start(), sa.end(), sb.start(), sb.end()) {
                if a0.distance(b1) + a1.distance(b0) < a0.distance(b0) + a1.distance(b1) - 1e-9 {
                    b.subpaths[0] = reversed(sb);
                }
            }
        }
        _ => {}
    }

    // Pair subpaths: biggest first, each with the closest unclaimed one of similar size.
    let (ia, ib) = (infos(a), infos(b));
    let mut order: Vec<usize> = (0..ia.len()).collect();
    order.sort_by(|&i, &j| ia[j].area.total_cmp(&ia[i].area));
    let mut claimed = vec![false; ib.len()];
    let mut partner: Vec<Option<usize>> = vec![None; ia.len()];
    for &i in &order {
        let cost = |j: usize| {
            let size = ((ia[i].area + 1e-3) / (ib[j].area + 1e-3)).ln().abs();
            ia[i].rel.distance(ib[j].rel) + 0.25 * size
        };
        if let Some(j) = (0..ib.len()).filter(|&j| !claimed[j]).min_by(|&x, &y| cost(x).total_cmp(&cost(y))) {
            claimed[j] = true;
            partner[i] = Some(j);
        }
    }
    let (box_a, box_b) = (leaf_bbox(a), leaf_bbox(b));
    let (to_b, to_a) = (box_map(box_a, box_b), box_map(box_b, box_a));
    let mut out_a = Vec::new();
    let mut out_b = Vec::new();
    for (i, sa) in a.subpaths.iter().enumerate() {
        match partner[i] {
            Some(j) => {
                let (pa, pb) = align_subpaths(sa, &b.subpaths[j], a.subpaths.len() == 1 && b.subpaths.len() == 1);
                out_a.push(pa);
                out_b.push(pb);
            }
            None => {
                out_a.push(sa.clone());
                out_b.push(collapsed(sa, to_b(ia[i].center)));
            }
        }
    }
    for (j, sb) in b.subpaths.iter().enumerate() {
        if !claimed[j] {
            out_a.push(collapsed(sb, to_a(ib[j].center)));
            out_b.push(sb.clone());
        }
    }
    a.subpaths = out_a;
    b.subpaths = out_b;
}

/// Equal curve counts, and for loops the start point (and, for lone paths, direction)
/// that moves points the least.
fn align_subpaths(a: &Subpath, b: &Subpath, may_reverse: bool) -> (Subpath, Subpath) {
    let k = a.curves.len().max(b.curves.len());
    let (mut a, mut b) = (resample(a, k), resample(b, k));
    match (is_loop(&a), is_loop(&b)) {
        (true, true) => {
            let (s, cost) = best_shift(&a, &b);
            b = rotated(&b, s);
            if may_reverse {
                let r = reversed(&b);
                let (s2, cost2) = best_shift(&a, &r);
                if cost2 < cost * 0.8 {
                    b = rotated(&r, s2);
                }
            }
        }
        // A loop opening into a stroke (a ring unrolling): open the loop where the ends meet.
        (true, false) => {
            let (s, cost) = best_shift(&b, &a);
            let mut best = rotated(&a, s);
            if may_reverse {
                let r = reversed(&a);
                let (s2, cost2) = best_shift(&b, &r);
                if cost2 < cost {
                    best = rotated(&r, s2);
                }
            }
            a = best;
        }
        (false, true) => {
            let (s, cost) = best_shift(&a, &b);
            let mut best = rotated(&b, s);
            if may_reverse {
                let r = reversed(&b);
                let (s2, cost2) = best_shift(&a, &r);
                if cost2 < cost {
                    best = rotated(&r, s2);
                }
            }
            b = best;
        }
        (false, false) => {}
    }
    (a, b)
}

/// Where a fill or stroke is invisible on one side, give it the other side's color (and
/// stroke width), so it fades in or out in its own color rather than through another.
pub(crate) fn match_invisible_style(a: &mut VMobject, b: &mut VMobject) {
    fn fix(x: &mut VMobject, y: &VMobject) {
        if x.fill_opacity <= 0.0 && y.fill_opacity > 0.0 {
            x.fill_color = y.fill_color;
        }
        let x_hidden = x.stroke_width <= 0.0 || x.stroke_opacity <= 0.0;
        let y_shown = y.stroke_width > 0.0 && y.stroke_opacity > 0.0;
        if x_hidden && y_shown {
            x.stroke_color = y.stroke_color;
            if x.stroke_opacity <= 0.0 {
                x.stroke_width = y.stroke_width;
            }
        }
    }
    fix(a, b);
    fix(b, a);
}

// ---------------------------------------------------------------------- leaves with leaves

/// Same outline up to position and size (the same glyph, possibly scaled).
pub(crate) fn same_shape(a: &VMobject, b: &VMobject) -> bool {
    if a.subpaths.len() != b.subpaths.len()
        || a.subpaths.iter().zip(&b.subpaths).any(|(x, y)| x.curves.len() != y.curves.len())
    {
        return false;
    }
    let norm = |m: &VMobject| {
        let (lo, hi) = leaf_bbox(m);
        let c = (lo + hi) / 2.0;
        let s = (hi - lo).x.max((hi - lo).y).max(1e-9);
        move |p: Point| (p - c) / s
    };
    let (na, nb) = (norm(a), norm(b));
    a.subpaths
        .iter()
        .flat_map(sp_points)
        .zip(b.subpaths.iter().flat_map(sp_points))
        .all(|(p, q)| na(*p).distance(nb(*q)) < 0.01)
}

struct LeafInfo {
    /// Center relative to the whole mobject's box.
    rel: Point,
    /// Height relative to the whole mobject's height.
    size: f64,
}

fn leaf_infos(leaves: &[VMobject]) -> Vec<LeafInfo> {
    let boxes: Vec<_> = leaves.iter().map(leaf_bbox).collect();
    let (lo, hi) = boxes.iter().fold((boxes[0].0, boxes[0].1), |(l, h), (a, b)| (l.min(*a), h.max(*b)));
    let span = (hi - lo).max(Point::splat(1e-9));
    boxes
        .iter()
        .map(|(a, b)| {
            let mut rel = ((*a + *b) / 2.0 - lo) / span;
            rel.z = 0.0;
            LeafInfo { rel, size: ((b.y - a.y).max(b.x - a.x) / span.y.max(span.x)).max(1e-6) }
        })
        .collect()
}

/// Pair each leaf of the longer list with one of the shorter, in order, every leaf of
/// both used: the cheapest monotone path by position and shape (glyphs keep their place
/// in reading order; extra ones merge into, or split from, their neighbours).
fn monotone_pairs(long: &[VMobject], short: &[VMobject]) -> Vec<usize> {
    let (n, m) = (long.len(), short.len());
    if m == 0 || n == 0 {
        return vec![0; n];
    }
    // short[j] is reachable from long[i] only for lo(i) <= j <= i, a band of width n-m+1.
    let w = n - m + 1;
    if n.saturating_mul(w) > MAX_ALIGN_CELLS {
        // Too big to search: spread the short list evenly over the long one.
        return (0..n).map(|i| (i * m / n).min(m - 1)).collect();
    }
    let lo = |i: usize| i.saturating_sub(n - m);
    let (il, is) = (leaf_infos(long), leaf_infos(short));
    let cost = |i: usize, j: usize| {
        let shape = if same_shape(&long[i], &short[j]) { 0.0 } else { 0.3 };
        il[i].rel.distance(is[j].rel) + 0.15 * (il[i].size / is[j].size).ln().abs() + shape
    };
    // d[i*w + (j - lo(i))]: cheapest way to map long[..=i] with long[i] → short[j].
    let mut d = vec![f64::INFINITY; n * w];
    let mut from_diag = vec![false; n * w];
    let at = |i: usize, j: usize| -> Option<usize> {
        let l = lo(i);
        (j >= l && j <= i && j - l < w).then(|| i * w + (j - l))
    };
    d[0] = cost(0, 0);
    for i in 1..n {
        for j in lo(i)..=i.min(m - 1) {
            let stay = at(i - 1, j).map_or(f64::INFINITY, |k| d[k]);
            let step = if j > 0 { at(i - 1, j - 1).map_or(f64::INFINITY, |k| d[k]) } else { f64::INFINITY };
            let (best, diag) = if step <= stay { (step, true) } else { (stay, false) };
            if best.is_finite() {
                let k = at(i, j).unwrap();
                d[k] = best + cost(i, j);
                from_diag[k] = diag;
            }
        }
    }
    let mut out = vec![0; n];
    let mut j = m - 1;
    for i in (0..n).rev() {
        out[i] = j;
        if i > 0 && j > 0 && at(i, j).is_some_and(|k| from_diag[k]) {
            j -= 1;
        }
    }
    out
}

/// Largest alignment table (cells) searched exactly; ~20 MB of costs.
const MAX_ALIGN_CELLS: usize = 2_500_000;

/// A repeated leaf that isn't fully opaque is made invisible (as Manim does), so stacked
/// copies don't render darker than the original.
fn ghost_if_translucent(mut m: VMobject) -> VMobject {
    if m.fill_opacity < 1.0 {
        m.fill_opacity = 0.0;
    }
    if m.stroke_opacity < 1.0 {
        m.stroke_opacity = 0.0;
    }
    m
}

/// Pair up two leaf lists (one or both may be longer) into equal-length aligned lists.
pub(crate) fn align_leaves(la: Vec<VMobject>, lb: Vec<VMobject>) -> (Vec<VMobject>, Vec<VMobject>) {
    let pairs: Vec<(usize, usize)> = if la.len() == lb.len() {
        (0..la.len()).map(|i| (i, i)).collect()
    } else if la.len() > lb.len() {
        monotone_pairs(&la, &lb).into_iter().enumerate().collect()
    } else {
        monotone_pairs(&lb, &la).into_iter().enumerate().map(|(j, i)| (i, j)).collect()
    };
    let (mut seen_a, mut seen_b) = (vec![false; la.len()], vec![false; lb.len()]);
    let mut out_a = Vec::with_capacity(pairs.len());
    let mut out_b = Vec::with_capacity(pairs.len());
    for (i, j) in pairs {
        let mut x = if seen_a[i] { ghost_if_translucent(la[i].clone()) } else { la[i].clone() };
        let mut y = if seen_b[j] { ghost_if_translucent(lb[j].clone()) } else { lb[j].clone() };
        seen_a[i] = true;
        seen_b[j] = true;
        match_invisible_style(&mut x, &mut y);
        align_pair(&mut x, &mut y);
        out_a.push(x);
        out_b.push(y);
    }
    (out_a, out_b)
}
