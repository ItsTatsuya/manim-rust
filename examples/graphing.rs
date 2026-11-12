//! Axes, function plots, value trackers and always_redraw.
//! cargo run --release --example graphing -- -qm

use manim::prelude::*;

fn main() -> anyhow::Result<()> {
    manim::render("Graphing", |s| {
        let axes = Axes::new([-4.0, 4.0, 1.0], [-1.5, 1.5, 0.5]).x_length(10.0).y_length(5.0).with_numbers();
        let ax = s.insert(&axes);
        s.play(Create::new(ax).run_time(1.5).lag_ratio(0.05));

        let sin = s.insert(axes.plot(f64::sin).set_stroke(BLUE, 4.0));
        let cos = s.insert(axes.plot(f64::cos).set_stroke(RED, 4.0));
        s.play(Create::new(sin).run_time(1.5));
        s.play(TransformFromCopy::new(sin, cos));

        // A dot that rides the sine curve, driven by a tracker.
        let x = s.tracker(-4.0);
        let a1 = axes.clone();
        let dot = s.always_redraw(move |w| {
            let t = w.value(x);
            Dot::at(a1.c2p(t, t.sin())).set_color(YELLOW).scale(1.5)
        });
        let a2 = axes.clone();
        let line = s.always_redraw(move |w| {
            let t = w.value(x);
            a2.v_line(t, t.sin()).set_color(YELLOW)
        });
        s.play(ChangeValue::new(x, 4.0).run_time(4.0).rate_func(linear));
        s.wait(0.5);
        s.clear_updaters();

        // Warp a number plane.
        s.play((FadeOut::new(ax), FadeOut::new(sin), FadeOut::new(cos), FadeOut::new(dot), FadeOut::new(line)));
        let plane = s.insert(NumberPlane::new().insert_curves(8));
        s.play(Create::new(plane).run_time(1.5).lag_ratio(0.02));
        s.play(plane.animate(|m| m.apply_function(|p| pt(p.x + p.y.sin(), p.y + p.x.sin() * 0.5))).run_time(2.0));
        s.wait(1.0);
    })
}
