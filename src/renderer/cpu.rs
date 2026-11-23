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
    FillRule, FilterQuality, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap, PixmapPaint, PixmapRef, Stroke,
    Transform,
};

use super::{DisplayList, DrawPath, Frame, FrameFinisher, FramePool, Layer, PxGradient, PxShadow, Renderer};

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

/// Rasterizes frames in parallel across all cores, one frame per core.
pub struct CpuRenderer {
    threads: usize,
    raster: Raster,
    pool: FramePool,
    #[cfg(feature = "vello")]
    calib: Calibration,
}

impl CpuRenderer {
    /// The best rasterizer this build has (adaptive when vello is compiled in).
    pub fn new() -> Self {
        Self::with_raster(Raster::best())
    }

    /// A renderer fixed to a specific [`Raster`].
    pub fn with_raster(raster: Raster) -> Self {
        CpuRenderer {
            threads: rayon::current_num_threads(),
            raster,
            pool: FramePool::new(),
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
                if dl.needs_reference() {
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
fn rasterize_with(dl: &DisplayList, raster: usize, pool: &FramePool) -> Vec<u8> {
    if raster == 1 {
        if let Some(px) = super::vello::rasterize_pooled(dl, pool) {
            return px;
        }
    }
    rasterize_pooled(dl, pool)
}

impl Default for CpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

fn color(c: [f32; 4]) -> tiny_skia::Color {
    // Non-finite or out-of-range components draw nothing (matching the vello path).
    let ok = c.iter().all(|v| v.is_finite());
    let c = c.map(|v| v.clamp(0.0, 1.0));
    tiny_skia::Color::from_rgba(c[0], c[1], c[2], c[3]).filter(|_| ok).unwrap_or(tiny_skia::Color::TRANSPARENT)
}

/// Rasterized background layers, keyed by the layer's pointer. The `Arc` is kept so a
/// freed layer's address can't be reused by a different one while it is cached.
type LayerCache = Vec<(Arc<Vec<DrawPath>>, u32, u32, [u32; 4], Arc<Pixmap>)>;
static LAYERS: Mutex<LayerCache> = Mutex::new(Vec::new());
const MAX_CACHED_LAYERS: usize = 4;

pub(super) fn base_pixmap(dl: &DisplayList, pool: &FramePool) -> Pixmap {
    let (w, h) = (dl.width.max(1), dl.height.max(1));
    // A pixmap over a pooled buffer; every byte is overwritten by the caller's fill or copy.
    let pooled = || {
        let size = tiny_skia::IntSize::from_wh(w, h).expect("valid frame size");
        Pixmap::from_vec(pool.take(w as usize * h as usize * 4), size).expect("buffer matches frame size")
    };
    let blank = || {
        let mut pm = pooled();
        pm.fill(color(dl.background));
        pm
    };
    let Some(Layer(layer)) = &dl.background_layer else { return blank() };
    let bg = dl.background.map(f32::to_bits);
    let found = {
        let cache = LAYERS.lock().unwrap_or_else(|e| e.into_inner());
        cache.iter().find(|e| Arc::ptr_eq(&e.0, layer) && e.1 == w && e.2 == h && e.3 == bg).map(|e| e.4.clone())
    };
    let pm = match found {
        Some(pm) => pm,
        None => {
            // Not from the pool: the cache keeps this one.
            let mut pm = Pixmap::new(w, h).expect("valid frame size");
            pm.fill(color(dl.background));
            draw_items(&mut pm, layer.iter());
            let pm = Arc::new(pm);
            let mut cache = LAYERS.lock().unwrap_or_else(|e| e.into_inner());
            if cache.len() >= MAX_CACHED_LAYERS {
                cache.remove(0);
            }
            cache.push((layer.clone(), w, h, bg, pm.clone()));
            pm
        }
    };
    let mut out = pooled();
    out.data_mut().copy_from_slice(pm.data());
    out
}

fn draw_items<'a>(pm: &mut Pixmap, items: impl IntoIterator<Item = &'a DrawPath>) {
    let mut paint = Paint { anti_alias: true, ..Default::default() };
    // The clip mask of the last item that had one, reused while the rectangle stays the same.
    let mut clip_cache: Option<([f32; 4], Option<Mask>)> = None;
    for item in items {
        let clip: Option<&Mask> = match item.clip {
            None => None,
            Some(rect) => {
                if clip_cache.as_ref().is_none_or(|(r, _)| *r != rect) {
                    clip_cache = Some((rect, clip_mask(pm.width(), pm.height(), rect)));
                }
                // A rectangle with no area clips everything away.
                match &clip_cache {
                    Some((_, Some(m))) => Some(m),
                    _ => continue,
                }
            }
        };
        if let Some(img) = &item.image {
            let data = &img.image.0;
            let Some(src) = PixmapRef::from_bytes(&data.premultiplied, data.width, data.height) else { continue };
            let [sx, ky, kx, sy, tx, ty] = img.transform;
            let pp = PixmapPaint {
                opacity: img.opacity.clamp(0.0, 1.0),
                quality: FilterQuality::Bicubic,
                ..Default::default()
            };
            pm.draw_pixmap(0, 0, src, &pp, Transform::from_row(sx, ky, kx, sy, tx, ty), clip);
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
        let rule = if item.nonzero { FillRule::Winding } else { FillRule::EvenOdd };
        if item.backdrop_blur > 0.0 {
            blur_behind(pm, &path, rule, item.backdrop_blur, clip);
        }
        if let (Some(shadow), Some(_)) = (&item.shadow, item.fill) {
            draw_shadow(pm, &path, rule, shadow, clip);
        }
        if let Some(fill) = item.fill {
            paint.set_color(color(fill));
            if let Some(shader) = item.gradient.as_ref().and_then(gradient_shader) {
                paint.shader = shader;
            }
            pm.fill_path(&path, &paint, rule, Transform::identity(), clip);
        }
        if let Some((c, width)) = item.stroke {
            paint.set_color(color(c)); // also resets any gradient shader
            let stroke = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
            pm.stroke_path(&path, &paint, &stroke, Transform::identity(), clip);
        }
    }
}

fn gradient_shader(g: &PxGradient) -> Option<tiny_skia::Shader<'static>> {
    let stops = g.stops.iter().map(|(at, c)| tiny_skia::GradientStop::new(*at, color(*c))).collect();
    tiny_skia::LinearGradient::new(
        tiny_skia::Point::from_xy(g.start[0], g.start[1]),
        tiny_skia::Point::from_xy(g.end[0], g.end[1]),
        stops,
        tiny_skia::SpreadMode::Pad,
        Transform::identity(),
    )
}

/// A coverage mask of the pixel rectangle `[left, top, right, bottom]`, or `None` if it is empty.
fn clip_mask(w: u32, h: u32, r: [f32; 4]) -> Option<Mask> {
    let rect = tiny_skia::Rect::from_ltrb(r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3]))?;
    let mut mask = Mask::new(w, h)?;
    mask.fill_path(&PathBuilder::from_rect(rect), FillRule::Winding, false, Transform::identity());
    Some(mask)
}

/// An integer pixel rectangle `(x, y, w, h)` inside a `w × h` canvas, or `None` if nothing is left.
fn region(l: f32, t: f32, r: f32, b: f32, w: u32, h: u32) -> Option<(usize, usize, usize, usize)> {
    if ![l, t, r, b].iter().all(|v| v.is_finite()) {
        return None;
    }
    let x0 = (l.floor().max(0.0) as i64).min(w as i64);
    let y0 = (t.floor().max(0.0) as i64).min(h as i64);
    let x1 = (r.ceil().max(0.0) as i64).min(w as i64);
    let y1 = (b.ceil().max(0.0) as i64).min(h as i64);
    (x1 > x0 && y1 > y0).then_some((x0 as usize, y0 as usize, (x1 - x0) as usize, (y1 - y0) as usize))
}

/// Box widths whose three passes approximate a Gaussian of standard deviation `sigma`.
fn boxes_for_gauss(sigma: f32) -> [usize; 3] {
    let ideal = ((12.0 * sigma * sigma / 3.0) + 1.0).sqrt();
    let mut wl = ideal.floor() as usize;
    if wl % 2 == 0 {
        wl = wl.saturating_sub(1);
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - 3.0 * (wl * wl) as f32 - 12.0 * wl as f32 - 9.0) / (-4.0 * wl as f32 - 4.0))
        .round()
        .clamp(0.0, 3.0) as usize;
    std::array::from_fn(|i| if i < m { wl } else { wu })
}

/// Gaussian-blur an interleaved `w × h` buffer of `ch` channels in place (three box passes per
/// axis). Outside the buffer is either its edge colour (`clamp`) or zero.
fn gaussian_blur(buf: &mut [f32], w: usize, h: usize, ch: usize, sigma: f32, clamp: bool) {
    if sigma < 0.3 || w == 0 || h == 0 {
        return;
    }
    let mut tmp = vec![0.0f32; buf.len()];
    for width in boxes_for_gauss(sigma) {
        let r = (width - 1) / 2;
        box_pass(buf, &mut tmp, (w, h, ch), r, true, clamp);
        box_pass(&tmp, buf, (w, h, ch), r, false, clamp);
    }
}

fn box_pass(src: &[f32], dst: &mut [f32], (w, h, ch): (usize, usize, usize), r: usize, horizontal: bool, clamp: bool) {
    let (len, lines, step, line_step) = if horizontal { (w, h, ch, w * ch) } else { (h, w, w * ch, ch) };
    let norm = 1.0 / (2 * r + 1) as f32;
    for line in 0..lines {
        let base = line * line_step;
        for c in 0..ch {
            let at = |i: isize| -> f32 {
                if i < 0 || i >= len as isize {
                    if clamp {
                        let j = i.clamp(0, len as isize - 1) as usize;
                        src[base + j * step + c]
                    } else {
                        0.0
                    }
                } else {
                    src[base + i as usize * step + c]
                }
            };
            let mut sum = 0.0;
            for i in -(r as isize)..=(r as isize) {
                sum += at(i);
            }
            for i in 0..len {
                dst[base + i * step + c] = sum * norm;
                sum += at(i as isize + r as isize + 1) - at(i as isize - r as isize);
            }
        }
    }
}

/// Blur what is already drawn inside `path` (frosted glass).
fn blur_behind(pm: &mut Pixmap, path: &tiny_skia::Path, rule: FillRule, sigma: f32, clip: Option<&Mask>) {
    let (cw, ch) = (pm.width(), pm.height());
    let b = path.bounds();
    let Some((bx, by, bw, bh)) = region(b.left(), b.top(), b.right(), b.bottom(), cw, ch) else { return };
    let margin = (sigma * 3.0).ceil();
    let Some((rx, ry, rw, rh)) =
        region(bx as f32 - margin, by as f32 - margin, (bx + bw) as f32 + margin, (by + bh) as f32 + margin, cw, ch)
    else {
        return;
    };
    let stride = cw as usize * 4;
    let mut plane = vec![0.0f32; rw * rh * 4];
    for y in 0..rh {
        let row = &pm.data()[(ry + y) * stride + rx * 4..][..rw * 4];
        for (d, s) in plane[y * rw * 4..][..rw * 4].iter_mut().zip(row) {
            *d = *s as f32;
        }
    }
    gaussian_blur(&mut plane, rw, rh, 4, sigma, true);
    let Some(mut shape) = Mask::new(bw as u32, bh as u32) else { return };
    shape.fill_path(path, rule, true, Transform::from_translate(-(bx as f32), -(by as f32)));
    let data = pm.data_mut();
    for y in 0..bh {
        for x in 0..bw {
            let mut cov = shape.data()[y * bw + x] as f32 / 255.0;
            if let Some(c) = clip {
                cov *= c.data()[(by + y) * cw as usize + bx + x] as f32 / 255.0;
            }
            if cov <= 0.0 {
                continue;
            }
            let p = (y + by - ry) * rw * 4 + (x + bx - rx) * 4;
            let o = (by + y) * stride + (bx + x) * 4;
            for c in 0..4 {
                let old = data[o + c] as f32;
                data[o + c] = (old + (plane[p + c] - old) * cov).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Draw a soft shadow of `path` under it, outside its outline.
fn draw_shadow(pm: &mut Pixmap, path: &tiny_skia::Path, rule: FillRule, s: &PxShadow, clip: Option<&Mask>) {
    let (cw, ch) = (pm.width(), pm.height());
    let b = path.bounds();
    let margin = (s.sigma * 3.0).ceil();
    let Some((rx, ry, rw, rh)) = region(
        b.left() + s.offset[0] - margin,
        b.top() + s.offset[1] - margin,
        b.right() + s.offset[0] + margin,
        b.bottom() + s.offset[1] + margin,
        cw,
        ch,
    ) else {
        return;
    };
    let Some(mut cast) = Mask::new(rw as u32, rh as u32) else { return };
    cast.fill_path(path, rule, true, Transform::from_translate(s.offset[0] - rx as f32, s.offset[1] - ry as f32));
    let mut plane: Vec<f32> = cast.data().iter().map(|v| *v as f32 / 255.0).collect();
    gaussian_blur(&mut plane, rw, rh, 1, s.sigma, false);
    // The shadow is only drawn outside the outline, so a see-through card does not show it.
    let Some(mut hole) = Mask::new(rw as u32, rh as u32) else { return };
    hole.fill_path(path, rule, true, Transform::from_translate(-(rx as f32), -(ry as f32)));
    let stride = cw as usize * 4;
    let [r, g, bl, a] = s.color;
    let rgb = [r * 255.0, g * 255.0, bl * 255.0];
    let data = pm.data_mut();
    for y in 0..rh {
        for x in 0..rw {
            let mut k = a * plane[y * rw + x] * (1.0 - hole.data()[y * rw + x] as f32 / 255.0);
            if let Some(c) = clip {
                k *= c.data()[(ry + y) * cw as usize + rx + x] as f32 / 255.0;
            }
            if k <= 0.0 {
                continue;
            }
            let o = (ry + y) * stride + (rx + x) * 4;
            for (c, v) in rgb.into_iter().enumerate() {
                let old = data[o + c] as f32;
                data[o + c] = (old + (v - old) * k).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Rasterize one display list into RGBA8. Test helper, not a stable API.
#[doc(hidden)]
pub fn rasterize(dl: &DisplayList) -> Vec<u8> {
    rasterize_pooled(dl, &FramePool::new())
}

/// [`rasterize`] into a buffer from `pool`.
pub(crate) fn rasterize_pooled(dl: &DisplayList, pool: &FramePool) -> Vec<u8> {
    let mut pm = base_pixmap(dl, pool);
    draw_items(&mut pm, &dl.items);
    // The background is opaque, so premultiplied == straight alpha here.
    pm.take()
}

impl CpuRenderer {
    /// Rasterize every frame in parallel and pass each through `finish` on the same thread
    /// the moment it is drawn. Only the rasterizing itself is timed for calibration.
    fn render_each<R: Send>(
        &mut self,
        frames: &[&DisplayList],
        finish: &(dyn Fn(usize, Vec<u8>) -> R + Sync),
    ) -> Vec<R> {
        let pool = &self.pool;
        match self.raster {
            Raster::Skia => {
                frames.par_iter().enumerate().map(|(i, dl)| finish(i, rasterize_pooled(dl, pool))).collect()
            }
            #[cfg(feature = "vello")]
            Raster::Vello => {
                frames.par_iter().enumerate().map(|(i, dl)| finish(i, rasterize_with(dl, 1, pool))).collect()
            }
            #[cfg(feature = "vello")]
            Raster::Adaptive => {
                let plans = self.calib.plan(frames);
                type Timed = Option<(usize, usize, f64)>;
                let out: Vec<(R, Timed)> = frames
                    .par_iter()
                    .zip(&plans)
                    .enumerate()
                    .map(|(i, (dl, plan))| match *plan {
                        Plan::Use(r) => (finish(i, rasterize_with(dl, r, pool)), None),
                        Plan::Time(b, r) => {
                            let t = std::time::Instant::now();
                            let px = rasterize_with(dl, r, pool);
                            let secs = t.elapsed().as_secs_f64();
                            (finish(i, px), Some((b, r, secs)))
                        }
                    })
                    .collect();
                let mut frames_out = Vec::with_capacity(out.len());
                for (frame, timing) in out {
                    if let Some((b, r, secs)) = timing {
                        self.calib.record(b, r, secs);
                    }
                    frames_out.push(frame);
                }
                frames_out
            }
        }
    }
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
        Ok(self.render_each(frames, &|_, px| px))
    }

    fn render_batch_with(&mut self, frames: &[&DisplayList], finish: &FrameFinisher) -> Result<Vec<Frame>> {
        self.render_each(frames, finish).into_iter().collect()
    }

    fn use_pool(&mut self, pool: FramePool) {
        self.pool = pool;
    }

    fn batch_size(&self) -> usize {
        (self.threads * 2).clamp(2, 64)
    }
}
