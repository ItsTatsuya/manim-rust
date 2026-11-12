//! `Text` mobjects built from font outlines (no Pango/Cairo needed).
//!
//! - Fonts: the system fonts, plus any files or folders you register
//!   ([`register_font_file`], [`register_font_dir`]).
//! - Weights 100–900, including variable fonts (the `wght` axis is set to the weight).
//! - Shaping with rustybuzz (a HarfBuzz port): GPOS kerning, ligatures, complex scripts
//!   such as Devanagari and Arabic.
//! - Per-character fallback: characters the chosen font lacks come from the fallback
//!   fonts, then from any installed font that has them.
//! - Wrapping to a maximum width, left/center/right alignment, and styled spans
//!   ([`Text::markup`], [`Text::rich`]).
//!
//! Each glyph is a child [`VMobject`] tagged with the index (in chars) of the character it
//! draws, so `Write` staggers per letter and [`VMobject::chars`] / [`VMobject::part`]
//! select words by position in the string.

#![allow(clippy::new_ret_no_self)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use anyhow::{bail, Context, Result};
use rustybuzz::ttf_parser;

use crate::bezier::{self, Cubic};
use crate::color::Color;
use crate::constants::*;
use crate::mobject::{Subpath, VMobject};

/// Scene units per em at font size 1. Font size 48 gives ~0.52-unit capitals, close to Manim.
const UNITS_PER_FONT_SIZE: f64 = 0.015;

/// Tried, in order, for characters the requested font and the theme's fallbacks lack.
const BUILTIN_FALLBACKS: &[&str] = &[
    "Segoe UI",
    "Arial",
    "Noto Sans",
    "DejaVu Sans",
    "Helvetica",
    "Nirmala UI",
    "Noto Sans Devanagari",
    "Segoe UI Symbol",
    "Cambria Math",
    "Noto Sans Math",
    "Noto Sans Symbols",
    "Noto Sans Symbols 2",
];

// ---------------------------------------------------------------------- font database

type FaceData = Arc<dyn AsRef<[u8]> + Send + Sync>;
type GlyphKey = (fontdb::ID, u16, u16);

struct Fonts {
    db: RwLock<fontdb::Database>,
    /// Face bytes kept alive (memory-mapped or owned) so several faces can be parsed at once.
    data: Mutex<HashMap<fontdb::ID, (FaceData, u32)>>,
    /// Glyph outlines in em units, keyed by face, glyph and weight.
    outlines: Mutex<HashMap<GlyphKey, Arc<Vec<Subpath>>>>,
    /// Which face covers a character no font in the chain had.
    rescue: Mutex<HashMap<char, Option<fontdb::ID>>>,
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Fonts {
            db: RwLock::new(db),
            data: Mutex::new(HashMap::new()),
            outlines: Mutex::new(HashMap::new()),
            rescue: Mutex::new(HashMap::new()),
        }
    })
}

/// Load the system font database ahead of first use (called on a background thread).
pub fn preload_fonts() {
    fonts();
}

/// Make a font file (`.ttf`, `.otf`, `.ttc`) usable by family name. Returns the families it adds.
pub fn register_font_file(path: impl AsRef<Path>) -> Result<Vec<String>> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).with_context(|| format!("reading font {}", path.display()))?;
    register_font_data(bytes).with_context(|| format!("loading font {}", path.display()))
}

/// Register every font file in a folder (recursively). Returns how many faces were added.
pub fn register_font_dir(path: impl AsRef<Path>) -> Result<usize> {
    let path = path.as_ref();
    if !path.is_dir() {
        bail!("font folder {} does not exist", path.display());
    }
    let f = fonts();
    let mut db = f.db.write().unwrap();
    let before = db.len();
    db.load_fonts_dir(path);
    let added = db.len() - before;
    drop(db);
    f.rescue.lock().unwrap().clear();
    Ok(added)
}

/// Register font bytes. Returns the families they add.
pub fn register_font_data(bytes: Vec<u8>) -> Result<Vec<String>> {
    let f = fonts();
    let mut db = f.db.write().unwrap();
    let before: Vec<fontdb::ID> = db.faces().map(|i| i.id).collect();
    db.load_font_data(bytes);
    let mut families: Vec<String> =
        db.faces().filter(|i| !before.contains(&i.id)).flat_map(|i| i.families.first().map(|f| f.0.clone())).collect();
    drop(db);
    if families.is_empty() {
        bail!("no font faces found in the data");
    }
    families.dedup();
    f.rescue.lock().unwrap().clear();
    Ok(families)
}

/// Whether a font family is installed or registered.
pub fn has_font(family: &str) -> bool {
    let db = fonts().db.read().unwrap();
    let found = db.faces().any(|i| i.families.iter().any(|f| f.0.eq_ignore_ascii_case(family)));
    found
}

/// Every known font family, sorted.
pub fn font_families() -> Vec<String> {
    let db = fonts().db.read().unwrap();
    let mut v: Vec<String> = db.faces().flat_map(|i| i.families.iter().map(|f| f.0.clone())).collect();
    v.sort();
    v.dedup();
    v
}

fn face_data(id: fontdb::ID) -> Option<(FaceData, u32)> {
    let f = fonts();
    if let Some(d) = f.data.lock().unwrap().get(&id) {
        return Some(d.clone());
    }
    // SAFETY: fontdb memory-maps font files; a file changing on disk while mapped is the
    // documented hazard, the same one every font stack accepts.
    let d = unsafe { f.db.write().unwrap().make_shared_face_data(id) }?;
    f.data.lock().unwrap().insert(id, d.clone());
    Some(d)
}

fn query(family: &str, weight: u16, italic: bool) -> Option<fontdb::ID> {
    let db = fonts().db.read().unwrap();
    let families = if family.is_empty() { vec![fontdb::Family::SansSerif] } else { vec![fontdb::Family::Name(family)] };
    let q = fontdb::Query {
        families: &families,
        weight: fontdb::Weight(weight),
        stretch: fontdb::Stretch::Normal,
        style: if italic { fontdb::Style::Italic } else { fontdb::Style::Normal },
    };
    let id = db.query(&q)?;
    // fontdb falls back to any face for a named family that doesn't exist; only accept
    // faces of the family asked for.
    if !family.is_empty() {
        let info = db.face(id)?;
        if !info.families.iter().any(|f| f.0.eq_ignore_ascii_case(family)) {
            return None;
        }
    }
    Some(id)
}

fn covers(id: fontdb::ID, ch: char) -> bool {
    let Some((data, index)) = face_data(id) else { return false };
    ttf_parser::Face::parse((*data).as_ref(), index).map(|f| f.glyph_index(ch).is_some()).unwrap_or(false)
}

/// Any face that has `ch` (slow scan, cached per character).
fn rescue_face(ch: char) -> Option<fontdb::ID> {
    let f = fonts();
    if let Some(r) = f.rescue.lock().unwrap().get(&ch) {
        return *r;
    }
    let ids: Vec<fontdb::ID> = f.db.read().unwrap().faces().map(|i| i.id).collect();
    let found = ids.into_iter().find(|id| covers(*id, ch));
    f.rescue.lock().unwrap().insert(ch, found);
    found
}

// ---------------------------------------------------------------------- styles

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// Font family; empty means the theme's (or the system's) default.
    pub font: String,
    pub font_size: f64,
    /// 100 (thin) … 400 (regular) … 700 (bold) … 900 (black).
    pub weight: u16,
    pub italic: bool,
    pub color: Color,
    /// Line height as a multiple of the em size.
    pub line_spacing: f64,
    /// Wrap lines longer than this many scene units (at spaces).
    pub max_width: Option<f64>,
    pub align: Align,
    /// Extra fonts tried for characters the main font lacks.
    pub fallback_fonts: Vec<String>,
}

pub const THIN: u16 = 100;
pub const LIGHT: u16 = 300;
pub const NORMAL: u16 = 400;
pub const MEDIUM: u16 = 500;
pub const SEMIBOLD: u16 = 600;
pub const BOLD: u16 = 700;
pub const HEAVY: u16 = 900;

impl Default for TextStyle {
    /// The current [`defaults`](crate::defaults) (set by the theme).
    fn default() -> Self {
        let d = crate::defaults::get();
        TextStyle {
            font: d.font,
            font_size: d.font_size,
            weight: NORMAL,
            italic: false,
            color: d.text_color,
            line_spacing: 1.3,
            max_width: None,
            align: Align::Left,
            fallback_fonts: d.fallback_fonts,
        }
    }
}

impl TextStyle {
    pub fn font(mut self, f: &str) -> Self {
        self.font = f.to_string();
        self
    }
    pub fn size(mut self, s: f64) -> Self {
        self.font_size = s;
        self
    }
    pub fn weight(mut self, w: u16) -> Self {
        self.weight = w.clamp(1, 1000);
        self
    }
    pub fn bold(self) -> Self {
        self.weight(BOLD)
    }
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }
    pub fn color(mut self, c: Color) -> Self {
        self.color = c;
        self
    }
    pub fn line_spacing(mut self, s: f64) -> Self {
        self.line_spacing = s;
        self
    }
    pub fn max_width(mut self, w: f64) -> Self {
        self.max_width = Some(w);
        self
    }
    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }
    pub fn fallback(mut self, family: &str) -> Self {
        self.fallback_fonts.push(family.to_string());
        self
    }
}

/// A run of text with its own style (see [`Text::rich`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: TextStyle,
}

impl Span {
    pub fn new(text: &str, style: TextStyle) -> Self {
        Span { text: text.to_string(), style }
    }
}

// ---------------------------------------------------------------------- outlines

struct GlyphPen {
    subpaths: Vec<Subpath>,
    cur: Vec<Cubic>,
    start: Point,
    last: Point,
    scale: f64,
}

impl GlyphPen {
    fn p(&self, x: f32, y: f32) -> Point {
        pt(x as f64, y as f64) * self.scale
    }
    fn flush(&mut self, closed: bool) {
        if !self.cur.is_empty() {
            self.subpaths.push(Subpath::new(std::mem::take(&mut self.cur), closed));
        }
    }
}

impl ttf_parser::OutlineBuilder for GlyphPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush(false);
        self.start = self.p(x, y);
        self.last = self.start;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.cur.push(bezier::line(self.last, p));
        self.last = p;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (q, p) = (self.p(x1, y1), self.p(x, y));
        self.cur.push(bezier::from_quadratic(self.last, q, p));
        self.last = p;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.p(x1, y1), self.p(x2, y2), self.p(x, y));
        self.cur.push([self.last, c1, c2, p]);
        self.last = p;
    }
    fn close(&mut self) {
        if self.last.distance(self.start) > 1e-9 {
            self.cur.push(bezier::line(self.last, self.start));
        }
        self.last = self.start;
        self.flush(true);
    }
}

const WGHT: ttf_parser::Tag = ttf_parser::Tag::from_bytes(b"wght");

/// Set the `wght` axis of a variable font; returns the weight actually used (0 = static font).
fn apply_weight(face: &mut ttf_parser::Face, weight: u16) -> u16 {
    let Some(axis) = face.variation_axes().into_iter().find(|a| a.tag == WGHT) else { return 0 };
    let w = (weight as f32).clamp(axis.min_value, axis.max_value);
    face.set_variation(WGHT, w);
    w as u16
}

fn outline(id: fontdb::ID, face: &ttf_parser::Face, gid: u16, weight_key: u16) -> Arc<Vec<Subpath>> {
    let key = (id, gid, weight_key);
    if let Some(o) = fonts().outlines.lock().unwrap().get(&key) {
        return o.clone();
    }
    let mut pen = GlyphPen {
        subpaths: Vec::new(),
        cur: Vec::new(),
        start: ORIGIN,
        last: ORIGIN,
        scale: 1.0 / face.units_per_em() as f64,
    };
    face.outline_glyph(ttf_parser::GlyphId(gid), &mut pen);
    pen.flush(false);
    let o = Arc::new(pen.subpaths);
    fonts().outlines.lock().unwrap().insert(key, o.clone());
    o
}

// ---------------------------------------------------------------------- layout

struct Glyph {
    face: fontdb::ID,
    gid: u16,
    weight_key: u16,
    /// Pen position and offset, in scene units relative to the line start.
    x: f64,
    y: f64,
    advance: f64,
    em: f64,
    char_index: u32,
    is_space: bool,
    span: usize,
}

struct Resolved {
    primary: Option<fontdb::ID>,
    chain: Vec<fontdb::ID>,
}

fn resolve_fonts(style: &TextStyle) -> Resolved {
    let primary = query(&style.font, style.weight, style.italic)
        .or_else(|| query(&style.font, style.weight, false))
        .or_else(|| query("", style.weight, style.italic));
    let mut chain: Vec<fontdb::ID> = primary.into_iter().collect();
    for fam in style.fallback_fonts.iter().map(String::as_str).chain(BUILTIN_FALLBACKS.iter().copied()) {
        if let Some(id) = query(fam, style.weight, style.italic).or_else(|| query(fam, style.weight, false)) {
            if !chain.contains(&id) {
                chain.push(id);
            }
        }
    }
    Resolved { primary, chain }
}

fn is_mark(c: char) -> bool {
    // Combining marks and joiners stay with the preceding character's font.
    matches!(c as u32, 0x0300..=0x036F | 0x0900..=0x0903 | 0x093A..=0x094F | 0x0951..=0x0957 | 0x0962..=0x0963
        | 0x200C | 0x200D | 0xFE00..=0xFE0F | 0x1AB0..=0x1AFF | 0x20D0..=0x20FF)
}

/// Shape one line (no newlines) into positioned glyphs.
fn shape_line(chars: &[(char, u32, usize)], spans: &[Span], resolved: &[Resolved]) -> Vec<Glyph> {
    let mut out = Vec::new();
    // Pick a face per character.
    let mut faces: Vec<Option<fontdb::ID>> = Vec::with_capacity(chars.len());
    let mut coverage: HashMap<(fontdb::ID, char), bool> = HashMap::new();
    let mut has = |id: fontdb::ID, c: char| *coverage.entry((id, c)).or_insert_with(|| covers(id, c));
    for (i, &(c, _, span)) in chars.iter().enumerate() {
        let prev = if i > 0 && chars[i - 1].2 == span { faces[i - 1] } else { None };
        if c.is_whitespace() || is_mark(c) {
            faces.push(prev.or(resolved[span].primary));
            continue;
        }
        let r = &resolved[span];
        let found = r.chain.iter().copied().find(|id| has(*id, c)).or_else(|| rescue_face(c)).or(r.primary);
        faces.push(found);
    }
    // Runs of equal (span, face), shaped together.
    let mut pen = 0.0;
    let mut i = 0;
    while i < chars.len() {
        let (span, face) = (chars[i].2, faces[i]);
        let mut j = i + 1;
        while j < chars.len() && chars[j].2 == span && faces[j] == face {
            j += 1;
        }
        let style = &spans[span].style;
        let em = style.font_size * UNITS_PER_FONT_SIZE;
        let run: String = chars[i..j].iter().map(|c| c.0).collect();
        let Some(id) = face else {
            i = j;
            continue;
        };
        let Some((data, index)) = face_data(id) else {
            i = j;
            continue;
        };
        let Ok(mut tface) = ttf_parser::Face::parse((*data).as_ref(), index) else {
            i = j;
            continue;
        };
        let weight_key = apply_weight(&mut tface, style.weight);
        let upem = tface.units_per_em() as f64;
        let scale = em / upem;
        let hb = rustybuzz::Face::from_face(tface.clone());
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(&run);
        buf.guess_segment_properties();
        let shaped = rustybuzz::shape(&hb, &[], buf);
        // Byte offset in `run` → index into chars[i..j].
        let byte_to_char: HashMap<usize, usize> = run.char_indices().enumerate().map(|(k, (b, _))| (b, k)).collect();
        for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            let k = byte_to_char.get(&(info.cluster as usize)).copied().unwrap_or(0);
            let (c, char_index, _) = chars[i + k];
            out.push(Glyph {
                face: id,
                gid: info.glyph_id as u16,
                weight_key,
                x: pen + pos.x_offset as f64 * scale,
                y: pos.y_offset as f64 * scale,
                advance: pos.x_advance as f64 * scale,
                em,
                char_index,
                is_space: c.is_whitespace(),
                span,
            });
            pen += pos.x_advance as f64 * scale;
        }
        i = j;
    }
    out
}

/// Break a shaped line at spaces so no visual line is wider than `max_width`.
fn wrap(glyphs: Vec<Glyph>, max_width: Option<f64>) -> Vec<Vec<Glyph>> {
    let Some(max) = max_width else { return vec![glyphs] };
    let mut lines: Vec<Vec<Glyph>> = vec![Vec::new()];
    let mut origin = 0.0; // pen x where the current visual line starts
    for g in glyphs {
        let line = lines.last_mut().unwrap();
        let right = g.x + g.advance - origin;
        if right > max && !g.is_space {
            // Break after the last space on this line, if there is one.
            if let Some(cut) = line.iter().rposition(|x| x.is_space) {
                let rest: Vec<Glyph> = line.drain(cut + 1..).collect();
                while line.last().is_some_and(|x| x.is_space) {
                    line.pop();
                }
                origin = rest.first().map(|x| x.x).unwrap_or(g.x);
                lines.push(rest);
            }
        }
        let wrapped = lines.len() > 1;
        let line = lines.last_mut().unwrap();
        if line.is_empty() && g.is_space && wrapped {
            origin = g.x + g.advance; // drop leading spaces of a wrapped line
            continue;
        }
        line.push(g);
    }
    // Re-base each line at x = 0.
    for line in &mut lines {
        if let Some(x0) = line.first().map(|g| g.x) {
            for g in line.iter_mut() {
                g.x -= x0;
            }
        }
    }
    lines
}

fn layout(spans: &[Span], para: &TextStyle) -> VMobject {
    let label: String = spans.iter().map(|s| s.text.as_str()).collect();
    let resolved: Vec<Resolved> = spans.iter().map(|s| resolve_fonts(&s.style)).collect();
    // (char, global char index, span) per hard line.
    let mut hard_lines: Vec<Vec<(char, u32, usize)>> = vec![Vec::new()];
    let mut n = 0u32;
    for (si, s) in spans.iter().enumerate() {
        for c in s.text.chars() {
            if c == '\n' {
                hard_lines.push(Vec::new());
            } else if c != '\r' {
                hard_lines.last_mut().unwrap().push((c, n, si));
            }
            n += 1;
        }
    }
    let default_em = para.font_size * UNITS_PER_FONT_SIZE;
    let mut lines: Vec<(Vec<Glyph>, f64)> = Vec::new();
    for hl in &hard_lines {
        let glyphs = shape_line(hl, spans, &resolved);
        let em = hl.iter().map(|c| spans[c.2].style.font_size * UNITS_PER_FONT_SIZE).fold(0.0, f64::max);
        let em = if em > 0.0 { em } else { default_em };
        for l in wrap(glyphs, para.max_width) {
            lines.push((l, em));
        }
    }
    let widths: Vec<f64> = lines.iter().map(|(l, _)| l.iter().map(|g| g.x + g.advance).fold(0.0, f64::max)).collect();
    let block = widths.iter().copied().fold(0.0, f64::max);
    let mut children = Vec::new();
    let mut baseline = 0.0;
    for (li, (line, em)) in lines.iter().enumerate() {
        if li > 0 {
            baseline -= em * para.line_spacing;
        }
        let shift = match para.align {
            Align::Left => 0.0,
            Align::Center => (block - widths[li]) / 2.0,
            Align::Right => block - widths[li],
        };
        for g in line {
            if g.is_space {
                continue;
            }
            let Some((data, index)) = face_data(g.face) else { continue };
            let Ok(mut face) = ttf_parser::Face::parse((*data).as_ref(), index) else { continue };
            if g.weight_key > 0 {
                face.set_variation(WGHT, g.weight_key as f32);
            }
            let unit = outline(g.face, &face, g.gid, g.weight_key);
            if unit.is_empty() {
                continue;
            }
            let origin = pt(g.x + shift, baseline + g.y);
            let em = g.em;
            let subpaths = unit
                .iter()
                .map(|sp| Subpath::new(sp.curves.iter().map(|c| c.map(|p| origin + p * em)).collect(), sp.closed))
                .collect();
            let color = spans[g.span].style.color;
            let mut m = VMobject {
                fill_color: color,
                stroke_color: color,
                fill_opacity: 1.0,
                stroke_width: 0.0,
                ..VMobject::from_subpaths(subpaths)
            };
            m.meta.char_index = Some(g.char_index);
            m.meta.nonzero = true;
            children.push(m);
        }
    }
    let mut group = VMobject::group(children).center();
    group.meta.kind = Some("Text".into());
    group.meta.label = Some(label.into());
    group
}

pub struct Text;

impl Text {
    pub fn new(text: &str) -> VMobject {
        Self::styled(text, TextStyle::default())
    }

    pub fn sized(text: &str, font_size: f64) -> VMobject {
        Self::styled(text, TextStyle::default().size(font_size))
    }

    pub fn styled(text: &str, style: TextStyle) -> VMobject {
        let span = Span::new(text, style.clone());
        layout(&[span], &style)
    }

    /// Several differently styled runs laid out as one text. Paragraph settings
    /// (line spacing, wrapping, alignment) come from `para`.
    pub fn rich(spans: &[Span], para: &TextStyle) -> VMobject {
        layout(spans, para)
    }

    /// Pango-style markup on top of `base`:
    /// `<b>`, `<i>`, `<span color="#E11D48" weight="600" font="Inter" size="36" style="italic">`,
    /// plus `<color=#hex>…</color>` as a shorthand. Escape `<`, `>` and `&` as `&lt;`, `&gt;`, `&amp;`.
    pub fn markup(src: &str, base: TextStyle) -> Result<VMobject> {
        let spans = parse_markup(src, &base)?;
        Ok(layout(&spans, &base))
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn named_weight(v: &str) -> Option<u16> {
    Some(match v.to_ascii_lowercase().as_str() {
        "thin" => THIN,
        "ultralight" | "extralight" => 200,
        "light" => LIGHT,
        "normal" | "regular" => NORMAL,
        "medium" => MEDIUM,
        "semibold" | "demibold" => SEMIBOLD,
        "bold" => BOLD,
        "ultrabold" | "extrabold" => 800,
        "heavy" | "black" => HEAVY,
        n => n.parse().ok()?,
    })
}

fn parse_markup(src: &str, base: &TextStyle) -> Result<Vec<Span>> {
    let mut spans = Vec::new();
    let mut stack: Vec<(String, TextStyle)> = vec![(String::new(), base.clone())];
    let mut rest = src;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            spans.push(Span { text: unescape(rest), style: stack.last().unwrap().1.clone() });
            break;
        };
        if lt > 0 {
            spans.push(Span { text: unescape(&rest[..lt]), style: stack.last().unwrap().1.clone() });
        }
        let Some(gt) = rest[lt..].find('>') else { bail!("markup: unclosed '<' in {src:?}") };
        let tag = rest[lt + 1..lt + gt].trim();
        rest = &rest[lt + gt + 1..];
        if let Some(name) = tag.strip_prefix('/') {
            let name = name.trim();
            let (open, _) = stack.pop().filter(|_| !stack.is_empty()).context("markup: unexpected closing tag")?;
            if open != name {
                bail!("markup: </{name}> closes <{open}>");
            }
            if stack.is_empty() {
                bail!("markup: </{name}> has no opening tag");
            }
            continue;
        }
        let mut style = stack.last().unwrap().1.clone();
        let (name, attrs) = match tag.find(|c: char| c.is_whitespace() || c == '=') {
            Some(i) => (&tag[..i], &tag[i..]),
            None => (tag, ""),
        };
        match name {
            "b" => style.weight = BOLD,
            "i" => style.italic = true,
            "u" | "s" | "tt" | "small" | "big" | "sub" | "sup" => {}
            "color" => {
                let v = attrs.trim_start_matches('=').trim().trim_matches('"');
                style.color = Color::hex(v).with_context(|| format!("markup: bad color {v:?}"))?;
            }
            "span" => {
                for (k, v) in parse_attrs(attrs) {
                    match k.as_str() {
                        "color" | "foreground" | "fgcolor" => {
                            style.color = Color::hex(&v).with_context(|| format!("markup: bad color {v:?}"))?
                        }
                        "font" | "font_family" | "face" => style.font = v,
                        "weight" | "font_weight" => {
                            style.weight = named_weight(&v).with_context(|| format!("markup: bad weight {v:?}"))?
                        }
                        "size" | "font_size" => {
                            style.font_size = v.parse().with_context(|| format!("markup: bad size {v:?}"))?
                        }
                        "style" | "font_style" => style.italic = v.eq_ignore_ascii_case("italic"),
                        _ => {}
                    }
                }
            }
            other => bail!("markup: unknown tag <{other}>"),
        }
        stack.push((name.to_string(), style));
    }
    if stack.len() > 1 {
        bail!("markup: <{}> is never closed", stack.last().unwrap().0);
    }
    spans.retain(|s| !s.text.is_empty());
    Ok(spans)
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_ascii_lowercase();
        let after = rest[eq + 1..].trim_start();
        let (val, next) = if let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') {
            let end = after[1..].find(q).map(|e| e + 1).unwrap_or(after.len());
            (&after[1..end], &after[(end + 1).min(after.len())..])
        } else {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            (&after[..end], &after[end..])
        };
        out.push((key, val.to_string()));
        rest = next.trim_start();
    }
    out
}

/// A number shown as text (Manim's `DecimalNumber`). Rebuild it in an updater to animate it.
pub struct DecimalNumber;
impl DecimalNumber {
    pub fn new(value: f64, decimals: usize) -> VMobject {
        Self::styled(value, decimals, TextStyle::default())
    }

    pub fn styled(value: f64, decimals: usize, style: TextStyle) -> VMobject {
        let s = format!("{value:.decimals$}");
        let s =
            if s.starts_with('-') && s.trim_start_matches(['-', '0', '.']).is_empty() { s[1..].to_string() } else { s };
        Text::styled(&s.replace('-', "\u{2212}"), style).with_kind("DecimalNumber")
    }
}

/// Manim's `Integer`.
pub struct Integer;
impl Integer {
    pub fn new(value: i64) -> VMobject {
        DecimalNumber::new(value as f64, 0).with_kind("Integer")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_spans() {
        let base = TextStyle { color: Color::rgb(1.0, 1.0, 1.0), ..TextStyle::default() };
        let spans =
            parse_markup("a <b>bold</b> &amp; <span color=\"#ff0000\" weight=\"semibold\">red</span>", &base).unwrap();
        let texts: Vec<&str> = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["a ", "bold", " & ", "red"]);
        assert_eq!(spans[1].style.weight, BOLD);
        assert_eq!(spans[3].style.weight, SEMIBOLD);
        assert_eq!(spans[3].style.color, Color::rgb(1.0, 0.0, 0.0));
        assert!(parse_markup("<b>x", &base).is_err());
        assert!(parse_markup("<b>x</i>", &base).is_err());
    }
}
