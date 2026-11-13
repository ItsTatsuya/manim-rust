//! SVG import (Manim's `SVGMobject`): every path becomes a child [`VMobject`] with its
//! fill and stroke. Text in the SVG is converted to outlines using the system fonts.

#![allow(clippy::new_ret_no_self)]

use std::path::Path;

use anyhow::{Context, Result};
use usvg::tiny_skia_path::PathSegment;

use crate::bezier::{self, Cubic};
use crate::color::Color;
use crate::constants::*;
use crate::mobject::{Subpath, VMobject};

/// Stroke-width unit: SVG user units → Manim stroke width (1 = 0.01 scene units).
const STROKE_UNITS: f64 = 100.0;

pub struct SVGMobject;

impl SVGMobject {
    /// Load an SVG file, scaled to 2 units tall (Manim's default) and centered.
    pub fn open(path: impl AsRef<Path>) -> Result<VMobject> {
        let path = path.as_ref();
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Ok(Self::from_data(&data)
            .with_context(|| format!("in {}", path.display()))?
            .with_label(&path.display().to_string()))
    }

    /// Parse SVG text, scaled to 2 units tall and centered.
    #[allow(clippy::should_implement_trait)] // returns a VMobject, not Self
    pub fn from_str(svg: &str) -> Result<VMobject> {
        Self::from_data(svg.as_bytes())
    }

    pub fn from_data(data: &[u8]) -> Result<VMobject> {
        Self::from_data_height(data, 2.0)
    }

    /// Parse SVG scaled to `height` scene units (stroke widths scale along).
    pub fn from_data_height(data: &[u8], height: f64) -> Result<VMobject> {
        let tree = load(data)?;
        let h = tree.root().abs_bounding_box().map(|b| b.height() as f64).filter(|h| *h > 0.0);
        let scale = h.map(|h| height / h).unwrap_or(1.0 / 100.0);
        Ok(from_tree(&tree, scale).center().with_kind("SVG"))
    }
}

/// Parse SVG into a group; one SVG user unit becomes `scale` scene units (no centering).
pub fn parse(data: &[u8], scale: f64) -> Result<VMobject> {
    Ok(from_tree(&load(data)?, scale))
}

fn load(data: &[u8]) -> Result<usvg::Tree> {
    let mut opt = usvg::Options::default();
    // Only pay for the system font scan when the SVG actually has text, and only once.
    if data.windows(5).any(|w| w == b"<text") {
        opt.fontdb = svg_fonts();
    }
    usvg::Tree::from_data(data, &opt).context("parsing SVG")
}

static SVG_FONTS: std::sync::Mutex<Option<std::sync::Arc<usvg::fontdb::Database>>> = std::sync::Mutex::new(None);

/// System fonts plus fonts registered through `text::register_font_*`, scanned once.
fn svg_fonts() -> std::sync::Arc<usvg::fontdb::Database> {
    let mut g = SVG_FONTS.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        std::sync::Arc::new(db)
    })
    .clone()
}

/// Make a registered font available to SVG `<text>` too.
#[allow(dead_code)]
pub(crate) fn register_font_data(bytes: Vec<u8>) {
    let mut db = svg_fonts();
    std::sync::Arc::make_mut(&mut db).load_font_data(bytes);
    *SVG_FONTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(db);
}

fn from_tree(tree: &usvg::Tree, scale: f64) -> VMobject {
    let mut children = Vec::new();
    walk(tree.root(), scale, 1.0, &mut children);
    VMobject::group(children)
}

fn color(c: usvg::Color) -> Color {
    Color::rgb(c.red as f32 / 255.0, c.green as f32 / 255.0, c.blue as f32 / 255.0)
}

fn walk(group: &usvg::Group, scale: f64, opacity: f64, out: &mut Vec<VMobject>) {
    let opacity = opacity * group.opacity().get() as f64;
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => walk(g, scale, opacity, out),
            usvg::Node::Text(t) => walk(t.flattened(), scale, opacity, out),
            usvg::Node::Path(p) => {
                if p.is_visible() {
                    if let Some(m) = path(p, scale, opacity) {
                        out.push(m);
                    }
                }
            }
            usvg::Node::Image(_) => {}
        }
    }
}

fn path(p: &usvg::Path, scale: f64, opacity: f64) -> Option<VMobject> {
    let ts = p.abs_transform();
    let map = |x: f32, y: f32| -> Point {
        let (x, y) = (x as f64, y as f64);
        let nx = ts.sx as f64 * x + ts.kx as f64 * y + ts.tx as f64;
        let ny = ts.ky as f64 * x + ts.sy as f64 * y + ts.ty as f64;
        pt(nx * scale, -ny * scale) // SVG y points down
    };
    let mut subpaths = Vec::new();
    let mut cur: Vec<Cubic> = Vec::new();
    let (mut start, mut last) = (ORIGIN, ORIGIN);
    let flush = |cur: &mut Vec<Cubic>, closed: bool, subpaths: &mut Vec<Subpath>| {
        if !cur.is_empty() {
            subpaths.push(Subpath::new(std::mem::take(cur), closed));
        }
    };
    for seg in p.data().segments() {
        match seg {
            PathSegment::MoveTo(a) => {
                flush(&mut cur, false, &mut subpaths);
                start = map(a.x, a.y);
                last = start;
            }
            PathSegment::LineTo(a) => {
                let q = map(a.x, a.y);
                cur.push(bezier::line(last, q));
                last = q;
            }
            PathSegment::QuadTo(a, b) => {
                let (c, q) = (map(a.x, a.y), map(b.x, b.y));
                cur.push(bezier::from_quadratic(last, c, q));
                last = q;
            }
            PathSegment::CubicTo(a, b, c) => {
                let (c1, c2, q) = (map(a.x, a.y), map(b.x, b.y), map(c.x, c.y));
                cur.push([last, c1, c2, q]);
                last = q;
            }
            PathSegment::Close => {
                if last.distance(start) > 1e-12 {
                    cur.push(bezier::line(last, start));
                }
                last = start;
                flush(&mut cur, true, &mut subpaths);
            }
        }
    }
    flush(&mut cur, false, &mut subpaths);
    if subpaths.is_empty() {
        return None;
    }
    let mut m = VMobject::from_subpaths(subpaths);
    m.fill_opacity = 0.0;
    m.stroke_width = 0.0;
    if let Some(f) = p.fill() {
        if let usvg::Paint::Color(c) = f.paint() {
            m.fill_color = color(*c);
        }
        // Gradients and patterns fall back to their first stop / the default color.
        if let usvg::Paint::LinearGradient(g) = f.paint() {
            if let Some(s) = g.stops().first() {
                m.fill_color = color(s.color());
            }
        }
        if let usvg::Paint::RadialGradient(g) = f.paint() {
            if let Some(s) = g.stops().first() {
                m.fill_color = color(s.color());
            }
        }
        m.fill_opacity = f.opacity().get() as f64 * opacity;
        m.meta.nonzero = matches!(f.rule(), usvg::FillRule::NonZero);
    }
    if let Some(s) = p.stroke() {
        if let usvg::Paint::Color(c) = s.paint() {
            m.stroke_color = color(*c);
        }
        let k = (ts.sx as f64 * ts.sy as f64 - ts.kx as f64 * ts.ky as f64).abs().sqrt();
        m.stroke_width = s.width().get() as f64 * k * scale * STROKE_UNITS;
        m.stroke_opacity = s.opacity().get() as f64 * opacity;
    } else {
        m.stroke_color = m.fill_color;
    }
    Some(m)
}

/// Make a registered font folder available to SVG `<text>` too.
#[allow(dead_code)]
pub(crate) fn register_font_dir(path: &std::path::Path) {
    let mut db = svg_fonts();
    std::sync::Arc::make_mut(&mut db).load_fonts_dir(path);
    *SVG_FONTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(db);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shapes() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50">
            <rect x="0" y="0" width="40" height="50" fill="#ff0000"/>
            <circle cx="75" cy="25" r="20" fill="none" stroke="#0000ff" stroke-width="4"/>
        </svg>"##;
        let m = SVGMobject::from_str(svg).unwrap();
        assert_eq!(m.children.len(), 2);
        assert!((m.height() - 2.0).abs() < 1e-6);
        assert_eq!(m.children[0].fill_color, Color::rgb(1.0, 0.0, 0.0));
        assert!(m.children[0].fill_opacity > 0.99);
        assert!(m.children[1].stroke_width > 0.0 && m.children[1].fill_opacity == 0.0);
        // Red rect is on the left, so after the y flip it's still left of the circle.
        assert!(m.children[0].get_center().x < m.children[1].get_center().x);
    }
}
