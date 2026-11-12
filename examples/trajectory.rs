//! A ~2 minute explainer on projectile trajectories.
//!   cargo run --release --example trajectory -- -qh
//!
//! Outline:
//!  1. Title
//!  2. Throw a ball, trace its path
//!  3. Split velocity into horizontal (constant) and vertical (gravity) parts
//!  4. The equations -> a parabola
//!  5. Key numbers: max height, time of flight, range
//!  6. Launch angle: 45° goes farthest, complementary angles tie
//!  7. Air resistance
//!  8. Summary

use manim::bezier;
use manim::prelude::*;

const G: f64 = 9.8;
const V: f64 = 10.0;
/// Scene units per m/s for drawing velocity arrows.
const ARROW_SCALE: f64 = 0.2;

fn pos(theta: f64, t: f64) -> (f64, f64) {
    (V * theta.cos() * t, V * theta.sin() * t - 0.5 * G * t * t)
}
fn flight_time(theta: f64) -> f64 {
    2.0 * V * theta.sin() / G
}
fn max_height(theta: f64) -> f64 {
    (V * theta.sin()).powi(2) / (2.0 * G)
}
fn range(theta: f64) -> f64 {
    V * V * (2.0 * theta).sin() / G
}

fn text(s: &str, size: f64, color: Color) -> VMobject {
    Text::styled(s, TextStyle::default().size(size).color(color))
}

/// Lines stacked downward, left-aligned, with the block's top-left at `top_left`.
fn column(lines: Vec<VMobject>, buff: f64, top_left: Point) -> VMobject {
    let mut g = VMobject::group(lines).arrange_buff(DOWN, buff);
    let first = g.children[0].clone();
    for c in g.children.iter_mut() {
        c.apply(|m| m.align_to(&first, LEFT));
    }
    g.move_to_aligned(top_left, UL)
}

/// Swap the caption at the top of the screen.
fn caption(s: &mut Scene, old: Option<MobjectId>, line: &str) -> MobjectId {
    let new = s.insert(text(line, 34.0, WHITE).to_edge_buff(UP, 0.35));
    match old {
        Some(o) => s.play((FadeOut::new(o).shift(UP * 0.3), FadeIn::new(new).shift(UP * 0.3))),
        None => s.play(FadeIn::new(new).shift(UP * 0.3)),
    }
    new
}

fn fade_out(s: &mut Scene, ids: &[MobjectId]) {
    s.play(ids.iter().map(|&id| FadeOut::new(id).boxed()).collect::<Vec<_>>());
}

/// Trajectory with quadratic air drag (simple Euler integration), in metres.
fn drag_path(theta: f64, k: f64) -> Vec<(f64, f64)> {
    let (mut x, mut y) = (0.0, 0.0);
    let (mut vx, mut vy) = (V * theta.cos(), V * theta.sin());
    let dt = 0.001;
    let mut pts = vec![(x, y)];
    for i in 0.. {
        let speed = (vx * vx + vy * vy).sqrt();
        vx += -k * speed * vx * dt;
        vy += (-G - k * speed * vy) * dt;
        x += vx * dt;
        y += vy * dt;
        if y < 0.0 {
            break;
        }
        if i % 20 == 0 {
            pts.push((x, y));
        }
    }
    pts.push((x, 0.0));
    pts
}

fn main() -> anyhow::Result<()> {
    manim::render("Trajectory", |s| {
        // ------------------------------------------------------------ 1. Title
        let title_m = Text::styled("Trajectories", TextStyle::default().size(84.0).bold().color(BLUE)).shift(UP * 0.4);
        let sub_m = text("the path a thrown object follows", 38.0, GRAY_A).next_to_buff(&title_m, DOWN, 0.4);
        let title = s.insert(title_m);
        let sub = s.insert(sub_m);
        s.play(Write::new(title).run_time(1.5));
        s.play(FadeIn::new(sub).shift(UP * 0.3));
        s.wait(2.0);
        s.play((FadeOut::new(title).shift(UP), FadeOut::new(sub).shift(UP)));

        // ------------------------------------------------------------ 2. Throw a ball
        let axes = Axes::new([0.0, 11.0, 1.0], [0.0, 5.0, 1.0])
            .x_length(10.0)
            .y_length(4.8)
            .move_to(pt(-0.5, -1.15))
            .with_numbers();
        let mut cap = caption(s, None, "Throw a ball, and it traces a curved path");
        let ax = s.insert(&axes);
        let x_label = s.insert(text("distance (m)", 24.0, GRAY_B).next_to_point_buff(axes.c2p(11.0, 0.0), RIGHT, 0.25));
        let y_label = s.insert(text("height (m)", 24.0, GRAY_B).next_to_point_buff(axes.c2p(0.0, 5.0), RIGHT, 0.3));
        s.play(Create::new(ax).run_time(2.0).lag_ratio(0.02));
        s.play((FadeIn::new(x_label), FadeIn::new(y_label)));

        let th = 55.0 * DEGREES;
        let t_end = flight_time(th);
        let tr = s.tracker(0.0);

        let a = axes.clone();
        let path = s.always_redraw(move |w| {
            let t = w.value(tr);
            if t <= 1e-3 {
                return VMobject::new();
            }
            a.plot_parametric(move |u| pos(th, u), [0.0, t]).set_stroke(YELLOW, 5.0)
        });
        let a = axes.clone();
        let ball = s.always_redraw(move |w| {
            let (x, y) = pos(th, w.value(tr));
            Dot::at(a.c2p(x, y)).scale(1.9).set_color(ORANGE).set_z_index(3)
        });
        s.wait(0.5);
        s.play(ChangeValue::new(tr, t_end).run_time(3.0).rate_func(linear));
        let apex = axes.c2p(range(th) / 2.0, max_height(th));
        let traj_label = s.insert(text("trajectory", 32.0, YELLOW).next_to_point_buff(apex, UP, 0.25));
        s.play(Write::new(traj_label));
        s.wait(1.5);
        cap = caption(s, Some(cap), "Every throw follows the same kind of curve. But why?");
        s.wait(2.0);

        // ------------------------------------------------------------ 3. Two motions
        s.play(FadeOut::new(traj_label));
        s.set_value(tr, 0.0);
        cap = caption(s, Some(cap), "Split the launch velocity into two parts");
        let p0 = axes.c2p(0.0, 0.0);
        let (vx0, vy0) = (V * th.cos(), V * th.sin());
        let v_static = s.insert(Arrow::with_buff(p0, p0 + pt(vx0, vy0) * ARROW_SCALE, 0.0).set_color(GREEN));
        s.play(GrowFromPoint::new(v_static, p0));
        s.wait(1.0);
        let vx_static = s.insert(Arrow::with_buff(p0, p0 + RIGHT * vx0 * ARROW_SCALE, 0.0).set_color(BLUE));
        let vy_static = s.insert(Arrow::with_buff(p0, p0 + UP * vy0 * ARROW_SCALE, 0.0).set_color(RED));
        s.play((GrowFromPoint::new(vx_static, p0), GrowFromPoint::new(vy_static, p0)));

        let legend = s.insert(column(
            vec![
                text("horizontal: no force, so constant speed", 26.0, BLUE),
                text("vertical: gravity slows it, then pulls it down", 26.0, RED),
            ],
            0.2,
            pt(-0.6, 2.75),
        ));
        s.play(LaggedStart::new(anims![FadeIn::new(legend).shift(UP * 0.2)]));
        s.wait(3.0);

        // Swap the static arrows for live ones that ride along with the ball.
        for id in [v_static, vx_static, vy_static] {
            s.remove(id);
        }
        let live_arrow = |s: &mut Scene, color: Color, comp: fn(f64) -> (f64, f64)| {
            let a = axes.clone();
            s.always_redraw(move |w| {
                let t = w.value(tr);
                let (x, y) = pos(th, t);
                let p = a.c2p(x, y);
                let (dx, dy) = comp(t);
                Arrow::with_buff(p, p + pt(dx, dy) * ARROW_SCALE, 0.0).set_color(color)
            })
        };
        let v_live = live_arrow(s, GREEN, |t| (V * (55.0 * DEGREES).cos(), V * (55.0 * DEGREES).sin() - G * t));
        let vx_live = live_arrow(s, BLUE, |_| (V * (55.0 * DEGREES).cos(), 0.0));
        let vy_live = live_arrow(s, RED, |t| (0.0, V * (55.0 * DEGREES).sin() - G * t));
        s.play(ChangeValue::new(tr, t_end / 2.0).run_time(2.5).rate_func(linear));
        let top_note =
            s.insert(text("at the top, vertical speed is zero", 26.0, RED).next_to_point_buff(apex, UP, 0.3));
        s.play(FadeIn::new(top_note).shift(UP * 0.2));
        s.wait(2.0);
        s.play(FadeOut::new(top_note));
        s.play(ChangeValue::new(tr, t_end).run_time(2.5).rate_func(linear));
        s.wait(1.0);
        cap = caption(s, Some(cap), "The blue arrow never changes. Only the red one does.");
        s.wait(2.5);

        // ------------------------------------------------------------ 4. Equations
        s.clear_updaters();
        let graph_parts = [ax, x_label, y_label, path, ball, v_live, vx_live, vy_live, legend];
        fade_out(s, &graph_parts);
        for id in [v_live, vx_live, vy_live] {
            s.remove(id);
        }
        cap = caption(s, Some(cap), "Each part follows a simple rule");
        let eq_x = s.insert(text("x(t) = v · cos θ · t", 40.0, BLUE).move_to(pt(0.0, 1.8)));
        let eq_y = s.insert(text("y(t) = v · sin θ · t  −  ½ · g · t²", 40.0, RED).move_to(pt(0.0, 0.9)));
        let eq_g = s.insert(text("g = 9.8 m/s²  (gravity)", 28.0, GRAY_B).move_to(pt(0.0, 0.15)));
        s.play(Write::new(eq_x));
        s.wait(1.5);
        s.play(Write::new(eq_y));
        s.play(FadeIn::new(eq_g));
        s.wait(2.5);
        let elim = s.insert(text("eliminate t", 28.0, GRAY_B).move_to(pt(0.0, -0.75)));
        let eq_path = s.insert(text("y = x · tan θ  −  g · x² / (2 v² cos² θ)", 42.0, YELLOW).move_to(pt(0.0, -1.6)));
        s.play(FadeIn::new(elim).shift(DOWN * 0.2));
        s.play(Write::new(eq_path).run_time(2.0));
        let parabola_note =
            s.insert(text("a quadratic in x, so the path is a parabola", 30.0, WHITE).move_to(pt(0.0, -2.6)));
        s.play(FadeIn::new(parabola_note).shift(UP * 0.2));
        s.play(Indicate::with(eq_path, 1.08, YELLOW_A).run_time(1.5));
        s.wait(3.0);
        fade_out(s, &[eq_x, eq_y, eq_g, elim, eq_path, parabola_note]);

        // ------------------------------------------------------------ 5. Key numbers
        cap = caption(s, Some(cap), "Three numbers describe every throw");
        let full_path = s.insert(axes.plot_parametric(move |u| pos(th, u), [0.0, t_end]).set_stroke(YELLOW, 5.0));
        s.play((FadeIn::new(ax), FadeIn::new(x_label), FadeIn::new(y_label), FadeIn::new(full_path)));

        let (h, r, t_air) = (max_height(th), range(th), flight_time(th));
        let h_line = s.insert(axes.v_line(r / 2.0, h).set_color(GREEN));
        let h_text = s.insert(text(&format!("max height  H = v² sin² θ / 2g  ≈ {h:.1} m"), 26.0, GREEN));
        let r_arrow = s.insert(DoubleArrow::new(axes.c2p(0.0, 0.35), axes.c2p(r, 0.35)).set_color(BLUE));
        let r_text = s.insert(text(&format!("range  R = v² sin 2θ / g  ≈ {r:.1} m"), 26.0, BLUE));
        let t_text = s.insert(text(&format!("time in the air  T = 2v sin θ / g  ≈ {t_air:.1} s"), 26.0, PURPLE_A));
        let block =
            column(vec![s.get(h_text).clone(), s.get(r_text).clone(), s.get(t_text).clone()], 0.22, pt(-0.9, 2.75));
        for (id, m) in [h_text, r_text, t_text].into_iter().zip(block.children) {
            s.modify(id, |_| m);
        }
        s.play(Create::new(h_line));
        s.play(FadeIn::new(h_text).shift(RIGHT * 0.2));
        s.wait(2.0);
        s.play(GrowFromCenter::new(r_arrow));
        s.play(FadeIn::new(r_text).shift(RIGHT * 0.2));
        s.wait(2.0);
        s.play(FadeIn::new(t_text).shift(RIGHT * 0.2));
        s.wait(1.5);
        let v_note = s.insert(
            text("(launch speed v = 10 m/s, angle θ = 55°)", 24.0, GRAY_B)
                .next_to_buff(s.get(t_text), DOWN, 0.25)
                .align_to(s.get(t_text), LEFT),
        );
        s.play(FadeIn::new(v_note));
        s.wait(3.0);
        fade_out(s, &[h_line, h_text, r_arrow, r_text, t_text, v_note, full_path]);

        // ------------------------------------------------------------ 6. Launch angle
        cap = caption(s, Some(cap), "Same speed, different launch angles");
        let angles = [15.0, 30.0, 45.0, 60.0, 75.0];
        let colors = color_gradient(&[TEAL, BLUE, YELLOW, RED, PURPLE], angles.len());
        let mut curves = Vec::new();
        let mut labels = Vec::new();
        for (deg, col) in angles.iter().zip(&colors) {
            let th = deg * DEGREES;
            let c = axes.plot_parametric(move |u| pos(th, u), [0.0, flight_time(th)]).set_stroke(*col, 4.0);
            curves.push(s.insert(c));
            let top = axes.c2p(range(th) / 2.0, max_height(th));
            labels.push(s.insert(text(&format!("{deg:.0}°"), 24.0, *col).next_to_point_buff(top, UP, 0.12)));
        }
        s.play(
            LaggedStart::new(curves.iter().map(|&c| Create::new(c).run_time(1.5).boxed()).collect::<Vec<_>>())
                .lag_ratio(0.35),
        );
        s.play(labels.iter().map(|&l| FadeIn::new(l).boxed()).collect::<Vec<_>>());
        s.wait(2.0);

        cap = caption(s, Some(cap), "45° goes the farthest (when there is no air resistance)");
        s.play((
            curves[2].animate(|m| m.set_stroke_width(9.0)),
            curves[0].animate(|m| m.fade(0.6)),
            curves[1].animate(|m| m.fade(0.6)),
            curves[3].animate(|m| m.fade(0.6)),
            curves[4].animate(|m| m.fade(0.6)),
        ));
        let best = s.insert(Dot::at(axes.c2p(range(45.0 * DEGREES), 0.0)).scale(1.6).set_color(YELLOW));
        s.play(GrowFromCenter::new(best));
        s.play(Indicate::new(best));
        s.wait(2.0);

        cap = caption(s, Some(cap), "30° and 60° land in exactly the same spot");
        s.play((
            curves[2].animate(|m| m.set_stroke_width(4.0).fade(0.6)),
            curves[1].animate(|m| m.set_opacity(1.0).set_stroke_width(8.0).set_fill_opacity(0.0)),
            curves[3].animate(|m| m.set_opacity(1.0).set_stroke_width(8.0).set_fill_opacity(0.0)),
            FadeOut::new(best),
        ));
        let tie = s.insert(Dot::at(axes.c2p(range(30.0 * DEGREES), 0.0)).scale(1.6).set_color(WHITE));
        s.play(GrowFromCenter::new(tie));
        let tie_note = s.insert(column(
            vec![text("sin(2 · 30°) = sin(2 · 60°)", 26.0, GRAY_A), text("so their ranges are equal", 26.0, GRAY_A)],
            0.15,
            axes.c2p(6.6, 4.9),
        ));
        s.play(FadeIn::new(tie_note).shift(UP * 0.2));
        s.wait(3.5);

        // ------------------------------------------------------------ 7. Air resistance
        let mut to_clear: Vec<MobjectId> = curves.iter().chain(&labels).copied().filter(|&c| c != curves[2]).collect();
        to_clear.extend([tie, tie_note]);
        fade_out(s, &to_clear);
        cap = caption(s, Some(cap), "Real air pushes back on the ball");
        s.play(curves[2].animate(|m| m.set_stroke(YELLOW, 5.0).set_opacity(1.0).set_fill_opacity(0.0)));
        let drag_pts: Vec<Point> = drag_path(45.0 * DEGREES, 0.06).into_iter().map(|(x, y)| axes.c2p(x, y)).collect();
        let drag = s.insert(
            VMobject::from_subpaths(vec![Subpath::new(bezier::smooth_through(&drag_pts), false)])
                .set_stroke(ORANGE, 5.0),
        );
        s.play(Create::new(drag).run_time(2.5));
        let drag_note = s.insert(column(
            vec![
                text("no air: symmetric parabola", 26.0, YELLOW),
                text("with drag: shorter, lower, and it falls more steeply", 26.0, ORANGE),
            ],
            0.2,
            pt(-0.6, 2.75),
        ));
        s.play(FadeIn::new(drag_note).shift(UP * 0.2));
        s.wait(4.0);

        // ------------------------------------------------------------ 8. Summary
        fade_out(s, &[curves[2], labels[2], drag, drag_note, ax, x_label, y_label]);
        cap = caption(s, Some(cap), "Summary");
        let lines: Vec<VMobject> = [
            ("Horizontal motion: constant speed", BLUE),
            ("Vertical motion: constant pull of gravity, g = 9.8 m/s²", RED),
            ("Together they make a parabola", YELLOW),
            ("Height, range and time in the air follow from v and θ", GREEN),
            ("45° gives the longest range (ignoring air)", WHITE),
        ]
        .iter()
        .map(|(t, c)| text(&format!("•  {t}"), 32.0, *c))
        .collect();
        let bullets: Vec<MobjectId> =
            column(lines, 0.35, pt(-5.6, 1.9)).children.into_iter().map(|m| s.insert(m)).collect();
        s.play(
            LaggedStart::new(bullets.iter().map(|&b| FadeIn::new(b).shift(RIGHT * 0.3).boxed()).collect::<Vec<_>>())
                .lag_ratio(0.5)
                .run_time(4.0),
        );
        s.wait(5.0);
        let mut all = bullets.clone();
        all.push(cap);
        fade_out(s, &all);
        s.wait(0.5);
    })
}
