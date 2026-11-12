//! A tour of shapes, groups, text and animation types.
//! cargo run --release --example showcase -- -qm

use manim::prelude::*;

fn main() -> anyhow::Result<()> {
    manim::render("Showcase", |s| {
        // --- Title
        let title = s.insert(Text::new("Manim, in Rust").set_color(BLUE).to_edge(UP));
        s.play(Write::new(title));

        // --- A row of shapes, drawn with a stagger
        let shapes = vgroup![
            Circle::new(0.7).set_fill(RED, 0.6),
            Square::new(1.3).set_fill(GREEN, 0.6),
            Triangle::new().set_fill(YELLOW, 0.6),
            Star::new(5, 0.8, 0.35).set_fill(PURPLE, 0.6),
            RoundedRectangle::new(1.5, 1.1, 0.3).set_fill(TEAL, 0.6),
        ]
        .arrange_buff(RIGHT, 0.5);
        let parts: Vec<MobjectId> = shapes.children.iter().map(|c| s.insert(c.clone())).collect();
        s.play(
            LaggedStart::new(parts.iter().map(|&p| DrawBorderThenFill::new(p).boxed()).collect::<Vec<_>>())
                .lag_ratio(0.2),
        );

        // --- Indicate, rotate, morph along an arc
        s.play(Indicate::new(parts[2]));
        s.play((Rotate::new(parts[3], TAU).run_time(1.5), parts[1].animate(|m| m.rotate(PI / 4.0).scale(1.2))));
        let target = s.get(parts[4]).clone();
        let first = s.get(parts[0]).clone();
        s.play((
            Transform::new(parts[0], target.set_color(ORANGE)).path_arc(PI),
            Transform::new(parts[4], first.set_color(MAROON)).path_arc(PI),
        ));

        // --- Arrow + label, with an updater keeping the label attached
        s.play(parts.iter().map(|&p| FadeOut::new(p).shift(DOWN).boxed()).collect::<Vec<_>>());
        let dot = s.add(Dot::at(LEFT * 3.0).set_color(YELLOW).scale(2.0));
        let label = s.add(Text::sized("moving dot", 28.0));
        s.add_updater(move |w, _| {
            let p = w[dot].get_center();
            w.modify(label, |l| l.next_to_point(p, UP));
        });
        let arrow = s.insert(Arrow::new(LEFT * 3.0 + DOWN * 1.5, RIGHT * 3.0 + DOWN * 1.5));
        s.play(GrowFromPoint::new(arrow, LEFT * 3.0 + DOWN * 1.5));
        s.play(dot.animate(|m| m.shift(RIGHT * 6.0)).run_time(2.0).rate_func(ease_in_out_cubic));
        s.play(
            MoveAlongPath::new(dot, ArcBetweenPoints::new(RIGHT * 3.0, LEFT * 3.0, PI * 0.8))
                .run_time(2.0)
                .rate_func(smooth),
        );
        s.clear_updaters();

        // --- Wrap up
        s.play((FadeOut::new(dot), FadeOut::new(label), Uncreate::new(arrow), FadeOut::new(title).shift(UP)));
        let end = s.insert(Text::styled("Fast.", TextStyle::default().size(96.0).bold().color(GOLD)));
        s.play(SpinInFromNothing::new(end));
        s.wait(1.0);
    })
}
