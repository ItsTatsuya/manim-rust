//! Cubic Bézier utilities.

use crate::constants::Point;

pub type Cubic = [Point; 4];

pub fn point_at(c: &Cubic, t: f64) -> Point {
    let u = 1.0 - t;
    c[0] * (u * u * u) + c[1] * (3.0 * u * u * t) + c[2] * (3.0 * u * t * t) + c[3] * (t * t * t)
}

/// Split at `t` with de Casteljau.
pub fn split(c: &Cubic, t: f64) -> (Cubic, Cubic) {
    let p01 = c[0].lerp(c[1], t);
    let p12 = c[1].lerp(c[2], t);
    let p23 = c[2].lerp(c[3], t);
    let p012 = p01.lerp(p12, t);
    let p123 = p12.lerp(p23, t);
    let m = p012.lerp(p123, t);
    ([c[0], p01, p012, m], [m, p123, p23, c[3]])
}

/// The portion of the curve between parameters `a` and `b` (0 ≤ a ≤ b ≤ 1).
pub fn partial(c: &Cubic, a: f64, b: f64) -> Cubic {
    if a <= 0.0 && b >= 1.0 {
        return *c;
    }
    let (left, _) = split(c, b.clamp(0.0, 1.0));
    if a <= 0.0 || b <= 0.0 {
        return left;
    }
    let (_, right) = split(&left, (a / b).clamp(0.0, 1.0));
    right
}

/// Split one curve into `n` equal-parameter pieces.
pub fn subdivide(c: &Cubic, n: usize) -> Vec<Cubic> {
    if n <= 1 {
        return vec![*c];
    }
    (0..n).map(|i| partial(c, i as f64 / n as f64, (i + 1) as f64 / n as f64)).collect()
}

/// A straight line as a cubic.
pub fn line(a: Point, b: Point) -> Cubic {
    [a, a.lerp(b, 1.0 / 3.0), a.lerp(b, 2.0 / 3.0), b]
}

/// Quadratic → cubic degree elevation.
pub fn from_quadratic(a: Point, q: Point, b: Point) -> Cubic {
    [a, a + (q - a) * (2.0 / 3.0), b + (q - b) * (2.0 / 3.0), b]
}

/// Approximate arc length by sampling.
pub fn length(c: &Cubic) -> f64 {
    const N: usize = 12;
    let mut prev = c[0];
    let mut len = 0.0;
    for i in 1..=N {
        let p = point_at(c, i as f64 / N as f64);
        len += prev.distance(p);
        prev = p;
    }
    len
}

/// Smooth curves through `points` (Catmull-Rom converted to cubics).
pub fn smooth_through(points: &[Point]) -> Vec<Cubic> {
    let n = points.len();
    if n < 2 {
        return Vec::new();
    }
    (0..n - 1)
        .map(|i| {
            let p0 = points[i.saturating_sub(1)];
            let p1 = points[i];
            let p2 = points[i + 1];
            let p3 = points[(i + 2).min(n - 1)];
            [p1, p1 + (p2 - p0) / 6.0, p2 - (p3 - p1) / 6.0, p2]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::pt;

    #[test]
    fn split_endpoints_match() {
        let c = [pt(0., 0.), pt(1., 2.), pt(3., 2.), pt(4., 0.)];
        let (l, r) = split(&c, 0.3);
        assert!(l[3].distance(point_at(&c, 0.3)) < 1e-12);
        assert_eq!(l[0], c[0]);
        assert_eq!(r[3], c[3]);
    }

    #[test]
    fn partial_matches_curve() {
        let c = [pt(0., 0.), pt(1., 2.), pt(3., 2.), pt(4., 0.)];
        let p = partial(&c, 0.25, 0.75);
        assert!(p[0].distance(point_at(&c, 0.25)) < 1e-12);
        assert!(p[3].distance(point_at(&c, 0.75)) < 1e-12);
        assert!(point_at(&p, 0.5).distance(point_at(&c, 0.5)) < 1e-12);
    }

    #[test]
    fn line_length() {
        assert!((length(&line(pt(0., 0.), pt(3., 4.))) - 5.0).abs() < 1e-9);
    }
}
