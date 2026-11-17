//! GPU renderer: lyon tessellates paths (in parallel on the CPU), wgpu rasterizes them
//! with 4× MSAA into an offscreen texture, and frames are read back in batches.

use std::sync::{mpsc, Arc, Mutex};

use anyhow::{anyhow, bail, Context, Result};
use bytemuck::{Pod, Zeroable};
use lyon::math::point;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions,
    StrokeTessellator, StrokeVertex, VertexBuffers,
};
use rayon::prelude::*;
use wgpu::util::DeviceExt;

use super::{DisplayList, DrawPath, FramePool, Renderer};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Curve flattening tolerance in pixels.
const TOLERANCE: f32 = 0.15;
/// Most frames per GPU submission (fewer when frames are big, see [`chunk_for`]).
const MAX_CHUNK: usize = 16;
/// Frames handed over per `render_batch` call (several pipelined chunks).
const BATCH: usize = 48;
/// Memory for readback buffers (two chunks of padded frames are alive at once).
const READBACK_BUDGET: u64 = 256 << 20;

/// Frames per GPU submission: one per thread that tessellates (they run in parallel), but
/// two chunks of readback buffers must fit in [`READBACK_BUDGET`].
fn chunk_for(threads: usize, padded_frame_bytes: u64) -> usize {
    let fit = (READBACK_BUDGET / (2 * padded_frame_bytes.max(1))) as usize;
    threads.clamp(4, MAX_CHUNK).min(fit.max(2))
}

const SHADER: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>) -> VsOut {
    var o: VsOut;
    o.pos = vec4<f32>(pos, 0.0, 1.0);
    o.color = color;
    return o;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    return i.color;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 2],
    color: [f32; 4],
}

fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
}

/// Human-readable list of adapters, plus which one `Auto` would choose.
pub fn adapter_report() -> String {
    let inst = instance();
    let mut out = String::new();
    let adapters = pollster::block_on(inst.enumerate_adapters(wgpu::Backends::PRIMARY));
    if adapters.is_empty() {
        out.push_str("No GPU adapters found.\n");
    }
    for a in adapters {
        let i = a.get_info();
        out.push_str(&format!(
            "- {} [{:?}, {:?}] driver: {} {}\n",
            i.name, i.device_type, i.backend, i.driver, i.driver_info
        ));
    }
    match GpuRenderer::new(256, 256, true) {
        Ok(r) => out.push_str(&format!("Auto renderer would use: {}\n", r.name())),
        Err(e) => out.push_str(&format!("Auto renderer would use the CPU: {e:#}\n")),
    }
    out
}

/// wgpu renderer: lyon tessellates paths in parallel, wgpu draws them with 4x MSAA. Created with [`GpuRenderer::new`].
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    msaa_view: Option<wgpu::TextureView>,
    resolve: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    readbacks: Vec<wgpu::Buffer>,
    width: u32,
    height: u32,
    padded_row: u32,
    label: String,
    /// Validation errors, out-of-memory and device loss, reported by wgpu's callbacks
    /// (whose default is to panic). A set error fails every later batch.
    error: Arc<Mutex<Option<String>>>,
    max_buffer: u64,
    /// Frames per submission (also the readback ring's half size).
    chunk: usize,
    /// Per-frame geometry, written with `queue.write_buffer` into buffers that only grow.
    vertices: Option<wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
    /// Tessellated background layers (most recent last), so a static backdrop is
    /// tessellated and uploaded once instead of every frame.
    layers: Vec<Arc<LayerGpu>>,
    pool: FramePool,
}

/// A background layer on the GPU. The `Arc` of paths is kept so the pointer used as the
/// key cannot be reused by a different layer while this entry lives.
struct LayerGpu {
    paths: Arc<Vec<DrawPath>>,
    size: (u32, u32),
    /// `None` when the layer draws nothing, or is too big for the GPU's buffers.
    buffers: Option<(wgpu::Buffer, wgpu::Buffer, u32)>,
    /// Its geometry exceeds the GPU's buffer limit: chunks that show it go to the CPU.
    too_big: bool,
}

const MAX_CACHED_LAYERS: usize = 4;

impl GpuRenderer {
    /// `require_hardware` rejects software adapters (WARP, llvmpipe), which are slower
    /// than the tiny-skia path.
    pub fn new(width: u32, height: u32, require_hardware: bool) -> Result<Self> {
        let inst = instance();
        let adapter = pollster::block_on(inst.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|e| anyhow!("no GPU adapter found: {e}"))?;
        let info = adapter.get_info();
        if require_hardware && info.device_type == wgpu::DeviceType::Cpu {
            bail!("only a software adapter is available ({})", info.name);
        }
        let limits = adapter.limits();
        if width > limits.max_texture_dimension_2d || height > limits.max_texture_dimension_2d {
            bail!("{}x{} exceeds the GPU's max texture size {}", width, height, limits.max_texture_dimension_2d);
        }
        let samples = if adapter.get_texture_format_features(FORMAT).flags.sample_count_supported(4) { 4 } else { 1 };

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("manim"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits {
                // Readback buffers hold a whole frame: allow what the adapter can do.
                max_buffer_size: limits.max_buffer_size,
                ..wgpu::Limits::downlevel_defaults().using_resolution(limits)
            },
            memory_hints: wgpu::MemoryHints::Performance,
            ..Default::default()
        }))
        .context("could not open GPU device")?;

        let error: Arc<Mutex<Option<String>>> = Arc::default();
        {
            let slot = error.clone();
            device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
                slot.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(|| format!("GPU error: {e}"));
            }));
            let slot = error.clone();
            device.set_device_lost_callback(move |reason, msg| {
                slot.lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get_or_insert_with(|| format!("GPU device lost ({reason:?}): {msg}"));
            });
        }
        let max_buffer = device.limits().max_buffer_size;
        let padded_row = (width * 4).div_ceil(256) * 256;
        if padded_row as u64 * height as u64 > max_buffer {
            bail!("{width}x{height} frames exceed the GPU's {} MB buffer limit", max_buffer >> 20);
        }

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("manim shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        // Blend color in sRGB space like Cairo/tiny-skia; keep destination alpha at 1.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("manim pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false },
            multiview_mask: None,
            cache: None,
        });

        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let tex = |samples: u32, usage: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage,
                view_formats: &[],
            })
        };
        let msaa_view = (samples > 1)
            .then(|| tex(samples, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default()));
        let resolve = tex(1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let resolve_view = resolve.create_view(&Default::default());

        Ok(GpuRenderer {
            label: format!("GPU ({}, {:?}, {}x MSAA)", info.name, info.backend, samples),
            device,
            queue,
            pipeline,
            msaa_view,
            resolve,
            resolve_view,
            readbacks: Vec::new(),
            width,
            height,
            padded_row,
            error,
            max_buffer,
            chunk: chunk_for(rayon::current_num_threads(), padded_row as u64 * height as u64),
            vertices: None,
            indices: None,
            layers: Vec::new(),
            pool: FramePool::new(),
        })
    }

    /// The GPU copy of a frame's background layer (tessellated on first sight).
    fn layer_gpu(&mut self, dl: &DisplayList) -> Option<Arc<LayerGpu>> {
        let layer = &dl.background_layer.as_ref()?.0;
        let size = (dl.width, dl.height);
        if let Some(i) = self.layers.iter().position(|l| Arc::ptr_eq(&l.paths, layer) && l.size == size) {
            let hit = self.layers.remove(i);
            self.layers.push(hit.clone());
            return Some(hit);
        }
        let geom = tessellate(layer.iter(), dl.width, dl.height);
        let (vb, ib) =
            (std::mem::size_of_val(&geom.vertices[..]) as u64, std::mem::size_of_val(&geom.indices[..]) as u64);
        let too_big = vb > self.max_buffer || ib > self.max_buffer;
        let buffers = (!geom.indices.is_empty() && !too_big).then(|| {
            let make = |contents: &[u8], usage| {
                self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents, usage })
            };
            (
                make(bytemuck::cast_slice(&geom.vertices), wgpu::BufferUsages::VERTEX),
                make(bytemuck::cast_slice(&geom.indices), wgpu::BufferUsages::INDEX),
                geom.indices.len() as u32,
            )
        });
        let entry = Arc::new(LayerGpu { paths: layer.clone(), size, buffers, too_big });
        if self.layers.len() >= MAX_CACHED_LAYERS {
            self.layers.remove(0);
        }
        self.layers.push(entry.clone());
        Some(entry)
    }

    /// Grow `buf` to hold `bytes` (rounded up to a power of two); never shrinks.
    fn ensure(&self, buf: &mut Option<wgpu::Buffer>, bytes: u64, usage: wgpu::BufferUsages) {
        if buf.as_ref().is_some_and(|b| b.size() >= bytes) {
            return;
        }
        let size = bytes.next_power_of_two().clamp(1 << 16, self.max_buffer);
        *buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }

    /// The first error wgpu reported, if any.
    fn check(&self) -> Result<()> {
        match &*self.error.lock().unwrap_or_else(|p| p.into_inner()) {
            Some(e) => Err(anyhow!("{e}")),
            None => Ok(()),
        }
    }

    fn readback(&mut self, i: usize) -> &wgpu::Buffer {
        while self.readbacks.len() <= i {
            self.readbacks.push(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: self.padded_row as u64 * self.height as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
        }
        &self.readbacks[i]
    }
}

fn tessellate<'a>(
    items: impl IntoIterator<Item = &'a DrawPath>,
    width: u32,
    height: u32,
) -> VertexBuffers<Vertex, u32> {
    let (w, h) = (width as f32, height as f32);
    let ndc = move |x: f32, y: f32| [x / w * 2.0 - 1.0, 1.0 - y / h * 2.0];
    let mut geom: VertexBuffers<Vertex, u32> = VertexBuffers::new();
    let mut fill_t = FillTessellator::new();
    let mut stroke_t = StrokeTessellator::new();
    for item in items.into_iter().filter(|i| i.image.is_none()) {
        let mut b = Path::builder();
        for sp in &item.subpaths {
            b.begin(point(sp.start[0], sp.start[1]));
            for c in &sp.curves {
                b.cubic_bezier_to(point(c[0][0], c[0][1]), point(c[1][0], c[1][1]), point(c[2][0], c[2][1]));
            }
            b.end(sp.closed);
        }
        let path = b.build();
        if let Some(color) = item.fill {
            let rule = if item.nonzero { FillRule::NonZero } else { FillRule::EvenOdd };
            let opts = FillOptions::tolerance(TOLERANCE).with_fill_rule(rule);
            let _ = fill_t.tessellate_path(
                &path,
                &opts,
                &mut BuffersBuilder::new(&mut geom, |v: FillVertex| {
                    let p = v.position();
                    Vertex { pos: ndc(p.x, p.y), color }
                }),
            );
        }
        if let Some((color, width)) = item.stroke {
            let opts = StrokeOptions::tolerance(TOLERANCE)
                .with_line_width(width)
                .with_line_join(LineJoin::Round)
                .with_line_cap(LineCap::Round);
            let _ = stroke_t.tessellate_path(
                &path,
                &opts,
                &mut BuffersBuilder::new(&mut geom, |v: StrokeVertex| {
                    let p = v.position();
                    Vertex { pos: ndc(p.x, p.y), color }
                }),
            );
        }
    }
    geom
}

impl Renderer for GpuRenderer {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn batch_size(&self) -> usize {
        BATCH
    }

    fn use_pool(&mut self, pool: FramePool) {
        self.pool = pool;
    }

    /// Chunks are pipelined: while the GPU renders chunk k, the CPU reads back chunk k-1
    /// and tessellates chunk k+1. Readback buffers alternate between two slots.
    fn render_batch(&mut self, frames: &[&DisplayList]) -> Result<Vec<Vec<u8>>> {
        // Raster images are drawn by the CPU rasterizer (a GPU texture path would cost
        // more in uploads than these frames take to rasterize on all cores).
        if frames.iter().any(|f| f.has_images()) {
            let pool = &self.pool;
            return Ok(frames.par_iter().map(|f| super::cpu::rasterize_pooled(f, pool)).collect());
        }
        for dl in frames {
            if dl.width != self.width || dl.height != self.height {
                bail!("frame size {}x{} does not match renderer {}x{}", dl.width, dl.height, self.width, self.height);
            }
        }
        self.check()?;
        let mut out = Vec::with_capacity(frames.len());
        let mut in_flight: Option<InFlight> = None;
        let result = (|| -> Result<()> {
            let chunk_len = self.chunk;
            for (k, chunk) in frames.chunks(chunk_len).enumerate() {
                let layers: Vec<Option<Arc<LayerGpu>>> = chunk.iter().map(|dl| self.layer_gpu(dl)).collect();
                let geoms: Vec<_> = chunk.par_iter().map(|dl| tessellate(&dl.items, dl.width, dl.height)).collect();
                let vertex_bytes: u64 =
                    geoms.iter().map(|g| (g.vertices.len() * std::mem::size_of::<Vertex>()) as u64).sum();
                let index_bytes: u64 = geoms.iter().map(|g| (g.indices.len() * 4) as u64).sum();
                let layer_too_big = layers.iter().flatten().any(|l| l.too_big);
                if vertex_bytes > self.max_buffer || index_bytes > self.max_buffer || layer_too_big {
                    // Geometry past the GPU's buffer limit: rasterize this chunk on the CPU.
                    if let Some(prev) = in_flight.take() {
                        out.extend(self.collect(prev)?);
                    }
                    let pool = &self.pool;
                    out.extend(chunk.par_iter().map(|f| super::cpu::rasterize_pooled(f, pool)).collect::<Vec<_>>());
                    continue;
                }
                let next = self.submit_chunk(chunk, &geoms, &layers, (k % 2) * chunk_len)?;
                if let Err(e) = self.check() {
                    // The readbacks are already mapping: wait them out and unmap, or the
                    // next batch finds its buffers still pending.
                    self.release(&next);
                    return Err(e);
                }
                if let Some(prev) = in_flight.replace(next) {
                    out.extend(self.collect(prev)?);
                }
            }
            if let Some(prev) = in_flight.take() {
                out.extend(self.collect(prev)?);
            }
            self.check()
        })();
        if let Err(e) = result {
            // Leave no buffer mapped or pending, so a later batch can reuse them.
            if let Some(f) = in_flight.take() {
                self.release(&f);
            }
            return Err(e);
        }
        Ok(out)
    }
}

/// Longest wait for the GPU to finish a chunk; a hung driver becomes an error, not a hang.
const GPU_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

struct InFlight {
    first_buffer: usize,
    count: usize,
    submission: wgpu::SubmissionIndex,
    rx: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

impl GpuRenderer {
    /// Wait for a chunk and unmap its buffers, discarding the frames.
    fn release(&self, f: &InFlight) {
        let _ = self
            .device
            .poll(wgpu::PollType::Wait { submission_index: Some(f.submission.clone()), timeout: Some(GPU_WAIT) });
        for buf in &self.readbacks[f.first_buffer..f.first_buffer + f.count] {
            buf.unmap();
        }
    }

    fn collect(&mut self, f: InFlight) -> Result<Vec<Vec<u8>>> {
        let waited = self
            .device
            .poll(wgpu::PollType::Wait { submission_index: Some(f.submission.clone()), timeout: Some(GPU_WAIT) })
            .context("waiting for the GPU")
            .and_then(|_| {
                for _ in 0..f.count {
                    f.rx.recv().context("GPU readback channel closed")?.context("GPU readback failed")?;
                }
                Ok(())
            });
        if let Err(e) = waited {
            self.release(&f);
            return Err(e);
        }
        let row = (self.width * 4) as usize;
        let padded = self.padded_row as usize;
        let mut out = Vec::with_capacity(f.count);
        for buf in &self.readbacks[f.first_buffer..f.first_buffer + f.count] {
            {
                let data = buf.slice(..).get_mapped_range();
                let len = row * self.height as usize;
                let mut frame = self.pool.take(len);
                if padded == row {
                    // Rows are not padded (width a multiple of 64): one copy.
                    frame.copy_from_slice(&data[..len]);
                } else {
                    for (dst, src) in frame.chunks_exact_mut(row).zip(data.chunks_exact(padded)) {
                        dst.copy_from_slice(&src[..row]);
                    }
                }
                out.push(frame);
            }
            buf.unmap();
        }
        Ok(out)
    }

    fn submit_chunk(
        &mut self,
        frames: &[&DisplayList],
        geoms: &[VertexBuffers<Vertex, u32>],
        layers: &[Option<Arc<LayerGpu>>],
        first_buffer: usize,
    ) -> Result<InFlight> {
        for i in 0..frames.len() {
            self.readback(first_buffer + i);
        }
        // All of the chunk's geometry goes into two persistent buffers, one slice per frame.
        // `write_buffer` is ordered with the submissions around it, so the previous chunk,
        // still drawing from the same buffers, is unaffected.
        let vertex_bytes: u64 = geoms.iter().map(|g| (g.vertices.len() * std::mem::size_of::<Vertex>()) as u64).sum();
        let index_bytes: u64 = geoms.iter().map(|g| (g.indices.len() * 4) as u64).sum();
        let (mut vertices, mut indices) = (self.vertices.take(), self.indices.take());
        self.ensure(&mut vertices, vertex_bytes, wgpu::BufferUsages::VERTEX);
        self.ensure(&mut indices, index_bytes, wgpu::BufferUsages::INDEX);
        let mut slices = Vec::with_capacity(geoms.len());
        let (mut voff, mut ioff) = (0u64, 0u64);
        for g in geoms {
            let (vlen, ilen) =
                ((g.vertices.len() * std::mem::size_of::<Vertex>()) as u64, (g.indices.len() * 4) as u64);
            if let (Some(vb), Some(ib), true) = (&vertices, &indices, ilen > 0) {
                self.queue.write_buffer(vb, voff, bytemuck::cast_slice(&g.vertices));
                self.queue.write_buffer(ib, ioff, bytemuck::cast_slice(&g.indices));
            }
            slices.push((voff, vlen, ioff, ilen));
            voff += vlen;
            ioff += ilen;
        }

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let size = wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 };
        for (i, (dl, geom)) in frames.iter().zip(geoms).enumerate() {
            let (voff, vlen, ioff, ilen) = slices[i];
            {
                let bg = dl.background;
                let (view, resolve_target) = match &self.msaa_view {
                    Some(v) => (v, Some(&self.resolve_view)),
                    None => (&self.resolve_view, None),
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: bg[0] as f64,
                                g: bg[1] as f64,
                                b: bg[2] as f64,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.pipeline);
                if let Some((vb, ib, n)) = layers[i].as_ref().and_then(|l| l.buffers.as_ref()) {
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..*n, 0, 0..1);
                }
                if let (Some(vb), Some(ib), true) = (&vertices, &indices, ilen > 0) {
                    pass.set_vertex_buffer(0, vb.slice(voff..voff + vlen));
                    pass.set_index_buffer(ib.slice(ioff..ioff + ilen), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..geom.indices.len() as u32, 0, 0..1);
                }
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.resolve,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &self.readbacks[first_buffer + i],
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(self.padded_row),
                        rows_per_image: Some(self.height),
                    },
                },
                size,
            );
        }
        let submission = self.queue.submit([encoder.finish()]);
        self.vertices = vertices;
        self.indices = indices;

        let (tx, rx) = mpsc::channel();
        for i in 0..frames.len() {
            let tx = tx.clone();
            self.readbacks[first_buffer + i].slice(..).map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
        }
        Ok(InFlight { first_buffer, count: frames.len(), submission, rx })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_follow_threads_within_the_readback_budget() {
        let frame = |w: u64, h: u64| (w * 4).div_ceil(256) * 256 * h;
        assert_eq!(chunk_for(16, frame(1920, 1080)), 16);
        assert_eq!(chunk_for(32, frame(1920, 1080)), MAX_CHUNK);
        assert_eq!(chunk_for(2, frame(1280, 720)), 4); // never below a few frames
                                                       // 4K: two chunks of padded frames fit in the budget, so fewer frames per chunk.
        let big = chunk_for(16, frame(3840, 2160));
        assert!((2..16).contains(&big));
        assert!(2 * big as u64 * frame(3840, 2160) <= READBACK_BUDGET);
        // Absurd frames still make progress.
        assert_eq!(chunk_for(16, READBACK_BUDGET), 2);
    }
}
