//! Stress test: hundreds of moving, semi-transparent shapes. Compare backends with
//!   cargo run --release --example benchmark -- --renderer cpu --format png
//!   cargo run --release --example benchmark -- --renderer gpu --format png

use manim::prelude::*;

fn main() -> anyhow::Result<()> {
    manim::render("Benchmark", |s| {
        let mut ids = Vec::new();
        for i in 0..400 {
            let t = i as f64;
            let x = (t * 0.37).sin() * 6.0;
            let y = (t * 0.61).cos() * 3.3;
            let colors = [BLUE, RED, GREEN, YELLOW, PURPLE, TEAL];
            let m = if i % 2 == 0 { Circle::new(0.3) } else { RegularPolygon::with_radius(5 + i % 4, 0.35) };
            ids.push(s.add(m.set_fill(colors[i % colors.len()], 0.5).move_to(pt(x, y))));
        }
        let anims: Vec<Box<dyn Animation>> = ids
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                Rotate::new(id, TAU * if i % 2 == 0 { 1.0 } else { -1.0 }).about(ORIGIN).run_time(3.0).boxed()
            })
            .collect();
        s.play(anims);
    })
}
