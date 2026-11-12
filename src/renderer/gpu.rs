//! GPU renderer: lyon tessellates paths (in parallel on the CPU), wgpu rasterizes them
//! with 4× MSAA into an offscreen texture, and frames are read back in batches.

use std::sync::mpsc;

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

use super::{DisplayList, Renderer};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Curve flattening tolerance in pixels.
const TOLERANCE: f32 = 0.15;
/// Frames per GPU submission.
const CHUNK: usize = 8;
/// Frames handed over per `render_batch` call (several pipelined chunks).
const BATCH: usize = 48;

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
    wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::PRIMARY, ..Default::default() })
}

/// Human-readable list of adapters, plus which one `Auto` would choose.
pub fn adapter_report() -> String {
    let inst = instance();
    let mut out = String::new();
    let adapters = inst.enumerate_adapters(wgpu::Backends::PRIMARY);
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
}

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
        .ok_or_else(|| anyhow!("no GPU adapter found"))?;
        let info = adapter.get_info();
        if require_hardware && info.device_type == wgpu::DeviceType::Cpu {
            bail!("only a software adapter is available ({})", info.name);
        }
        let limits = adapter.limits();
        if width > limits.max_texture_dimension_2d || height > limits.max_texture_dimension_2d {
            bail!("{}x{} exceeds the GPU's max texture size {}", width, height, limits.max_texture_dimension_2d);
        }
        let samples = if adapter.get_texture_format_features(FORMAT).flags.sample_count_supported(4) { 4 } else { 1 };

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("manim"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults().using_resolution(limits),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .context("could not open GPU device")?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("manim shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            push_constant_ranges: &[],
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
            multiview: None,
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
        let padded_row = (width * 4).div_ceil(256) * 256;

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
        })
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

fn tessellate(dl: &DisplayList) -> VertexBuffers<Vertex, u32> {
    let (w, h) = (dl.width as f32, dl.height as f32);
    let ndc = move |x: f32, y: f32| [x / w * 2.0 - 1.0, 1.0 - y / h * 2.0];
    let mut geom: VertexBuffers<Vertex, u32> = VertexBuffers::new();
    let mut fill_t = FillTessellator::new();
    let mut stroke_t = StrokeTessellator::new();
    for item in dl.all_items().filter(|i| i.image.is_none()) {
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

    /// Chunks are pipelined: while the GPU renders chunk k, the CPU reads back chunk k-1
    /// and tessellates chunk k+1. Readback buffers alternate between two slots.
    fn render_batch(&mut self, frames: &[&DisplayList]) -> Result<Vec<Vec<u8>>> {
        // Raster images are drawn by the CPU rasterizer (a GPU texture path would cost
        // more in uploads than these frames take to rasterize on all cores).
        if frames.iter().any(|f| f.has_images()) {
            return Ok(frames.par_iter().map(|f| super::cpu::rasterize(f)).collect());
        }
        for dl in frames {
            if dl.width != self.width || dl.height != self.height {
                bail!("frame size {}x{} does not match renderer {}x{}", dl.width, dl.height, self.width, self.height);
            }
        }
        let mut out = Vec::with_capacity(frames.len());
        let mut in_flight: Option<InFlight> = None;
        for (k, chunk) in frames.chunks(CHUNK).enumerate() {
            let geoms: Vec<_> = chunk.par_iter().map(|dl| tessellate(dl)).collect();
            let next = self.submit_chunk(chunk, &geoms, (k % 2) * CHUNK)?;
            if let Some(prev) = in_flight.replace(next) {
                out.extend(self.collect(prev)?);
            }
        }
        if let Some(prev) = in_flight {
            out.extend(self.collect(prev)?);
        }
        Ok(out)
    }
}

struct InFlight {
    first_buffer: usize,
    count: usize,
    submission: wgpu::SubmissionIndex,
    rx: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

impl GpuRenderer {
    fn collect(&mut self, f: InFlight) -> Result<Vec<Vec<u8>>> {
        let _ = self.device.poll(wgpu::Maintain::WaitForSubmissionIndex(f.submission));
        for _ in 0..f.count {
            f.rx.recv().context("GPU readback channel closed")?.context("GPU readback failed")?;
        }
        let row = (self.width * 4) as usize;
        let mut out = Vec::with_capacity(f.count);
        for buf in &self.readbacks[f.first_buffer..f.first_buffer + f.count] {
            {
                let data = buf.slice(..).get_mapped_range();
                let mut frame = Vec::with_capacity(row * self.height as usize);
                for y in 0..self.height as usize {
                    let s = y * self.padded_row as usize;
                    frame.extend_from_slice(&data[s..s + row]);
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
        first_buffer: usize,
    ) -> Result<InFlight> {
        for i in 0..frames.len() {
            self.readback(first_buffer + i);
        }

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let mut keep_alive = Vec::new();
        let size = wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 };
        for (i, (dl, geom)) in frames.iter().zip(geoms).enumerate() {
            let buffers = (!geom.indices.is_empty()).then(|| {
                let vb = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&geom.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let ib = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&geom.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                (vb, ib)
            });
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
                });
                if let Some((vb, ib)) = &buffers {
                    pass.set_pipeline(&self.pipeline);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
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
            keep_alive.push(buffers);
        }
        let submission = self.queue.submit([encoder.finish()]);
        drop(keep_alive); // wgpu keeps submitted resources alive internally

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
