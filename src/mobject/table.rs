//! Manim's `Table` (and `MathTable`, `IntegerTable`, `DecimalTable`, `MobjectTable`).
//!
//! ```no_run
//! # use manim::prelude::*;
//! let table = Table::new([["First", "Second"], ["Third", "Fourth"]])
//!     .row_labels(vec![Text::new("R1"), Text::new("R2")])
//!     .col_labels(vec![Text::new("C1"), Text::new("C2")])
//!     .build()
//!     .unwrap()
//!     .add_highlighted_cell(2, 2, GREEN);
//! ```
//!
//! The built mobject (kind `"Table"`) holds, in draw order: a group for highlights and
//! background rectangles (kind `"TableBackgrounds"`), the entries group (kind `"TableEntries"`, every entry row by row, labels
//! included), the horizontal lines and the vertical lines. [`GridMobject`] reads and
//! edits it: `get_rows`, `get_columns`, `get_cell`, `set_row_colors`, ...

#![allow(clippy::new_ret_no_self)]

use anyhow::{bail, Result};

use super::geometry::Line;
use super::grid::{background_rect, push_background, BACKGROUNDS, HORIZONTAL_LINES, VERTICAL_LINES};
use super::matrix::{ce_decimal, ce_integer};
use super::{Grid, VMobject};
use crate::color::{Color, BLACK};
use crate::constants::*;
use crate::text::Text;

pub use super::grid::GridMobject;

/// `kind` of a table's entries group.
pub const ENTRIES: &str = "TableEntries";

/// A table of mobjects with optional row and column labels and grid lines.
#[derive(Clone, Debug)]
pub struct Table {
    /// The cells, row by row.
    pub entries: Vec<Vec<VMobject>>,
    /// Labels shown left of each row (empty for none).
    pub row_labels: Vec<VMobject>,
    /// Labels shown above each column (empty for none).
    pub col_labels: Vec<VMobject>,
    /// Shown above the row labels and left of the column labels.
    pub top_left_entry: Option<VMobject>,
    /// Vertical gap between rows (default 0.8).
    pub v_buff: f64,
    /// Horizontal gap between columns (default 1.3).
    pub h_buff: f64,
    /// Lines around the whole table (default off).
    pub include_outer_lines: bool,
    /// Lines between rows and columns (default on).
    pub include_inner_lines: bool,
    /// Grid line color (default: the default mobject color).
    pub line_color: Option<Color>,
    /// Grid line width in stroke units.
    pub line_width: f64,
    /// Put a 75%-opaque rectangle behind each entry.
    pub add_background_rectangles_to_entries: bool,
    /// Color of the per-entry background rectangles.
    pub entries_background_color: Color,
    /// Put a 75%-opaque rectangle behind the whole table.
    pub include_background_rectangle: bool,
    /// Color of the background rectangle behind the whole table.
    pub background_rectangle_color: Color,
}

fn texts<R, S>(rows: R) -> Vec<Vec<VMobject>>
where
    R: IntoIterator,
    R::Item: IntoIterator<Item = S>,
    S: ToString,
{
    rows.into_iter().map(|r| r.into_iter().map(|s| Text::new(&s.to_string())).collect()).collect()
}

impl Table {
    /// Text entries (Manim's `Table`, whose entries are `Paragraph`s; `\n` breaks lines).
    pub fn new<R, S>(rows: R) -> Self
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = S>,
        S: ToString,
    {
        Self::from_mobjects(texts(rows))
    }

    /// Any mobjects as entries (Manim's `MobjectTable`).
    pub fn from_mobjects(entries: Vec<Vec<VMobject>>) -> Self {
        Table {
            entries,
            row_labels: Vec::new(),
            col_labels: Vec::new(),
            top_left_entry: None,
            v_buff: 0.8,
            h_buff: 1.3,
            include_outer_lines: false,
            include_inner_lines: true,
            line_color: None,
            line_width: DEFAULT_STROKE_WIDTH,
            add_background_rectangles_to_entries: false,
            entries_background_color: BLACK,
            include_background_rectangle: false,
            background_rectangle_color: BLACK,
        }
    }

    /// Integer entries (Manim's `IntegerTable`).
    pub fn integers<R, I>(rows: R) -> Self
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = I>,
        I: Into<i64>,
    {
        Self::from_mobjects(rows.into_iter().map(|r| r.into_iter().map(|v| ce_integer(v.into())).collect()).collect())
    }

    /// Decimal entries with `places` decimals (Manim's `DecimalTable`; Manim uses 1).
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

    /// Equation entries in Typst math syntax (Manim's `MathTable`).
    #[cfg(feature = "math")]
    pub fn math<R, S>(rows: R) -> Result<Self>
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = S>,
        S: ToString,
    {
        let entries = rows
            .into_iter()
            .map(|r| {
                r.into_iter()
                    .map(|s| {
                        let color = crate::defaults::get().text_color;
                        crate::math::MathTex::styled(&s.to_string(), super::matrix::ce_math_size(), color)
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self::from_mobjects(entries))
    }

    /// Set the row labels (one per row).
    pub fn row_labels(mut self, labels: Vec<VMobject>) -> Self {
        self.row_labels = labels;
        self
    }
    /// Set the column labels (one per column).
    pub fn col_labels(mut self, labels: Vec<VMobject>) -> Self {
        self.col_labels = labels;
        self
    }
    /// Set the mobject shown in the corner above the row labels and left of the column labels.
    pub fn top_left_entry(mut self, m: VMobject) -> Self {
        self.top_left_entry = Some(m);
        self
    }
    /// Set the vertical gap between rows.
    pub fn v_buff(mut self, b: f64) -> Self {
        self.v_buff = b;
        self
    }
    /// Set the horizontal gap between columns.
    pub fn h_buff(mut self, b: f64) -> Self {
        self.h_buff = b;
        self
    }
    /// Draw (or not) lines around the whole table.
    pub fn include_outer_lines(mut self, on: bool) -> Self {
        self.include_outer_lines = on;
        self
    }
    /// Draw (or not) lines between rows and columns.
    pub fn include_inner_lines(mut self, on: bool) -> Self {
        self.include_inner_lines = on;
        self
    }
    /// Set the grid line color.
    pub fn line_color(mut self, c: Color) -> Self {
        self.line_color = Some(c);
        self
    }
    /// Set the grid line width in stroke units.
    pub fn line_width(mut self, w: f64) -> Self {
        self.line_width = w;
        self
    }
    /// Put a translucent rectangle of `color` behind each entry.
    pub fn add_background_rectangles_to_entries(mut self, color: Color) -> Self {
        self.add_background_rectangles_to_entries = true;
        self.entries_background_color = color;
        self
    }
    /// Put a translucent rectangle of `color` behind the whole table.
    pub fn include_background_rectangle(mut self, color: Color) -> Self {
        self.include_background_rectangle = true;
        self.background_rectangle_color = color;
        self
    }

    /// Lay the table out, centered at the origin. Fails when rows differ in length or
    /// the label counts don't match the table.
    pub fn build(self) -> Result<VMobject> {
        let Some(n_cols) = self.entries.first().map(Vec::len) else { bail!("Table needs at least one row") };
        if n_cols == 0 {
            bail!("Table needs at least one column");
        }
        if self.entries.iter().any(|r| r.len() != n_cols) {
            bail!("not all rows in the table have the same length");
        }
        let has_rows = !self.row_labels.is_empty();
        let has_cols = !self.col_labels.is_empty();
        if has_rows && self.row_labels.len() != self.entries.len() {
            bail!("{} row labels for {} rows", self.row_labels.len(), self.entries.len());
        }
        if has_cols && self.col_labels.len() != n_cols {
            bail!("{} column labels for {} columns", self.col_labels.len(), n_cols);
        }
        let mut grid: Vec<Vec<VMobject>> = Vec::new();
        if has_cols {
            let mut first = Vec::new();
            if has_rows {
                // An empty placeholder keeps the grid rectangular (Manim does the same).
                first.push(self.top_left_entry.clone().unwrap_or_else(|| VMobject::group(vec![])));
            }
            first.extend(self.col_labels.iter().cloned());
            grid.push(first);
        }
        for (i, row) in self.entries.iter().enumerate() {
            let mut r = Vec::new();
            if has_rows {
                r.push(self.row_labels[i].clone());
            }
            r.extend(row.iter().cloned());
            grid.push(r);
        }
        let entries = arrange_grid(grid, self.h_buff, self.v_buff);
        let (rows, cols) = (entries.len(), entries[0].len());
        let mut elements = VMobject::group(entries.into_iter().flatten().collect()).with_kind(ENTRIES);
        let center = elements.get_center();
        elements = elements.shift(-center);
        elements.meta.grid = Some(Grid {
            rows,
            cols,
            h_buff: self.h_buff,
            v_buff: self.v_buff,
            row_labels: has_rows,
            col_labels: has_cols,
        });
        let color = self.line_color.unwrap_or_else(crate::defaults::color);
        let line = |a: Point, b: Point| Line::new(a, b).set_stroke(color, self.line_width);
        let rows_box: Vec<(Point, Point)> =
            (0..rows).filter_map(|r| span(&elements, r * cols..(r + 1) * cols, 1)).collect();
        let cols_box: Vec<(Point, Point)> = (0..cols).filter_map(|c| span(&elements, c..rows * cols, cols)).collect();
        let (lo, hi) = elements.bounding_box().unwrap_or((ORIGIN, ORIGIN));
        let (h, v) = (self.h_buff / 2.0, self.v_buff / 2.0);
        let (left, right, bottom, top) = (lo.x - h, hi.x + h, lo.y - v, hi.y + v);
        let mut hlines = Vec::new();
        let mut vlines = Vec::new();
        if self.include_outer_lines {
            hlines.push(line(pt(left, top), pt(right, top)));
            hlines.push(line(pt(left, bottom), pt(right, bottom)));
            vlines.push(line(pt(left, top), pt(left, bottom)));
            vlines.push(line(pt(right, top), pt(right, bottom)));
        }
        if self.include_inner_lines {
            for w in rows_box.windows(2) {
                let y = (w[0].0.y + w[1].1.y) / 2.0;
                hlines.push(line(pt(left, y), pt(right, y)));
            }
            for w in cols_box.windows(2) {
                let x = (w[0].1.x + w[1].0.x) / 2.0;
                vlines.push(line(pt(x, bottom), pt(x, top)));
            }
        }
        let mut table = VMobject::group(vec![
            VMobject::group(vec![]).with_kind(BACKGROUNDS),
            elements,
            VMobject::group(hlines).with_kind(HORIZONTAL_LINES),
            VMobject::group(vlines).with_kind(VERTICAL_LINES),
        ])
        .with_kind("Table");
        if self.add_background_rectangles_to_entries {
            table = table.add_background_to_entries(self.entries_background_color);
        }
        if self.include_background_rectangle {
            let bg = background_rect(&table, self.background_rectangle_color, 0.0);
            push_background(&mut table, bg);
        }
        Ok(table)
    }
}

/// Bounding box of the children `range.step_by(step)` of `g`, skipping empty ones.
fn span(g: &VMobject, range: std::ops::Range<usize>, step: usize) -> Option<(Point, Point)> {
    let picked: Vec<VMobject> = g.children[range].iter().step_by(step).cloned().collect();
    VMobject::group(picked).bounding_box()
}

/// Manim's `arrange_in_grid(buff=(h_buff, v_buff))`: each row as tall as its tallest
/// entry, each column as wide as its widest, entries centered in their cells.
pub(crate) fn arrange_grid(mut grid: Vec<Vec<VMobject>>, h_buff: f64, v_buff: f64) -> Vec<Vec<VMobject>> {
    let cols = grid.first().map_or(0, Vec::len);
    let heights: Vec<f64> = grid.iter().map(|r| r.iter().map(VMobject::height).fold(0.0, f64::max)).collect();
    let widths: Vec<f64> = (0..cols).map(|c| grid.iter().map(|r| r[c].width()).fold(0.0, f64::max)).collect();
    let mut y = 0.0;
    for (r, row) in grid.iter_mut().enumerate() {
        let mut x = 0.0;
        for (c, m) in row.iter_mut().enumerate() {
            if m.bounding_box().is_some() {
                m.apply(|e| e.move_to(pt(x + widths[c] / 2.0, y - heights[r] / 2.0)));
            }
            x += widths[c] + h_buff;
        }
        y -= heights[r] + v_buff;
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobject::geometry::Square;

    fn boxes() -> Vec<Vec<VMobject>> {
        vec![vec![Square::new(1.0), Square::new(0.5)], vec![Square::new(0.5), Square::new(1.0)]]
    }

    #[test]
    fn lays_out_cells_and_lines() {
        let t = Table::from_mobjects(boxes()).build().unwrap();
        let g = t.grid().unwrap();
        assert_eq!((g.rows, g.cols), (2, 2));
        // Columns 1.0 wide + 1.3 gap, rows 1.0 tall + 0.8 gap.
        let e = t.get_entry(1, 1).unwrap().get_center();
        let f = t.get_entry(2, 2).unwrap().get_center();
        assert!(((f.x - e.x) - 2.3).abs() < 1e-9 && ((e.y - f.y) - 1.8).abs() < 1e-9);
        assert_eq!(t.get_horizontal_lines().children.len(), 1);
        assert_eq!(t.get_vertical_lines().children.len(), 1);
        // Lines reach half a buffer past the entries.
        let w = t.get_horizontal_lines().width();
        assert!((w - (2.0 + 1.3 + 1.3)).abs() < 1e-9, "{w}");
        assert!(t.get_center().length() < 1e-9);
    }

    #[test]
    fn outer_lines_and_labels() {
        let t = Table::from_mobjects(boxes())
            .row_labels(vec![Square::new(0.3), Square::new(0.3)])
            .col_labels(vec![Square::new(0.3), Square::new(0.3)])
            .include_outer_lines(true)
            .build()
            .unwrap();
        let g = t.grid().unwrap();
        assert_eq!((g.rows, g.cols, g.row_labels, g.col_labels), (3, 3, true, true));
        assert_eq!(t.get_horizontal_lines().children.len(), 4);
        assert_eq!(t.get_vertical_lines().children.len(), 4);
        assert_eq!(t.get_labels().children.len(), 4);
        assert_eq!(t.get_entries_without_labels().children.len(), 4);
        assert!(Table::from_mobjects(boxes()).row_labels(vec![Square::new(1.0)]).build().is_err());
        assert!(Table::from_mobjects(vec![vec![Square::new(1.0)], vec![]]).build().is_err());
    }

    #[test]
    fn cells_follow_transforms() {
        let t = Table::from_mobjects(boxes()).build().unwrap();
        let cell = t.get_cell(1, 1);
        let moved = t.clone().scale(2.0).shift(RIGHT);
        let cell2 = moved.get_cell(1, 1);
        assert!((cell2.width() - 2.0 * cell.width()).abs() < 1e-9);
        assert!((cell2.get_center() - (cell.get_center() * 2.0 + RIGHT)).length() < 1e-9);
        // Cell 1,1 spans the widest entry of its column plus a buffer.
        assert!((cell.width() - 2.3).abs() < 1e-9 && (cell.height() - 1.8).abs() < 1e-9);
        let hl = t.add_highlighted_cell(1, 2, crate::color::GREEN);
        assert_eq!(hl.children.len(), 4, "highlights go in the backgrounds group");
        assert_eq!(hl.children[0].meta.kind(), BACKGROUNDS);
        assert_eq!(hl.children[0].children[0].meta.kind(), crate::mobject::grid::HIGHLIGHT);
        assert!(hl.get_entry(1, 1).is_some());
    }
}
