//! Camera: turns the world into a renderer-agnostic display list in pixel space.

use std::sync::{Arc, Mutex};

use crate::color::{Color, BLACK};
use crate::constants::*;
use crate::mobject::VMobject;
use crate::renderer::{DisplayList, DrawImage, DrawPath, Layer, PxSubpath};
use crate::world::World;

/// Matches Cairo Manim: 1 stroke-width unit = 0.01 scene units.
const STROKE_WIDTH_UNIT: f64 = 0.01;

/// Maps scene coordinates to pixels and turns a [`World`] into a [`DisplayList`].
#[derive(Clone, Debug)]
pub struct Camera {
    /// Output width in pixels.
    pub pixel_width: u32,
    /// Output height in pixels.
    pub pixel_height: u32,
    /// Height of the visible area in scene units; the width follows from the pixel aspect ratio.
    pub frame_height: f64,
    /// Scene point shown at the center of the frame.
    pub frame_center: Point,
    /// Frame background; its alpha is ignored (frames are always opaque).
    pub background: Color,
    layer_cache: Arc<Mutex<Option<Layer>>>,
}

impl Camera {
    /// A camera with the default frame (height 8 units, centered on the origin) and a black background.
    pub fn new(pixel_width: u32, pixel_height: u32) -> Self {
        Camera {
            pixel_width,
            pixel_height,
            frame_height: FRAME_HEIGHT,
            frame_center: ORIGIN,
            background: BLACK,
            layer_cache: Default::default(),
        }
    }

    /// Width of the visible area in scene units.
    pub fn frame_width(&self) -> f64 {
        self.frame_height * self.pixel_width as f64 / self.pixel_height as f64
    }

    /// Pixel position (x right, y down, origin top-left) of a scene point.
    pub fn to_pixel(&self, p: Point) -> [f32; 2] {
        let (w, h) = (self.pixel_width as f64, self.pixel_height as f64);
        let d = p - self.frame_center;
        [((d.x / self.frame_width() + 0.5) * w) as f32, ((0.5 - d.y / self.frame_height) * h) as f32]
    }

    /// Build the display list for the current state of `world`: displayed mobjects sorted by z-index, then the overlay. The background layer is reused between frames when unchanged.
    pub fn display_list(&self, world: &World) -> DisplayList {
        let mut ids: Vec<_> = world.displayed().to_vec();
        ids.sort_by_key(|id| world[*id].z_index); // stable: keeps add order within a z level
        let mut items = Vec::new();
        for id in ids.into_iter().chain(world.overlay().iter().copied()) {
            self.push_items(&world[id], &mut items);
        }
        let background_layer = if world.background().is_empty() {
            None
        } else {
            let mut layer = Vec::new();
            for id in world.background() {
                self.push_items(&world[*id], &mut layer);
            }
            // Reuse the previous frame's Arc when nothing changed, so renderers can
            // cache the rasterized layer and frame dedup compares pointers.
            let prev = self.layer_cache.lock().unwrap_or_else(|e| e.into_inner()).clone();
            match prev {
                Some(prev) if *prev.0 == layer => Some(prev),
                _ => {
                    let l = Layer(Arc::new(layer));
                    *self.layer_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(l.clone());
                    Some(l)
                }
            }
        };
        DisplayList {
            width: self.pixel_width,
            height: self.pixel_height,
            // The frame is always opaque (premultiplied and straight pixels coincide, and the
            // PNG / video writers expect that): a translucent background color is made solid.
            background: {
                let b = self.background;
                let mut rgba = Color::rgb(b.r, b.g, b.b).to_array(1.0);
                rgba[3] = 1.0;
                rgba
            },
            background_layer,
            items,
        }
    }

    fn push_items(&self, mob: &VMobject, items: &mut Vec<DrawPath>) {
        let px_per_unit = self.pixel_width as f64 / self.frame_width();
        for m in mob.family_iter() {
            if let (Some(image), Some([ul, ur, dl])) = (&m.meta.image, m.meta.anchors) {
                let opacity = m.fill_opacity as f32;
                if opacity <= 0.0 {
                    continue;
                }
                let (w, h) = (image.0.width.max(1) as f32, image.0.height.max(1) as f32);
                let (o, x, y) = (self.to_pixel(ul), self.to_pixel(ur), self.to_pixel(dl));
                let transform =
                    [(x[0] - o[0]) / w, (x[1] - o[1]) / w, (y[0] - o[0]) / h, (y[1] - o[1]) / h, o[0], o[1]];
                if transform.iter().all(|v| v.is_finite()) {
                    items.push(DrawPath {
                        subpaths: Vec::new(),
                        fill: None,
                        stroke: None,
                        image: Some(DrawImage { image: image.clone(), transform, opacity }),
                        nonzero: false,
                    });
                }
                continue;
            }
            if m.subpaths.is_empty() {
                continue;
            }
            let fill = (m.fill_opacity > 0.0).then(|| m.fill_color.to_array(m.fill_opacity)).filter(|c| c[3] > 0.0);
            let stroke_px = m.stroke_width * STROKE_WIDTH_UNIT * px_per_unit;
            let stroke = (stroke_px > 0.0 && m.stroke_opacity > 0.0)
                .then(|| (m.stroke_color.to_array(m.stroke_opacity), stroke_px as f32))
                .filter(|(c, _)| c[3] > 0.0);
            if fill.is_none() && stroke.is_none() {
                continue;
            }
            let subpaths: Vec<PxSubpath> = m
                .subpaths
                .iter()
                .filter(|s| !s.curves.is_empty())
                .map(|s| PxSubpath {
                    start: self.to_pixel(s.curves[0][0]),
                    curves: s
                        .curves
                        .iter()
                        .map(|c| [self.to_pixel(c[1]), self.to_pixel(c[2]), self.to_pixel(c[3])])
                        .collect(),
                    closed: s.closed,
                })
                // Drop non-finite subpaths (e.g. NaN from a user's `apply_function`, or
                // coordinates beyond f32 range) instead of handing them to the rasterizers.
                .filter(|s| s.start.iter().chain(s.curves.iter().flatten().flatten()).all(|v| v.is_finite()))
                .collect();
            if !subpaths.is_empty() {
                items.push(DrawPath { subpaths, fill, stroke, image: None, nonzero: m.meta.nonzero });
            }
        }
    }
}
