//! Frame pipeline: the scene thread builds display lists while a worker thread
//! rasterizes and encodes them, so scene logic and rendering overlap.
//!
//! Consecutive identical frames are collapsed into one entry with a repeat count before
//! they leave the scene thread, so waits and static moments are rasterized once.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::output::FrameWriter;
use crate::renderer::{DisplayList, Frame, FramePool, Renderer};

type Batch = Vec<(DisplayList, u32)>;

/// Memory the frames in flight (one batch being drawn, the batch finished before it and
/// the encoder queue) may use together, before the fixed cost of the renderer itself.
pub const FRAME_MEMORY_BUDGET: usize = 512 << 20;

/// Batch size and encoder queue length for frames of `rgba_bytes` (as rasterized) and
/// `out_bytes` (as queued for the encoder, smaller after YUV conversion).
///
/// The renderer's own `batch_size` is the ceiling (it keeps every core busy); big frames
/// lower it so that the batch plus the two batches' worth of queue fit in `budget` bytes,
/// but never below 2 frames. A frame size of 0 means "unknown": the ceiling is used as is.
pub fn plan_queues(renderer_batch: usize, rgba_bytes: usize, out_bytes: usize, budget: usize) -> (usize, usize) {
    let ceiling = renderer_batch.max(1);
    if rgba_bytes == 0 {
        return (ceiling, ceiling * 2);
    }
    let per_slot = rgba_bytes + 3 * out_bytes;
    let batch = ceiling.min((budget / per_slot).max(2));
    (batch, batch * 2)
}

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
    mode: Option<Mode>,
    /// Set to drop the output instead of finishing it (see [`Pipeline::abort`]).
    abort: Arc<AtomicBool>,
    /// Set by [`Pipeline::finish`] once every frame is queued. The output is only
    /// published when this is set and `abort` is not: anything else (an error, a panic,
    /// a pipeline dropped half-way) leaves the previous file untouched.
    commit: Arc<AtomicBool>,
    /// Polled while waiting for the render thread, so a cancel is not stuck behind a full queue.
    interrupt: Option<Box<dyn Fn() -> bool + Send>>,
    /// Not yet sent. The last entry is held back so a following identical frame can
    /// bump its repeat count instead of being rendered again.
    pending: Batch,
    renderer_name: String,
    output_description: String,
}

impl Pipeline {
    pub fn new(renderer: Box<dyn Renderer>, writer: Option<Box<dyn FrameWriter>>) -> Result<Self> {
        Self::with_frame_size(renderer, writer, None)
    }

    /// Like [`new`](Self::new), knowing the frame size, so the queues are sized by bytes
    /// (see [`plan_queues`]) instead of frames alone.
    pub fn with_frame_size(
        renderer: Box<dyn Renderer>,
        writer: Option<Box<dyn FrameWriter>>,
        size: Option<(u32, u32)>,
    ) -> Result<Self> {
        let renderer_name = renderer.name();
        let abort = Arc::new(AtomicBool::new(false));
        let commit = Arc::new(AtomicBool::new(false));
        let output_description = writer.as_ref().map(|w| w.describe()).unwrap_or_else(|| "last frame only".into());
        let mode = match writer {
            None => Mode::LastOnly { renderer, last: None },
            Some(writer) => {
                let rgba_bytes = size.map_or(0, |(w, h)| w as usize * h as usize * 4);
                let out_bytes = writer.preparer().map_or(rgba_bytes, |p| p.frame_len());
                let (batch_size, queue) =
                    plan_queues(renderer.batch_size(), rgba_bytes, out_bytes, FRAME_MEMORY_BUDGET);
                let (tx, rx) = sync_channel::<Batch>(2);
                let flags = Flags { abort: abort.clone(), commit: commit.clone() };
                let worker = std::thread::Builder::new()
                    .name("manim-render".into())
                    .spawn(move || worker(renderer, writer, rx, flags, queue))
                    .map_err(|e| anyhow!("cannot start the render thread: {e}"))?;
                Mode::Streaming { tx: Some(tx), worker: Some(worker), batch_size }
            }
        };
        Ok(Pipeline {
            mode: Some(mode),
            abort,
            commit,
            interrupt: None,
            pending: Vec::new(),
            renderer_name,
            output_description,
        })
    }

    /// Make [`push`](Self::push) give up (with an error) when `interrupted` returns true
    /// while it waits for the render thread.
    pub fn set_interrupt(&mut self, interrupted: impl Fn() -> bool + Send + 'static) {
        self.interrupt = Some(Box::new(interrupted));
    }

    pub fn renderer_name(&self) -> String {
        self.renderer_name.clone()
    }

    pub fn output_description(&self) -> String {
        self.output_description.clone()
    }

    pub fn push(&mut self, dl: DisplayList) -> Result<()> {
        match self.mode.as_mut().expect("pipeline already finished") {
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
                    send_batch(tx, batch, self.interrupt.as_deref())?;
                }
                Ok(())
            }
        }
    }

    /// Send everything, wait for the worker and return its state.
    pub fn finish(mut self) -> Result<Finished> {
        match self.mode.take().expect("pipeline already finished") {
            Mode::LastOnly { renderer, last } => Ok(Finished {
                stats: Stats { frames_written: 0, frames_rasterized: 0, renderer: renderer.name(), output: None },
                renderer,
                last_frame: None,
                last_list: last,
            }),
            Mode::Streaming { mut tx, mut worker, .. } => {
                let sent = match tx.take() {
                    Some(tx) => {
                        let r = if self.pending.is_empty() {
                            Ok(())
                        } else {
                            send_batch(&tx, std::mem::take(&mut self.pending), self.interrupt.as_deref())
                        };
                        if r.is_ok() {
                            self.commit.store(true, Ordering::SeqCst);
                        }
                        r // dropping `tx` ends the stream
                    }
                    None => Err(anyhow!("pipeline closed")),
                };
                if sent.is_err() {
                    self.abort.store(true, Ordering::SeqCst);
                }
                let handle = worker.take().ok_or_else(|| anyhow!("pipeline closed"))?;
                let joined = handle.join().map_err(|_| anyhow!("render thread panicked"))?;
                // The worker's own error (ffmpeg died, ...) is the real cause of a failed send.
                match (sent, joined) {
                    (Ok(()), r) => r,
                    (Err(_), Err(worker_err)) => Err(worker_err),
                    (Err(e), Ok(_)) => Err(e),
                }
            }
        }
    }

    /// Stop without producing output: frames still queued are dropped and the writer
    /// discards its partial file, so the previous output (if any) stays as it was.
    /// Returns the worker's own error, if it had one (e.g. ffmpeg exited).
    pub fn abort(mut self) -> Option<anyhow::Error> {
        self.stop()
    }

    fn stop(&mut self) -> Option<anyhow::Error> {
        if let Some(Mode::Streaming { tx, worker, .. }) = self.mode.take() {
            self.abort.store(true, Ordering::SeqCst);
            drop(tx);
            return match worker?.join() {
                Ok(r) => r.err(),
                Err(_) => Some(anyhow!("render thread panicked")),
            };
        }
        None
    }
}

impl Drop for Pipeline {
    /// Dropped without `finish` (an error unwound the scene): treat it as an abort.
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Queue a batch for the render thread. Waits while the queue is full, but keeps
/// checking `interrupted` so a cancel is noticed within a millisecond or so.
fn send_batch(tx: &SyncSender<Batch>, mut batch: Batch, interrupted: Option<&(dyn Fn() -> bool + Send)>) -> Result<()> {
    let mut spins = 0u32;
    loop {
        match tx.try_send(batch) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Disconnected(_)) => return Err(anyhow!("render thread stopped")),
            Err(TrySendError::Full(b)) => {
                if interrupted.is_some_and(|f| f()) {
                    return Err(anyhow!("render cancelled"));
                }
                batch = b;
                spins += 1;
                if spins < 20 {
                    std::thread::yield_now();
                } else {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }
}

/// The two signals that decide whether the encoder publishes its file (see [`Pipeline`]).
struct Flags {
    abort: Arc<AtomicBool>,
    commit: Arc<AtomicBool>,
}

/// Sets `abort` when dropped while still armed, so a panic in the rasterizer can never
/// look like a clean end of stream to the encoder.
struct AbortOnDrop {
    flag: Arc<AtomicBool>,
    armed: bool,
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.flag.store(true, Ordering::SeqCst);
        }
    }
}

/// Rasterizes batches and hands frames to a separate encoder thread, so rasterizing
/// batch k+1 overlaps with encoding batch k (three stages in flight: scene, raster, encode).
fn worker(
    mut renderer: Box<dyn Renderer>,
    writer: Box<dyn FrameWriter>,
    rx: std::sync::mpsc::Receiver<Batch>,
    flags: Flags,
    queue: usize,
) -> Result<Finished> {
    /// What the encoder thread is handed: RGBA shared with `last_frame`, or a converted frame.
    enum Payload {
        Shared(Arc<Vec<u8>>),
        Prepared(Vec<u8>),
    }
    let Flags { abort, commit } = flags;
    let abort_enc = abort.clone();
    let output = Some(writer.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty());
    let pool = FramePool::new();
    renderer.use_pool(pool.clone());
    // Colour conversion (RGBA to YUV for MP4) runs on the render threads, so the encoder
    // thread below only moves bytes.
    let preparer = writer.preparer();
    let (ftx, frx) = sync_channel::<(Payload, u32)>(queue.max(1));
    let enc_pool = pool.clone();
    let encoder = std::thread::Builder::new()
        .name("manim-encode".into())
        .spawn(move || -> Result<u64> {
            let mut writer = writer;
            let mut written = 0u64;
            let trace = std::env::var_os("MANIM_TRACE").is_some();
            let (mut t_wait, mut t_write) = (0.0, 0.0);
            let mut t = std::time::Instant::now();
            for (frame, n) in frx {
                if abort_enc.load(Ordering::SeqCst) {
                    break; // cancelled: don't write what is still queued
                }
                t_wait += t.elapsed().as_secs_f64();
                t = std::time::Instant::now();
                match frame {
                    Payload::Shared(rgba) => {
                        writer.write(&rgba, n)?;
                        enc_pool.put_shared(rgba);
                    }
                    Payload::Prepared(data) => {
                        writer.write_prepared(&data, n)?;
                        enc_pool.put(data);
                    }
                }
                t_write += t.elapsed().as_secs_f64();
                t = std::time::Instant::now();
                written += n as u64;
            }
            let t_fin = std::time::Instant::now();
            // Publish only after an explicit clean finish; every other way out discards the partial file.
            if abort_enc.load(Ordering::SeqCst) || !commit.load(Ordering::SeqCst) {
                drop(writer);
                return Ok(written);
            }
            writer.finish()?;
            if trace {
                eprintln!(
                    "trace: encoder waited {t_wait:.2}s for frames, wrote {t_write:.2}s, finish {:.2}s",
                    t_fin.elapsed().as_secs_f64()
                );
            }
            Ok(written)
        })
        .map_err(|e| anyhow!("cannot start the encoder thread: {e}"))?;
    // Declared after `ftx` so that it drops first when a panic unwinds this function.
    let mut guard = AbortOnDrop { flag: abort.clone(), armed: true };

    // The newest RGBA frame, shared with the encoder (no preparer) or kept by itself.
    let mut last_shared: Option<Arc<Vec<u8>>> = None;
    let mut last_owned: Option<Vec<u8>> = None;
    let mut last_list = None;
    let mut rasterized = 0u64;
    let mut send_failed = false;
    let mut render_err = None;
    let trace = std::env::var_os("MANIM_TRACE").is_some();
    let (mut t_wait, mut t_render) = (0.0, 0.0);
    let mut t = std::time::Instant::now();
    for batch in rx {
        if abort.load(Ordering::SeqCst) {
            break;
        }
        t_wait += t.elapsed().as_secs_f64();
        t = std::time::Instant::now();
        let refs: Vec<&DisplayList> = batch.iter().map(|(d, _)| d).collect();
        let newest = refs.len() - 1;
        let finish = |i: usize, rgba: Vec<u8>| -> Result<Frame> {
            let Some(p) = &preparer else { return Ok(Frame::Rgba(rgba)) };
            let mut data = pool.take(p.frame_len());
            p.prepare(&rgba, &mut data)?;
            if i == newest {
                return Ok(Frame::Prepared { data, rgba: Some(rgba) });
            }
            pool.put(rgba);
            Ok(Frame::Prepared { data, rgba: None })
        };
        let result = renderer.render_batch_with(&refs, &finish);
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
            let payload = match f {
                Frame::Rgba(v) => {
                    let a = Arc::new(v);
                    if let Some(old) = last_shared.replace(a.clone()) {
                        pool.put_shared(old);
                    }
                    Payload::Shared(a)
                }
                Frame::Prepared { data, rgba } => {
                    if let Some(old) = rgba.and_then(|r| last_owned.replace(r)) {
                        pool.put(old);
                    }
                    Payload::Prepared(data)
                }
            };
            if ftx.send((payload, *n)).is_err() {
                send_failed = true;
                break;
            }
        }
        last_list = batch.into_iter().last().map(|(d, _)| d);
        if send_failed {
            break;
        }
    }
    // Only a clean run may let the encoder publish; any failure (or an abort) is
    // signalled *before* the stream ends so the encoder cannot mistake it for completion.
    if render_err.is_some() || send_failed {
        abort.store(true, Ordering::SeqCst);
    }
    guard.armed = false;
    drop(ftx);
    if trace {
        eprintln!("trace: rasterizer waited {t_wait:.2}s for scene, rendered {t_render:.2}s");
    }
    let written = encoder.join().map_err(|_| anyhow!("encode thread panicked"))??;
    if let Some(e) = render_err {
        return Err(e);
    }
    let last_frame = last_owned.or_else(|| last_shared.map(|a| Arc::try_unwrap(a).unwrap_or_else(|a| (*a).clone())));
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

#[cfg(test)]
mod tests {
    use super::*;

    const MB: usize = 1 << 20;

    #[test]
    fn small_frames_keep_the_renderers_batch() {
        let (w, h) = (1920usize, 1080usize);
        // 1080p on 16 threads: close to the renderer's 32 (the byte cap trims a few frames).
        let (batch, queue) = plan_queues(32, w * h * 4, w * h * 3 / 2, FRAME_MEMORY_BUDGET);
        assert!((28..=32).contains(&batch) && queue == batch * 2);
        // 720p and below are not capped at all.
        let (w, h) = (1280usize, 720usize);
        assert_eq!(plan_queues(32, w * h * 4, w * h * 3 / 2, FRAME_MEMORY_BUDGET), (32, 64));
        // Unknown frame size: the ceiling as is.
        assert_eq!(plan_queues(32, 0, 0, FRAME_MEMORY_BUDGET), (32, 64));
        assert_eq!(plan_queues(0, 0, 0, FRAME_MEMORY_BUDGET), (1, 2));
    }

    #[test]
    fn big_frames_are_capped_by_bytes() {
        let (w, h) = (3840usize, 2160usize);
        let (rgba, yuv) = (w * h * 4, w * h * 3 / 2);
        let (batch, queue) = plan_queues(32, rgba, yuv, FRAME_MEMORY_BUDGET);
        assert!((2..32).contains(&batch));
        // One batch being drawn, the one finished before it and the queue stay in budget.
        let total = batch * rgba + 3 * batch * yuv;
        assert!(total <= FRAME_MEMORY_BUDGET, "{} MB", total / MB);
        assert_eq!(queue, batch * 2);
        // Absurdly large frames still make progress with two at a time.
        assert_eq!(plan_queues(32, 600 * MB, 300 * MB, FRAME_MEMORY_BUDGET).0, 2);
        // A renderer that wants fewer frames is never raised.
        assert_eq!(plan_queues(1, 8 * MB, 3 * MB, FRAME_MEMORY_BUDGET).0, 1);
    }
}
