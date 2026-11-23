//! Card styling beyond plain fill and stroke: dash patterns, linear gradients, soft drop
//! shadows, background blur, screen-pinned mobjects and clip rectangles.
//!
//! Everything here is style data on a [`VMobject`] (see [`Effects`]). The [`Camera`](crate::camera::Camera)
//! resolves it into pixel-space data on the [`DrawPath`](crate::renderer::DrawPath)s of a frame.
//! Frames that use gradients, shadows, blur or clips are drawn by the tiny-skia rasterizer (the
//! GPU renderer hands them to the CPU), so every renderer produces the same pixels for them.

use crate::color::Color;
use crate::constants::Point;
use crate::mobject::VMobject;

/// A dash pattern along a stroke, in scene units. A dash is `length` long and followed by a
/// `gap`; `phase` slides the pattern along the path (animating it makes dashes march).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dash {
    /// Length of one dash.
    pub length: f64,
    /// Space between dashes.
    pub gap: f64,
    /// How far the pattern is shifted along the path, in scene units.
    pub phase: f64,
}

/// One colour stop of a [`Gradient`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    /// Position along the gradient line, `0` to `1`.
    pub at: f64,
    /// Colour at that position; its alpha is kept (and multiplied by the mobject's fill opacity).
    pub color: Color,
}

/// A linear gradient that replaces the fill colour. It is laid over the mobject's bounding box,
/// so it follows the mobject when it moves or scales.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    /// Direction of the gradient in radians, counter-clockwise from "towards the right"
    /// (CSS `135deg`, towards the bottom right, is `-PI / 4`).
    pub angle: f64,
    /// Colour stops, ordered by position (at least two).
    pub stops: Vec<GradientStop>,
}

impl Gradient {
    /// A gradient from `from` to `to` in direction `angle`.
    pub fn new(angle: f64, from: Color, to: Color) -> Self {
        Gradient { angle, stops: vec![GradientStop { at: 0.0, color: from }, GradientStop { at: 1.0, color: to }] }
    }

    fn lerp(&self, other: &Gradient, t: f64) -> Gradient {
        if self.stops.len() != other.stops.len() {
            return if t >= 0.5 { other.clone() } else { self.clone() };
        }
        Gradient {
            angle: self.angle + (other.angle - self.angle) * t,
            stops: self
                .stops
                .iter()
                .zip(&other.stops)
                .map(|(a, b)| GradientStop { at: a.at + (b.at - a.at) * t, color: a.color.lerp(b.color, t) })
                .collect(),
        }
    }
}

/// A soft drop shadow, drawn outside the mobject's outline (like CSS `box-shadow`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    /// Shadow colour; its alpha is the shadow's strength (and is multiplied by the mobject's fill opacity).
    pub color: Color,
    /// Blur radius (standard deviation) in scene units.
    pub blur: f64,
    /// Horizontal offset in scene units (right is positive).
    pub dx: f64,
    /// Vertical offset in scene units (up is positive).
    pub dy: f64,
}

/// Style data for the effects in this module. All off by default.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Effects {
    /// Dash pattern for the stroke.
    pub dash: Option<Dash>,
    /// Gradient fill, replacing the fill colour.
    pub gradient: Option<Gradient>,
    /// Drop shadow.
    pub shadow: Option<Shadow>,
    /// Blur (standard deviation, scene units) applied to everything drawn below this mobject, inside its outline.
    pub backdrop_blur: f64,
    /// Drawn in the scene's base frame, ignoring camera moves (work columns, watermarks, card chrome).
    /// [`VMobject::pin`] sets it on the whole family, so children added later are not pinned.
    pub pinned: bool,
    /// Clip this mobject and its family to a rectangle `[left, bottom, right, top]` of the base frame,
    /// so content zoomed by the camera does not spill out of its panel. [`VMobject::clip_to`] sets it on the
    /// whole family, so children added later are not clipped.
    pub clip: Option<[f64; 4]>,
}

impl Effects {
    /// Whether every effect is off.
    pub fn is_none(&self) -> bool {
        *self == Effects::default()
    }

    /// Interpolate numeric parts; discrete ones switch at the end.
    pub(crate) fn lerp(&self, other: &Effects, t: f64) -> Effects {
        let l = |a: f64, b: f64| a + (b - a) * t;
        let pick = t >= 1.0;
        Effects {
            dash: match (self.dash, other.dash) {
                (Some(a), Some(b)) => {
                    Some(Dash { length: l(a.length, b.length), gap: l(a.gap, b.gap), phase: l(a.phase, b.phase) })
                }
                (a, b) => {
                    if pick {
                        b
                    } else {
                        a
                    }
                }
            },
            gradient: match (&self.gradient, &other.gradient) {
                (Some(a), Some(b)) => Some(a.lerp(b, t)),
                (a, b) => {
                    if pick {
                        b.clone()
                    } else {
                        a.clone()
                    }
                }
            },
            shadow: match (self.shadow, other.shadow) {
                (Some(a), Some(b)) => Some(Shadow {
                    color: a.color.lerp(b.color, t),
                    blur: l(a.blur, b.blur),
                    dx: l(a.dx, b.dx),
                    dy: l(a.dy, b.dy),
                }),
                // A shadow that appears or disappears fades in or out.
                (Some(a), None) => Some(Shadow { color: scale_alpha(a.color, 1.0 - t), ..a }).filter(|_| !pick),
                (None, Some(b)) => Some(Shadow { color: scale_alpha(b.color, t), ..b }),
                (None, None) => None,
            },
            backdrop_blur: l(self.backdrop_blur, other.backdrop_blur),
            pinned: if pick { other.pinned } else { self.pinned },
            clip: if pick { other.clip } else { self.clip },
        }
    }
}

fn scale_alpha(mut c: Color, k: f64) -> Color {
    c.a *= k.clamp(0.0, 1.0) as f32;
    c
}

impl VMobject {
    /// Dash the stroke of the whole family: dashes `length` long with `gap` between them, in scene
    /// units. Dashes are cut from the path at draw time, so `Create` and partial paths draw a
    /// dashed line on, and animating `phase` (see [`Dash`]) makes the dashes march.
    pub fn set_dash(self, length: f64, gap: f64, phase: f64) -> Self {
        let dash = (length.is_finite() && gap.is_finite() && length > 0.0 && gap >= 0.0).then_some(Dash {
            length,
            gap,
            phase: if phase.is_finite() { phase } else { 0.0 },
        });
        self.map_style(|m| m.effects.dash = dash)
    }

    /// Remove the dash pattern of the whole family.
    pub fn clear_dash(self) -> Self {
        self.map_style(|m| m.effects.dash = None)
    }

    /// Fill the whole family with a linear gradient (see [`Gradient`]).
    pub fn set_gradient(self, gradient: Gradient) -> Self {
        let ok = gradient.stops.len() >= 2 && gradient.angle.is_finite();
        self.map_style(|m| m.effects.gradient = ok.then(|| gradient.clone()))
    }

    /// Give every part of the family a soft drop shadow (see [`Shadow`]).
    pub fn set_shadow(self, shadow: Shadow) -> Self {
        let ok = shadow.blur.is_finite() && shadow.dx.is_finite() && shadow.dy.is_finite() && shadow.blur >= 0.0;
        self.map_style(|m| m.effects.shadow = ok.then_some(shadow))
    }

    /// Remove the drop shadow of the whole family.
    pub fn clear_shadow(self) -> Self {
        self.map_style(|m| m.effects.shadow = None)
    }

    /// Blur everything drawn below each part of the family, inside its outline, by `sigma` scene units.
    pub fn set_backdrop_blur(self, sigma: f64) -> Self {
        let sigma = if sigma.is_finite() { sigma.max(0.0) } else { 0.0 };
        self.map_style(|m| m.effects.backdrop_blur = sigma)
    }

    /// Draw this mobject (and its family) in the scene's base frame, whatever the camera does.
    pub fn pin(self, pinned: bool) -> Self {
        self.map_style(|m| m.effects.pinned = pinned)
    }

    /// Clip this mobject and its family to the rectangle between corners `lo` and `hi`, given in
    /// the base frame (the frame before any camera move).
    pub fn clip_to(self, lo: Point, hi: Point) -> Self {
        let r = [lo.x.min(hi.x), lo.y.min(hi.y), lo.x.max(hi.x), lo.y.max(hi.y)];
        let ok = r.iter().all(|v| v.is_finite());
        self.map_style(|m| m.effects.clip = ok.then_some(r))
    }

    /// Remove the clip rectangle.
    pub fn clear_clip(self) -> Self {
        self.map_style(|m| m.effects.clip = None)
    }
}
