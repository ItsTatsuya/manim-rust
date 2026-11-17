//! Raster images (Manim's `ImageMobject`): PNG or JPEG files, or raw pixels, placed
//! as a rectangle that moves, scales, rotates and fades like any mobject.

#![allow(clippy::new_ret_no_self)]

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::constants::*;
use crate::mobject::{ImageRef, RasterImage, Subpath, VMobject};

/// Image pixels per scene unit at the default size: 1080 pixels span the frame height,
/// so a 1080p-sized image shows pixel for pixel in a 1080p render.
const PIXELS_PER_UNIT: f64 = 1080.0 / FRAME_HEIGHT;

/// Namespace for constructing image mobjects; the constructors return a [`VMobject`].
pub struct ImageMobject;

impl ImageMobject {
    /// Load a PNG or JPEG.
    pub fn open(path: impl AsRef<Path>) -> Result<VMobject> {
        let path = path.as_ref();
        let img = image::open(path).with_context(|| format!("opening image {}", path.display()))?.to_rgba8();
        let (w, h) = img.dimensions();
        Ok(Self::from_rgba(w, h, img.into_raw())?.with_label(&path.display().to_string()))
    }

    /// Decode PNG or JPEG bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<VMobject> {
        let img = image::load_from_memory(bytes).context("decoding image")?.to_rgba8();
        let (w, h) = img.dimensions();
        Self::from_rgba(w, h, img.into_raw())
    }

    /// Straight (non-premultiplied) RGBA8 pixels, row by row from the top. Fails when
    /// `rgba` is not exactly `width * height * 4` bytes.
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<VMobject> {
        let image = ImageRef(Arc::new(RasterImage::from_rgba(width, height, rgba)?));
        let (hw, hh) = (width as f64 / PIXELS_PER_UNIT / 2.0, height as f64 / PIXELS_PER_UNIT / 2.0);
        let (ul, ur, dr, dl) = (pt(-hw, hh), pt(hw, hh), pt(hw, -hh), pt(-hw, -hh));
        let mut m = VMobject::from_subpaths(vec![Subpath::polyline(&[ul, ur, dr, dl], true)]);
        m.fill_opacity = 1.0;
        m.stroke_width = 0.0;
        m.meta.kind = Some("Image".into());
        m.meta.image = Some(image);
        m.meta.anchors = Some([ul, ur, dl]);
        Ok(m)
    }
}
