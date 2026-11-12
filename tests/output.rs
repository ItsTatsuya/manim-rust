//! Frame pipeline (dedup, batching) and the video/image writers.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use common::*;
use manim::output::{self, create_writer, Encoder, FrameWriter, OutputFormat};
use manim::pipeline::Pipeline;
use manim::prelude::*;
use manim::renderer::{DisplayList, Renderer};

/// Renders frame k as 4 bytes all equal to the display list's item count.
struct CountingRenderer {
    rasterized: Arc<Mutex<u64>>,
    batch: usize,
}

impl Renderer for CountingRenderer {
    fn name(&self) -> String {
        "counting".into()
    }
    fn render_batch(&mut self, frames: &[&DisplayList]) -> anyhow::Result<Vec<Vec<u8>>> {
        *self.rasterized.lock().unwrap() += frames.len() as u64;
        Ok(frames.iter().map(|f| vec![f.items.len() as u8; 4]).collect())
    }
    fn batch_size(&self) -> usize {
        self.batch
    }
}

struct RecordingWriter(Arc<Mutex<Vec<(u8, u32)>>>, PathBuf);

impl FrameWriter for RecordingWriter {
    fn write(&mut self, rgba: &[u8], repeat: u32) -> anyhow::Result<()> {
        self.0.lock().unwrap().push((rgba[0], repeat));
        Ok(())
    }
    fn finish(self: Box<Self>) -> anyhow::Result<()> {
        Ok(())
    }
    fn path(&self) -> &Path {
        &self.1
    }
    fn describe(&self) -> String {
        "recording".into()
    }
}

fn dl(n: usize) -> DisplayList {
    let mut w = World::new();
    for i in 0..n {
        let id = w.insert(Dot::at(RIGHT * i as f64));
        w.show(id);
    }
    Camera::new(16, 9).display_list(&w)
}

fn run_pipeline(seq: &[usize], batch: usize) -> (u64, Vec<(u8, u32)>, u64) {
    let rasterized = Arc::new(Mutex::new(0));
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut p = Pipeline::new(
        Box::new(CountingRenderer { rasterized: rasterized.clone(), batch }),
        Some(Box::new(RecordingWriter(log.clone(), PathBuf::from("x")))),
    );
    for &n in seq {
        p.push(dl(n)).unwrap();
    }
    let fin = p.finish().unwrap();
    let r = *rasterized.lock().unwrap();
    let l = log.lock().unwrap().clone();
    (r, l, fin.stats.frames_written)
}

#[test]
fn pipeline_dedups_consecutive_frames() {
    let (r, log, written) = run_pipeline(&[1, 1, 1, 2, 2, 1], 64);
    assert_eq!(r, 3);
    assert_eq!(log, vec![(1, 3), (2, 2), (1, 1)]);
    assert_eq!(written, 6);
}

#[test]
fn pipeline_dedups_across_batch_boundaries() {
    let seq: Vec<usize> = std::iter::repeat_n(1, 50).chain(std::iter::repeat_n(2, 7)).chain([3]).collect();
    for batch in [1, 2, 3, 8] {
        let (r, log, written) = run_pipeline(&seq, batch);
        assert_eq!(r, 3, "batch {batch}");
        assert_eq!(log, vec![(1, 50), (2, 7), (3, 1)], "batch {batch}");
        assert_eq!(written, 58);
    }
}

#[test]
fn pipeline_preserves_order_with_many_batches() {
    let seq: Vec<usize> = (0..200).map(|i| i % 5).collect();
    let (r, log, _) = run_pipeline(&seq, 7);
    assert_eq!(r, 200);
    assert!(log.iter().enumerate().all(|(i, (v, n))| *v as usize == i % 5 && *n == 1));
}

#[test]
fn last_frame_only_mode_never_rasterizes_early() {
    let rasterized = Arc::new(Mutex::new(0));
    let mut p = Pipeline::new(Box::new(CountingRenderer { rasterized: rasterized.clone(), batch: 4 }), None);
    for n in 0..100 {
        p.push(dl(n % 3)).unwrap();
    }
    let fin = p.finish().unwrap();
    assert_eq!(*rasterized.lock().unwrap(), 0);
    assert_eq!(fin.last_list.unwrap().items.len(), 0, "keeps the last pushed frame (99 % 3 == 0)");
}

fn solid(w: u32, h: u32, v: u8) -> Vec<u8> {
    (0..w * h).flat_map(|_| [v, 255 - v, v / 2, 255]).collect()
}

#[test]
fn gif_writer_merges_short_frames() {
    let dir = temp_dir("gif");
    let mut wr = create_writer(OutputFormat::Gif, Encoder::Auto, &dir.join("a"), 32, 18, 60.0).unwrap();
    for i in 0..30u8 {
        wr.write(&solid(32, 18, i * 8), 1).unwrap();
    }
    wr.write(&solid(32, 18, 3), 60).unwrap();
    let path = wr.path().to_path_buf();
    wr.finish().unwrap();
    let mut dec = gif::DecodeOptions::new();
    dec.set_color_output(gif::ColorOutput::RGBA);
    let mut d = dec.read_info(std::fs::File::open(&path).unwrap()).unwrap();
    let (mut frames, mut centis) = (0, 0u32);
    while let Some(f) = d.read_next_frame().unwrap() {
        frames += 1;
        centis += f.delay as u32;
        assert!(f.delay >= 2, "no sub-2cs delays (players clamp them)");
    }
    assert!(frames < 31, "60 fps frames are merged to ≥2cs");
    assert!((centis as i32 - 150).abs() <= 3, "total duration preserved: {centis} cs");
}

#[test]
fn png_sequence_expands_repeats() {
    let dir = temp_dir("png");
    let mut wr = create_writer(OutputFormat::Png, Encoder::Auto, &dir.join("seq"), 8, 6, 10.0).unwrap();
    wr.write(&solid(8, 6, 10), 3).unwrap();
    wr.write(&solid(8, 6, 20), 1).unwrap();
    wr.finish().unwrap();
    assert_eq!(std::fs::read_dir(dir.join("seq")).unwrap().count(), 4);
}

fn ffprobe_frames(ffmpeg: &Path, file: &Path) -> Option<u64> {
    let probe = ffmpeg.with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" });
    let probe = if probe.is_file() { probe } else { PathBuf::from("ffprobe") };
    let out = Command::new(probe)
        .args(["-v", "error", "-count_packets", "-select_streams", "v:0", "-show_entries", "stream=nb_read_packets"])
        .args(["-of", "csv=p=0"])
        .arg(file)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

#[test]
fn mp4_writer_even_and_odd_sizes() {
    let Some(ff) = output::find_ffmpeg() else {
        eprintln!("skipping: ffmpeg not found");
        return;
    };
    let dir = temp_dir("mp4");
    for (w, h) in [(64, 48), (65, 49)] {
        let mut wr = create_writer(OutputFormat::Mp4, Encoder::X264, &dir.join(format!("v{w}")), w, h, 30.0).unwrap();
        for i in 0..20u8 {
            wr.write(&solid(w, h, i * 10), if i == 5 { 11 } else { 1 }).unwrap();
        }
        let path = wr.path().to_path_buf();
        wr.finish().unwrap();
        if let Some(n) = ffprobe_frames(&ff, &path) {
            assert_eq!(n, 30, "{w}x{h}");
        }
    }
}

#[test]
fn yuv_conversion_roundtrips_through_ffmpeg() {
    // Encode a flat color through our RGBA→YUV path and decode it again with ffmpeg.
    let Some(ff) = output::find_ffmpeg() else { return };
    let dir = temp_dir("yuv");
    let mut wr = create_writer(OutputFormat::Mp4, Encoder::X264, &dir.join("c"), 64, 64, 10.0).unwrap();
    let color = [252u8, 98, 85, 255];
    let frame: Vec<u8> = color.iter().copied().cycle().take(64 * 64 * 4).collect();
    wr.write(&frame, 5).unwrap();
    let path = wr.path().to_path_buf();
    wr.finish().unwrap();
    let out = Command::new(&ff)
        .args(["-v", "error", "-i"])
        .arg(&path)
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .unwrap();
    let mid = &out.stdout[(32 * 64 + 32) * 3..][..3];
    for c in 0..3 {
        assert!((mid[c] as i32 - color[c] as i32).abs() <= 4, "decoded {mid:?} vs {color:?}");
    }
}

#[test]
fn scene_with_no_frames_saves_png_and_no_broken_video() {
    for format in [OutputFormat::Gif, OutputFormat::Mp4] {
        if format == OutputFormat::Mp4 && output::find_ffmpeg().is_none() {
            continue;
        }
        let name = format!("still_{format:?}");
        let mut cfg = test_config(&name);
        cfg.format = format;
        render_with(&cfg, &name, |s| {
            s.add(Circle::new(1.0));
        })
        .expect("a scene without play/wait must still succeed");
        let png = cfg.media_dir.join("images").join(&name).join(format!("{name}.png"));
        assert!(png.is_file(), "falls back to saving the still frame");
        let vids = cfg.media_dir.join("videos");
        let leftovers: Vec<_> = walk(&vids).into_iter().filter(|p| p.is_file()).collect();
        assert!(leftovers.is_empty(), "no empty video files left behind: {leftovers:?}");
    }
}

#[test]
fn full_scene_to_gif_and_png() {
    let mut cfg = test_config("full_gif");
    cfg.format = OutputFormat::Gif;
    cfg.save_last_frame = true;
    render_with(&cfg, "Full", |s| {
        let c = s.add(Circle::new(1.0));
        s.play(Transform::new(c, Square::new(2.0)));
        s.wait(0.5);
    })
    .unwrap();
    assert!(cfg.media_dir.join("videos/Full/90p10/Full.gif").is_file());
    assert!(cfg.media_dir.join("images/Full/Full.png").is_file());
}

#[test]
fn output_flag_writes_flat_files() {
    // Directory: <dir>/<Scene>.<ext>, and nothing else is created.
    let dir = temp_dir("out_dir");
    let mut cfg = test_config("out_dir");
    cfg.format = OutputFormat::Gif;
    cfg.save_last_frame = true;
    cfg.output = Some(dir.join("renders"));
    render_with(&cfg, "Flat", |s| {
        let c = s.add(Circle::new(1.0));
        s.play(Indicate::new(c));
    })
    .unwrap();
    let mut names: Vec<String> = std::fs::read_dir(dir.join("renders"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["Flat.gif", "Flat.png"]);
    assert!(!cfg.media_dir.join("videos").exists(), "media/ tree is not used with --output");

    // Explicit file name: extension follows the format.
    let mut cfg = test_config("out_file");
    cfg.format = OutputFormat::Gif;
    cfg.output = Some(dir.join("clip.mp4"));
    render_with(&cfg, "Named", |s| s.wait(0.3)).unwrap();
    assert!(dir.join("clip.gif").is_file());

    let p = Config::parse(["-o", "out"].map(String::from)).unwrap();
    assert_eq!(p.video_base("S"), Path::new("out").join("S"));
}

/// End-to-end check of the `manim-rs` script runner. Slow on first run (compiles the
/// engine into a private cache), so it's opt-in: `cargo test --release -- --ignored`.
#[test]
#[ignore]
fn script_runner_leaves_only_script_and_video() {
    let dir = temp_dir("runner");
    let cache = dir.join("cache");
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let runner = env!("CARGO_BIN_EXE_manim-rs");
    let ok = Command::new(runner).args(["new", "demo.rs"]).current_dir(&work).status().unwrap();
    assert!(ok.success());
    let ok = Command::new(runner)
        .args(["demo.rs", "-ql", "--format", "gif", "-q"])
        .current_dir(&work)
        .env("MANIM_RS_CACHE", &cache)
        .status()
        .unwrap();
    assert!(ok.success());
    let mut names: Vec<String> =
        std::fs::read_dir(&work).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["Demo.gif", "demo.rs"]);
    let leftovers: Vec<_> = walk(&cache)
        .into_iter()
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("scene"))
        .filter(|p| p.is_file() && matches!(p.extension().and_then(|e| e.to_str()), None | Some("exe" | "pdb")))
        .collect();
    assert!(leftovers.is_empty(), "compiled scene left behind: {leftovers:?}");
}

fn walk(p: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                out.extend(walk(&path));
            }
            out.push(path);
        }
    }
    out
}

#[test]
fn config_parsing() {
    let p = |a: &[&str]| Config::parse(a.iter().map(|s| s.to_string()));
    let c = p(&["-qk"]).unwrap();
    assert_eq!((c.pixel_width, c.pixel_height, c.fps), (3840, 2160, 60.0));
    let c = p(&["-r", "1280x720", "--fps", "24"]).unwrap();
    assert_eq!((c.pixel_width, c.pixel_height, c.fps), (1280, 720, 24.0));
    assert_eq!(p(&["-r", "640,360"]).unwrap().pixel_width, 640);
    assert_eq!(p(&["--format", "GIF", "--encoder", "nvenc"]).unwrap().encoder, Encoder::Nvenc);
    for bad in [
        &["--fps"][..],
        &["--fps", "x"],
        &["-r", "100"],
        &["-r", "0x10"],
        &["-qz"],
        &["--renderer", "vulkan"],
        &["--wat"],
    ] {
        assert!(p(bad).is_err(), "{bad:?} should be rejected");
    }
}
