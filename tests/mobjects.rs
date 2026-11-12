//! Geometry, layout, text, graphing and color behaviour.

mod common;

use common::*;
use manim::bezier;
use manim::mobject::{align, lerp_leaf, PathFunc};
use manim::prelude::*;

#[test]
fn circle_points_lie_on_the_circle() {
    let c = Circle::new(2.0);
    assert!((c.width() - 4.0).abs() < 1e-9 && (c.height() - 4.0).abs() < 1e-9);
    for sp in &c.subpaths {
        for cu in &sp.curves {
            for i in 0..=10 {
                let r = bezier::point_at(cu, i as f64 / 10.0).length();
                assert!((r - 2.0).abs() < 2e-3, "cubic arc error too large: r = {r}");
            }
        }
    }
    assert!(c.subpaths[0].closed);
}

#[test]
fn basic_shape_dimensions() {
    let sq = Square::new(3.0);
    assert!((sq.width() - 3.0).abs() < 1e-9 && (sq.height() - 3.0).abs() < 1e-9);
    let r = Rectangle::new(4.0, 1.0);
    assert!((r.width() - 4.0).abs() < 1e-9 && (r.height() - 1.0).abs() < 1e-9);
    let rr = RoundedRectangle::new(4.0, 2.0, 0.5);
    assert!((rr.width() - 4.0).abs() < 1e-9 && (rr.height() - 2.0).abs() < 1e-9);
    let e = Ellipse::new(4.0, 2.0);
    assert!((e.width() - 4.0).abs() < 1e-9 && (e.height() - 2.0).abs() < 1e-9);
    assert_eq!(RegularPolygon::new(0).num_curves(), 3, "fewer than 3 sides clamps to a triangle");
    assert_eq!(RegularPolygon::new(7).num_curves(), 7);
    assert_eq!(Star::new(5, 1.0, 0.5).num_curves(), 10);
    assert!(Dot::new().fill_opacity == 1.0 && Dot::new().stroke_width == 0.0);
    assert_eq!(Annulus::new(0.5, 1.0).subpaths.len(), 2);
    assert_eq!(DashedLine::new(LEFT * 2.0, RIGHT * 2.0).subpaths.len(), 40);
}

#[test]
fn default_colors_match_manim() {
    assert_eq!(Circle::new(1.0).stroke_color, RED);
    assert_eq!(Square::new(1.0).stroke_color, WHITE);
    assert_eq!(Triangle::new().stroke_color, BLUE);
    assert_eq!(Star::new(5, 1.0, 0.5).stroke_color, BLUE);
    assert_eq!(Polygon::new(&[ORIGIN, RIGHT, UP]).stroke_color, BLUE);
    assert_eq!(YELLOW, Color::hex("#F7D96F").unwrap());
    assert_eq!(BLUE_E, Color::hex("#236B8E").unwrap());
}

#[test]
fn arrows_point_the_right_way_even_when_short() {
    for len in [0.05, 0.3, 0.5, 1.0, 5.0] {
        let a = Arrow::new(ORIGIN, RIGHT * len);
        let pts = all_points(&a);
        assert!(!pts.is_empty());
        let xs: Vec<f64> = pts.iter().map(|p| p.x).collect();
        let (lo, hi) = xs.iter().fold((f64::MAX, f64::MIN), |(l, h), x| (l.min(*x), h.max(*x)));
        assert!(lo >= -1e-9 && hi <= len + 1e-9, "len {len}: arrow spans {lo}..{hi}");
        let tip = &a.children[1];
        assert!(tip.get_right().x >= a.children[0].get_right().x - 1e-9, "len {len}: tip must be at the head");
    }
    assert!(all_points(&Arrow::new(UP, UP)).is_empty(), "zero-length arrow is empty, not NaN");
    assert!(all_points(&Vector::new(RIGHT)).iter().all(|p| p.is_finite()));
}

#[test]
fn transforms() {
    let sq = Square::new(2.0).shift(RIGHT * 3.0);
    assert_close(sq.get_center(), RIGHT * 3.0);
    let r = sq.clone().rotate(PI / 3.0);
    assert_close(r.get_center(), RIGHT * 3.0);
    let s = sq.clone().scale(2.0);
    assert!((s.width() - 4.0).abs() < 1e-9);
    assert_close(s.get_center(), RIGHT * 3.0);
    let s2 = sq.clone().scale_about(2.0, ORIGIN);
    assert_close(s2.get_center(), RIGHT * 6.0);
    let st = sq.clone().stretch(3.0, 1);
    assert!((st.height() - 6.0).abs() < 1e-9 && (st.width() - 2.0).abs() < 1e-9);
    assert!((sq.clone().set_width(5.0).width() - 5.0).abs() < 1e-9);
    assert!((sq.clone().set_height(1.0).height() - 1.0).abs() < 1e-9);
    let tri = Triangle::new();
    let f = tri.clone().flip();
    assert!((f.get_left().x + tri.get_right().x).abs() < 1e-9);
    // set_width on an empty / zero-width mobject must not produce NaN.
    assert!(all_points(&Line::new(UP, DOWN).set_width(3.0)).iter().all(|p| p.is_finite()));
    assert!(VGroup::new(vec![]).scale(2.0).bounding_box().is_none());
}

#[test]
fn layout_helpers() {
    let a = Square::new(2.0);
    let b = Circle::new(0.5).next_to(&a, UP);
    assert!((b.get_bottom().y - (1.0 + DEFAULT_MOBJECT_TO_MOBJECT_BUFFER)).abs() < 1e-9);
    let c = Circle::new(0.5).next_to_buff(&a, DL, 0.0);
    assert_close(c.get_corner(UR), a.get_corner(DL));
    let e = Square::new(1.0).to_edge(DOWN);
    assert!((e.get_bottom().y - (-FRAME_Y_RADIUS + DEFAULT_MOBJECT_TO_EDGE_BUFFER)).abs() < 1e-9);
    let k = Square::new(1.0).to_corner(UR);
    assert!((k.get_right().x - (FRAME_X_RADIUS - 0.5)).abs() < 1e-9);
    assert!((k.get_top().y - (FRAME_Y_RADIUS - 0.5)).abs() < 1e-9);
    let al = Circle::new(0.3).shift(DOWN * 2.0).align_to(&a, LEFT);
    assert!((al.get_left().x - a.get_left().x).abs() < 1e-9);
    assert!((al.get_center().y + 2.0).abs() < 1e-9, "align_to only moves along the given axis");

    let row = vgroup![Square::new(1.0), Circle::new(0.5), Triangle::new()].arrange_buff(RIGHT, 0.5);
    let xs: Vec<f64> = row.children.iter().map(|c| c.get_center().x).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]));
    assert_close(row.get_center(), ORIGIN);
    for w in row.children.windows(2) {
        assert!((w[1].get_left().x - w[0].get_right().x - 0.5).abs() < 1e-9);
    }
    let grid = VGroup::new((0..6).map(|_| Square::new(1.0)).collect()).arrange_in_grid(3, 0.2);
    // 3 columns of unit squares on a 1.2 pitch: 2 × 1.2 + 1.
    assert!((grid.width() - 3.4).abs() < 1e-6 && (grid.height() - 2.2).abs() < 1e-6);
    assert_close(grid.get_center(), ORIGIN);
}

#[test]
fn style_setters_reach_the_whole_family() {
    let g = vgroup![Circle::new(1.0), vgroup![Square::new(1.0), Triangle::new()]].set_color(GREEN).set_opacity(0.3);
    for m in g.family() {
        assert_eq!(m.stroke_color, GREEN);
        assert_eq!(m.fill_color, GREEN);
        assert_eq!(m.fill_opacity, 0.3);
    }
    let faded = Square::new(1.0).set_fill(RED, 1.0).fade(0.75);
    assert!((faded.fill_opacity - 0.25).abs() < 1e-12);
    let grad = VGroup::new((0..3).map(|_| Dot::new()).collect()).set_color_by_gradient(&[RED, BLUE]);
    assert_eq!(grad.children[0].fill_color, RED);
    assert_eq!(grad.children[2].fill_color, BLUE);
}

#[test]
fn proportions_and_partials() {
    let sq = Square::new(2.0);
    assert_close(sq.point_from_proportion(0.0), sq.get_start());
    assert_close(sq.point_from_proportion(1.0), sq.get_end());
    let mid = sq.point_from_proportion(0.5);
    assert!((mid.x.abs() - 1.0).abs() < 1e-6 || (mid.y.abs() - 1.0).abs() < 1e-6, "on the perimeter");
    let mut prev = 0;
    for i in 0..=20 {
        let n = sq.partial(i as f64 / 20.0).num_curves();
        assert!(n >= prev);
        prev = n;
    }
    assert_eq!(sq.partial(1.0).subpaths, sq.subpaths);
    assert!(sq.partial_range(0.6, 0.4).subpaths.is_empty());
    assert!(sq.partial(-1.0).subpaths.is_empty() && sq.partial(5.0).subpaths == sq.subpaths);
}

#[test]
fn align_and_interpolate() {
    let pairs = [
        (Square::new(2.0), Circle::new(1.0)),
        (VGroup::new(vec![]), Circle::new(1.0)),
        (Circle::new(1.0), VGroup::new(vec![])),
        (vgroup![Dot::new(), Dot::at(RIGHT)], Star::new(7, 1.0, 0.3)),
        (Annulus::new(0.5, 1.0), Square::new(1.0)),
        (Polygon::new(&[]), Square::new(1.0)),
    ];
    for (a, b) in pairs {
        let (la, lb) = align(&a, &b);
        assert_eq!(la.len(), lb.len());
        for (x, y) in la.iter().zip(&lb) {
            assert_eq!(x.subpaths.len(), y.subpaths.len());
            for (sx, sy) in x.subpaths.iter().zip(&y.subpaths) {
                assert_eq!(sx.curves.len(), sy.curves.len());
                assert!(!sx.curves.is_empty(), "aligned subpaths must have curves");
            }
            assert!(same_points(&lerp_leaf(x, y, 0.0, PathFunc::Straight), x));
            assert!(same_points(&lerp_leaf(x, y, 1.0, PathFunc::Straight), y));
            assert!(same_points(&lerp_leaf(x, y, 1.0, PathFunc::Arc(PI / 2.0)), y));
        }
    }
}

#[test]
fn graphing() {
    let axes = Axes::new([-2.0, 4.0, 1.0], [-1.0, 9.0, 1.0]).x_length(6.0).y_length(5.0).move_to(RIGHT);
    for (x, y) in [(-2.0, -1.0), (4.0, 9.0), (0.0, 0.0), (1.5, 2.25)] {
        let p = axes.c2p(x, y);
        let (bx, by) = axes.p2c(p);
        assert!((bx - x).abs() < 1e-9 && (by - y).abs() < 1e-9);
    }
    assert_close(axes.c2p(1.0, 4.0), RIGHT);
    let g = axes.plot(|x| x * x);
    assert_close(g.get_start(), axes.c2p(-2.0, 4.0));
    assert_close(g.get_end(), axes.c2p(4.0, 16.0));
    let m = axes.to_mobject();
    assert!(!m.leaves().is_empty());
    // Non-finite samples split the curve instead of poisoning it.
    let tan = FunctionGraph::new(|x| if x.abs() < 0.05 { f64::NAN } else { 1.0 / x }, [-1.0, 1.0]);
    assert_eq!(tan.subpaths.len(), 2);
    assert!(all_points(&tan).iter().all(|p| p.is_finite()));
    let plane = NumberPlane::with_ranges([-2.0, 2.0, 1.0], [-1.0, 1.0, 1.0]);
    assert_eq!(plane.leaves().len(), 4 + 2 + 2, "grid lines (excluding axes) + 2 axes");
}

#[test]
fn text_layout() {
    if !fonts_available() {
        eprintln!("skipping: no system fonts");
        return;
    }
    let t = Text::new("Hi there");
    assert_eq!(t.children.len(), 7, "one child per visible glyph");
    assert_close(t.get_center(), ORIGIN);
    assert!(t.height() > 0.3 && t.height() < 1.0, "48pt text height {}", t.height());
    let big = Text::sized("Hi there", 96.0);
    assert!((big.width() / t.width() - 2.0).abs() < 0.01);
    let two = Text::new("A\nB");
    assert!(two.children[1].get_center().y < two.children[0].get_center().y);
    assert!(Text::new("").children.is_empty());
    assert!(Text::new("   ").children.is_empty());
    assert!(!Text::styled("x", TextStyle::default().font("No Such Font 123")).children.is_empty(), "falls back");
    assert!(Text::new("θ²½→").children.len() >= 3, "common math symbols render");
    let c = Text::styled("ok", TextStyle::default().color(RED).bold());
    assert!(c.children.iter().all(|g| g.fill_color == RED && g.fill_opacity == 1.0));
}

#[test]
fn colors() {
    assert_eq!(Color::hex("#58C4DD"), Some(BLUE));
    assert_eq!(Color::hex("fff"), Some(WHITE));
    assert_eq!(Color::hex("#12345"), None);
    assert_eq!(Color::hex("zzzzzz"), None);
    assert_eq!(RED.lerp(BLUE, 0.0), RED);
    assert_eq!(RED.lerp(BLUE, 1.0), BLUE);
    assert_eq!(Color::rgb(2.0, -1.0, 0.5).to_array(3.0), [1.0, 0.0, 0.5, 1.0]);
    let g = color_gradient(&[RED, GREEN, BLUE], 5);
    assert_eq!((g[0], g[2], g[4]), (RED, GREEN, BLUE));
    assert!(color_gradient(&[], 3).is_empty());
    assert_eq!(color_gradient(&[RED], 2), vec![RED, RED]);
}

#[test]
fn rate_functions_are_bounded_and_continuous() {
    let fs: [(&str, RateFunc); 12] = [
        ("linear", linear),
        ("smooth", smooth),
        ("rush_into", rush_into),
        ("rush_from", rush_from),
        ("slow_into", slow_into),
        ("double_smooth", double_smooth),
        ("there_and_back", there_and_back),
        ("there_and_back_with_pause", there_and_back_with_pause),
        ("lingering", lingering),
        ("ease_in_out_sine", ease_in_out_sine),
        ("ease_in_out_cubic", ease_in_out_cubic),
        ("ease_out_bounce", ease_out_bounce),
    ];
    for (name, f) in fs {
        let mut prev = f(0.0);
        for i in 1..=1000 {
            let v = f(i as f64 / 1000.0);
            assert!(v.is_finite() && (-1e-9..=1.0 + 1e-9).contains(&v), "{name}({}) = {v}", i as f64 / 1000.0);
            assert!((v - prev).abs() < 0.05, "{name} jumps near {}", i as f64 / 1000.0);
            prev = v;
        }
    }
}
