//! Randomized stress tests: thousands of random shapes, transforms and animation
//! sequences must never panic or produce non-finite geometry.

mod common;

use common::*;
use manim::mobject::{align, lerp_leaves, PathFunc};
use manim::prelude::*;
use manim::renderer::cpu::rasterize;

/// Scale iterations with MANIM_FUZZ=<n> (default 1) for long soak runs.
fn scale(n: usize) -> usize {
    n * std::env::var("MANIM_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(1)
}

#[test]
fn random_align_and_interpolate() {
    let mut rng = Rng::new(1);
    for _ in 0..scale(1500) {
        let (a, b) = (rng.mobject(), rng.mobject());
        let (la, lb) = align(&a, &b);
        assert_eq!(la.len(), lb.len());
        let t = rng.f(-0.3, 1.3);
        let path = if rng.usize(2) == 0 { PathFunc::Straight } else { PathFunc::Arc(rng.f(-6.0, 6.0)) };
        let m = lerp_leaves(&la, &lb, t, path, 0);
        assert!(all_points(&m).iter().all(|p| p.is_finite()), "non-finite interpolation");
        let p = m.partial_range(rng.f(-0.2, 1.0), rng.f(0.0, 1.2));
        assert!(all_points(&p).iter().all(|p| p.is_finite()));
        if !all_points(&a).is_empty() {
            assert!(a.point_from_proportion(rng.f(-1.0, 2.0)).is_finite());
        }
    }
}

#[test]
fn random_frames_rasterize() {
    let mut rng = Rng::new(2);
    for _ in 0..scale(150) {
        let mut w = World::new();
        for _ in 0..rng.usize(12) {
            let id = w.insert(rng.mobject());
            w.show(id);
        }
        let dl = Camera::new(64, 36).display_list(&w);
        assert_eq!(rasterize(&dl).len(), 64 * 36 * 4);
    }
}

fn random_animation(rng: &mut Rng, s: &mut Scene, ids: &[MobjectId]) -> Box<dyn Animation> {
    let id = ids[rng.usize(ids.len())];
    let other = ids[rng.usize(ids.len())];
    let rate: fn(f64) -> f64 = [smooth, linear, there_and_back, ease_out_back, rush_into][rng.usize(5)];
    let rt = [0.0, 0.1, 0.35, 1.0][rng.usize(4)];
    let target = rng.mobject();
    let shift = rng.point(2.0);
    let a: Box<dyn Animation> = match rng.usize(17) {
        0 => Create::new(id).boxed(),
        1 => Uncreate::new(id).boxed(),
        2 => DrawBorderThenFill::new(id).boxed(),
        3 => Write::new(id).boxed(),
        4 => Transform::new(id, target).path_arc(rng.f(-3.0, 3.0)).boxed(),
        5 => FadeIn::new(id).shift(shift).boxed(),
        6 => FadeOut::new(id).scale(rng.f(0.0, 2.0)).boxed(),
        7 => GrowFromCenter::new(id).boxed(),
        8 => Indicate::new(id).boxed(),
        9 => Rotate::new(id, rng.f(-7.0, 7.0)).boxed(),
        10 => id.animate(move |m| m.shift(shift).scale(1.1)).boxed(),
        11 => MoveAlongPath::new(id, Circle::new(1.0)).boxed(),
        12 if id != other => {
            let b = s.insert(rng.mobject());
            ReplacementTransform::new(id, b).boxed()
        }
        13 if id != other => TransformFromCopy::new(id, other).boxed(),
        14 => ShowPassingFlash::new(id).boxed(),
        15 => SpinInFromNothing::new(id).boxed(),
        _ => Wait::new(0.2).boxed(),
    };
    let mut a = a;
    if rt > 0.0 || rng.usize(3) == 0 {
        a.config_mut().run_time = rt;
    }
    a.config_mut().rate_func = rate;
    a
}

#[test]
fn random_animation_sequences() {
    for seed in 0..scale(25) as u64 {
        let name = format!("fuzz_{seed}");
        let mut cfg = test_config(&name);
        cfg.pixel_width = 64;
        cfg.pixel_height = 36;
        cfg.fps = 8.0;
        render_with(&cfg, &name, |s| {
            let mut rng = Rng::new(100 + seed);
            let ids: Vec<MobjectId> = (0..4).map(|_| s.insert(rng.mobject())).collect();
            for &id in &ids {
                if rng.usize(2) == 0 {
                    s.show(id);
                }
            }
            if rng.usize(2) == 0 {
                let t = s.tracker(0.0);
                let target = ids[0];
                s.add_updater(move |w, dt| {
                    let v = w.value(t) + dt;
                    w.set_value(t, v);
                    w.modify(target, |m| m.rotate(dt));
                });
            }
            for _ in 0..12 {
                match rng.usize(4) {
                    0 => {
                        let n = 1 + rng.usize(3);
                        let anims: Vec<Box<dyn Animation>> =
                            (0..n).map(|_| random_animation(&mut rng, s, &ids)).collect();
                        let lag = [0.0, 0.3, 1.0][rng.usize(3)];
                        s.play(AnimationGroup::new(anims).lag_ratio(lag));
                    }
                    1 => s.wait(rng.f(0.0, 0.4)),
                    _ => {
                        let a = random_animation(&mut rng, s, &ids);
                        s.play(a);
                    }
                }
                for &id in &ids {
                    assert!(all_points(s.get(id)).iter().all(|p| p.is_finite()), "seed {seed}");
                }
            }
        })
        .unwrap_or_else(|e| panic!("seed {seed}: {e:#}"));
    }
}
