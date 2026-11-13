//! Manim's `Matrix` (and `IntegerMatrix`, `DecimalMatrix`, `MobjectMatrix`).
//!
//! ```no_run
//! # use manim::prelude::*;
//! let m = Matrix::new([["2", "pi"], ["-1", "1"]]).unwrap().build();
//! let ints = Matrix::integers([[1, 2], [3, 4]]).brackets("(", ")").build();
//! ```
//!
//! Entries are equations (Typst math syntax) with the `math` feature, text without it.
//! The built mobject (kind `"Matrix"`) holds a group for background rectangles (kind
//! `"TableBackgrounds"`, empty unless asked for), the entries group (kind `"MatrixEntries"`,
//! every entry row by row) and the brackets group (kind `"MatrixBrackets"`: left,
//! right). [`GridMobject`] reads and edits it: `get_rows`, `get_columns`,
//! `set_column_colors`, `get_brackets`, ...

#![allow(clippy::new_ret_no_self)]

use anyhow::{bail, Result};

use super::geometry::Brace;
use super::grid::{background_rect, push_background, BACKGROUNDS, BRACKETS};
use super::{Grid, Subpath, VMobject};
use crate::bezier::{self, Cubic};
use crate::color::{Color, BLACK};
use crate::constants::*;
use crate::text::DecimalNumber;

pub use super::grid::GridMobject;

/// `kind` of a matrix's entries group.
pub const ENTRIES: &str = "MatrixEntries";

/// Height of one TeX bracket step; unstretched brackets come in multiples of it.
const BRACKET_HEIGHT: f64 = 0.5977;

/// A matrix of mobjects between brackets.
#[derive(Clone, Debug)]
pub struct Matrix {
    pub entries: Vec<Vec<VMobject>>,
    /// Distance between row anchors (default 0.8).
    pub v_buff: f64,
    /// Distance between column anchors (default 1.3).
    pub h_buff: f64,
    /// Gap between the entries and each bracket (default `MED_SMALL_BUFF`).
    pub bracket_h_buff: f64,
    /// How far the brackets reach above and below the entries (default `MED_SMALL_BUFF`).
    pub bracket_v_buff: f64,
    /// Which corner of each entry sits on its grid anchor (default `DR`: right-aligned).
    pub element_alignment_corner: Point,
    /// `"["`, `"("`, `"{"`, `"|"`, `"||"`, `"<"` (or `\langle`), `"."` for none.
    pub left_bracket: String,
    pub right_bracket: String,
    /// Fit the brackets to the entries' height (default on); off rounds up to TeX's steps.
    pub stretch_brackets: bool,
    /// Bracket color (default: the default text color).
    pub bracket_color: Option<Color>,
    pub add_background_rectangles_to_entries: bool,
    pub include_background_rectangle: bool,
}

impl Matrix {
    /// Equation entries: Typst math with the `math` feature (`"pi"`, `"x^2"`), text otherwise.
    /// Numbers work too: `Matrix::new([[1, 0], [0, 1]])`.
    pub fn new<R, S>(rows: R) -> Result<Self>
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = S>,
        S: ToString,
    {
        let entries = rows
            .into_iter()
            .map(|r| r.into_iter().map(|s| entry(&s.to_string())).collect::<Result<Vec<_>>>())
            .collect::<Result<Vec<_>>>()?;
        Ok(Self::from_mobjects(entries))
    }

    /// Any mobjects as entries (Manim's `MobjectMatrix`).
    pub fn from_mobjects(entries: Vec<Vec<VMobject>>) -> Self {
        Matrix {
            entries,
            v_buff: 0.8,
            h_buff: 1.3,
            bracket_h_buff: MED_SMALL_BUFF,
            bracket_v_buff: MED_SMALL_BUFF,
            element_alignment_corner: DR,
            left_bracket: "[".into(),
            right_bracket: "]".into(),
            stretch_brackets: true,
            bracket_color: None,
            add_background_rectangles_to_entries: false,
            include_background_rectangle: false,
        }
    }

    /// Integer entries (Manim's `IntegerMatrix`).
    pub fn integers<R, I>(rows: R) -> Self
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = I>,
        I: Into<i64>,
    {
        Self::from_mobjects(rows.into_iter().map(|r| r.into_iter().map(|v| ce_integer(v.into())).collect()).collect())
    }

    /// Decimal entries with `places` decimals (Manim's `DecimalMatrix`; Manim uses 1).
    pub fn decimals<R, F>(rows: R, places: usize) -> Self
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = F>,
        F: Into<f64>,
    {
        Self::from_mobjects(
            rows.into_iter().map(|r| r.into_iter().map(|v| ce_decimal(v.into(), places)).collect()).collect(),
        )
    }

    pub fn v_buff(mut self, b: f64) -> Self {
        self.v_buff = b;
        self
    }
    pub fn h_buff(mut self, b: f64) -> Self {
        self.h_buff = b;
        self
    }
    pub fn bracket_h_buff(mut self, b: f64) -> Self {
        self.bracket_h_buff = b;
        self
    }
    pub fn bracket_v_buff(mut self, b: f64) -> Self {
        self.bracket_v_buff = b;
        self
    }
    pub fn element_alignment_corner(mut self, corner: Point) -> Self {
        self.element_alignment_corner = corner;
        self
    }
    /// Bracket pair, e.g. `("(", ")")`, `("{", "}")`, `("|", "|")`.
    pub fn brackets(mut self, left: &str, right: &str) -> Self {
        self.left_bracket = left.into();
        self.right_bracket = right.into();
        self
    }
    pub fn stretch_brackets(mut self, on: bool) -> Self {
        self.stretch_brackets = on;
        self
    }
    pub fn bracket_color(mut self, c: Color) -> Self {
        self.bracket_color = Some(c);
        self
    }
    pub fn add_background_rectangles_to_entries(mut self, on: bool) -> Self {
        self.add_background_rectangles_to_entries = on;
        self
    }
    pub fn include_background_rectangle(mut self, on: bool) -> Self {
        self.include_background_rectangle = on;
        self
    }

    /// Lay the matrix out, centered at the origin. Rows shorter than the longest are
    /// padded with empty entries.
    pub fn build(self) -> VMobject {
        let cols = self.entries.iter().map(Vec::len).max().unwrap_or(0);
        let rows = self.entries.len();
        let mut flat = Vec::with_capacity(rows * cols);
        for (i, row) in self.entries.into_iter().enumerate() {
            let n = row.len();
            for (j, m) in row.into_iter().enumerate() {
                let anchor = pt(j as f64 * self.h_buff, -(i as f64) * self.v_buff);
                flat.push(if m.bounding_box().is_some() {
                    m.move_to_aligned(anchor, self.element_alignment_corner)
                } else {
                    m
                });
            }
            flat.extend((n..cols).map(|_| VMobject::group(vec![])));
        }
        let mut elements = VMobject::group(flat).with_kind(ENTRIES);
        elements.meta.grid =
            Some(Grid { rows, cols, h_buff: self.h_buff, v_buff: self.v_buff, row_labels: false, col_labels: false });
        let color = self.bracket_color.unwrap_or_else(|| crate::defaults::get().text_color);
        let content_h = elements.height();
        let height = if self.stretch_brackets {
            content_h + 2.0 * self.bracket_v_buff
        } else {
            ((content_h / BRACKET_HEIGHT).floor() + 1.0) * BRACKET_HEIGHT
        };
        let left = bracket(&self.left_bracket, height, true, color).next_to_buff(&elements, LEFT, self.bracket_h_buff);
        let right =
            bracket(&self.right_bracket, height, false, color).next_to_buff(&elements, RIGHT, self.bracket_h_buff);
        let backgrounds = VMobject::group(vec![]).with_kind(BACKGROUNDS);
        let mut m =
            VMobject::group(vec![backgrounds, elements, VMobject::group(vec![left, right]).with_kind(BRACKETS)])
                .with_kind("Matrix")
                .center();
        if self.add_background_rectangles_to_entries {
            m = m.add_background_to_entries(BLACK);
        }
        if self.include_background_rectangle {
            let bg = background_rect(&m, BLACK, 0.0);
            push_background(&mut m, bg);
        }
        m
    }
}

/// Height of a digit in a Manim CE `MathTex` at font size 48, measured on its docs images.
const CE_DIGIT_HEIGHT: f64 = 0.333;

/// Font size at which `MathTex` digits are as tall as in Manim CE's matrices and math
/// tables (Manim sets `MathTex` smaller than `Text` at the same font size).
#[cfg(feature = "math")]
pub(crate) fn ce_math_size() -> f64 {
    static SIZE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *SIZE.get_or_init(|| {
        let h = crate::math::MathTex::styled("1", DEFAULT_FONT_SIZE, crate::color::WHITE).map(|m| m.height());
        match h {
            Ok(h) if h > 0.0 => DEFAULT_FONT_SIZE * CE_DIGIT_HEIGHT / h,
            _ => DEFAULT_FONT_SIZE,
        }
    })
}

/// Text style whose digits are as tall as Manim CE's `Integer`/`DecimalNumber` (which
/// are typeset with TeX there).
pub(crate) fn ce_number_style() -> crate::text::TextStyle {
    let style = crate::text::TextStyle::default();
    let h = crate::text::Text::styled("1", style.clone().size(DEFAULT_FONT_SIZE)).height();
    let size = if h > 0.0 { DEFAULT_FONT_SIZE * CE_DIGIT_HEIGHT / h } else { DEFAULT_FONT_SIZE };
    style.size(size)
}

pub(crate) fn ce_integer(v: i64) -> VMobject {
    DecimalNumber::styled(v as f64, 0, ce_number_style()).with_kind("Integer")
}

pub(crate) fn ce_decimal(v: f64, places: usize) -> VMobject {
    DecimalNumber::styled(v, places, ce_number_style())
}

/// A matrix entry: Typst math with the `math` feature, text otherwise; CE-sized.
fn entry(s: &str) -> Result<VMobject> {
    #[cfg(feature = "math")]
    {
        let color = crate::defaults::get().text_color;
        crate::math::MathTex::styled(s, ce_math_size(), color)
    }
    #[cfg(not(feature = "math"))]
    {
        Ok(crate::text::Text::styled(s, ce_number_style()))
    }
}

/// A bracket glyph `height` tall, its middle at the origin: `[`, `(`, `{`, `|`, `||`,
/// `<` (also `\langle`), or `.` (none). `left` picks the opening or closing side.
/// Unknown brackets fall back to `[`.
pub fn bracket(kind: &str, height: f64, left: bool, color: Color) -> VMobject {
    let kind = kind.trim().trim_start_matches("\\left").trim_start_matches("\\right").trim();
    let h = height / 2.0;
    // Proportions measured on Manim CE's TeX delimiters: thin stems, and wider curves
    // the taller the bracket.
    let t = (0.022 + height * 0.002).min(0.04);
    let m = match kind {
        "." | "" => return VMobject::group(vec![]).with_kind("Bracket"),
        "(" | ")" => paren(h, t * 1.8, (0.1 + height * 0.08).min(0.4)),
        "{" | "}" | "\\{" | "\\}" | "\\lbrace" | "\\rbrace" => {
            let b = Brace::shape(height, (0.12 + height * 0.075).min(0.35));
            return b.rotate_about(-PI / 2.0, ORIGIN).set_color(color).with_kind("Bracket").flip_if(!left);
        }
        "|" | "\\vert" | "\\lvert" | "\\rvert" => bar(h, t, 0.0),
        "||" | "\\|" | "\\Vert" | "\\lVert" | "\\rVert" => {
            VMobject::group(vec![bar(h, t, 0.0), bar(h, t, t + 0.06)]).center().set_fill(color, 1.0)
        }
        "<" | ">" | "\\langle" | "\\rangle" | "⟨" | "⟩" => angle(h, t, (0.08 + height * 0.1).min(0.4)),
        _ => square(h, t, (0.1 + height * 0.02).min(0.2)),
    };
    let m = m.set_fill(color, 1.0).set_stroke(color, 0.0).with_kind("Bracket");
    if left {
        m
    } else {
        m.flip()
    }
}

trait FlipIf {
    fn flip_if(self, on: bool) -> Self;
}
impl FlipIf for VMobject {
    fn flip_if(self, on: bool) -> Self {
        if on {
            self.flip()
        } else {
            self
        }
    }
}

fn filled(points: &[Point]) -> VMobject {
    let mut m = VMobject::from_subpaths(vec![Subpath::polyline(points, true)]);
    m.fill_opacity = 1.0;
    m.stroke_width = 0.0;
    m
}

/// `[`: a stem with serifs `w` long, opening to the right.
fn square(h: f64, t: f64, w: f64) -> VMobject {
    filled(&[pt(w, h), pt(0.0, h), pt(0.0, -h), pt(w, -h), pt(w, -h + t), pt(t, -h + t), pt(t, h - t), pt(w, h - t)])
}

/// `|`, `x` from the left edge.
fn bar(h: f64, t: f64, x: f64) -> VMobject {
    filled(&[pt(x, h), pt(x, -h), pt(x + t, -h), pt(x + t, h)])
}

/// `<`: a chevron `w` wide.
fn angle(h: f64, t: f64, w: f64) -> VMobject {
    filled(&[pt(w, h), pt(0.0, 0.0), pt(w, -h), pt(w + t, -h), pt(t, 0.0), pt(w + t, h)])
}

/// `(`: a crescent `w` deep, thickest (`t`) in the middle, tapering to the tips.
fn paren(h: f64, t: f64, w: f64) -> VMobject {
    let tip = t * 0.35;
    // Outer edge top → bottom, inner edge bottom → top.
    let outer: [Cubic; 2] = [
        [pt(w, h), pt(w * 0.25, h * 0.72), pt(0.0, h * 0.4), pt(0.0, 0.0)],
        [pt(0.0, 0.0), pt(0.0, -h * 0.4), pt(w * 0.25, -h * 0.72), pt(w, -h)],
    ];
    let inner: [Cubic; 2] = [
        [pt(w + tip, -h + tip * 0.3), pt(w * 0.25 + t, -h * 0.7), pt(t, -h * 0.4), pt(t, 0.0)],
        [pt(t, 0.0), pt(t, h * 0.4), pt(w * 0.25 + t, h * 0.7), pt(w + tip, h - tip * 0.3)],
    ];
    let mut curves: Vec<Cubic> = outer.to_vec();
    curves.push(bezier::line(pt(w, -h), inner[0][0]));
    curves.extend(inner);
    curves.push(bezier::line(inner[1][3], pt(w, h)));
    let mut m = VMobject::from_subpaths(vec![Subpath::new(curves, true)]);
    m.fill_opacity = 1.0;
    m.stroke_width = 0.0;
    m
}

/// Manim's `get_det_text`: parentheses around `matrix` with "det" (twice the entry size)
/// before them, and `= determinant` after them when given.
pub fn get_det_text(matrix: &VMobject, determinant: Option<&str>) -> Result<VMobject> {
    if matrix.grid().is_none() {
        bail!("get_det_text needs a matrix");
    }
    let color = crate::defaults::get().text_color;
    let h = matrix.height();
    let left = bracket("(", h, true, color).next_to_buff(matrix, LEFT, SMALL_BUFF);
    let right = bracket(")", h, false, color).next_to_buff(matrix, RIGHT, SMALL_BUFF);
    let det = math_text("det", 2.0)?.next_to_buff(&left, LEFT, SMALL_BUFF);
    let mut parts = vec![det, left, right.clone()];
    if let Some(d) = determinant {
        let eq = math_text("=", 1.0)?.next_to_buff(&right, RIGHT, SMALL_BUFF);
        let value = math_text(d, 1.0)?.next_to_buff(&eq, RIGHT, 0.2);
        parts.push(eq);
        parts.push(value);
    }
    Ok(VMobject::group(parts))
}

/// Math (or, without the `math` feature, text) at `scale` times a CE matrix entry's size.
fn math_text(s: &str, scale: f64) -> Result<VMobject> {
    #[cfg(feature = "math")]
    {
        crate::math::MathTex::styled(s, ce_math_size() * scale, crate::defaults::get().text_color)
    }
    #[cfg(not(feature = "math"))]
    {
        let st = ce_number_style();
        let size = st.font_size * scale;
        Ok(crate::text::Text::styled(s, st.size(size)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobject::geometry::Square;

    #[test]
    fn entries_sit_on_anchors() {
        let m = Matrix::from_mobjects(vec![
            vec![Square::new(0.4), Square::new(0.2)],
            vec![Square::new(0.2), Square::new(0.4)],
        ])
        .build();
        let g = m.grid().unwrap();
        assert_eq!((g.rows, g.cols), (2, 2));
        // Bottom-right corners lie on a 1.3 × 0.8 lattice.
        let a = m.get_entry(1, 1).unwrap().get_corner(DR);
        let b = m.get_entry(2, 2).unwrap().get_corner(DR);
        let c = m.get_entry(1, 2).unwrap().get_corner(DR);
        assert!(((b.x - a.x) - 1.3).abs() < 1e-9 && ((a.y - b.y) - 0.8).abs() < 1e-9);
        assert!((c.y - a.y).abs() < 1e-9);
        let br = m.get_brackets();
        assert_eq!(br.children.len(), 2);
        let entries = m.get_entries();
        // Brackets reach bracket_v_buff past the entries and sit bracket_h_buff away.
        assert!((br.children[0].height() - (entries.height() + 0.5)).abs() < 1e-9);
        assert!((entries.get_left().x - br.children[0].get_right().x - 0.25).abs() < 1e-9);
        assert!((br.children[1].get_left().x - entries.get_right().x - 0.25).abs() < 1e-9);
        assert!(m.get_center().length() < 1e-9);
    }

    #[test]
    fn every_bracket_kind_has_the_right_height() {
        for k in ["[", "(", "{", "|", "||", "<", "\\langle", "\\{", "?"] {
            for left in [true, false] {
                let b = bracket(k, 2.0, left, crate::color::WHITE);
                assert!((b.height() - 2.0).abs() < 0.02, "{k}: {}", b.height());
                assert!(b.width() > 0.0 && b.width() < 0.4, "{k}: {}", b.width());
            }
        }
        assert!(bracket(".", 2.0, true, crate::color::WHITE).bounding_box().is_none());
        // `[` opens to the right, `]` to the left.
        let l = bracket("[", 2.0, true, crate::color::WHITE);
        assert!(l.get_corner(UL).x < l.get_corner(UR).x);
    }

    #[test]
    fn row_and_column_colors() {
        let m = Matrix::integers([[1, 2], [3, 4]]).build().set_column_colors(&[crate::color::RED]);
        assert_eq!(m.get_entry(2, 1).unwrap().leaves()[0].fill_color, crate::color::RED);
        assert_ne!(m.get_entry(2, 2).unwrap().leaves()[0].fill_color, crate::color::RED);
        let m = m.set_row_colors(&[crate::color::GREEN, crate::color::BLUE]);
        assert_eq!(m.get_entry(2, 1).unwrap().leaves()[0].fill_color, crate::color::BLUE);
        assert_eq!(m.get_columns().children.len(), 2);
        assert_eq!(m.get_rows().children[1].children.len(), 2);
    }
}
