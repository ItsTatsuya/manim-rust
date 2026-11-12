//! The classic Manim quickstart.
//! cargo run --release --example square_to_circle -- -ql

use manim::prelude::*;

fn main() -> anyhow::Result<()> {
    manim::render("SquareToCircle", |s| {
        let circle = Circle::new(1.0).set_fill(PINK, 0.5);
        let square = s.add(Square::new(2.0).rotate(PI / 4.0));

        s.play(Create::new(square));
        s.play(Transform::new(square, circle));
        s.play(square.animate(|m| m.shift(RIGHT * 2.0).set_color(BLUE)));
        s.play(FadeOut::new(square).shift(DOWN));
        s.wait(0.5);
    })
}
