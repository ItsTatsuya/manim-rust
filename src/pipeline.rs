//! Frame pipeline: the scene thread builds display lists while a worker thread
//! rasterizes and encodes them, so scene logic and rendering overlap.
//!
//! Consecutive identical frames are collapsed into one entry with a repeat count before
//! they leave the scene thread, so waits and static moments are rasterized once.

use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::thread::JoinHandle;

use anyhow::{anyhow, Result};

use crate::output::FrameWriter;
use crate::renderer::{DisplayList, Renderer};

type Batch = Vec<(DisplayList, u32)>;

pub struct Stats {
    pub frames_written: u64,
    pub frames_rasterized: u64,
    pub renderer: String,
    pub output: Option<PathBuf>,
}

pub struct Finished {
    pub renderer: Box<dyn Renderer>,
    pub last_frame: Option<Vec<u8>>,
    pub last_list: Option<DisplayList>,
    pub stats: Stats,
}

enum Mode {
    /// `-s`: keep only the latest display list; nothing is rasterized until the end.
    LastOnly {
        renderer: Box<dyn Renderer>,
        last: Option<DisplayList>,
    },
    Streaming {
        tx: Option<SyncSender<Batch>>,
        worker: Option<JoinHandle<Result<Finished>>>,
        batch_size: usize,
    },
}

pub struct Pipeline {
    mode: Mode,
    /// Not yet sent. The last entry is held back so a following identical frame can
    /// bump its repeat count instead of being rendered again.
    pending: Batch,
    renderer_name: String,
    output_description: String,
}

impl Pipeline {
    pub fn new(renderer: Box<dyn Renderer>, writer: Option<Box<dyn FrameWriter>>) -> Self {
        let renderer_name = renderer.name();
        let output_description = writer.as_ref().map(|w| w.describe()).unwrap_or_else(|| "last frame only".into());
        let mode = match writer {
            None => Mode::LastOnly { renderer, last: None },
            Some(writer) => {
                let batch_size = renderer.batch_size();
                let (tx, rx) = sync_channel::<Batch>(2);
                let worker = std::thread::Builder::new()
                    .name("manim-render".into())
                    .spawn(move || worker(renderer, writer, rx))
                    .expect("spawn render thread");
                Mode::Streaming { tx: Some(tx), worker: Some(worker), batch_size }
            }
        };
        Pipeline { mode, pending: Vec::new(), renderer_name, output_description }
    }

    pub fn renderer_name(&self) -> String {
        self.renderer_name.clone()
    }

    pub fn output_description(&self) -> String {
        self.output_description.clone()
    }

    pub fn push(&mut self, dl: DisplayList) -> Result<()> {
        match &mut self.mode {
            Mode::LastOnly { last, .. } => {
                *last = Some(dl);
                Ok(())
            }
            Mode::Streaming { tx, batch_size, .. } => {
                if let Some((last, n)) = self.pending.last_mut() {
                    if *last == dl {
                        *n += 1;
                        return Ok(());
                    }
                }
                self.pending.push((dl, 1));
                if self.pending.len() > *batch_size {
                    let held = self.pending.pop().expect("non-empty");
                    let batch = std::mem::replace(&mut self.pending, vec![held]);
                    let tx = tx.as_ref().ok_or_else(|| anyhow!("pipeline closed"))?;
                    if tx.send(batch).is_err() {
                        return Err(anyhow!("render thread stopped"));
                    }
                }
                Ok(())
            }
        }
    }

    /// Send everything, wait for the worker and return its state.
    pub fn finish(self) -> Result<Finished> {
        match self.mode {
            Mode::LastOnly { renderer, last } => Ok(Finished {
                stats: Stats { frames_written: 0, frames_rasterized: 0, renderer: renderer.name(), output: None },
                renderer,
                last_frame: None,
                last_list: last,
            }),
            Mode::Streaming { mut tx, mut worker, .. } => {
                if let Some(tx) = tx.take() {
                    if !self.pending.is_empty() {
                        let _ = tx.send(self.pending);
                    }
                }
                let handle = worker.take().expect("worker present");
                handle.join().map_err(|_| anyhow!("render thread panicked"))?
            }
        }
    }
}

/// Rasterizes batches and hands frames to a separate encoder thread, so rasterizing
/// batch k+1 overlaps with encoding batch k (three stages in flight: scene, raster, encode).
fn worker(
    mut renderer: Box<dyn Renderer>,
    writer: Box<dyn FrameWriter>,
    rx: std::sync::mpsc::Receiver<Batch>,
) -> Result<Finished> {
    let output = Some(writer.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty());
    let (ftx, frx) = sync_channel::<(std::sync::Arc<Vec<u8>>, u32)>(renderer.batch_size() * 2);
    let encoder = std::thread::Builder::new()
        .name("manim-encode".into())
        .spawn(move || -> Result<u64> {
            let mut writer = writer;
            let mut written = 0u64;
            let trace = std::env::var_os("MANIM_TRACE").is_some();
            let (mut t_wait, mut t_write) = (0.0, 0.0);
            let mut t = std::time::Instant::now();
            for (frame, n) in frx {
                t_wait += t.elapsed().as_secs_f64();
                t = std::time::Instant::now();
                writer.write(&frame, n)?;
                t_write += t.elapsed().as_secs_f64();
                t = std::time::Instant::now();
                written += n as u64;
            }
            let t_fin = std::time::Instant::now();
            writer.finish()?;
            if trace {
                eprintln!(
                    "trace: encoder waited {t_wait:.2}s for frames, wrote {t_write:.2}s, finish {:.2}s",
                    t_fin.elapsed().as_secs_f64()
                );
            }
            Ok(written)
        })
        .expect("spawn encode thread");

    let mut last_frame: Option<std::sync::Arc<Vec<u8>>> = None;
    let mut last_list = None;
    let mut rasterized = 0u64;
    let mut send_failed = false;
    let mut render_err = None;
    let trace = std::env::var_os("MANIM_TRACE").is_some();
    let (mut t_wait, mut t_render) = (0.0, 0.0);
    let mut t = std::time::Instant::now();
    for batch in rx {
        t_wait += t.elapsed().as_secs_f64();
        t = std::time::Instant::now();
        let refs: Vec<&DisplayList> = batch.iter().map(|(d, _)| d).collect();
        let result = renderer.render_batch(&refs);
        t_render += t.elapsed().as_secs_f64();
        t = std::time::Instant::now();
        let frames = match result {
            Ok(f) => f,
            Err(e) => {
                render_err = Some(e);
                break;
            }
        };
        rasterized += frames.len() as u64;
        for (f, (_, n)) in frames.into_iter().zip(&batch) {
            let f = std::sync::Arc::new(f);
            last_frame = Some(f.clone());
            if ftx.send((f, *n)).is_err() {
                send_failed = true;
                break;
            }
        }
        last_list = batch.into_iter().last().map(|(d, _)| d);
        if send_failed {
            break;
        }
    }
    drop(ftx);
    if trace {
        eprintln!("trace: rasterizer waited {t_wait:.2}s for scene, rendered {t_render:.2}s");
    }
    let written = encoder.join().map_err(|_| anyhow!("encode thread panicked"))??;
    if let Some(e) = render_err {
        return Err(e);
    }
    let last_frame = last_frame.map(|a| std::sync::Arc::try_unwrap(a).unwrap_or_else(|a| (*a).clone()));
    Ok(Finished {
        stats: Stats {
            frames_written: written,
            frames_rasterized: rasterized,
            renderer: renderer.name(),
            // Writers delete their file when no frames arrived.
            output: output.filter(|_| written > 0),
        },
        renderer,
        last_frame,
        last_list,
    })
}
