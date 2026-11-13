//! Morph correspondence and text animation quality: no twisting or shredding mid-morph,
//! styles that don't fade through hidden colors, Manim CE timing for text reveals.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::*;
use manim::mobject::{align, lerp_leaves, PathFunc, Subpath};
use manim::prelude::*;

fn recorder<T: 'static>(s: &mut Scene, f: impl Fn(&World) -> T + 'static) -> Rc<RefCell<Vec<T>>> {
    let log = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    s.add_updater(move |w, _| l.borrow_mut().push(f(w)));
    log
}

fn signed_area(sp: &Subpath) -> f64 {
    let mut pts = Vec::new();
    for c in &sp.curves {
        for k in 0..16 {
            pts.push(manim::bezier::point_at(c, k as f64 / 16.0));
        }
    }
    let n = pts.len();
    (0..n).map(|i| pts[i].x * pts[(i + 1) % n].y - pts[(i + 1) % n].x * pts[i].y).sum::<f64>() / 2.0
}

/// Area enclosed, per subpath: a twisted (figure-eight) path has little.
fn area(m: &VMobject) -> f64 {
    m.family().iter().flat_map(|f| f.subpaths.iter()).map(|sp| signed_area(sp).abs()).sum()
}

fn morph_at(a: &VMobject, b: &VMobject, t: f64) -> VMobject {
    let (x, y) = align(a, b);
    lerp_leaves(&x, &y, t, PathFunc::Straight, 0)
}

fn mirror(m: VMobject) -> VMobject {
    m.apply_function(|p| Point::new(-p.x, p.y, p.z))
}

#[test]
fn closed_shapes_morph_without_twisting() {
    let cases = [
        ("square → circle", Square::new(2.0), Circle::new(1.1)),
        ("star → circle", Star::new(5, 1.2, 0.5), Circle::new(1.0)),
        ("triangle → hexagon", Triangle::new().scale(1.4), RegularPolygon::new(6).scale(1.4)),
        // Opposite winding: matched naively, every point crosses the middle at t = 0.5.
        ("square → mirrored circle", Square::new(2.0).rotate(0.3), mirror(Circle::new(1.1))),
    ];
    for (name, a, b) in cases {
        let floor = area(&a).min(area(&b)) * 0.75;
        for t in [0.25, 0.5, 0.75] {
            let got = area(&morph_at(&a, &b, t));
            assert!(got > floor, "{name}: area {got:.3} at t = {t} (ends: {:.3}, {:.3})", area(&a), area(&b));
        }
        assert!(same_points(&morph_at(&a, &b, 0.0), &morph_at(&a, &a, 0.0)) || area(&morph_at(&a, &b, 0.0)) > 0.0);
    }
}

#[test]
fn morph_ends_look_exactly_like_the_shapes() {
    // Re-parameterizing (rotation, direction, subdivision) must not change what is drawn.
    let (a, b) = (Star::new(5, 1.2, 0.5), mirror(Circle::new(1.0)));
    for (m, t) in [(&a, 0.0), (&b, 1.0)] {
        let got = area(&morph_at(&a, &b, t));
        // Sampled areas: the subdivided path is sampled more finely, hence the tolerance.
        assert!((got - area(m)).abs() < 1e-3 * area(m), "{got} vs {}", area(m));
    }
}

#[test]
fn an_unmatched_contour_shrinks_in_place() {
    // An "o"-like leaf (outer square + hole) into a plain square further right.
    let outer = Subpath::polyline(&[pt(-1.0, -1.0), pt(1.0, -1.0), pt(1.0, 1.0), pt(-1.0, 1.0)], true);
    let hole = Subpath::polyline(&[pt(0.3, 0.3), pt(0.3, 0.7), pt(0.7, 0.7), pt(0.7, 0.3)], true);
    let o = VMobject::from_subpaths(vec![outer, hole]).set_fill(WHITE, 1.0);
    let target = Square::new(2.0).shift(RIGHT * 3.0).set_fill(WHITE, 1.0);
    let (_, b) = align(&o, &target);
    let collapsed: Vec<_> = b[0].subpaths.iter().filter(|s| s.curves.iter().all(|c| c[0] == c[3])).collect();
    assert_eq!(collapsed.len(), 1, "the hole has no partner");
    // It vanishes at the hole's own spot (mapped into the target), not at some contour's end.
    assert_close(collapsed[0].curves[0][0], pt(3.5, 0.5));
}

#[test]
fn leaves_pair_in_reading_order_by_position_and_shape() {
    let row = |xs: &[(f64, f64)]| VMobject::group(xs.iter().map(|&(x, s)| Square::new(s).shift(RIGHT * x)).collect());
    let a = row(&[(-3.0, 0.5), (-1.0, 1.0), (1.0, 0.3), (3.0, 0.8)]);
    let b = row(&[(-3.0, 0.5), (-1.0, 1.0), (3.0, 0.8)]);
    let (la, lb) = align(&a, &b);
    assert_eq!((la.len(), lb.len()), (4, 4));
    let x = |m: &VMobject| m.get_center().x;
    // By index (Manim CE) the second square would go to the first; here each keeps its partner.
    assert!((x(&lb[0]) + 3.0).abs() < 1e-9 && (x(&lb[1]) + 1.0).abs() < 1e-9 && (x(&lb[3]) - 3.0).abs() < 1e-9);
}

#[test]
fn hidden_fill_and_stroke_take_the_visible_color() {
    let ring = Circle::new(1.0).set_stroke(YELLOW, 4.0).set_fill(BLUE, 0.0);
    let block = Square::new(2.0).set_fill(RED, 1.0).set_stroke(GREEN, 0.0);
    let mid = morph_at(&ring, &block, 0.5);
    let leaf = &mid.children[0];
    assert_eq!(leaf.fill_color, RED, "the fill fades in red, not through invisible blue");
    assert_eq!(leaf.stroke_color, YELLOW, "the stroke fades out yellow, not toward invisible green");
}

#[test]
fn write_outline_keeps_the_glyph_color_and_hidden_parts_hidden() {
    run("write_styles", |s| {
        let m = s.insert(VMobject::group(vec![
            Square::new(1.0).set_fill(RED, 1.0).set_stroke(BLUE, 0.0),
            Square::new(1.0).shift(RIGHT * 2.0).set_fill(WHITE, 0.0).set_stroke(WHITE, 0.0).set_stroke_opacity(0.0),
        ]));
        let before = s.get(m).clone();
        let log = recorder(s, move |w| {
            let c = &w[m].children;
            let first = c.first().map(|l| (l.stroke_color, l.stroke_width * l.stroke_opacity));
            (first, c.get(1).map(|l| l.stroke_width * l.stroke_opacity))
        });
        s.play(Write::new(m).run_time(1.0));
        assert_eq!(s.get(m), &before);
        let log = log.borrow();
        assert!(log.iter().filter(|(f, _)| f.is_some_and(|(_, w)| w > 0.0)).count() >= 5, "the outline is drawn");
        for (first, hidden) in log.iter().skip(1) {
            // Wherever the outline shows, it is in the fill color (not the hidden blue stroke).
            if let Some((c, width)) = first.filter(|(_, w)| *w > 0.0) {
                let off = (c.r - RED.r).abs() + (c.g - RED.g).abs() + (c.b - RED.b).abs();
                assert!(off < 1e-5, "outline in the fill color: {c:?} (width {width})");
            }
            assert_eq!(hidden.unwrap_or(0.0), 0.0, "an invisible part never shows an outline");
        }
    });
}

#[test]
fn write_lasts_exactly_its_run_time() {
    if !fonts_available() {
        return;
    }
    run("write_runtime", |s| {
        let t = s.insert(Text::new("A sentence long enough to stagger"));
        let t0 = s.time();
        s.play(Write::new(t).run_time(1.3));
        assert!((s.time() - t0 - 1.3).abs() < 1e-9, "the per-glyph lag stays inside the run time");
    });
}

#[test]
fn create_eases_each_part_on_its_own() {
    run("create_parts", |s| {
        let g = s.insert(VMobject::group(vec![Line::new(LEFT, RIGHT), Line::new(LEFT, RIGHT).shift(UP)]));
        let lens = recorder(s, move |w| {
            w[g].children
                .first()
                .map(|l| l.subpaths.first().map_or(0.0, |sp| manim::bezier::length(&sp.curves[0])))
                .unwrap_or(0.0)
        });
        s.play(Create::new(g)); // smooth, lag ratio 1: each line drawn in half the time
                                // test_config: 10 fps, so the third recorded frame is t = 0.2, the first line at smooth(0.4).
        let got = lens.borrow()[2] / 2.0;
        assert!((got - smooth(0.4)).abs() < 1e-6, "first line drawn {got}, expected {}", smooth(0.4));
    });
}

#[test]
fn letter_by_letter_reveals_one_letter_per_tick() {
    if !fonts_available() {
        return;
    }
    run("letters", |s| {
        let t = s.insert(Text::new("abcde"));
        let before = s.get(t).clone();
        let shown = recorder(s, move |w| if w.is_displayed(t) { w[t].children.len() } else { 0 });
        let t0 = s.time();
        s.play(AddTextLetterByLetter::new(t));
        assert!((s.time() - t0 - 0.5).abs() < 1e-9, "0.1 s per letter");
        assert_eq!(&shown.borrow()[1..], &[1, 2, 3, 4, 5], "a letter on the first frame, one more each frame");
        assert_eq!(s.get(t), &before);
        s.play(RemoveTextLetterByLetter::new(t));
        assert!(!s.world().is_displayed(t));
    });
}

#[test]
fn unwrite_erases_last_glyph_first_and_removes() {
    if !fonts_available() {
        return;
    }
    run("unwrite", |s| {
        let t = s.add(Text::new("abcdef"));
        let first = s.get(t).leaves()[0].clone();
        let log = recorder(s, move |w| {
            let c = &w[t].children;
            (c.first().cloned(), c.last().map(|l| l.fill_opacity))
        });
        s.play(Unwrite::new(t));
        let early = &log.borrow()[2];
        assert_eq!(early.0.as_ref(), Some(&first), "the first glyph is untouched early on");
        assert!(early.1.unwrap() < 1.0, "the last glyph is already fading");
        assert!(!s.world().is_displayed(t));
    });
}

#[test]
fn type_with_cursor_moves_the_cursor_along() {
    if !fonts_available() {
        return;
    }
    run("type_cursor", |s| {
        let t = s.insert(Text::new("typing"));
        let right = s.get(t).get_critical_point(RIGHT).x;
        let cursor = s.insert(Rectangle::new(0.05, 0.4));
        let y = s.get(cursor).get_center().y;
        let xs = recorder(s, move |w| w[cursor].get_center().x);
        s.play(TypeWithCursor::new(t, cursor));
        let xs = xs.borrow();
        assert!(xs[1..].windows(2).all(|w| w[0] < w[1]), "{xs:?}");
        assert!(*xs.last().unwrap() > right, "ends after the last letter");
        assert!((s.get(cursor).get_center().y - y).abs() < 1e-9, "keeps its height");
        assert!(s.world().is_displayed(cursor) && s.world().is_displayed(t));
    });
}

#[test]
fn transform_matching_shapes_moves_matches_and_fades_the_rest() {
    run("matching", |s| {
        let a = s.add(VMobject::group(vec![Square::new(1.0).shift(LEFT * 2.0), Circle::new(0.5).shift(RIGHT * 2.0)]));
        let b = s.insert(VMobject::group(vec![Circle::new(0.5).shift(LEFT * 2.0), Triangle::new().shift(RIGHT * 2.0)]));
        let mid = recorder(s, move |w| {
            w[a].children.iter().map(|c| (c.get_center().x, c.fill_opacity.max(c.stroke_opacity))).collect::<Vec<_>>()
        });
        s.play(TransformMatchingShapes::new(a, b).rate_func(linear));
        let frames = mid.borrow();
        let half = &frames[frames.len() / 2];
        assert_eq!(half.len(), 3, "the circle pair, the square fading out and the triangle fading in");
        assert!(
            half.iter().any(|(x, o)| x.abs() < 0.5 && *o > 0.99),
            "the circle travels across, fully shown: {half:?}"
        );
        assert!(half.iter().filter(|(_, o)| *o < 0.99).count() == 2, "the others are mid-fade: {half:?}");
        assert!(!s.world().is_displayed(a) && s.world().is_displayed(b));
    });
}

#[test]
fn fade_transform_cross_fades_and_replaces() {
    run("fade_transform", |s| {
        let a = s.add(Square::new(1.0));
        let b = s.insert(Circle::new(1.5).shift(RIGHT));
        let n = recorder(s, move |w| w[a].children.len());
        s.play(FadeTransform::new(a, b));
        assert_eq!(n.borrow()[3], 2, "both shapes on screen while cross-fading");
        assert!(!s.world().is_displayed(a) && s.world().is_displayed(b));
    });
}
