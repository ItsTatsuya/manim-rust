//! Queries and edits shared by [`Table`](super::table::Table) and
//! [`Matrix`](super::matrix::Matrix): rows, columns, cells, row/column colors.
//!
//! Both build a group whose entries child holds every entry row by row and carries a
//! [`Grid`] in its metadata, so these work on the built mobject, also after it was
//! moved or scaled. Positions are 1-based `(row, column)` like Manim's, labels included.

use super::geometry::{Polygon, Rectangle};
use super::{Grid, VMobject};
use crate::color::{Color, BLUE};
use crate::constants::*;

/// `kind` of a table's or matrix's horizontal grid lines group.
pub const HORIZONTAL_LINES: &str = "TableHorizontalLines";
/// `kind` of a table's or matrix's vertical grid lines group.
pub const VERTICAL_LINES: &str = "TableVerticalLines";
/// `kind` of a matrix's brackets group.
pub const BRACKETS: &str = "MatrixBrackets";
/// `kind` of highlight rectangles added with [`GridMobject::add_highlighted_cell`].
pub const HIGHLIGHT: &str = "TableHighlight";
/// `kind` of the first child of a table or matrix: the group of highlights and
/// background rectangles drawn behind everything else. Keeping them in their own group
/// keeps the other parts at fixed child indices.
pub const BACKGROUNDS: &str = "TableBackgrounds";

/// Manim's `Table`/`Matrix` methods, on the built mobject.
pub trait GridMobject: Sized {
    /// The group holding every entry, row by row (`table.elements` in Manim).
    fn grid_entries(&self) -> Option<&VMobject>;
    fn grid_entries_mut(&mut self) -> Option<&mut VMobject>;

    fn grid(&self) -> Option<Grid> {
        self.grid_entries().and_then(|e| e.meta.grid)
    }

    /// Entry at 1-based `(row, col)` (labels count as rows/columns).
    fn get_entry(&self, row: usize, col: usize) -> Option<&VMobject> {
        let g = self.grid()?;
        if row == 0 || col == 0 || row > g.rows || col > g.cols {
            return None;
        }
        self.grid_entries()?.children.get((row - 1) * g.cols + col - 1)
    }

    /// Every entry, row by row (copies).
    fn get_mob_matrix(&self) -> Vec<Vec<VMobject>> {
        let (Some(g), Some(e)) = (self.grid(), self.grid_entries()) else { return Vec::new() };
        e.children.chunks(g.cols.max(1)).map(|r| r.to_vec()).collect()
    }

    /// One group per row (copies).
    fn get_rows(&self) -> VMobject {
        VMobject::group(self.get_mob_matrix().into_iter().map(VMobject::group).collect())
    }

    /// One group per column (copies).
    fn get_columns(&self) -> VMobject {
        let rows = self.get_mob_matrix();
        let cols = rows.first().map_or(0, Vec::len);
        VMobject::group(
            (0..cols).map(|c| VMobject::group(rows.iter().filter_map(|r| r.get(c).cloned()).collect())).collect(),
        )
    }

    /// Every entry (a copy of the entries group).
    fn get_entries(&self) -> VMobject {
        self.grid_entries().cloned().unwrap_or_default()
    }

    /// Entries that are not labels (copies).
    fn get_entries_without_labels(&self) -> VMobject {
        let Some(g) = self.grid() else { return VMobject::group(vec![]) };
        let rows = self.get_mob_matrix();
        let (r0, c0) = (g.col_labels as usize, g.row_labels as usize);
        VMobject::group(rows.into_iter().skip(r0).flat_map(|r| r.into_iter().skip(c0)).collect())
    }

    /// The row labels (copies; empty when the table has none).
    fn get_row_labels(&self) -> VMobject {
        let Some(g) = self.grid().filter(|g| g.row_labels) else { return VMobject::group(vec![]) };
        VMobject::group(
            self.get_mob_matrix().into_iter().skip(g.col_labels as usize).filter_map(|r| r.first().cloned()).collect(),
        )
    }

    /// The column labels (copies; empty when the table has none).
    fn get_col_labels(&self) -> VMobject {
        let Some(g) = self.grid().filter(|g| g.col_labels) else { return VMobject::group(vec![]) };
        let first = self.get_mob_matrix().into_iter().next().unwrap_or_default();
        VMobject::group(first.into_iter().skip(g.row_labels as usize).collect())
    }

    /// Row labels, then column labels (copies).
    fn get_labels(&self) -> VMobject {
        VMobject::group([self.get_row_labels().children, self.get_col_labels().children].concat())
    }

    /// Color each row (labels included); rows past the list keep their color.
    fn set_row_colors(mut self, colors: &[Color]) -> Self {
        if let Some(g) = self.grid() {
            if let Some(e) = self.grid_entries_mut() {
                for (i, c) in e.children.iter_mut().enumerate() {
                    if let Some(col) = colors.get(i / g.cols.max(1)) {
                        c.apply(|m| m.set_color(*col));
                    }
                }
            }
        }
        self
    }

    /// Color each column (labels included); columns past the list keep their color.
    fn set_column_colors(mut self, colors: &[Color]) -> Self {
        if let Some(g) = self.grid() {
            if let Some(e) = self.grid_entries_mut() {
                for (i, c) in e.children.iter_mut().enumerate() {
                    if let Some(col) = colors.get(i % g.cols.max(1)) {
                        c.apply(|m| m.set_color(*col));
                    }
                }
            }
        }
        self
    }

    /// Bounds `(left, right, bottom, top)` of cell `(row, col)`: the row's and column's
    /// extent plus half the buffers (Manim's `get_cell`).
    fn cell_bounds(&self, row: usize, col: usize) -> Option<(f64, f64, f64, f64)> {
        let g = self.grid()?;
        if row == 0 || col == 0 || row > g.rows || col > g.cols {
            return None;
        }
        let rows = self.get_mob_matrix();
        let span = |ms: Vec<&VMobject>| VMobject::group(ms.into_iter().cloned().collect()).bounding_box();
        let (rlo, rhi) = span(rows.get(row - 1)?.iter().collect())?;
        let (clo, chi) = span(rows.iter().filter_map(|r| r.get(col - 1)).collect())?;
        Some((clo.x - g.h_buff / 2.0, chi.x + g.h_buff / 2.0, rlo.y - g.v_buff / 2.0, rhi.y + g.v_buff / 2.0))
    }

    /// The outline of cell `(row, col)` as a polygon (Manim's `get_cell`; blue like a `Polygon`).
    fn get_cell(&self, row: usize, col: usize) -> VMobject {
        match self.cell_bounds(row, col) {
            Some((l, r, b, t)) => Polygon::new(&[pt(l, t), pt(r, t), pt(r, b), pt(l, b)]).set_color(BLUE),
            None => VMobject::group(vec![]),
        }
    }

    /// A filled rectangle covering cell `(row, col)` (Manim's `get_highlighted_cell`).
    fn get_highlighted_cell(&self, row: usize, col: usize, color: Color) -> VMobject {
        match self.cell_bounds(row, col) {
            Some((l, r, b, t)) => Rectangle::new(r - l, t - b)
                .move_to(pt((l + r) / 2.0, (b + t) / 2.0))
                .set_fill(color, 0.75)
                .set_stroke(color, 0.0)
                .set_stroke_opacity(0.0)
                .with_kind(HIGHLIGHT),
            None => VMobject::group(vec![]),
        }
    }

    /// Put a highlight behind cell `(row, col)` (Manim's `add_highlighted_cell`).
    /// Manim's default `color` is `PURE_YELLOW`.
    fn add_highlighted_cell(self, row: usize, col: usize, color: Color) -> Self;

    /// Put a 75%-opaque rectangle of `color` behind each entry.
    fn add_background_to_entries(mut self, color: Color) -> Self {
        if let Some(e) = self.grid_entries_mut() {
            for c in &mut e.children {
                if c.bounding_box().is_some() {
                    let bg = background_rect(c, color, 0.0);
                    c.children.insert(0, bg);
                }
            }
        }
        self
    }

    /// Copy of a part of the table by its `kind` (lines, brackets).
    fn part_of_kind(&self, kind: &str) -> VMobject;

    /// The horizontal lines between (and around) rows.
    fn get_horizontal_lines(&self) -> VMobject {
        self.part_of_kind(HORIZONTAL_LINES)
    }

    /// The vertical lines between (and around) columns.
    fn get_vertical_lines(&self) -> VMobject {
        self.part_of_kind(VERTICAL_LINES)
    }

    /// A matrix's left and right bracket.
    fn get_brackets(&self) -> VMobject {
        self.part_of_kind(BRACKETS)
    }
}

/// Add `m` behind a table's or matrix's other parts.
pub(crate) fn push_background(table: &mut VMobject, m: VMobject) {
    match table.child_of_kind_mut(BACKGROUNDS) {
        Some(g) => g.children.push(m),
        None => table.children.insert(0, m),
    }
}

/// A rectangle `buff` around `m`, filled with `color` at 75% and no outline
/// (Manim's `BackgroundRectangle`).
pub(crate) fn background_rect(m: &VMobject, color: Color, buff: f64) -> VMobject {
    Rectangle::new(m.width() + 2.0 * buff, m.height() + 2.0 * buff)
        .move_to(m.get_center())
        .set_fill(color, 0.75)
        .set_stroke(color, 0.0)
        .set_stroke_opacity(0.0)
        .with_kind("BackgroundRectangle")
}

impl GridMobject for VMobject {
    fn grid_entries(&self) -> Option<&VMobject> {
        if self.meta.grid.is_some() {
            return Some(self);
        }
        self.children.iter().find(|c| c.meta.grid.is_some())
    }

    fn grid_entries_mut(&mut self) -> Option<&mut VMobject> {
        if self.meta.grid.is_some() {
            return Some(self);
        }
        self.children.iter_mut().find(|c| c.meta.grid.is_some())
    }

    fn add_highlighted_cell(mut self, row: usize, col: usize, color: Color) -> Self {
        let cell = self.get_highlighted_cell(row, col, color);
        if cell.bounding_box().is_some() {
            push_background(&mut self, cell);
        }
        self
    }

    fn part_of_kind(&self, kind: &str) -> VMobject {
        self.child_of_kind(kind).cloned().unwrap_or_else(|| VMobject::group(vec![]))
    }
}
