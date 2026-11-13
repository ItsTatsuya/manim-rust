#![allow(dead_code)]

use std::path::PathBuf;

use manim::prelude::*;

/// Small, fast, deterministic settings: tiny-skia renderer, frames rendered but discarded.
pub fn test_config(name: &str) -> Config {
    Config {
        pixel_width: 160,
        pixel_height: 90,
        fps: 10.0,
        renderer: RendererKind::Skia,
        format: OutputFormat::None,
        media_dir: temp_dir(name),
        quiet: true,
        ..Config::default()
    }
}

pub fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("manim-rs-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Run a scene body with the test config; panics inside the body fail the test.
pub fn run(name: &str, f: impl FnOnce(&mut Scene)) {
    render_with(&test_config(name), name, f).expect("scene failed");
}

pub fn close(a: Point, b: Point) -> bool {
    a.distance(b) < 1e-6
}

pub fn assert_close(a: Point, b: Point) {
    assert!(close(a, b), "expected {b:?}, got {a:?}");
}

pub fn all_points(m: &VMobject) -> Vec<Point> {
    m.family()
        .iter()
        .flat_map(|f| f.subpaths.iter())
        .flat_map(|s| s.curves.iter())
        .flat_map(|c| c.iter().copied())
        .collect()
}

pub fn same_points(a: &VMobject, b: &VMobject) -> bool {
    let (pa, pb) = (all_points(a), all_points(b));
    pa.len() == pb.len() && pa.iter().zip(&pb).all(|(x, y)| x.distance(*y) < 1e-6)
}

#[cfg(feature = "text")]
pub fn fonts_available() -> bool {
    !Text::new("A").children.is_empty()
}

/// Tiny deterministic RNG for randomized tests (no extra dependency).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn f(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn usize(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
    pub fn point(&mut self, r: f64) -> Point {
        pt(self.f(-r, r), self.f(-r, r))
    }
    pub fn color(&mut self) -> Color {
        [RED, BLUE, GREEN, YELLOW, PURPLE, TEAL, WHITE, ORANGE][self.usize(8)]
    }

    /// A random mobject from the shape library, randomly transformed and styled.
    pub fn mobject(&mut self) -> VMobject {
        let m = match self.usize(16) {
            0 => Circle::new(self.f(0.05, 3.0)),
            1 => Square::new(self.f(0.05, 3.0)),
            2 => Rectangle::new(self.f(0.01, 4.0), self.f(0.01, 4.0)),
            3 => RegularPolygon::with_radius(self.usize(9), self.f(0.1, 2.0)),
            4 => Star::new(2 + self.usize(8), self.f(0.5, 2.0), self.f(0.1, 0.5)),
            5 => Line::new(self.point(5.0), self.point(5.0)),
            6 => Arrow::new(self.point(5.0), self.point(5.0)),
            7 => DashedLine::new(self.point(5.0), self.point(5.0)),
            8 => Arc::new(self.f(0.1, 2.0), self.f(-6.0, 6.0), self.f(-7.0, 7.0)),
            9 => Annulus::new(self.f(0.1, 0.5), self.f(0.6, 2.0)),
            10 => RoundedRectangle::new(self.f(0.2, 3.0), self.f(0.2, 3.0), self.f(0.0, 1.0)),
            11 => Dot::at(self.point(4.0)),
            12 => DoubleArrow::new(self.point(5.0), self.point(5.0)),
            13 => vgroup![Circle::new(0.5), Square::new(0.5), Triangle::new()].arrange(RIGHT),
            14 => FunctionGraph::new(|x| (x * 1.7).sin(), [-3.0, 3.0]),
            _ => VGroup::new(vec![]),
        };
        m.shift(self.point(3.0))
            .rotate(self.f(-6.0, 6.0))
            .scale(self.f(0.1, 2.0))
            .set_fill(self.color(), self.f(0.0, 1.0))
            .set_stroke(self.color(), self.f(0.0, 8.0))
    }
}
