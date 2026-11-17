//! Inputs a host application can pass by mistake must give an error or a sane result,
//! never a panic, hang or runaway allocation.

use manim::config::{Config, Quality};
use manim::prelude::*;

fn cfg() -> Config {
    let mut c = Config::default().quality(Quality::Low);
    c.dry_run = true;
    c.progress = manim::events::ProgressMode::None;
    c
}

#[test]
fn degenerate_geometry_stays_bounded() {
    // Negative, zero and non-finite steps used to loop forever or overflow.
    let plane = NumberPlane::with_ranges([-7.0, 7.0, -1.0], [-4.0, 4.0, 0.0]);
    assert_eq!(plane.children[0].children.len(), 0);
    let plane = NumberPlane::with_ranges([-7.0, 7.0, 1e-9], [-4.0, 4.0, 1.0]);
    assert!(plane.children[0].children.len() <= 10_000 + 8);
    let _ = NumberPlane::with_ranges([f64::NEG_INFINITY, f64::INFINITY, 1.0], [-4.0, 4.0, 1.0]);

    assert!(Arc::new(1.0, 0.0, f64::INFINITY).subpaths[0].curves.is_empty());
    assert!(Arc::new(1.0, 0.0, 1e12).subpaths[0].curves.len() <= 512);
    assert!(DashedLine::with_dash(ORIGIN, RIGHT * 10.0, 0.0, 0.5).subpaths.len() <= 100_000);

    // A zero-width axis range maps instead of producing NaN.
    let axes = Axes::new([1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]);
    assert!(axes.c2p(1.0, 0.0).is_finite());

    // Descending or NaN ranges draw something instead of panicking in `clamp`.
    for (x, y) in [([5.0, -5.0, 1.0], [3.0, -3.0, 1.0]), ([f64::NAN, 1.0, 1.0], [2.0, f64::NAN, 1.0])] {
        let _ = Axes::new(x, y).to_mobject();
    }

    // Out-of-range stretch axis is ignored.
    let c = Circle::new(1.0).stretch(2.0, 5);
    assert!((c.width() - 2.0).abs() < 1e-6);
}

#[test]
fn zero_length_child_finishes_before_the_next_starts() {
    let mut s = Scene::new(&cfg(), "Z").unwrap();
    let a = s.add(Square::new(2.0));
    s.play(Succession::new((
        Transform::new(a, Circle::new(1.0)).run_time(0.0),
        a.animate(|m| m.shift(RIGHT * 2.0)).run_time(0.5),
    )));
    let m = &s[a];
    // Still the circle (radius 1, width 2), and moved right.
    assert!((m.get_center().x - 2.0).abs() < 1e-6, "center {:?}", m.get_center());
    let circle = Circle::new(1.0).shift(RIGHT * 2.0);
    assert!(m.subpaths[0].curves[0][0].distance(circle.subpaths[0].curves[0][0]) < 1e-6);
}

#[test]
fn non_finite_durations_fail_instead_of_hanging() {
    let mut s = Scene::new(&cfg(), "Inf").unwrap();
    let d = s.add(Dot::new());
    s.wait(f64::INFINITY);
    s.play(d.animate(|m| m.shift(RIGHT)).run_time(f64::NAN));
    assert!(s.finish(std::time::Instant::now()).is_err());
}

#[test]
fn bad_config_is_rejected() {
    for args in [&["--fps", "inf"][..], &["--fps", "NaN"], &["--fps", "0"], &["-r", "0x10"], &["-r", "100000x100000"]] {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        assert!(Config::parse(args.clone()).is_err(), "{args:?} should be rejected");
    }
    let mut c = cfg();
    c.fps = f64::INFINITY;
    assert!(Scene::new(&c, "Bad").is_err());
    // --help no longer exits the process.
    assert!(Config::parse(["--help".to_string()]).unwrap().help);
}

#[test]
fn seeking_restores_the_camera() {
    let mut c = cfg();
    c.record = true;
    let mut s = Scene::new(&c, "Cam").unwrap();
    let d = s.add(Dot::new());
    let before = s.camera().background;
    s.play(d.animate(|m| m.shift(RIGHT)));
    s.set_background_color(RED);
    s.camera().frame_center = RIGHT * 3.0;
    s.play(d.animate(|m| m.shift(LEFT)));
    s.seek(0.5).unwrap();
    assert_eq!(s.camera().background, before);
    assert_eq!(s.camera().frame_center, ORIGIN);
    s.seek(s.duration()).unwrap();
    assert_eq!(s.camera().background, RED);
    // Playing after a seek continues from the end of the timeline.
    s.seek(0.2).unwrap();
    let end = s.duration();
    s.play(d.animate(|m| m.shift(UP)));
    assert!(s.time() > end);
}

#[test]
fn a_group_with_an_infinite_member_fails_cleanly() {
    let mut s = Scene::new(&cfg(), "InfChild").unwrap();
    let d = s.add(Dot::new());
    let e = s.add(Dot::new());
    s.play(
        AnimationGroup::new((
            d.animate(|m| m.shift(RIGHT)).run_time(f64::INFINITY),
            e.animate(|m| m.shift(LEFT)).run_time(1.0),
        ))
        .run_time(1.0),
    );
    assert!(s.finish(std::time::Instant::now()).is_err());
}

#[test]
fn stale_or_foreign_ids_are_errors_not_panics() {
    let mut other = World::new();
    for _ in 0..5 {
        other.insert(Dot::new());
    }
    let foreign = other.insert(Dot::new()); // an id this scene's world has never seen
    other.tracker(0.0);
    let foreign_tracker = other.tracker(1.0);
    for case in 0..5 {
        let mut s = Scene::new(&cfg(), "Stale").unwrap();
        s.add(Dot::new());
        match case {
            0 => s.play(FadeIn::new(foreign)),
            1 => s.play(Transform::new(foreign, Dot::new())),
            2 => s.play(DrawBorderThenFill::new(foreign)),
            3 => s.play(Create::new(foreign)),
            _ => s.play(ChangeValue::new(foreign_tracker, 2.0)),
        }
        assert!(s.finish(std::time::Instant::now()).is_err(), "case {case}");
    }
    assert!(World::new().try_value(foreign_tracker).is_err());
    assert!(World::new().try_set(foreign, Dot::new()).is_err());
}
