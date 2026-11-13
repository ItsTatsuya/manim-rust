//! Render configuration and a Manim-style command line (`-ql`, `-s`, `--renderer gpu`, ...).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::events::ProgressMode;
use crate::theme::Theme;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::color::{Color, BLACK};
use crate::output::{Encoder, OutputFormat};
use crate::renderer::RendererKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// 854x480 @ 15 fps (`-ql`)
    Low,
    /// 1280x720 @ 30 fps (`-qm`)
    Medium,
    /// 1920x1080 @ 60 fps (`-qh`, default)
    High,
    /// 2560x1440 @ 60 fps (`-qp`)
    Production,
    /// 3840x2160 @ 60 fps (`-qk`)
    FourK,
}

impl Quality {
    pub fn settings(self) -> (u32, u32, f64) {
        match self {
            Quality::Low => (854, 480, 15.0),
            Quality::Medium => (1280, 720, 30.0),
            Quality::High => (1920, 1080, 60.0),
            Quality::Production => (2560, 1440, 60.0),
            Quality::FourK => (3840, 2160, 60.0),
        }
    }

    fn from_letter(s: &str) -> Result<Self> {
        Ok(match s {
            "l" | "low" => Quality::Low,
            "m" | "medium" => Quality::Medium,
            "h" | "high" => Quality::High,
            "p" | "production" => Quality::Production,
            "k" | "4k" => Quality::FourK,
            _ => bail!("unknown quality '{s}' (expected l, m, h, p or k)"),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub fps: f64,
    pub background: Color,
    pub renderer: RendererKind,
    pub format: OutputFormat,
    pub encoder: Encoder,
    pub media_dir: PathBuf,
    /// Write output here instead of the `media/` tree. A directory (existing, or a path
    /// without an extension) gets `<Scene>.<ext>`; a file path is used as-is with the
    /// extension set by the format.
    pub output: Option<PathBuf>,
    /// Write the animation (off with `-s`, which only saves the last frame).
    pub write_video: bool,
    pub save_last_frame: bool,
    /// Print GPU adapters and exit.
    pub gpu_info: bool,
    pub quiet: bool,
    /// Run the scene logic only: nothing is rasterized or written (fast checks, previews).
    pub dry_run: bool,
    /// Keep keyframes so any time can be shown again afterwards (`Scene::seek`).
    pub record: bool,
    /// How progress is reported on stderr.
    pub progress: ProgressMode,
    /// Report layout problems (overlapping or off-frame text, crossfade ghosting).
    pub check: bool,
    /// Save PNG snapshots at these moments.
    pub snapshots: Vec<SnapshotAt>,
    /// Write `<output>.timeline.json` (plays, markers, sounds, duration).
    pub write_timeline: bool,
    /// Theme applied when the scene starts.
    pub theme: Option<Theme>,
    /// Only render the scene with this name (others are skipped).
    pub scene_filter: Option<String>,
    /// Print scene names instead of rendering.
    pub list_scenes: bool,
    /// `-h`/`--help` was given; the caller should print [`HELP`] instead of rendering.
    pub help: bool,
    /// Set to true from another thread to stop the render (it returns an error).
    pub cancel: Option<Arc<AtomicBool>>,
    /// Stop the render (it returns an error) once this moment passes; settable from any
    /// thread. Checked every frame, so long `play`/`wait` calls stop too.
    pub deadline: Option<Deadline>,
    /// ffmpeg executable to use (otherwise `MANIM_FFMPEG`, PATH, then winget's folder).
    pub ffmpeg: Option<PathBuf>,
}

/// A shared, adjustable time limit (see [`Config::deadline`]).
pub type Deadline = Arc<std::sync::Mutex<Option<std::time::Instant>>>;

/// Largest frame side accepted (16K); bigger frames would exhaust memory long before encoding.
pub const MAX_FRAME_SIDE: u32 = 16384;

impl Config {
    /// Check values a host may have set directly (the CLI parser calls this too).
    pub fn validate(&self) -> Result<()> {
        if self.pixel_width == 0 || self.pixel_height == 0 {
            bail!("resolution must be positive (got {}x{})", self.pixel_width, self.pixel_height);
        }
        if self.pixel_width > MAX_FRAME_SIDE || self.pixel_height > MAX_FRAME_SIDE {
            bail!("resolution {}x{} is above the {MAX_FRAME_SIDE}px limit", self.pixel_width, self.pixel_height);
        }
        if !(self.fps.is_finite() && self.fps > 0.0 && self.fps <= 1000.0) {
            bail!("fps must be a number between 0 and 1000 (got {})", self.fps);
        }
        Ok(())
    }

    /// Whether the render was cancelled or ran past its deadline.
    pub fn interrupted(&self) -> bool {
        self.cancel.as_ref().is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed))
            || self.deadline.as_ref().is_some_and(|d| {
                d.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| std::time::Instant::now() > t)
            })
    }
}

/// When to save a snapshot.
#[derive(Clone, Debug, PartialEq)]
pub enum SnapshotAt {
    /// Whenever the scene calls `mark`/`section`.
    Markers,
    /// After every `play`.
    Plays,
    /// At the end of the scene.
    End,
    /// At this scene time (seconds).
    Time(f64),
}

impl std::str::FromStr for SnapshotAt {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s.trim() {
            "markers" | "marks" => SnapshotAt::Markers,
            "plays" => SnapshotAt::Plays,
            "end" | "last" => SnapshotAt::End,
            t => SnapshotAt::Time(
                t.trim_end_matches('s').parse().with_context(|| format!("snapshot time '{t}' is not a number"))?,
            ),
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        let (w, h, fps) = Quality::High.settings();
        Config {
            pixel_width: w,
            pixel_height: h,
            fps,
            background: BLACK,
            renderer: RendererKind::Auto,
            format: OutputFormat::Auto,
            encoder: Encoder::Auto,
            media_dir: PathBuf::from("media"),
            output: None,
            write_video: true,
            save_last_frame: false,
            gpu_info: false,
            quiet: false,
            dry_run: false,
            record: false,
            progress: ProgressMode::Human,
            check: false,
            snapshots: Vec::new(),
            write_timeline: false,
            theme: None,
            scene_filter: None,
            list_scenes: false,
            help: false,
            cancel: None,
            deadline: None,
            ffmpeg: None,
        }
    }
}

pub const HELP: &str = "\
Options:
  -ql, -qm, -qh, -qp, -qk   Quality: 480p15, 720p30, 1080p60 (default), 1440p60, 2160p60
  -s, --save_last_frame     Only render and save the final frame as PNG
  --renderer auto|cpu|gpu|skia|vello
                            Rendering backend (default auto: times CPU and GPU, keeps the
                            faster; cpu picks tiny-skia or vello per frame)
  --format FMT              auto|mp4|gif|png|none (default auto: mp4 if ffmpeg is on PATH,
                            else gif; none renders but discards frames, for benchmarking)
  --encoder auto|x264|nvenc|qsv|amf
                            MP4 encoder (default auto: first working hardware encoder,
                            else x264)
  --fps N                   Override frame rate
  -r, --resolution WxH      Override resolution (e.g. 1280x720 or 1280,720)
  --media_dir DIR           Output root (default ./media)
  -o, --output PATH         Write <PATH>/<Scene>.mp4 (directory) or PATH itself (file)
                            instead of the media/ tree
  --ffmpeg PATH             ffmpeg executable (else MANIM_FFMPEG, PATH, winget packages)
  --gpu-info                List GPU adapters and the renderer Auto would pick
  -q, --quiet               Less logging
  --progress human|json|none
                            Progress on stderr; json prints one event object per line
  --dry-run                 Run the scene logic without rendering or writing anything
  --check                   Report overlapping / off-frame text and crossfade ghosting
                            (implies --dry-run unless snapshots are requested)
  --snapshot WHEN           Save PNGs: markers, plays, end, or seconds (comma separated)
  --timeline                Also write <output>.timeline.json (plays, markers, sounds)
  --theme FILE              Apply a theme (JSON, Manim Studio style format)
  --fonts DIR               Register the fonts in DIR (repeatable)
  --scene NAME              Only render the scene called NAME
  --list-scenes             Print the scene names and exit
  -h, --help                Show this help";

impl Config {
    pub fn quality(mut self, q: Quality) -> Self {
        let (w, h, fps) = q.settings();
        self.pixel_width = w;
        self.pixel_height = h;
        self.fps = fps;
        self
    }

    /// Parse the process arguments. For a scene binary's `main`: on `--help` this prints
    /// the usage and exits the process. Hosts embedding the library use [`Config::parse`]
    /// or build a `Config` directly instead.
    pub fn from_args() -> Result<Self> {
        let c = Self::parse(std::env::args().skip(1))?;
        if c.help {
            println!("{HELP}");
            std::process::exit(0);
        }
        Ok(c)
    }

    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut c = Config::default();
        let mut it = args.into_iter();
        while let Some(a) = it.next() {
            let mut val = |name: &str| it.next().with_context(|| format!("{name} needs a value"));
            match a.as_str() {
                q if q.starts_with("-q") && q.len() == 3 && q != "-qq" => c = c.quality(Quality::from_letter(&q[2..])?),
                "--quality" => c = c.quality(Quality::from_letter(&val("--quality")?)?),
                "-s" | "--save_last_frame" => {
                    c.save_last_frame = true;
                    c.write_video = false;
                }
                "--renderer" => c.renderer = val("--renderer")?.parse()?,
                "--format" => c.format = val("--format")?.parse()?,
                "--encoder" => c.encoder = val("--encoder")?.parse()?,
                "--fps" => c.fps = val("--fps")?.parse().context("--fps must be a number")?,
                "-r" | "--resolution" => {
                    let v = val("--resolution")?;
                    let (w, h) = v.split_once(['x', ',']).context("resolution must look like 1920x1080")?;
                    c.pixel_width = w.trim().parse()?;
                    c.pixel_height = h.trim().parse()?;
                }
                "--media_dir" => c.media_dir = PathBuf::from(val("--media_dir")?),
                "-o" | "--output" => c.output = Some(PathBuf::from(val("--output")?)),
                // Picked up by output::find_ffmpeg before any writer is created.
                "--ffmpeg" => c.ffmpeg = Some(PathBuf::from(val("--ffmpeg")?)),
                "--gpu-info" => c.gpu_info = true,
                "-q" | "--quiet" => c.quiet = true,
                "--progress" => c.progress = val("--progress")?.parse()?,
                "--dry-run" => c.dry_run = true,
                "--check" => c.check = true,
                "--record" => c.record = true,
                "--timeline" => c.write_timeline = true,
                "--snapshot" | "--snapshots" => {
                    for part in val("--snapshot")?.split(',').filter(|p| !p.trim().is_empty()) {
                        c.snapshots.push(part.parse()?);
                    }
                }
                "--theme" => c.theme = Some(Theme::load(val("--theme")?)?),
                "--fonts" => {
                    let dir = val("--fonts")?;
                    #[cfg(feature = "text")]
                    crate::text::register_font_dir(&dir)?;
                    #[cfg(not(feature = "text"))]
                    let _ = dir;
                }
                "--scene" => c.scene_filter = Some(val("--scene")?),
                "--list-scenes" => c.list_scenes = true,
                "-h" | "--help" => c.help = true,
                other => bail!("unknown option '{other}'\n\n{HELP}"),
            }
        }
        c.validate()?;
        if c.check && c.snapshots.is_empty() {
            c.dry_run = true;
        }
        if c.quiet && c.progress == ProgressMode::Human {
            c.progress = ProgressMode::None;
        }
        Ok(c)
    }

    /// Path (without extension) where the video for `scene` goes.
    pub fn video_base(&self, scene: &str) -> PathBuf {
        match &self.output {
            Some(o) if is_dir_like(o) => o.join(scene),
            Some(o) => strip_media_ext(o),
            None => self.media_dir.join("videos").join(scene).join(self.quality_dir()).join(scene),
        }
    }

    /// Where the last-frame PNG for `scene` goes.
    pub fn image_path(&self, scene: &str) -> PathBuf {
        match &self.output {
            Some(_) => crate::output::append_ext(&self.video_base(scene), "png"),
            None => self.media_dir.join("images").join(scene).join(format!("{scene}.png")),
        }
    }

    pub fn quality_dir(&self) -> String {
        format!("{}p{}", self.pixel_height, self.fps)
    }
}

fn is_dir_like(p: &std::path::Path) -> bool {
    p.is_dir() || p.extension().is_none()
}

/// `-o x.partial.mp4` → `x.partial`: drops only a media extension the writers add back,
/// so dotted names (`x.partial`, `take.v2`) survive.
fn strip_media_ext(p: &std::path::Path) -> PathBuf {
    const MEDIA: [&str; 5] = ["mp4", "gif", "png", "mov", "webm"];
    match p.extension().and_then(|e| e.to_str()) {
        Some(e) if MEDIA.iter().any(|m| m.eq_ignore_ascii_case(e)) => p.with_extension(""),
        _ => p.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flags() {
        let c = Config::parse(["-ql", "--renderer", "cpu", "-s"].map(String::from)).unwrap();
        assert_eq!((c.pixel_width, c.pixel_height, c.fps), (854, 480, 15.0));
        assert_eq!(c.renderer, RendererKind::Cpu);
        assert!(c.save_last_frame && !c.write_video);
        assert!(Config::parse(["--bogus".to_string()]).is_err());
    }

    #[test]
    fn output_keeps_dotted_names() {
        let out = |o: &str| Config::parse(["-o", o].map(String::from)).unwrap();
        let c = out("render/x.partial.mp4");
        let video = crate::output::append_ext(&c.video_base("S"), "mp4");
        assert_eq!(video, PathBuf::from("render/x.partial.mp4"));
        assert_eq!(c.image_path("S"), PathBuf::from("render/x.partial.png"));
        let c = out("render/take.v2");
        assert_eq!(crate::output::append_ext(&c.video_base("S"), "gif"), PathBuf::from("render/take.v2.gif"));
    }
}
