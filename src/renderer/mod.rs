//! Rendering backends.
//!
//! Scenes produce [`DisplayList`]s (pixel-space Bézier paths with style). A [`Renderer`]
//! rasterizes batches of them into RGBA8 frames:
//! - [`cpu::CpuRenderer`]: one frame per core in parallel (rayon), on tiny-skia or (feature
//!   `vello`) vello_cpu, choosing per frame by measured speed.
//! - `gpu::GpuRenderer` (feature `gpu`): wgpu + lyon tessellation, 4× MSAA.

pub mod cpu;
#[cfg(feature = "vello")]
pub mod vello;
#[cfg(feature = "gpu")]
pub mod gpu;

use std::sync::Arc;

use anyhow::Result;

use crate::mobject::ImageRef;

#[derive(Clone, Debug, PartialEq)]
pub struct PxSubpath {
    pub start: [f32; 2],
    /// `[control1, control2, end]` per cubic.
    pub curves: Vec<[[f32; 2]; 3]>,
    pub closed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawPath {
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
    pub image: ImageRef,
    /// `[sx, ky, kx, sy, tx, ty]`: image pixel `(u, v)` lands on
    /// `(sx·u + kx·v + tx, ky·u + sy·v + ty)`.
    pub transform: [f32; 6],
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

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayList {
    pub width: u32,
    pub height: u32,
    pub background: [f32; 4],
    /// Drawn first, above the background color (grid paper, backdrops).
    pub background_layer: Option<Layer>,
    pub items: Vec<DrawPath>,
}

impl DisplayList {
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

pub trait Renderer: Send {
    fn name(&self) -> String;
    /// Rasterize frames into tightly packed RGBA8 buffers, in order.
    fn render_batch(&mut self, frames: &[&DisplayList]) -> Result<Vec<Vec<u8>>>;
    /// How many frames to hand over at once.
    fn batch_size(&self) -> usize;
}

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
pub fn create(kind: RendererKind, width: u32, height: u32) -> Result<Box<dyn Renderer>> {
    match kind {
        RendererKind::Cpu => Ok(Box::new(cpu::CpuRenderer::new())),
        RendererKind::Skia => Ok(Box::new(cpu::CpuRenderer::with_raster(cpu::Raster::Skia))),
        #[cfg(feature = "vello")]
        RendererKind::Vello => Ok(Box::new(cpu::CpuRenderer::with_raster(cpu::Raster::Vello))),
        #[cfg(not(feature = "vello"))]
        RendererKind::Vello => anyhow::bail!("vello renderer requested but manim was built without the `vello` feature"),
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
                Ok(g) => return Ok(Box::new(AutoRenderer::new(g))),
                Err(e) => eprintln!("manim: GPU unavailable ({e:#}); using CPU renderer"),
            }
            let _ = (width, height);
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
}

#[cfg(feature = "gpu")]
impl AutoRenderer {
    /// Batches smaller than this are too noisy to time.
    const MIN_FRAMES: usize = 8;
    const TRIALS: u32 = 2;

    pub fn new(gpu: gpu::GpuRenderer) -> Self {
        AutoRenderer { cpu: cpu::CpuRenderer::new(), gpu, best: [f64::INFINITY; 2], trials: [0; 2], choice: None }
    }

    fn run(&mut self, which: usize, frames: &[&DisplayList]) -> Result<Vec<Vec<u8>>> {
        if which == 0 {
            self.cpu.render_batch(frames)
        } else {
            self.gpu.render_batch(frames)
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
        let per_frame = t.elapsed().as_secs_f64() / frames.len() as f64;
        self.best[which] = self.best[which].min(per_frame);
        self.trials[which] += 1;
        if self.trials.iter().all(|&n| n >= Self::TRIALS) {
            let c = if self.best[1] < self.best[0] { 1 } else { 0 };
            self.choice = Some(c);
            eprintln!(
                "manim: auto renderer measured CPU {:.2} ms/frame, GPU {:.2} ms/frame → using {}",
                self.best[0] * 1e3,
                self.best[1] * 1e3,
                if c == 1 { "GPU" } else { "CPU" }
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
