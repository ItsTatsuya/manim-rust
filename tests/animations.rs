//! End states and timing of every animation type, driven through a real `Scene`.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::*;
use manim::prelude::*;

/// Record a value from the world after every frame.
fn recorder<T: 'static>(s: &mut Scene, f: impl Fn(&World) -> T + 'static) -> Rc<RefCell<Vec<T>>> {
    let log = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    s.add_updater(move |w, _| l.borrow_mut().push(f(w)));
    log
}

#[test]
fn create_draws_progressively_then_restores() {
    run("create", |s| {
        let sq = s.insert(Square::new(2.0));
        let before = s.get(sq).clone();
        let curves = recorder(s, move |w| if w.is_displayed(sq) { w[sq].num_curves() } else { 0 });
        s.play(Create::new(sq).rate_func(linear));
        assert_eq!(s.get(sq), &before, "Create must restore the original mobject");
        assert!(s.world().is_displayed(sq));
        let c = curves.borrow();
        assert!(c.windows(2).all(|w| w[0] <= w[1]), "curve count must not decrease: {c:?}");
        assert!(c[1] < 4, "should start partially drawn");
    });
}

#[test]
fn uncreate_hides_at_end() {
    run("uncreate", |s| {
        let c = s.add(Circle::new(1.0));
        s.play(Uncreate::new(c));
        assert!(!s.world().is_displayed(c));
    });
}

#[test]
fn transform_ends_exactly_at_target() {
    run("transform", |s| {
        let sq = s.add(Square::new(2.0));
        let target = Circle::new(1.0).set_fill(PINK, 0.5).shift(RIGHT);
        s.play(Transform::new(sq, target.clone()));
        assert_eq!(s.get(sq), &target);
    });
}

#[test]
fn transform_moves_monotonically_with_linear_rate() {
    run("transform_mono", |s| {
        let sq = s.add(Square::new(1.0).shift(LEFT * 3.0));
        let xs = recorder(s, move |w| w[sq].get_center().x);
        s.play(Transform::new(sq, Square::new(1.0).shift(RIGHT * 3.0)).rate_func(linear));
        let xs = xs.borrow();
        assert!(xs.windows(2).all(|w| w[0] <= w[1] + 1e-9), "{xs:?}");
        assert!((xs.last().unwrap() - 3.0).abs() < 1e-9);
    });
}

#[test]
fn transform_between_groups_of_different_sizes() {
    run("transform_groups", |s| {
        let g = s.add(vgroup![Circle::new(0.5), Square::new(1.0), Triangle::new()].arrange(RIGHT));
        s.play(Transform::new(g, Circle::new(2.0)));
        assert_eq!(s.get(g), &Circle::new(2.0));
        s.play(Transform::new(g, vgroup![Dot::new(), Dot::at(RIGHT), Dot::at(UP), Dot::at(LEFT)]));
        assert_eq!(s.get(g).children.len(), 4);
        s.play(Transform::new(g, VGroup::new(vec![])));
        assert!(s.get(g).leaves().is_empty());
    });
}

#[test]
fn transform_with_path_arc_lands_on_target() {
    run("path_arc", |s| {
        let a = s.add(Square::new(1.0).shift(LEFT * 2.0));
        let target = Square::new(1.0).shift(RIGHT * 2.0);
        s.play(Transform::new(a, target.clone()).path_arc(PI));
        assert_eq!(s.get(a), &target);
    });
}

#[test]
fn animate_applies_builder_chain() {
    run("animate", |s| {
        let sq = s.add(Square::new(1.0));
        s.play(sq.animate(|m| m.shift(RIGHT * 2.0).scale(2.0).set_color(BLUE)));
        assert_close(s.get(sq).get_center(), RIGHT * 2.0);
        assert!((s.get(sq).width() - 2.0).abs() < 1e-9);
        assert_eq!(s.get(sq).stroke_color, BLUE);
    });
}

#[test]
fn replacement_transform_swaps_display() {
    run("replacement", |s| {
        let a = s.add(Square::new(1.0));
        let b = s.insert(Circle::new(1.0));
        let other = s.add(Dot::new());
        s.play(ReplacementTransform::new(a, b));
        assert!(!s.world().is_displayed(a));
        assert!(s.world().is_displayed(b));
        assert_eq!(s.world().displayed(), &[b, other], "b takes a's place in the draw order");
        assert_eq!(s.get(b), &Circle::new(1.0));
    });
}

#[test]
fn transform_from_copy_leaves_source() {
    run("from_copy", |s| {
        let a = s.add(Square::new(1.0));
        let before = s.get(a).clone();
        let b = s.insert(Circle::new(1.0).shift(RIGHT * 3.0));
        s.play(TransformFromCopy::new(a, b));
        assert_eq!(s.get(a), &before);
        assert!(s.world().is_displayed(a) && s.world().is_displayed(b));
        assert_eq!(s.world().displayed().len(), 2, "the temporary copy must be gone");
    });
}

#[test]
fn fades() {
    run("fades", |s| {
        let a = s.insert(Square::new(1.0));
        let before = s.get(a).clone();
        let opacity = recorder(s, move |w| if w.is_displayed(a) { w[a].leaves()[0].stroke_opacity } else { -1.0 });
        s.play(FadeIn::new(a).shift(UP).scale(0.5).rate_func(linear));
        assert_eq!(s.get(a), &before);
        assert!(s.world().is_displayed(a));
        {
            let o = opacity.borrow();
            assert!(o.windows(2).all(|w| w[0] <= w[1] + 1e-9), "{o:?}");
        }
        s.play(FadeOut::new(a).shift(DOWN));
        assert!(!s.world().is_displayed(a));
        assert_eq!(s.get(a), &before, "FadeOut restores the mobject so it can be re-added");
    });
}

#[test]
fn grow_shrink_spin() {
    run("grow", |s| {
        let a = s.insert(Star::new(5, 1.0, 0.4).shift(RIGHT));
        let before = s.get(a).clone();
        s.play(GrowFromCenter::new(a));
        assert_eq!(s.get(a), &before);
        s.play(GrowFromPoint::new(a, LEFT * 3.0));
        assert_eq!(s.get(a), &before);
        s.play(SpinInFromNothing::new(a));
        assert_eq!(s.get(a), &before);
        s.play(ShrinkToCenter::new(a));
        assert!(!s.world().is_displayed(a));
    });
}

#[test]
fn indicate_returns_to_original() {
    run("indicate", |s| {
        let a = s.add(Circle::new(1.0));
        let before = s.get(a).clone();
        let widths = recorder(s, move |w| w[a].width());
        s.play(Indicate::new(a));
        assert_eq!(s.get(a), &before);
        let max = widths.borrow().iter().cloned().fold(0.0, f64::max);
        assert!(max > 2.2, "should grow during Indicate (max width {max})");
    });
}

#[test]
fn draw_border_then_fill_and_write_restore() {
    run("dbtf", |s| {
        let a = s.insert(Square::new(1.0).set_fill(RED, 0.8));
        let before = s.get(a).clone();
        s.play(DrawBorderThenFill::new(a));
        assert_eq!(s.get(a), &before);
        let t = s.insert(Text::new("Hello world"));
        let tb = s.get(t).clone();
        s.play(Write::new(t));
        assert_eq!(s.get(t), &tb);
        let empty = s.insert(Text::new(""));
        s.play(Write::new(empty));
    });
}

#[test]
fn write_timing_scales_with_glyphs() {
    if !fonts_available() {
        eprintln!("skipping: no system fonts");
        return;
    }
    run("write_timing", |s| {
        let short = s.insert(Text::new("Hi"));
        let t0 = s.time();
        s.play(Write::new(short));
        assert!((s.time() - t0 - 1.0).abs() < 1e-9, "short text: 1 s");
        let long = s.insert(Text::new("This sentence is definitely long"));
        let t1 = s.time();
        s.play(Write::new(long));
        assert!((s.time() - t1 - 2.0).abs() < 1e-9, "15+ glyphs: 2 s");
    });
}

#[test]
fn show_passing_flash_removes_itself() {
    run("flash", |s| {
        let a = s.insert(Circle::new(1.0));
        s.play(ShowPassingFlash::new(a).time_width(0.3));
        assert!(!s.world().is_displayed(a));
    });
}

#[test]
fn rotate_is_a_true_rotation() {
    run("rotate", |s| {
        let d = s.add(Dot::at(RIGHT * 2.0));
        let radii = recorder(s, move |w| w[d].get_center().length());
        s.play(Rotate::new(d, PI / 2.0).about(ORIGIN));
        assert_close(s.get(d).get_center(), UP * 2.0);
        assert!(radii.borrow().iter().all(|r| (r - 2.0).abs() < 1e-6), "stays on the circle");
        let sq = s.add(Square::new(1.0).shift(LEFT));
        let before = s.get(sq).clone();
        s.play(Rotate::new(sq, TAU));
        assert!(same_points(s.get(sq), &before));
    });
}

#[test]
fn move_along_path_ends_at_path_end() {
    run("move_along", |s| {
        let d = s.add(Dot::new());
        let path = Arc::new(2.0, 0.0, PI);
        s.play(MoveAlongPath::new(d, path.clone()));
        assert_close(s.get(d).get_center(), path.get_end());
    });
}

#[test]
fn trackers_and_always_redraw() {
    run("trackers", |s| {
        let t = s.tracker(0.0);
        let d = s.always_redraw(move |w| Dot::at(RIGHT * w.value(t)));
        s.play(ChangeValue::new(t, 3.0));
        assert_eq!(s.value(t), 3.0);
        assert_close(s.get(d).get_center(), RIGHT * 3.0);
        s.set_value(t, -1.0);
        s.wait(0.1);
        assert_close(s.get(d).get_center(), LEFT);
    });
}

#[test]
fn updaters_receive_dt_and_can_be_removed() {
    run("updaters", |s| {
        let total = Rc::new(RefCell::new(0.0));
        let t2 = total.clone();
        let u = s.add_updater(move |_, dt| *t2.borrow_mut() += dt);
        s.wait(1.0);
        assert!((*total.borrow() - 1.0).abs() < 1e-9);
        s.remove_updater(u);
        s.wait(1.0);
        assert!((*total.borrow() - 1.0).abs() < 1e-9, "removed updater must not run");
    });
}

#[test]
fn succession_begins_each_animation_after_the_previous() {
    run("succession", |s| {
        let a = s.add(Square::new(1.0));
        s.play(Succession::new(anims![a.animate(|m| m.shift(RIGHT)), a.animate(|m| m.shift(UP)), Wait::new(0.5)]));
        assert_close(s.get(a).get_center(), RIGHT + UP);
        assert!((s.time() - 2.5).abs() < 1e-9);
    });
}

#[test]
fn group_timing() {
    run("group_timing", |s| {
        let ids: Vec<_> = (0..3).map(|i| s.add(Dot::at(RIGHT * i as f64))).collect();
        let t0 = s.time();
        s.play(LaggedStart::new(ids.iter().map(|&i| Indicate::new(i).boxed()).collect::<Vec<_>>()).lag_ratio(0.5));
        assert!((s.time() - t0 - 2.0).abs() < 1e-9, "3 × 1 s with lag 0.5 = 2 s");
        let t1 = s.time();
        s.play((Indicate::new(ids[0]).run_time(1.0), Indicate::new(ids[1]).run_time(3.0)));
        assert!((s.time() - t1 - 3.0).abs() < 1e-9, "parallel play lasts as long as the longest");
        let t2 = s.time();
        s.play(AnimationGroup::new(anims![Indicate::new(ids[0]), Indicate::new(ids[1])]).run_time(4.0));
        assert!((s.time() - t2 - 4.0).abs() < 1e-9, "explicit group run_time wins");
    });
}

#[test]
fn degenerate_timelines() {
    run("degenerate", |s| {
        let a = s.add(Square::new(1.0));
        let t0 = s.time();
        s.play(a.animate(|m| m.shift(UP)).run_time(0.0));
        assert_close(s.get(a).get_center(), UP);
        s.play(Vec::<Box<dyn Animation>>::new());
        s.wait(0.0);
        s.wait(-1.0);
        assert_eq!(s.time(), t0);
    });
}

#[test]
fn overshooting_rate_functions_are_safe() {
    run("overshoot", |s| {
        let a = s.insert(Square::new(1.0));
        s.play(Create::new(a).rate_func(ease_out_back));
        s.play(a.animate(|m| m.shift(RIGHT)).rate_func(ease_out_back));
        s.play(FadeIn::new(a).rate_func(ease_out_bounce));
        s.play(a.animate(|m| m.shift(LEFT)).rate_func(wiggle));
        assert_close(s.get(a).get_center(), ORIGIN);
    });
}

#[test]
fn frame_counts_match_manim_ce() {
    // Manim CE: play() renders ceil(run_time * fps) frames; wait() renders floor(...).
    run("frames", |s| {
        let a = s.add(Square::new(1.0));
        let t0 = s.time();
        s.play(a.animate(|m| m.shift(UP)).run_time(0.25));
        assert!((s.time() - t0 - 0.3).abs() < 1e-9, "0.25 s at 10 fps → 3 frames");
        let t1 = s.time();
        s.wait(0.25);
        assert!((s.time() - t1 - 0.2).abs() < 1e-9, "0.25 s wait at 10 fps → 2 frames");
        let t2 = s.time();
        s.play(a.animate(|m| m.shift(UP)).run_time(1.0));
        assert!((s.time() - t2 - 1.0).abs() < 1e-9, "exact multiples are not rounded up");
    });
}

#[test]
fn z_index_controls_draw_order() {
    let mut w = World::new();
    let top = w.insert(Square::new(1.0).set_fill(RED, 1.0).set_z_index(5));
    let bottom = w.insert(Square::new(1.0).set_fill(BLUE, 1.0));
    w.show(top);
    w.show(bottom);
    let dl = Camera::new(64, 36).display_list(&w);
    assert_eq!(dl.items.len(), 2);
    assert_eq!(dl.items[1].fill.unwrap()[0], RED.r, "higher z_index drawn last");
}
