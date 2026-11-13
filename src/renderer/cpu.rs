//! CPU rasterizers. Frames are independent, so a batch is rasterized across all cores
//! with rayon, one frame per worker.
//!
//! Two rasterizers are available: tiny-skia (the Cairo equivalent, lowest fixed cost per
//! frame) and, with the `vello` feature, vello_cpu (scales far better with path count).
//! [`Raster::Adaptive`] groups frames by path count, times both rasterizers on the first
//! few live frames of each group, and from then on uses the faster one for that group.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use rayon::prelude::*;
use tiny_skia::{
    FillRule, FilterQuality, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, PixmapRef, Stroke, Transform,
};

use super::{DisplayList, DrawPath, Layer, Renderer};

/// Which rasterizer a [`CpuRenderer`] uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raster {
    /// tiny-skia only. Deterministic; used by tests and `--renderer skia`.
    Skia,
    /// vello_cpu, falling back to tiny-skia for frames with images.
    #[cfg(feature = "vello")]
    Vello,
    /// Measure both per complexity bucket and keep the faster.
    #[cfg(feature = "vello")]
    Adaptive,
}

impl Raster {
    /// The best default for this build.
    pub fn best() -> Self {
        #[cfg(feature = "vello")]
        return Raster::Adaptive;
        #[cfg(not(feature = "vello"))]
        Raster::Skia
    }
}

pub struct CpuRenderer {
    threads: usize,
    raster: Raster,
    #[cfg(feature = "vello")]
    calib: Calibration,
}

impl CpuRenderer {
    /// The best rasterizer this build has (adaptive when vello is compiled in).
    pub fn new() -> Self {
        Self::with_raster(Raster::best())
    }

    pub fn with_raster(raster: Raster) -> Self {
        CpuRenderer {
            threads: rayon::current_num_threads(),
            raster,
            #[cfg(feature = "vello")]
            calib: Calibration::default(),
        }
    }
}

/// Per-bucket timings for [`Raster::Adaptive`]. A frame's bucket is ⌊log2(curves + 1)⌋
/// over its scene items, so each bucket covers a 2× range of path complexity.
#[cfg(feature = "vello")]
struct Calibration {
    /// `[tiny-skia, vello]` fastest seconds seen and sample counts per bucket. The minimum
    /// rather than the mean, so one-off costs (a worker's first vello context) don't count.
    best: [[f64; 2]; 32],
    samples: [[u32; 2]; 32],
}

#[cfg(feature = "vello")]
impl Default for Calibration {
    fn default() -> Self {
        Calibration { best: [[f64::INFINITY; 2]; 32], samples: [[0; 2]; 32] }
    }
}

#[cfg(feature = "vello")]
#[derive(Clone, Copy)]
enum Plan {
    Use(usize),
    /// Run this rasterizer and record its time under the bucket.
    Time(usize, usize),
}

#[cfg(feature = "vello")]
impl Calibration {
    const SAMPLES: u32 = 3;

    fn bucket(dl: &DisplayList) -> usize {
        let curves: usize = dl.items.iter().flat_map(|i| &i.subpaths).map(|s| s.curves.len() + 1).sum();
        ((usize::BITS - 1 - (curves + 1).leading_zeros()) as usize).min(31)
    }

    fn calibrated(&self, b: usize) -> bool {
        self.samples[b].iter().all(|&n| n >= Self::SAMPLES)
    }

    /// Buckets at or above this go to vello before they are measured (measured crossover
    /// on a 16-thread Zen 3: vello wins from roughly 1–2k curves per frame).
    const VELLO_PRIOR: usize = 10;

    /// Pick a rasterizer per frame. Until a bucket has `SAMPLES` timings of both, its
    /// frames alternate between the two, counting frames already planned in this batch.
    /// Beyond that within the same batch (timings arrive only after it), the prior decides.
    fn plan(&self, frames: &[&DisplayList]) -> Vec<Plan> {
        let mut pending = self.samples;
        frames
            .iter()
            .map(|dl| {
                if dl.has_images() {
                    return Plan::Use(0);
                }
                let b = Self::bucket(dl);
                if self.calibrated(b) {
                    return Plan::Use(usize::from(self.best[b][1] < self.best[b][0]));
                }
                let n = &mut pending[b];
                if n.iter().all(|&k| k >= Self::SAMPLES) {
                    return Plan::Use(usize::from(b >= Self::VELLO_PRIOR));
                }
                let r = usize::from(n[1] < n[0]);
                n[r] += 1;
                Plan::Time(b, r)
            })
            .collect()
    }

    fn record(&mut self, b: usize, r: usize, secs: f64) {
        let was = self.calibrated(b);
        self.best[b][r] = self.best[b][r].min(secs);
        self.samples[b][r] += 1;
        if !was && self.calibrated(b) && std::env::var_os("MANIM_TRACE").is_some() {
            let [s, v] = self.best[b];
            eprintln!(
                "trace: cpu raster bucket {b} (~{} curves): tiny-skia {:.2} ms, vello {:.2} ms → {}",
                1usize << b,
                s * 1e3,
                v * 1e3,
                if v < s { "vello" } else { "tiny-skia" }
            );
        }
    }
}

/// 0 = tiny-skia, 1 = vello (falls back to tiny-skia for frames with images).
#[cfg(feature = "vello")]
fn rasterize_with(dl: &DisplayList, raster: usize) -> Vec<u8> {
    if raster == 1 {
        if let Some(px) = super::vello::rasterize(dl) {
            return px;
        }
    }
    rasterize(dl)
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

pub(super) fn base_pixmap(dl: &DisplayList) -> Pixmap {
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
        let r = match self.raster {
            Raster::Skia => "tiny-skia",
            #[cfg(feature = "vello")]
            Raster::Vello => "vello",
            #[cfg(feature = "vello")]
            Raster::Adaptive => "tiny-skia/vello adaptive",
        };
        format!("CPU ({r}, {} threads)", self.threads)
    }

    fn render_batch(&mut self, frames: &[&DisplayList]) -> Result<Vec<Vec<u8>>> {
        match self.raster {
            Raster::Skia => Ok(frames.par_iter().map(|dl| rasterize(dl)).collect()),
            #[cfg(feature = "vello")]
            Raster::Vello => Ok(frames.par_iter().map(|dl| rasterize_with(dl, 1)).collect()),
            #[cfg(feature = "vello")]
            Raster::Adaptive => {
                let plans = self.calib.plan(frames);
                type Timed = Option<(usize, usize, f64)>;
                let out: Vec<(Vec<u8>, Timed)> = frames
                    .par_iter()
                    .zip(&plans)
                    .map(|(dl, plan)| match *plan {
                        Plan::Use(r) => (rasterize_with(dl, r), None),
                        Plan::Time(b, r) => {
                            let t = std::time::Instant::now();
                            let px = rasterize_with(dl, r);
                            (px, Some((b, r, t.elapsed().as_secs_f64())))
                        }
                    })
                    .collect();
                let mut frames_out = Vec::with_capacity(out.len());
                for (px, timing) in out {
                    if let Some((b, r, secs)) = timing {
                        self.calib.record(b, r, secs);
                    }
                    frames_out.push(px);
                }
                Ok(frames_out)
            }
        }
    }

    fn batch_size(&self) -> usize {
        (self.threads * 2).clamp(2, 64)
    }
}
