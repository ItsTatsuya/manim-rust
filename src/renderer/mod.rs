//! Rendering backends.
//!
//! Scenes produce [`DisplayList`]s (pixel-space Bézier paths with style). A [`Renderer`]
//! rasterizes batches of them into RGBA8 frames:
//! - [`cpu::CpuRenderer`]: one frame per core in parallel (rayon), on tiny-skia or (feature
//!   `vello`) vello_cpu, choosing per frame by measured speed.
//! - `gpu::GpuRenderer` (feature `gpu`): wgpu + lyon tessellation, 4× MSAA.

pub mod cpu;
#[cfg(feature = "gpu")]
pub mod gpu;
#[cfg(feature = "vello")]
pub(crate) mod vello;

use std::sync::Arc;

use anyhow::Result;

use crate::events::Events;
use crate::mobject::ImageRef;

/// One closed or open path of connected cubic BÃ©ziers in pixel space.
#[derive(Clone, Debug, PartialEq)]
pub struct PxSubpath {
    /// Where the path starts, in pixels.
    pub start: [f32; 2],
    /// `[control1, control2, end]` per cubic.
    pub curves: Vec<[[f32; 2]; 3]>,
    /// Whether the path is closed back to `start`.
    pub closed: bool,
}

/// A styled path, or an image, in pixel space.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawPath {
    /// The outlines (one or more) the fill and stroke apply to.
    pub subpaths: Vec<PxSubpath>,
    /// Straight (non-premultiplied) RGBA.
    pub fill: Option<[f32; 4]>,
    /// RGBA and width in pixels.
    pub stroke: Option<([f32; 4], f32)>,
    /// A raster image drawn instead of fill and stroke.
    pub image: Option<DrawImage>,
    /// Non-zero winding fill (default even-odd).
    pub nonzero: bool,
}

/// A raster image placed by an affine map from image pixels to frame pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawImage {
    /// The decoded image.
    pub image: ImageRef,
    /// `[sx, ky, kx, sy, tx, ty]`: image pixel `(u, v)` lands on
    /// `(sx·u + kx·v + tx, ky·u + sy·v + ty)`.
    pub transform: [f32; 6],
    /// Opacity in `[0, 1]`.
    pub opacity: f32,
}

/// Paths shared across frames (the scene's background layer). Renderers may cache
/// the rasterized result by pointer, so an unchanged layer costs one copy per frame.
#[derive(Clone, Debug)]
pub struct Layer(pub Arc<Vec<DrawPath>>);

impl PartialEq for Layer {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || *self.0 == *other.0
    }
}

/// Everything drawn in one frame, in pixel space.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayList {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Opaque background color as straight RGBA (alpha is 1).
    pub background: [f32; 4],
    /// Drawn first, above the background color (grid paper, backdrops).
    pub background_layer: Option<Layer>,
    /// The scene's paths in draw order, above the background layer.
    pub items: Vec<DrawPath>,
}

impl DisplayList {
    /// Whether any item (including the background layer) is an image.
    pub fn has_images(&self) -> bool {
        let layer = self.background_layer.as_ref().map(|l| l.0.as_slice()).unwrap_or(&[]);
        layer.iter().chain(&self.items).any(|i| i.image.is_some())
    }

    /// Every item in draw order: background layer, then the scene.
    pub fn all_items(&self) -> impl Iterator<Item = &DrawPath> {
        let layer = self.background_layer.as_ref().map(|l| l.0.as_slice()).unwrap_or(&[]);
        layer.iter().chain(&self.items)
    }
}

/// Recycles frame buffers, so a long render does not allocate (and free, on another
/// thread) several megabytes per frame.
///
/// Buffers are plain `Vec<u8>`s moved in and out by value, so a buffer in the pool is never
/// referenced elsewhere. Contents are unspecified: callers overwrite every byte.
#[derive(Clone, Default)]
pub struct FramePool(Arc<std::sync::Mutex<Idle>>);

#[derive(Default)]
struct Idle {
    buffers: Vec<Vec<u8>>,
    bytes: usize,
}

impl FramePool {
    /// Most buffers kept; extra ones are freed.
    const MAX_KEPT: usize = 128;
    /// Most bytes kept idle. A buffer that does not fit is freed, so a renderer that never
    /// takes from the pool cannot make it grow.
    const MAX_KEPT_BYTES: usize = 256 << 20;

    /// An empty pool.
    pub fn new() -> Self {
        Self::default()
    }

    /// A buffer of exactly `len` bytes with unspecified contents. Only a pooled buffer of
    /// that exact size is reused (frames and their converted form have different sizes and
    /// must not stand in for each other).
    pub fn take(&self, len: usize) -> Vec<u8> {
        let reused = {
            let mut pool = self.0.lock().unwrap_or_else(|e| e.into_inner());
            let found = pool.buffers.iter().rposition(|b| b.len() == len);
            found.map(|i| {
                pool.bytes -= len;
                pool.buffers.swap_remove(i)
            })
        };
        reused.unwrap_or_else(|| vec![0; len])
    }

    /// Hand a buffer back for reuse.
    pub fn put(&self, buf: Vec<u8>) {
        if buf.is_empty() {
            return;
        }
        let mut pool = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if pool.buffers.len() < Self::MAX_KEPT && pool.bytes + buf.len() <= Self::MAX_KEPT_BYTES {
            pool.bytes += buf.len();
            pool.buffers.push(buf);
        }
    }

    /// Hand back a shared buffer. Only the last holder to call this recycles it: while
    /// another reference exists the buffer stays where it is.
    pub fn put_shared(&self, buf: Arc<Vec<u8>>) {
        if let Some(buf) = Arc::into_inner(buf) {
            self.put(buf);
        }
    }

    /// Buffers currently waiting for reuse.
    pub fn idle(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).buffers.len()
    }

    /// Bytes held by those buffers.
    pub fn idle_bytes(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).bytes
    }
}

/// A rasterized frame after its per-frame post-processing (see [`Renderer::render_batch_with`]).
pub enum Frame {
    /// Plain RGBA8, for writers that take it as is.
    Rgba(Vec<u8>),
    /// Converted by the writer's [`FramePreparer`](crate::output::FramePreparer). `rgba` is
    /// kept for the one frame per batch the pipeline needs as RGBA (the last frame).
    Prepared {
        /// The converted frame.
        data: Vec<u8>,
        /// The original RGBA8 frame, kept for the last frame of a batch (the pipeline needs it as RGBA).
        rgba: Option<Vec<u8>>,
    },
}

/// Post-processing applied to each rasterized frame, given its index in the batch.
pub type FrameFinisher<'a> = dyn Fn(usize, Vec<u8>) -> Result<Frame> + Sync + 'a;

/// A rasterizing backend. Implementations turn batches of [`DisplayList`]s into RGBA8 frames; the pipeline calls them from one thread.
pub trait Renderer: Send {
    /// Short name of the backend for logs and events, e.g. `tiny-skia`.
    fn name(&self) -> String;
    /// Rasterize frames into tightly packed RGBA8 buffers, in order.
    fn render_batch(&mut self, frames: &[&DisplayList]) -> Result<Vec<Vec<u8>>>;
    /// How many frames to hand over at once.
    fn batch_size(&self) -> usize;
    /// Rasterize frames and run `finish` on each as soon as it is ready, in parallel, so a
    /// frame's memory can be released (or shrunk by colour conversion) while other frames
    /// are still being drawn. The default rasterizes everything first.
    fn render_batch_with(&mut self, frames: &[&DisplayList], finish: &FrameFinisher) -> Result<Vec<Frame>> {
        use rayon::prelude::*;
        let raw = self.render_batch(frames)?;
        raw.into_par_iter().enumerate().map(|(i, f)| finish(i, f)).collect()
    }
    /// Draw frame buffers from `pool` instead of allocating them.
    fn use_pool(&mut self, _pool: FramePool) {}
}

/// Which backend [`create`] builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererKind {
    /// Hardware GPU if one is found, otherwise CPU.
    Auto,
    /// The CPU rasterizer that measures fastest (tiny-skia or vello, per frame).
    Cpu,
    /// tiny-skia only: the reference rasterizer, deterministic output.
    Skia,
    /// vello_cpu only (frames with images still go to tiny-skia).
    Vello,
    /// The wgpu renderer; an error if no usable GPU adapter is found.
    Gpu,
}

impl std::str::FromStr for RendererKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(RendererKind::Auto),
            "cpu" | "cairo" => Ok(RendererKind::Cpu),
            "skia" | "tiny-skia" => Ok(RendererKind::Skia),
            "vello" => Ok(RendererKind::Vello),
            "gpu" | "opengl" | "wgpu" => Ok(RendererKind::Gpu),
            _ => anyhow::bail!("unknown renderer '{s}' (expected auto, cpu, skia, vello or gpu)"),
        }
    }
}

/// Pick a backend.
///
/// `Auto` probes for a hardware adapter (software adapters like WARP/llvmpipe are
/// rejected). If one exists, the first batches of the scene are timed on both backends
/// and the faster one is kept: on many-core CPUs tiny-skia often beats GPU readback for
/// light 2D scenes, while heavy or high-resolution scenes favour the GPU.
/// An explicit `Gpu` request fails loudly if no usable adapter exists.
///
/// `events` receives a `warning` when the GPU probe fails and a `renderer` event when
/// auto calibration settles on a backend.
pub fn create(kind: RendererKind, width: u32, height: u32, events: Events) -> Result<Box<dyn Renderer>> {
    match kind {
        RendererKind::Cpu => Ok(Box::new(cpu::CpuRenderer::new())),
        RendererKind::Skia => Ok(Box::new(cpu::CpuRenderer::with_raster(cpu::Raster::Skia))),
        #[cfg(feature = "vello")]
        RendererKind::Vello => Ok(Box::new(cpu::CpuRenderer::with_raster(cpu::Raster::Vello))),
        #[cfg(not(feature = "vello"))]
        RendererKind::Vello => {
            anyhow::bail!("vello renderer requested but manim was built without the `vello` feature")
        }
        #[cfg(feature = "gpu")]
        RendererKind::Gpu => Ok(Box::new(gpu::GpuRenderer::new(width, height, false)?)),
        #[cfg(not(feature = "gpu"))]
        RendererKind::Gpu => {
            let _ = (width, height);
            anyhow::bail!("GPU renderer requested but manim was built without the `gpu` feature")
        }
        // Below 1080p the CPU rasterizes in well under a millisecond per frame, so the
        // ~0.3 s GPU initialization can't pay for itself.
        RendererKind::Auto if (width as u64 * height as u64) < 1920 * 1080 => Ok(Box::new(cpu::CpuRenderer::new())),
        RendererKind::Auto => {
            #[cfg(feature = "gpu")]
            match gpu::GpuRenderer::new(width, height, true) {
                Ok(g) => return Ok(Box::new(AutoRenderer::new(g, events))),
                Err(e) => {
                    let message = format!("GPU unavailable ({e:#}); using CPU renderer");
                    events.emit("warning", serde_json::json!({ "message": message }), || {
                        Some(format!("manim: {message}"))
                    });
                }
            }
            let _ = (width, height, events);
            Ok(Box::new(cpu::CpuRenderer::new()))
        }
    }
}

/// Times both backends on live batches, then commits to the faster.
#[cfg(feature = "gpu")]
pub struct AutoRenderer {
    cpu: cpu::CpuRenderer,
    gpu: gpu::GpuRenderer,
    /// Best seconds-per-frame observed for (cpu, gpu).
    best: [f64; 2],
    trials: [u32; 2],
    choice: Option<usize>,
    events: Events,
}

#[cfg(feature = "gpu")]
impl AutoRenderer {
    /// Batches smaller than this are too noisy to time.
    const MIN_FRAMES: usize = 8;
    const TRIALS: u32 = 2;

    /// Wrap `gpu` and a CPU renderer, timing both to settle on the faster. Calibration progress is reported through `events`.
    pub fn new(gpu: gpu::GpuRenderer, events: Events) -> Self {
        AutoRenderer {
            cpu: cpu::CpuRenderer::new(),
            gpu,
            best: [f64::INFINITY; 2],
            trials: [0; 2],
            choice: None,
            events,
        }
    }

    fn run(&mut self, which: usize, frames: &[&DisplayList]) -> Result<Vec<Vec<u8>>> {
        if which == 0 {
            return self.cpu.render_batch(frames);
        }
        match self.gpu.render_batch(frames) {
            Ok(out) => Ok(out),
            Err(e) => {
                // A failing GPU (driver error, device lost, out of memory) must not end the
                // render: switch to the CPU for good and redo this batch there.
                self.choice = Some(0);
                let message = format!("GPU failed ({e:#}); continuing on the CPU renderer");
                self.events
                    .emit("warning", serde_json::json!({ "message": message }), || Some(format!("manim: {message}")));
                self.cpu.render_batch(frames)
            }
        }
    }
}

#[cfg(feature = "gpu")]
impl Renderer for AutoRenderer {
    fn name(&self) -> String {
        match self.choice {
            Some(0) => format!("auto → {}", self.cpu.name()),
            Some(_) => format!("auto → {}", self.gpu.name()),
            None => format!("auto, calibrating: {} vs {}", self.cpu.name(), self.gpu.name()),
        }
    }

    fn batch_size(&self) -> usize {
        self.gpu.batch_size().max(self.cpu.batch_size())
    }

    fn use_pool(&mut self, pool: FramePool) {
        self.cpu.use_pool(pool.clone());
        self.gpu.use_pool(pool);
    }

    fn render_batch_with(&mut self, frames: &[&DisplayList], finish: &FrameFinisher) -> Result<Vec<Frame>> {
        // Once the CPU has won, it can finish each frame as it goes. While calibrating (and
        // on the GPU) the whole batch is timed as one, then finished.
        if self.choice == Some(0) {
            return self.cpu.render_batch_with(frames, finish);
        }
        use rayon::prelude::*;
        let raw = self.render_batch(frames)?;
        raw.into_par_iter().enumerate().map(|(i, f)| finish(i, f)).collect()
    }

    fn render_batch(&mut self, frames: &[&DisplayList]) -> Result<Vec<Vec<u8>>> {
        if let Some(c) = self.choice {
            return self.run(c, frames);
        }
        if frames.len() < Self::MIN_FRAMES {
            return self.cpu.render_batch(frames);
        }
        // Alternate GPU, CPU, GPU, CPU... (GPU first so its warm-up lands in trial 1).
        let which = if self.trials[1] <= self.trials[0] { 1 } else { 0 };
        let t = std::time::Instant::now();
        let out = self.run(which, frames)?;
        if self.choice.is_some() {
            return Ok(out); // the GPU failed and was dropped mid-calibration
        }
        let per_frame = t.elapsed().as_secs_f64() / frames.len() as f64;
        self.best[which] = self.best[which].min(per_frame);
        self.trials[which] += 1;
        if self.trials.iter().all(|&n| n >= Self::TRIALS) {
            let c = if self.best[1] < self.best[0] { 1 } else { 0 };
            self.choice = Some(c);
            let (cpu_ms, gpu_ms) = (self.best[0] * 1e3, self.best[1] * 1e3);
            let kind = if c == 1 { "gpu" } else { "cpu" };
            let name = if c == 1 { self.gpu.name() } else { self.cpu.name() };
            self.events.emit(
                "renderer",
                serde_json::json!({ "renderer": name, "kind": kind, "cpu_ms_per_frame": cpu_ms, "gpu_ms_per_frame": gpu_ms }),
                || {
                    Some(format!(
                        "manim: auto renderer measured CPU {cpu_ms:.2} ms/frame, GPU {gpu_ms:.2} ms/frame → using {}",
                        kind.to_ascii_uppercase()
                    ))
                },
            );
        }
        Ok(out)
    }
}

/// Describe the GPU adapters wgpu can see (for `--gpu-info`).
pub fn gpu_report() -> String {
    #[cfg(feature = "gpu")]
    {
        gpu::adapter_report()
    }
    #[cfg(not(feature = "gpu"))]
    {
        "built without the `gpu` feature".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_reuses_buffers_by_size() {
        let pool = FramePool::new();
        let a = pool.take(100);
        assert_eq!(a.len(), 100);
        let ptr = a.as_ptr();
        pool.put(a);
        assert_eq!(pool.idle(), 1);
        let b = pool.take(100); // the same size gets the same buffer back
        assert_eq!((b.len(), b.as_ptr()), (100, ptr));
        assert_eq!((pool.idle(), pool.idle_bytes()), (0, 0));
        pool.put(b);
        let c = pool.take(80); // a different size never reuses it (YUV must not take an RGBA frame)
        assert_eq!(c.len(), 80);
        assert_ne!(c.as_ptr(), ptr);
        assert_eq!((pool.idle(), pool.idle_bytes()), (1, 100));
    }

    #[test]
    fn pool_is_capped_in_bytes() {
        let pool = FramePool::new();
        let big = FramePool::MAX_KEPT_BYTES / 4 + 1;
        for _ in 0..10 {
            pool.put(vec![0; big]); // a renderer that never takes: nothing piles up
        }
        assert_eq!(pool.idle(), 3);
        assert!(pool.idle_bytes() <= FramePool::MAX_KEPT_BYTES);
    }

    #[test]
    fn shared_buffers_are_only_recycled_by_the_last_holder() {
        let pool = FramePool::new();
        let shared = Arc::new(vec![7u8; 16]);
        let other = shared.clone();
        pool.put_shared(shared);
        assert_eq!(pool.idle(), 0, "still referenced: must not be handed out");
        assert_eq!(other[0], 7);
        let fresh = pool.take(16);
        assert_ne!(fresh.as_ptr(), other.as_ptr());
        pool.put_shared(other);
        assert_eq!(pool.idle(), 1, "the last holder recycles it");
    }

    #[test]
    fn pool_is_bounded() {
        let pool = FramePool::new();
        for _ in 0..FramePool::MAX_KEPT + 10 {
            pool.put(vec![0; 8]);
        }
        assert_eq!(pool.idle(), FramePool::MAX_KEPT);
        pool.put(Vec::new()); // empty buffers are not worth keeping
        assert_eq!(pool.idle(), FramePool::MAX_KEPT);
    }
}
