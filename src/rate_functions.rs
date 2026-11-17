//! Rate functions (easing), matching `manim.utils.rate_functions`.

use std::f64::consts::PI;

/// A rate function: maps animation progress `t` in `[0, 1]` to eased progress.
pub type RateFunc = fn(f64) -> f64;

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// No easing: returns `t`.
pub fn linear(t: f64) -> f64 {
    t
}

/// Manim's default: sigmoid-based smoothstep with inflection 10.
pub fn smooth(t: f64) -> f64 {
    const INFLECTION: f64 = 10.0;
    let error = sigmoid(-INFLECTION / 2.0);
    ((sigmoid(INFLECTION * (t - 0.5)) - error) / (1.0 - 2.0 * error)).clamp(0.0, 1.0)
}

/// Classic cubic smoothstep `3tÂ² âˆ’ 2tÂ³` (input clamped to `[0, 1]`).
pub fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Starts slow, ends fast.
pub fn rush_into(t: f64) -> f64 {
    2.0 * smooth(t / 2.0)
}

/// Starts fast, ends slow.
pub fn rush_from(t: f64) -> f64 {
    2.0 * smooth(t / 2.0 + 0.5) - 1.0
}

/// Fast start that settles gently at the end (quarter-circle curve).
pub fn slow_into(t: f64) -> f64 {
    (1.0 - (1.0 - t) * (1.0 - t)).max(0.0).sqrt()
}

/// Two back-to-back [`smooth`] easings, each over half the duration.
pub fn double_smooth(t: f64) -> f64 {
    if t < 0.5 {
        0.5 * smooth(2.0 * t)
    } else {
        0.5 * (1.0 + smooth(2.0 * t - 1.0))
    }
}

/// Goes from 0 to 1 and back to 0 with [`smooth`] easing.
pub fn there_and_back(t: f64) -> f64 {
    smooth(if t < 0.5 { 2.0 * t } else { 2.0 * (1.0 - t) })
}

/// Like [`there_and_back`], holding at 1 for the middle third.
pub fn there_and_back_with_pause(t: f64) -> f64 {
    let a = 1.0 / 3.0;
    if t < a {
        smooth(t / a)
    } else if t < 1.0 - a {
        1.0
    } else {
        smooth((1.0 - t) / a)
    }
}

/// Oscillates around 0, ending at 0, with the amplitude shaped by [`there_and_back`].
pub fn wiggle(t: f64) -> f64 {
    there_and_back(t) * (2.0 * PI * t).sin()
}

/// Linear to the end over the first 80%, then holds (Manim's `squish_rate_func(linear, 0, 0.8)`).
pub fn lingering(t: f64) -> f64 {
    (t / 0.8).clamp(0.0, 1.0)
}

/// Fast rise that flattens towards 1 (`1 âˆ’ e^(âˆ’10t)`).
pub fn exponential_decay(t: f64) -> f64 {
    1.0 - (-t / 0.1).exp()
}

/// Sine ease-in.
pub fn ease_in_sine(t: f64) -> f64 {
    1.0 - (t * PI / 2.0).cos()
}
/// Sine ease-out.
pub fn ease_out_sine(t: f64) -> f64 {
    (t * PI / 2.0).sin()
}
/// Sine ease-in-out.
pub fn ease_in_out_sine(t: f64) -> f64 {
    -((PI * t).cos() - 1.0) / 2.0
}
/// Quadratic ease-in.
pub fn ease_in_quad(t: f64) -> f64 {
    t * t
}
/// Quadratic ease-out.
pub fn ease_out_quad(t: f64) -> f64 {
    1.0 - (1.0 - t) * (1.0 - t)
}
/// Quadratic ease-in-out.
pub fn ease_in_out_quad(t: f64) -> f64 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}
/// Cubic ease-in.
pub fn ease_in_cubic(t: f64) -> f64 {
    t * t * t
}
/// Cubic ease-out.
pub fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}
/// Cubic ease-in-out.
pub fn ease_in_out_cubic(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}
/// Overshoots 1 slightly (a "back" ease) before settling.
pub fn ease_out_back(t: f64) -> f64 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}
/// Bounces against 1 before settling there.
pub fn ease_out_bounce(t: f64) -> f64 {
    let (n1, d1) = (7.5625, 2.75);
    if t < 1.0 / d1 {
        n1 * t * t
    } else if t < 2.0 / d1 {
        let t = t - 1.5 / d1;
        n1 * t * t + 0.75
    } else if t < 2.5 / d1 {
        let t = t - 2.25 / d1;
        n1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / d1;
        n1 * t * t + 0.984375
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints() {
        for f in [smooth, linear, rush_into, rush_from, double_smooth, ease_in_out_cubic] {
            assert!(f(0.0).abs() < 1e-9);
            assert!((f(1.0) - 1.0).abs() < 1e-9);
        }
        assert!(there_and_back(1.0).abs() < 1e-9);
        assert!((there_and_back(0.5) - 1.0).abs() < 1e-9);
    }
}
