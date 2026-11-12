//! CPU rasterizer built on tiny-skia (the Cairo equivalent). Frames are independent,
//! so a batch is rasterized across all cores with rayon.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use rayon::prelude::*;
use tiny_skia::{
    FillRule, FilterQuality, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, PixmapRef, Stroke, Transform,
};

use super::{DisplayList, DrawPath, Layer, Renderer};

pub struct CpuRenderer {
    threads: usize,
}

impl CpuRenderer {
    pub fn new() -> Self {
        CpuRenderer { threads: rayon::current_num_threads() }
    }
}

impl Default for CpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

fn color(c: [f32; 4]) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(c[0], c[1], c[2], c[3]).unwrap_or(tiny_skia::Color::BLACK)
}

/// Rasterized background layers, keyed by the layer's pointer. The `Arc` is kept so a
/// freed layer's address can't be reused by a different one while it is cached.
type LayerCache = Vec<(Arc<Vec<DrawPath>>, u32, u32, [u32; 4], Arc<Pixmap>)>;
static LAYERS: Mutex<LayerCache> = Mutex::new(Vec::new());
const MAX_CACHED_LAYERS: usize = 4;

fn base_pixmap(dl: &DisplayList) -> Pixmap {
    let (w, h) = (dl.width.max(1), dl.height.max(1));
    let blank = || {
        let mut pm = Pixmap::new(w, h).expect("valid frame size");
        pm.fill(color(dl.background));
        pm
    };
    let Some(Layer(layer)) = &dl.background_layer else { return blank() };
    let bg = dl.background.map(f32::to_bits);
    let found = {
        let cache = LAYERS.lock().unwrap();
        cache.iter().find(|e| Arc::ptr_eq(&e.0, layer) && e.1 == w && e.2 == h && e.3 == bg).map(|e| e.4.clone())
    };
    let pm = match found {
        Some(pm) => pm,
        None => {
            let mut pm = blank();
            draw_items(&mut pm, layer.iter());
            let pm = Arc::new(pm);
            let mut cache = LAYERS.lock().unwrap();
            if cache.len() >= MAX_CACHED_LAYERS {
                cache.remove(0);
            }
            cache.push((layer.clone(), w, h, bg, pm.clone()));
            pm
        }
    };
    (*pm).clone()
}

fn draw_items<'a>(pm: &mut Pixmap, items: impl IntoIterator<Item = &'a DrawPath>) {
    let mut paint = Paint { anti_alias: true, ..Default::default() };
    for item in items {
        if let Some(img) = &item.image {
            let data = &img.image.0;
            let Some(src) = PixmapRef::from_bytes(&data.premultiplied, data.width, data.height) else { continue };
            let [sx, ky, kx, sy, tx, ty] = img.transform;
            let pp = PixmapPaint {
                opacity: img.opacity.clamp(0.0, 1.0),
                quality: FilterQuality::Bicubic,
                ..Default::default()
            };
            pm.draw_pixmap(0, 0, src, &pp, Transform::from_row(sx, ky, kx, sy, tx, ty), None);
            continue;
        }
        let mut pb = PathBuilder::new();
        for sp in &item.subpaths {
            pb.move_to(sp.start[0], sp.start[1]);
            for c in &sp.curves {
                pb.cubic_to(c[0][0], c[0][1], c[1][0], c[1][1], c[2][0], c[2][1]);
            }
            if sp.closed {
                pb.close();
            }
        }
        let Some(path) = pb.finish() else { continue };
        if let Some(fill) = item.fill {
            paint.set_color(color(fill));
            let rule = if item.nonzero { FillRule::Winding } else { FillRule::EvenOdd };
            pm.fill_path(&path, &paint, rule, Transform::identity(), None);
        }
        if let Some((c, width)) = item.stroke {
            paint.set_color(color(c));
            let stroke = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
            pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }
}

/// Rasterize one display list into RGBA8.
pub fn rasterize(dl: &DisplayList) -> Vec<u8> {
    let mut pm = base_pixmap(dl);
    draw_items(&mut pm, &dl.items);
    // The background is opaque, so premultiplied == straight alpha here.
    pm.take()
}

impl Renderer for CpuRenderer {
    fn name(&self) -> String {
        format!("CPU (tiny-skia, {} threads)", self.threads)
    }

    fn render_batch(&mut self, frames: &[&DisplayList]) -> Result<Vec<Vec<u8>>> {
        Ok(frames.par_iter().map(|dl| rasterize(dl)).collect())
    }

    fn batch_size(&self) -> usize {
        (self.threads * 2).clamp(2, 64)
    }
}
