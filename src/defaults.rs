//! Default style for new mobjects (Manim's `Mobject.set_default`), normally set from a
//! [`Theme`](crate::theme::Theme).
//!
//! Defaults are per thread, so two scenes built on different threads (a preview and a
//! render, say) never see each other's theme.

use std::cell::RefCell;

use crate::color::{Color, WHITE};
use crate::constants::DEFAULT_FONT_SIZE;

#[derive(Clone, Debug, PartialEq)]
pub struct Defaults {
    /// Stroke and fill color of new shapes.
    pub color: Color,
    /// Color of new `Text`.
    pub text_color: Color,
    /// Font family of new `Text` (empty = the system sans-serif).
    pub font: String,
    pub font_size: f64,
    /// Fonts tried, in order, for characters the chosen font lacks.
    pub fallback_fonts: Vec<String>,
}

impl Default for Defaults {
    fn default() -> Self {
        Defaults {
            color: WHITE,
            text_color: WHITE,
            font: String::new(),
            font_size: DEFAULT_FONT_SIZE,
            fallback_fonts: Vec::new(),
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Defaults> = RefCell::new(Defaults::default());
}

/// Default color of new shapes.
pub fn color() -> Color {
    CURRENT.with(|d| d.borrow().color)
}

pub fn get() -> Defaults {
    CURRENT.with(|d| d.borrow().clone())
}

pub fn set(defaults: Defaults) {
    CURRENT.with(|d| *d.borrow_mut() = defaults);
}

/// Change some fields: `defaults::update(|d| d.text_color = BLACK)`.
pub fn update(f: impl FnOnce(&mut Defaults)) {
    CURRENT.with(|d| f(&mut d.borrow_mut()));
}

pub fn reset() {
    set(Defaults::default());
}
