//! Manim's `Code`: a syntax-highlighted listing with line numbers on a rectangle or a
//! window-style background.
//!
//! ```no_run
//! # use manim::prelude::*;
//! let code = Code::new("def f(x):\n    return x * 2\n")
//!     .language("python")
//!     .background(CodeBackground::Window)
//!     .build()
//!     .unwrap();
//! ```
//!
//! The built mobject (kind `"Code"`) holds, in draw order: the background (kind
//! `"CodeBackground"`, with the window buttons as its children), the line numbers (kind
//! `"CodeLineNumbers"`, one group per line) and the code (kind `"CodeLines"`, one text
//! group per line whose label is the line). So `code_lines.children[2]` is the third
//! line, ready for `SurroundingRectangle` or `set_color_text`.

use std::path::Path;

use anyhow::{anyhow, Context, Result};

use super::geometry::{Dot, RoundedRectangle};
use super::highlight;
use super::VMobject;
use crate::color::{Color, WHITE};
use crate::constants::*;
use crate::text::{has_font, Align, Span, Text, TextStyle};

/// `kind` of the code lines group.
pub const LINES: &str = "CodeLines";
/// `kind` of the line numbers group.
pub const LINE_NUMBERS: &str = "CodeLineNumbers";
/// `kind` of the background.
pub const BACKGROUND: &str = "CodeBackground";

/// Fonts tried, in order, when neither the code nor the theme names one.
const MONOSPACE: &[&str] = &[
    "Consolas",
    "Cascadia Mono",
    "Menlo",
    "SF Mono",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Ubuntu Mono",
    "Noto Sans Mono",
    "Courier New",
];

/// The background behind a listing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CodeBackground {
    /// A rounded rectangle.
    #[default]
    Rectangle,
    /// A rounded rectangle with red, yellow and green buttons, like a macOS window.
    Window,
}

/// A syntax-highlighted code listing (Manim's `Code`).
#[derive(Clone, Debug)]
pub struct Code {
    pub code: String,
    /// Language name or alias (`"python"`, `"rs"`, `"ts"`, ...); `None` guesses.
    pub language: Option<String>,
    /// A Pygments style name: `"vim"` (Manim's default), `"monokai"`, `"dracula"`, ...
    pub formatter_style: String,
    pub tab_width: usize,
    pub add_line_numbers: bool,
    pub line_numbers_from: usize,
    pub background: CodeBackground,
    /// Font family; `None` uses the theme's code font, else an installed monospace font.
    pub font: Option<String>,
    pub font_size: f64,
    /// Baseline-to-baseline distance in multiples of the font size.
    pub line_spacing: f64,
    /// Padding between the text and the background's edge.
    pub background_buff: f64,
    pub corner_radius: f64,
    /// Background fill; `None` uses the style's background color.
    pub background_fill: Option<Color>,
    pub background_fill_opacity: f64,
    pub background_stroke_color: Color,
    pub background_stroke_width: f64,
}

impl Code {
    /// Highlight `code` (Manim's `Code(code_string=...)`).
    pub fn new(code: &str) -> Self {
        Code {
            code: code.to_string(),
            language: None,
            formatter_style: "vim".into(),
            tab_width: 4,
            add_line_numbers: true,
            line_numbers_from: 1,
            background: CodeBackground::Rectangle,
            font: None,
            font_size: 24.0,
            line_spacing: 1.04,
            background_buff: 0.3,
            corner_radius: 0.2,
            background_fill: None,
            background_fill_opacity: 1.0,
            background_stroke_color: WHITE,
            background_stroke_width: 1.0,
        }
    }

    /// Read and highlight a file; the language comes from its extension when known
    /// (Manim's `Code(code_file=...)`).
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut c = Self::new(&text);
        c.language =
            path.extension().and_then(|e| highlight::language_for_extension(&e.to_string_lossy())).map(String::from);
        Ok(c)
    }

    pub fn language(mut self, language: &str) -> Self {
        self.language = Some(language.to_string());
        self
    }
    /// Pygments style name (Manim's `formatter_style`); see [`Code::styles`].
    pub fn style(mut self, name: &str) -> Self {
        self.formatter_style = name.to_string();
        self
    }
    pub fn tab_width(mut self, n: usize) -> Self {
        self.tab_width = n;
        self
    }
    pub fn line_numbers(mut self, on: bool) -> Self {
        self.add_line_numbers = on;
        self
    }
    pub fn line_numbers_from(mut self, first: usize) -> Self {
        self.line_numbers_from = first;
        self
    }
    pub fn background(mut self, b: CodeBackground) -> Self {
        self.background = b;
        self
    }
    pub fn font(mut self, family: &str) -> Self {
        self.font = Some(family.to_string());
        self
    }
    pub fn font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }
    pub fn line_spacing(mut self, s: f64) -> Self {
        self.line_spacing = s;
        self
    }
    pub fn background_fill(mut self, c: Color, opacity: f64) -> Self {
        self.background_fill = Some(c);
        self.background_fill_opacity = opacity;
        self
    }
    pub fn background_stroke(mut self, c: Color, width: f64) -> Self {
        self.background_stroke_color = c;
        self.background_stroke_width = width;
        self
    }
    pub fn corner_radius(mut self, r: f64) -> Self {
        self.corner_radius = r;
        self
    }
    pub fn background_buff(mut self, b: f64) -> Self {
        self.background_buff = b;
        self
    }

    /// Available style names (Manim's `Code.get_styles_list()`).
    pub fn styles() -> Vec<&'static str> {
        highlight::styles()
    }

    /// Supported languages (canonical names).
    pub fn languages() -> Vec<&'static str> {
        highlight::languages()
    }

    /// Lay the listing out, centered at the origin. Fails for an unknown language or style.
    pub fn build(&self) -> Result<VMobject> {
        let style = highlight::style(&self.formatter_style)?;
        let lang = match &self.language {
            Some(l) => highlight::language_name(l)
                .ok_or_else(|| anyhow!("unknown language '{l}' (known: {})", highlight::languages().join(", ")))?,
            None => highlight::guess_language(&self.code),
        };
        let normalized = self.code.replace("\r\n", "\n");
        let expanded = expand_tabs(&normalized, self.tab_width);
        let code = expanded.strip_suffix('\n').unwrap_or(&expanded);
        let hex = |h: &str| Color::hex(h).unwrap_or(WHITE);
        let text_color = hex(style.text);
        // A color per char of `code`.
        let mut colors: Vec<Color> = Vec::with_capacity(code.len());
        for t in highlight::tokenize(code, lang)? {
            let c = hex(style.color(t.kind));
            colors.extend(t.text.chars().map(|_| c));
        }
        let lines: Vec<&str> = code.split('\n').collect();
        let last = lines.len() - 1;
        // Like Manim, pad the first and last line with glyphs that have an ascender and
        // a descender, so the listing's height doesn't depend on its contents. The
        // digits also give the line numbers a baseline to line up with.
        let suffix = format!(" pA{}", self.line_numbers_from);
        let digits = self.line_numbers_from.to_string().chars().count();
        let font = self.font.clone().unwrap_or_else(code_font);
        let base =
            TextStyle::default().font(&font).size(self.font_size).line_spacing(self.line_spacing).color(text_color);
        let mut spans: Vec<Span> = Vec::new();
        let mut push = |s: &str, c: Color| match spans.last_mut() {
            Some(sp) if sp.style.color == c => sp.text.push_str(s),
            _ => spans.push(Span::new(s, base.clone().color(c))),
        };
        // (first char, code chars, chars with padding) per line, in chars of the full text.
        let mut ranges = Vec::with_capacity(lines.len());
        let (mut at, mut ci) = (0usize, 0usize);
        for (i, line) in lines.iter().enumerate() {
            let n = line.chars().count();
            for (k, ch) in line.chars().enumerate() {
                push(ch.encode_utf8(&mut [0; 4]), colors[ci + k]);
            }
            ci += n + 1;
            let mut full = n;
            if i == 0 || i == last {
                push(&suffix, text_color);
                full += suffix.chars().count();
            }
            ranges.push((at, n, full));
            if i < last {
                push("\n", text_color);
            }
            at += full + 1;
        }
        let laid = Text::rich(&spans, &base);
        let mut line_glyphs: Vec<Vec<VMobject>> = vec![Vec::new(); lines.len()];
        let mut padding = Vec::new();
        let mut anchor_digits = Vec::new();
        for mut g in laid.children {
            let Some(c) = g.meta.char_index.map(|c| c as usize) else { continue };
            let Some(i) = ranges.iter().position(|(s, _, full)| c >= *s && c < s + full) else { continue };
            let (s, n, full) = ranges[i];
            if c - s < n {
                g.meta.char_index = Some((c - s) as u32);
                line_glyphs[i].push(g);
            } else {
                if i == 0 && c - s >= full - digits {
                    anchor_digits.push(g.clone());
                }
                padding.push(g);
            }
        }
        let code_lines = VMobject::group(
            line_glyphs
                .into_iter()
                .zip(&lines)
                .map(|(gs, l)| VMobject::group(gs).with_kind("Text").with_label(l))
                .collect(),
        )
        .with_kind(LINES);
        let padding = VMobject::group(padding);
        let text_box = VMobject::group(vec![code_lines.clone(), padding.clone()]);
        let mut parts = Vec::new();
        if self.add_line_numbers {
            let n_color = hex(style.line_number);
            let numbers: Vec<String> =
                (self.line_numbers_from..self.line_numbers_from + lines.len()).map(|n| n.to_string()).collect();
            let laid = Text::styled(&numbers.join("\n"), base.clone().color(n_color).align(Align::Right));
            let mut per_line: Vec<Vec<VMobject>> = vec![Vec::new(); numbers.len()];
            let mut start = 0usize;
            let starts: Vec<usize> = numbers
                .iter()
                .map(|n| {
                    let s = start;
                    start += n.chars().count() + 1;
                    s
                })
                .collect();
            for mut g in laid.children {
                let Some(c) = g.meta.char_index.map(|c| c as usize) else { continue };
                let i = starts.iter().rposition(|s| *s <= c).unwrap_or(0);
                g.meta.char_index = Some((c - starts[i]) as u32);
                per_line[i].push(g);
            }
            let mut nums = VMobject::group(
                per_line
                    .into_iter()
                    .zip(&numbers)
                    .map(|(gs, n)| VMobject::group(gs).with_kind("Text").with_label(n))
                    .collect(),
            )
            .with_kind(LINE_NUMBERS);
            nums = nums.next_to_buff(&text_box, LEFT, MED_SMALL_BUFF);
            let first = nums.children.first().map(|l| l.get_center().y);
            let anchor = VMobject::group(anchor_digits);
            if let (Some(y), Some(_)) = (first, anchor.bounding_box()) {
                nums = nums.shift(UP * (anchor.get_center().y - y));
            }
            parts.push(nums);
        }
        parts.push(code_lines);
        let content = VMobject::group([parts.clone(), vec![padding]].concat());
        let fill = self.background_fill.unwrap_or_else(|| hex(style.background));
        let rect = |around: &VMobject| {
            let mut r = RoundedRectangle::new(
                around.width() + 2.0 * self.background_buff,
                around.height() + 2.0 * self.background_buff,
                self.corner_radius,
            )
            .move_to(around.get_center())
            .set_fill(fill, self.background_fill_opacity)
            .set_stroke(self.background_stroke_color, self.background_stroke_width);
            r.meta.kind = Some(BACKGROUND.into());
            r
        };
        let background = match self.background {
            CodeBackground::Rectangle => rect(&content),
            CodeBackground::Window => {
                let buttons = VMobject::group(
                    ["#ff5f56", "#ffbd2e", "#27c93f"]
                        .iter()
                        .map(|c| Dot::new().scale(0.1 / DEFAULT_DOT_RADIUS).set_color(hex(c)).set_stroke_width(0.0))
                        .collect(),
                )
                .arrange_buff(RIGHT, 0.1)
                .next_to_buff(&content, UP, 0.1)
                .align_to(&content, LEFT)
                .shift(LEFT * 0.1);
                let bg = rect(&VMobject::group(vec![content.clone(), buttons.clone()]));
                bg.add(buttons.shift(UP * 0.1 + LEFT * 0.1))
            }
        };
        let mut children = vec![background];
        children.extend(parts);
        let mut m = VMobject::group(children).with_kind("Code").with_label(code);
        let center = m.get_center();
        m = m.shift(-center);
        Ok(m)
    }
}

/// The code font: the theme's, else the first installed monospace font.
fn code_font() -> String {
    let d = crate::defaults::get();
    if !d.code_font.is_empty() {
        return d.code_font;
    }
    MONOSPACE.iter().find(|f| has_font(f)).map(|f| f.to_string()).unwrap_or_default()
}

/// Python's `str.expandtabs`: each tab advances to the next multiple of `width`.
fn expand_tabs(s: &str, width: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for c in s.chars() {
        match c {
            '\t' => {
                if width > 0 {
                    let n = width - col % width;
                    out.extend(std::iter::repeat_n(' ', n));
                    col += n;
                }
            }
            '\n' => {
                out.push(c);
                col = 0;
            }
            _ => {
                out.push(c);
                col += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_expand_to_columns() {
        assert_eq!(expand_tabs("a\tb\n\tc", 4), "a   b\n    c");
        assert_eq!(expand_tabs("ab\t", 2), "ab  ");
        assert_eq!(expand_tabs("\tx", 0), "x");
    }
}
