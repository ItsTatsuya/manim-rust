//! Themes ("house styles"): named color roles, font roles, type sizes, margins and a
//! background pattern, shared by every scene of a project.
//!
//! The JSON shape matches Manim Studio's `styles.json` entries, so a style can be passed
//! straight through. Unknown fields are ignored and missing ones take defaults:
//!
//! ```json
//! { "name": "Studio",
//!   "fonts":  { "heading": "Plus Jakarta Sans", "body": "Inter", "code": "JetBrains Mono", "hand": "Caveat" },
//!   "colors": { "background": "#FFFFFF", "text": "#0F172A", "muted": "#64748B", "primary": "#4F46E5", ... },
//!   "sizes":  { "title": 54, "heading": 40, "body": 30, "small": 22 },
//!   "layout": { "margin": 0.65, "title": "top-left", "pattern": "grid" },
//!   "pacing": { "run_time": 1.0, "wait": 1.0, "words_per_second": 2.4 } }
//! ```
//!
//! [`Scene::set_theme`](crate::Scene::set_theme) applies one: background color and
//! pattern, and the defaults new shapes and `Text` use.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::color::Color;
use crate::constants::*;
use crate::defaults::Defaults;
use crate::mobject::geometry::{Circle, Line};
use crate::mobject::VMobject;

fn de_color<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Color, D::Error> {
    let s = String::deserialize(d)?;
    Color::hex(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid color {s:?} (expected #RRGGBB)")))
}

fn ser_color<S: Serializer>(c: &Color, s: S) -> std::result::Result<S::Ok, S::Error> {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    s.serialize_str(&format!("#{:02X}{:02X}{:02X}", b(c.r), b(c.g), b(c.b)))
}

macro_rules! color_roles {
    ($($name:ident = $default:expr),* $(,)?) => {
        /// The colors of a theme, one per named role (see [`ThemeColors::ROLES`]).
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct ThemeColors {
            $(
                #[doc = concat!("Color of the `", stringify!($name), "` role.")]
                #[serde(deserialize_with = "de_color", serialize_with = "ser_color")]
                pub $name: Color,
            )*
        }
        impl Default for ThemeColors {
            fn default() -> Self {
                ThemeColors { $($name: Color::from_hex($default)),* }
            }
        }
        impl ThemeColors {
            /// Look a role up by name (`"primary"`, `"key_fill"`, ...).
            pub fn get(&self, role: &str) -> Option<Color> {
                match role {
                    $(stringify!($name) => Some(self.$name),)*
                    _ => None,
                }
            }
            /// Every role name, in declaration order.
            pub const ROLES: &'static [&'static str] = &[$(stringify!($name)),*];
        }
    };
}

color_roles! {
    background = 0x0F172A,
    text = 0xF8FAFC,
    muted = 0x94A3B8,
    primary = 0x818CF8,
    secondary = 0x38BDF8,
    accent = 0xFB7185,
    positive = 0x34D399,
    negative = 0xF87171,
    key_fill = 0x1E1B4B,
    example_fill = 0x0C2A3A,
}

/// Font families by role (empty = system default).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeFonts {
    /// Font for titles and headings.
    pub heading: String,
    /// Font for body text.
    pub body: String,
    /// Font for code.
    pub code: String,
    /// Handwriting, for notes and annotations (falls back to `body`).
    pub hand: String,
    /// `"auto"`, `"typst"` or `"tex"`: how equations are typeset.
    pub math: String,
    /// Fonts tried for characters the role fonts lack (e.g. Devanagari).
    pub fallback: Vec<String>,
}

impl Default for ThemeFonts {
    fn default() -> Self {
        ThemeFonts {
            heading: String::new(),
            body: String::new(),
            code: "Consolas".into(),
            hand: String::new(),
            math: "auto".into(),
            fallback: Vec::new(),
        }
    }
}

/// Type sizes in points, by role.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeSizes {
    /// Slide title size.
    pub title: f64,
    /// Section heading size.
    pub heading: f64,
    /// Body text size.
    pub body: f64,
    /// Captions and labels.
    pub small: f64,
}

impl Default for ThemeSizes {
    fn default() -> Self {
        ThemeSizes { title: 50.0, heading: 38.0, body: 30.0, small: 22.0 }
    }
}

/// Background pattern drawn behind the scene.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Pattern {
    /// Plain background.
    #[default]
    None,
    /// A line grid.
    Grid,
    /// A dot grid.
    Dots,
}

/// Where slide titles sit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum TitlePosition {
    /// Top-left, at the margin.
    #[default]
    TopLeft,
    /// Top, horizontally centered.
    TopCenter,
    /// Centered in the frame.
    Center,
}

/// How boxes around content are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum BoxStyle {
    /// Outline only.
    #[default]
    Outline,
    /// Filled card.
    Card,
}

/// Margins, title placement and background pattern.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeLayout {
    /// Distance kept from the frame edges, in scene units.
    pub margin: f64,
    /// Where titles are placed.
    pub title: TitlePosition,
    /// A bar under titles.
    pub accent: bool,
    /// Box style.
    pub boxes: BoxStyle,
    /// Background pattern.
    pub pattern: Pattern,
    /// Spacing of the pattern, in scene units.
    pub pattern_spacing: f64,
}

impl Default for ThemeLayout {
    fn default() -> Self {
        ThemeLayout {
            margin: 0.6,
            title: TitlePosition::TopLeft,
            accent: false,
            boxes: BoxStyle::Outline,
            pattern: Pattern::None,
            pattern_spacing: 0.5,
        }
    }
}

/// Default timings for scripts that pace themselves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemePacing {
    /// Seconds per animation.
    pub run_time: f64,
    /// Seconds of pause between steps.
    pub wait: f64,
    /// Reading speed used to time text (words per second).
    pub words_per_second: f64,
}

impl Default for ThemePacing {
    fn default() -> Self {
        ThemePacing { run_time: 1.0, wait: 1.0, words_per_second: 2.4 }
    }
}

/// A complete house style: colors, fonts, sizes, layout and pacing. Apply it with [`Scene::set_theme`](crate::Scene::set_theme).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    /// Identifier (e.g. a key in a styles file).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Color roles.
    pub colors: ThemeColors,
    /// Font roles.
    pub fonts: ThemeFonts,
    /// Type sizes.
    pub sizes: ThemeSizes,
    /// Margins, title placement, pattern.
    pub layout: ThemeLayout,
    /// Pacing defaults.
    pub pacing: ThemePacing,
}

/// The text roles a theme styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextRole {
    /// Slide titles: heading font, title size, text color, semibold.
    Title,
    /// Section headings: heading font, heading size, primary color, medium.
    Heading,
    /// Running text: body font and size, text color.
    Body,
    /// Captions and labels: body font, small size, muted color.
    Caption,
    /// Code: code font, small size, text color.
    Code,
    /// Handwritten notes: hand font, a little larger, accent color.
    Hand,
}

impl std::str::FromStr for TextRole {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "title" => TextRole::Title,
            "heading" => TextRole::Heading,
            "body" => TextRole::Body,
            "caption" | "small" => TextRole::Caption,
            "code" => TextRole::Code,
            "hand" | "note" => TextRole::Hand,
            _ => anyhow::bail!("unknown text role '{s}' (title, heading, body, caption, code or hand)"),
        })
    }
}

impl Theme {
    /// Parse a theme from JSON (see the module docs for the shape).
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("parsing theme JSON")
    }

    /// Read and parse a theme file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).with_context(|| format!("reading theme {}", path.display()))?;
        Self::from_json(&text).with_context(|| format!("in {}", path.display()))
    }

    /// Serialize to pretty-printed JSON that [`Theme::from_json`] reads back.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("themes always serialize")
    }

    /// The defaults a scene gets from this theme.
    pub fn defaults(&self) -> Defaults {
        Defaults {
            color: self.colors.text,
            text_color: self.colors.text,
            font: self.fonts.body.clone(),
            font_size: self.sizes.body,
            fallback_fonts: self.fonts.fallback.clone(),
            code_font: self.fonts.code.clone(),
        }
    }

    /// Style for a text role.
    #[cfg(feature = "text")]
    pub fn text_style(&self, role: TextRole) -> crate::text::TextStyle {
        use crate::text::{TextStyle, MEDIUM, NORMAL, SEMIBOLD};
        let base = TextStyle { fallback_fonts: self.fonts.fallback.clone(), ..TextStyle::default() };
        let (font, size, color, weight) = match role {
            TextRole::Title => (&self.fonts.heading, self.sizes.title, self.colors.text, SEMIBOLD),
            TextRole::Heading => (&self.fonts.heading, self.sizes.heading, self.colors.primary, MEDIUM),
            TextRole::Body => (&self.fonts.body, self.sizes.body, self.colors.text, NORMAL),
            TextRole::Caption => (&self.fonts.body, self.sizes.small, self.colors.muted, NORMAL),
            TextRole::Code => (&self.fonts.code, self.sizes.small, self.colors.text, NORMAL),
            TextRole::Hand => {
                let hand = !self.fonts.hand.is_empty();
                let font = if hand { &self.fonts.hand } else { &self.fonts.body };
                // Handwriting fonts draw small and thin for their size.
                let size = if hand { self.sizes.body * 1.35 } else { self.sizes.body };
                (font, size, self.colors.accent, if hand { SEMIBOLD } else { NORMAL })
            }
        };
        base.font(font).size(size).color(color).weight(weight)
    }

    /// A `Text` in a role's style.
    #[cfg(feature = "text")]
    pub fn text(&self, role: TextRole, s: &str) -> VMobject {
        crate::text::Text::styled(s, self.text_style(role))
    }

    /// The area inside the margins, `(left, right, bottom, top)` in scene units, for a
    /// frame `frame_width` × `frame_height`.
    pub fn safe_area(&self, frame_width: f64, frame_height: f64) -> (f64, f64, f64, f64) {
        let m = self.layout.margin;
        (-frame_width / 2.0 + m, frame_width / 2.0 - m, -frame_height / 2.0 + m, frame_height / 2.0 - m)
    }

    /// The background pattern as one mobject covering the frame (`None` for no pattern).
    pub fn pattern_mobject(&self, frame_width: f64, frame_height: f64) -> Option<VMobject> {
        let step = self.layout.pattern_spacing.max(0.05);
        let paper = self.colors.background;
        let ink = self.colors.text;
        let (hw, hh) = (frame_width / 2.0, frame_height / 2.0);
        let cols = (hw / step).ceil() as i64 + 1;
        let rows = (hh / step).ceil() as i64 + 1;
        let mut children = Vec::new();
        match self.layout.pattern {
            Pattern::None => return None,
            Pattern::Grid => {
                let color = paper.lerp(ink, 0.07);
                for i in -cols..=cols {
                    let x = i as f64 * step;
                    children.push(Line::new(pt(x, -hh), pt(x, hh)).set_stroke(color, 1.2));
                }
                for j in -rows..=rows {
                    let y = j as f64 * step;
                    children.push(Line::new(pt(-hw, y), pt(hw, y)).set_stroke(color, 1.2));
                }
            }
            Pattern::Dots => {
                let color = paper.lerp(ink, 0.16);
                let r = (frame_width / 900.0).max(0.012);
                for i in -cols..=cols {
                    for j in -rows..=rows {
                        let dot = Circle::new(r).set_fill(color, 1.0).set_stroke_width(0.0);
                        children.push(dot.move_to(pt(i as f64 * step, j as f64 * step)));
                    }
                }
            }
        }
        Some(VMobject::group(children).with_kind("Pattern"))
    }

    /// Words per second for estimating how long a sentence takes to say.
    pub fn speech_seconds(&self, text: &str) -> f64 {
        (text.split_whitespace().count() as f64 / self.pacing.words_per_second.max(0.1)).max(0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_studio_style() {
        let json = r##"{"id":"clean","name":"Studio","description":"ignored",
            "fonts":{"heading":"Plus Jakarta Sans","body":"Inter","code":"JetBrains Mono","hand":"Caveat","math":"auto"},
            "colors":{"background":"#FFFFFF","text":"#0F172A","muted":"#64748B","primary":"#4F46E5",
                      "secondary":"#0284C7","accent":"#E11D48","positive":"#059669","negative":"#DC2626",
                      "key_fill":"#EEF0FF","example_fill":"#E6F4FB"},
            "sizes":{"title":54,"heading":40,"body":30,"small":22},
            "layout":{"margin":0.65,"title":"top-left","boxes":"card","accent":true,"pattern":"grid"},
            "pacing":{"run_time":1.0,"wait":1.0,"words_per_second":2.4},
            "voice":"x","rules":"y"}"##;
        let t = Theme::from_json(json).unwrap();
        assert_eq!(t.colors.background, Color::from_hex(0xFFFFFF));
        assert_eq!(t.colors.get("key_fill"), Some(Color::from_hex(0xEEF0FF)));
        assert_eq!(t.layout.pattern, Pattern::Grid);
        assert_eq!(t.layout.boxes, BoxStyle::Card);
        assert_eq!(t.sizes.title, 54.0);
        let back = Theme::from_json(&t.to_json()).unwrap();
        assert_eq!(back, t);
        assert!(Theme::from_json(r#"{"colors":{"text":"blue"}}"#).is_err());
        assert!(t.pattern_mobject(FRAME_WIDTH, FRAME_HEIGHT).is_some());
    }
}
