//! Table, Matrix and Code, laid out like Manim Community's.
//! cargo run --release --example tables -- -s

use manim::prelude::*;

const CODE: &str = "from manim import *

class Squares(Scene):
    def construct(self):
        s = Square(side_length=2)  # a square
        self.play(Create(s), run_time=1.5)
";

fn main() -> anyhow::Result<()> {
    manim::render("Tables", |s| {
        let table = Table::new([["First", "Second"], ["Third", "Fourth"]])
            .row_labels(vec![Text::new("R1"), Text::new("R2")])
            .col_labels(vec![Text::new("C1"), Text::new("C2")])
            .include_outer_lines(true)
            .build()
            .expect("table")
            .add_highlighted_cell(2, 2, GREEN)
            .scale(0.6)
            .to_corner(UL);
        let t = s.add(table);
        s.play(Create::new(t).run_time(1.5));

        let m = Matrix::new([["2", "pi"], ["-1", "1"]]).expect("matrix").build().set_column_colors(&[BLUE, YELLOW]);
        let column = SurroundingRectangle::new(&m.get_columns().children[1]);
        let det = manim::mobject::matrix::get_det_text(&m, Some("2 + pi")).expect("det");
        let ints = Matrix::integers([[1, 0, -3], [4, 12, 6]]).brackets("(", ")").build();
        // The column frame and det(...) are already placed around the matrix.
        let maths = vgroup![vgroup![m, column, det], ints].arrange_buff(DOWN, 0.5).scale(0.8).to_corner(UR);
        let id = s.add(maths);
        s.play(Write::new(id));

        let code = Code::new(CODE)
            .language("python")
            .style("monokai")
            .background(CodeBackground::Window)
            .build()
            .expect("code")
            .scale(0.8)
            .to_corner(DL);
        let c = s.add(code);
        s.play(FadeIn::new(c));
        s.wait(1.0);
    })
}
