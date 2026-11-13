//! Equations typeset by an embedded [Typst](https://typst.app) compiler: no TeX
//! installation, no subprocess, and typical equations compile in milliseconds.
//!
//! [`MathTex`] takes Typst math syntax (`x^2 + y^2 = r^2`, `integral_0^1 f(x) dif x`,
//! `sum_(k=1)^n k = (n(n+1))/2`); [`Typst`] takes any Typst markup.
//! The result is a group of glyph and line paths, so `Write`, `Create` and transforms
//! work as on any other mobject.

#![allow(clippy::new_ret_no_self)]

use std::sync::OnceLock;

use anyhow::{anyhow, bail, Result};
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

use crate::color::Color;
use crate::constants::DEFAULT_FONT_SIZE;
use crate::mobject::VMobject;

/// Typst sizes the page in points; the text is set at this size.
const BASE_PT: f64 = 11.0;
/// Scene units per em at font size 1 (same as `Text`).
const UNITS_PER_FONT_SIZE: f64 = 0.015;
/// `typst_svg` sizes the root in `pt`, which usvg converts to CSS px (96/72);
/// dividing it back out makes one parsed unit one Typst point again.
const PX_PER_PT: f64 = 96.0 / 72.0;

struct Shared {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| {
        let fonts: Vec<Font> = typst_assets::fonts().flat_map(|data| Font::iter(Bytes::new(data))).collect();
        let book = FontBook::from_fonts(&fonts);
        Shared { library: LazyHash::new(Library::default()), book: LazyHash::new(book), fonts }
    })
}

struct MathWorld {
    main: FileId,
    source: Source,
}

impl World for MathWorld {
    fn library(&self) -> &LazyHash<Library> {
        &shared().library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &shared().book
    }
    fn main(&self) -> FileId {
        self.main
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            Ok(self.source.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main {
            Ok(Bytes::from_string(self.source.clone()))
        } else {
            Err(FileError::AccessDenied)
        }
    }
    fn font(&self, index: usize) -> Option<Font> {
        shared().fonts.get(index).cloned()
    }
    fn today(&self, _offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        None
    }
}

fn hex(c: Color) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
}

/// Compile Typst markup to SVG (one page, sized to its content).
pub fn typst_to_svg(markup: &str, color: Color) -> Result<String> {
    let doc = format!(
        "#set page(width: auto, height: auto, margin: 0pt, fill: none)\n#set text(size: {BASE_PT}pt, fill: rgb(\"{}\"))\n{markup}\n",
        hex(color)
    );
    let path = VirtualPath::new("main.typ").map_err(|e| anyhow!("{e:?}"))?;
    let main = FileId::new(RootedPath::new(VirtualRoot::Project, path));
    let world = MathWorld { main, source: Source::new(main, doc) };
    let warned = typst::compile::<PagedDocument>(&world);
    let doc = warned.output.map_err(|errs| {
        let msgs: Vec<String> = errs.iter().map(|e| e.message.to_string()).collect();
        anyhow!("typst: {}", msgs.join("; "))
    })?;
    let Some(page) = doc.pages().first() else { bail!("typst produced no pages") };
    Ok(typst_svg::svg(page, &typst_svg::SvgOptions::default()))
}

/// Typeset markup at `font_size` (Manim units: 48 = default text size).
pub fn typeset(markup: &str, font_size: f64, color: Color) -> Result<VMobject> {
    let svg = typst_to_svg(markup, color)?;
    // One Typst point → scene units, so BASE_PT points = one em at `font_size`.
    let scale = font_size * UNITS_PER_FONT_SIZE / (BASE_PT * PX_PER_PT);
    let mut m = crate::svg::parse(svg.as_bytes(), scale)?.center();
    m.for_each_mut(&mut |c| c.meta.nonzero = true);
    Ok(m)
}

/// A display equation in Typst math syntax (Manim's `MathTex`).
pub struct MathTex;
impl MathTex {
    pub fn new(math: &str) -> Result<VMobject> {
        Self::styled(math, DEFAULT_FONT_SIZE, crate::defaults::get().text_color)
    }

    pub fn styled(math: &str, font_size: f64, color: Color) -> Result<VMobject> {
        Ok(typeset(&format!("$ {math} $"), font_size, color)?.with_kind("Math").with_label(math))
    }
}

/// Inline Typst markup (text with `$math$`, `*bold*`, lists...) (Manim's `Tex`).
pub struct Typst;
impl Typst {
    pub fn new(markup: &str) -> Result<VMobject> {
        Self::styled(markup, DEFAULT_FONT_SIZE, crate::defaults::get().text_color)
    }

    pub fn styled(markup: &str, font_size: f64, color: Color) -> Result<VMobject> {
        Ok(typeset(markup, font_size, color)?.with_kind("Math").with_label(markup))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typesets_an_equation() {
        let m = MathTex::new("x^2 + y^2 = r^2").unwrap();
        assert!(m.children.len() >= 5, "glyphs: {}", m.children.len());
        assert!(m.width() > m.height());
        assert!(m.height() > 0.2 && m.height() < 2.0, "height {}", m.height());
        assert!(MathTex::new("undefinedthing(x)").is_err());
    }

    #[test]
    fn digit_height_matches_font_size() {
        // A New Computer Modern digit is ~0.665 em, and an em is 0.015 units per font size:
        // at 48 that is ~0.48 units (it was 0.639 when usvg's pt→px factor leaked through).
        let h = MathTex::styled("2", 48.0, crate::color::WHITE).unwrap().height();
        assert!((h - 0.4795).abs() < 0.005, "digit height {h}");
        let h96 = MathTex::styled("2", 96.0, crate::color::WHITE).unwrap().height();
        assert!((h96 - 2.0 * h).abs() < 1e-6, "height must scale linearly: {h} vs {h96}");
    }
}
