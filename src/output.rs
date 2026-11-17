//! Frame sinks: MP4 via an ffmpeg pipe, GIF and PNG sequences in pure Rust.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;

use anyhow::{bail, Context, Result};

/// Container a render is written as (see [`Config::format`](crate::config::Config::format)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OutputFormat {
    /// MP4 if ffmpeg is on PATH, otherwise GIF.
    Auto,
    /// H.264 in an MP4 file, written through ffmpeg.
    Mp4,
    /// Animated GIF (256-color palette per frame).
    Gif,
    /// Numbered PNG frames in a directory.
    Png,
    /// Render but discard frames (benchmarking).
    None,
}

impl std::str::FromStr for OutputFormat {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "mp4" => Ok(Self::Mp4),
            "gif" => Ok(Self::Gif),
            "png" => Ok(Self::Png),
            "none" | "null" => Ok(Self::None),
            _ => bail!("unknown format '{s}' (expected auto, mp4, gif, png or none)"),
        }
    }
}

/// A `Command` for a helper tool (ffmpeg, ffprobe). On Windows it gets no console
/// window, so a GUI host without a console doesn't flash one per run; its pipes and
/// exit status work as usual.
pub(crate) fn tool_command(exe: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn runs(exe: &Path) -> bool {
    tool_command(exe).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

static FFMPEG: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Use this ffmpeg executable from now on (`--ffmpeg`, [`Config::ffmpeg`](crate::config::Config::ffmpeg)).
pub fn set_ffmpeg_path(path: impl Into<PathBuf>) {
    *FFMPEG.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.into());
}

/// Find an ffmpeg executable: `MANIM_FFMPEG`, then PATH, then (on Windows) winget's
/// package folder, where installs land without being on PATH until a new shell starts.
/// A hit is cached; a miss is not, so installing ffmpeg works without a restart.
pub fn find_ffmpeg() -> Option<PathBuf> {
    let mut cached = FFMPEG.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = &*cached {
        return Some(p.clone());
    }
    let found = (|| {
        if let Some(p) = std::env::var_os("MANIM_FFMPEG").map(PathBuf::from) {
            if runs(&p) {
                return Some(p);
            }
            eprintln!("manim: MANIM_FFMPEG={} does not run; searching elsewhere", p.display());
        }
        if runs(Path::new("ffmpeg")) {
            return Some(PathBuf::from("ffmpeg"));
        }
        winget_ffmpeg()
    })();
    cached.clone_from(&found);
    found
}

fn winget_ffmpeg() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Microsoft").join("WinGet").join("Packages");
    let mut candidates = Vec::new();
    for pkg in fs::read_dir(&root).ok()?.flatten() {
        if !pkg.file_name().to_string_lossy().to_ascii_lowercase().contains("ffmpeg") {
            continue;
        }
        // Layout: <pkg>/<build dir>/bin/ffmpeg.exe
        for build in fs::read_dir(pkg.path()).into_iter().flatten().flatten() {
            let exe = build.path().join("bin").join("ffmpeg.exe");
            if exe.is_file() {
                candidates.push(exe);
            }
        }
    }
    candidates.sort();
    candidates.into_iter().rev().find(|p| runs(p))
}

/// Whether an ffmpeg executable was found (see [`find_ffmpeg`]).
pub fn ffmpeg_available() -> bool {
    find_ffmpeg().is_some()
}

/// H.264 encoder used for MP4 output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Encoder {
    /// First working hardware encoder (NVENC, Quick Sync, AMF), else x264.
    Auto,
    /// Software x264 (CPU); the default fallback.
    X264,
    /// NVIDIA NVENC hardware encoder.
    Nvenc,
    /// Intel Quick Sync hardware encoder.
    Qsv,
    /// AMD AMF hardware encoder.
    Amf,
}

impl std::str::FromStr for Encoder {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "x264" | "libx264" | "cpu" => Ok(Self::X264),
            "nvenc" | "h264_nvenc" => Ok(Self::Nvenc),
            "qsv" | "h264_qsv" => Ok(Self::Qsv),
            "amf" | "h264_amf" => Ok(Self::Amf),
            _ => bail!("unknown encoder '{s}' (expected auto, x264, nvenc, qsv or amf)"),
        }
    }
}

impl Encoder {
    fn codec(self) -> &'static str {
        match self {
            Encoder::Auto | Encoder::X264 => "libx264",
            Encoder::Nvenc => "h264_nvenc",
            Encoder::Qsv => "h264_qsv",
            Encoder::Amf => "h264_amf",
        }
    }

    /// Visually near-lossless settings for each encoder. (x264's `-tune animation` was
    /// measured on the benchmark and showcase scenes: 9-15% slower and ~9% larger at the same
    /// CRF, so it is not used.)
    fn quality_args(self, x264: &X264Options) -> Vec<String> {
        let strs: &[&str] = match self {
            Encoder::Auto | Encoder::X264 => {
                return ["-preset", &x264.preset, "-crf", &x264.crf.to_string()].map(String::from).to_vec()
            }
            Encoder::Nvenc => &["-preset", "p3", "-rc", "vbr", "-cq", "19", "-b:v", "0"],
            Encoder::Qsv => &["-preset", "faster", "-global_quality", "20"],
            Encoder::Amf => &["-quality", "speed", "-rc", "cqp", "-qp_i", "20", "-qp_p", "20"],
        };
        strs.iter().map(|s| s.to_string()).collect()
    }

    /// Does this ffmpeg + driver actually encode with it? (Builds often list hardware
    /// encoders that fail at runtime without the matching GPU.)
    fn works(self, ffmpeg: &Path) -> bool {
        tool_command(ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=s=256x256:d=0.1",
                "-frames:v",
                "1",
                "-c:v",
                self.codec(),
                "-f",
                "null",
                "-",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Pixel rate (pixels/second) below which `Auto` keeps x264: probing and initializing
    /// a hardware encoder costs ~0.3 s, more than x264 needs for small videos.
    const HW_MIN_PIXEL_RATE: f64 = 1920.0 * 1080.0 * 30.0;

    /// Resolve `Auto` (the hardware probe is cached per process, per ffmpeg executable).
    pub fn resolve(self, ffmpeg: &Path, pixel_rate: f64) -> Encoder {
        self.resolve_with(ffmpeg, pixel_rate, Encoder::works)
    }

    fn resolve_with(self, ffmpeg: &Path, pixel_rate: f64, works: impl Fn(Encoder, &Path) -> bool) -> Encoder {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        if self != Encoder::Auto {
            return self;
        }
        if pixel_rate < Self::HW_MIN_PIXEL_RATE {
            return Encoder::X264;
        }
        // Another ffmpeg build can support other encoders, so the answer is per executable.
        static CHOSEN: OnceLock<Mutex<HashMap<PathBuf, Encoder>>> = OnceLock::new();
        let cache = CHOSEN.get_or_init(Default::default);
        if let Some(e) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(ffmpeg) {
            return *e;
        }
        let chosen = [Encoder::Nvenc, Encoder::Qsv, Encoder::Amf]
            .into_iter()
            .find(|e| works(*e, ffmpeg))
            .unwrap_or(Encoder::X264);
        cache.lock().unwrap_or_else(|e| e.into_inner()).insert(ffmpeg.to_path_buf(), chosen);
        chosen
    }
}

/// Converts an RGBA8 frame into the bytes a writer wants, so the work can run on several
/// threads at once while the writer itself only moves bytes (see [`FrameWriter::preparer`]).
pub trait FramePreparer: Send + Sync {
    /// Size in bytes of every prepared frame.
    fn frame_len(&self) -> usize;
    /// Fill `out` (resized to [`frame_len`](Self::frame_len)) from tightly packed RGBA8.
    fn prepare(&self, rgba: &[u8], out: &mut Vec<u8>) -> Result<()>;
}

/// Sink for rendered frames. Implemented by the MP4, GIF and PNG writers; hosts can supply their own.
pub trait FrameWriter: Send {
    /// Write one RGBA8 frame, shown for `repeat` consecutive frame slots.
    fn write(&mut self, rgba: &[u8], repeat: u32) -> Result<()>;
    /// A converter that turns frames into what [`write_prepared`](Self::write_prepared)
    /// takes. The pipeline runs it on its render threads, so the writer thread only does
    /// I/O. `None` (the default): frames go to [`write`](Self::write) as RGBA8.
    fn preparer(&self) -> Option<std::sync::Arc<dyn FramePreparer>> {
        None
    }
    /// Write a frame made by [`preparer`](Self::preparer), shown for `repeat` frame slots.
    fn write_prepared(&mut self, _data: &[u8], _repeat: u32) -> Result<()> {
        bail!("this writer does not take prepared frames")
    }
    /// Flush and close the output, making it visible at [`path`](Self::path). Dropping a writer without calling this discards the partial output.
    fn finish(self: Box<Self>) -> Result<()>;
    /// Final output path (or directory, for PNG sequences).
    fn path(&self) -> &Path;
    /// Short description for logs, e.g. "mp4 (h264_nvenc)".
    fn describe(&self) -> String;
}

/// Resolve `Auto` and open a writer at `base` (extension added here).
/// `base` + `.ext`. Unlike `Path::with_extension`, keeps dots already in the name
/// (`x.partial` → `x.partial.mp4`, not `x.mp4`).
pub fn append_ext(base: &Path, ext: &str) -> PathBuf {
    let mut s = base.as_os_str().to_owned();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

/// Quality settings for the x264 encoder (the hardware encoders have their own fixed ones).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct X264Options {
    /// x264 `-preset`: `ultrafast` to `veryslow`. Slower presets make smaller files.
    pub preset: String,
    /// x264 `-crf`, 0 (lossless) to 51; lower is better quality. 18 is visually near-lossless.
    pub crf: u8,
}

impl X264Options {
    /// The accepted x264 `-preset` names, fastest first.
    pub const PRESETS: [&'static str; 9] =
        ["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"];

    /// Check the preset is one of [`X264Options::PRESETS`] and the CRF is 0 to 51.
    pub fn validate(&self) -> Result<()> {
        if !Self::PRESETS.contains(&self.preset.as_str()) {
            bail!("unknown x264 preset '{}' (expected one of {})", self.preset, Self::PRESETS.join(", "));
        }
        if self.crf > 51 {
            bail!("x264 crf must be 0-51 (got {})", self.crf);
        }
        Ok(())
    }
}

impl Default for X264Options {
    fn default() -> Self {
        X264Options { preset: "fast".into(), crf: 18 }
    }
}

/// Resolve [`OutputFormat::Auto`] and open a writer at `base` (the extension is added here), using the default x264 settings.
pub fn create_writer(
    format: OutputFormat,
    encoder: Encoder,
    base: &Path,
    width: u32,
    height: u32,
    fps: f64,
) -> Result<Box<dyn FrameWriter>> {
    create_writer_with(format, encoder, &X264Options::default(), base, width, height, fps)
}

/// [`create_writer`] with explicit x264 settings.
pub fn create_writer_with(
    format: OutputFormat,
    encoder: Encoder,
    x264: &X264Options,
    base: &Path,
    width: u32,
    height: u32,
    fps: f64,
) -> Result<Box<dyn FrameWriter>> {
    x264.validate()?;
    let format = match format {
        OutputFormat::Auto if ffmpeg_available() => OutputFormat::Mp4,
        OutputFormat::Auto => {
            eprintln!(
                "manim: ffmpeg not found; writing a GIF instead (install ffmpeg, or set MANIM_FFMPEG / --ffmpeg)"
            );
            OutputFormat::Gif
        }
        f => f,
    };
    if let Some(dir) = base.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    Ok(match format {
        OutputFormat::Mp4 => {
            Box::new(FfmpegWriter::with_options(&append_ext(base, "mp4"), encoder, x264, width, height, fps)?)
        }
        OutputFormat::Gif => Box::new(GifWriter::new(&append_ext(base, "gif"), width, height, fps)?),
        OutputFormat::Png => Box::new(PngSequence::new(base, width, height)?),
        OutputFormat::None => Box::new(NullWriter(PathBuf::new())),
        OutputFormat::Auto => unreachable!(),
    })
}

pub(crate) struct NullWriter(PathBuf);

impl FrameWriter for NullWriter {
    fn write(&mut self, _rgba: &[u8], _repeat: u32) -> Result<()> {
        Ok(())
    }
    fn finish(self: Box<Self>) -> Result<()> {
        Ok(())
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn describe(&self) -> String {
        "discarded".into()
    }
}

/// Write tightly packed RGBA8 pixels as a PNG, creating parent directories. Errors if `rgba` is not `width * height * 4` bytes.
pub fn save_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let file = BufWriter::new(File::create(path).with_context(|| format!("creating {}", path.display()))?);
    let mut enc = png::Encoder::new(file, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

#[doc(hidden)]
/// YUV matrix. Players assume BT.601 for SD and BT.709 for HD when a file is untagged,
/// so we follow the same rule (and tag the file) to look right everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YuvMatrix {
    Bt601,
    Bt709,
}

impl YuvMatrix {
    pub fn for_height(height: u32) -> Self {
        if height >= 720 {
            YuvMatrix::Bt709
        } else {
            YuvMatrix::Bt601
        }
    }

    fn standard(self) -> yuvutils_rs::YuvStandardMatrix {
        match self {
            YuvMatrix::Bt601 => yuvutils_rs::YuvStandardMatrix::Bt601,
            YuvMatrix::Bt709 => yuvutils_rs::YuvStandardMatrix::Bt709,
        }
    }

    fn ffmpeg_tags(self) -> [&'static str; 8] {
        let n = match self {
            YuvMatrix::Bt601 => "smpte170m",
            YuvMatrix::Bt709 => "bt709",
        };
        ["-colorspace", n, "-color_primaries", n, "-color_trc", n, "-color_range", "tv"]
    }
}

#[doc(hidden)]
/// RGBA8 → planar YUV 4:2:0 (limited range), via yuvutils-rs (SIMD, rows split across
/// rayon). Doing this here instead of in ffmpeg's single-threaded swscale cuts encode time
/// and the pipe traffic by 2.7×. It is ~16× cheaper per frame than the scalar loop it
/// replaced, which matters because it shares the CPU with x264. `width` and `height` must
/// be even.
pub fn rgba_to_yuv420(rgba: &[u8], width: usize, height: usize, matrix: YuvMatrix, out: &mut Vec<u8>) -> Result<()> {
    if !width.is_multiple_of(2) || !height.is_multiple_of(2) || rgba.len() != width * height * 4 {
        bail!("rgba_to_yuv420: need even {width}x{height} and {} bytes, got {}", width * height * 4, rgba.len());
    }
    use yuvutils_rs::{BufferStoreMut, YuvConversionMode, YuvPlanarImageMut, YuvRange};
    let (cw, ch) = (width / 2, height / 2);
    out.resize(width * height + 2 * cw * ch, 0);
    let (y_plane, uv) = out.split_at_mut(width * height);
    let (u_plane, v_plane) = uv.split_at_mut(cw * ch);
    let mut img = YuvPlanarImageMut {
        y_plane: BufferStoreMut::Borrowed(y_plane),
        y_stride: width as u32,
        u_plane: BufferStoreMut::Borrowed(u_plane),
        u_stride: cw as u32,
        v_plane: BufferStoreMut::Borrowed(v_plane),
        v_stride: cw as u32,
        width: width as u32,
        height: height as u32,
    };
    yuvutils_rs::rgba_to_yuv420(
        &mut img,
        rgba,
        (width * 4) as u32,
        YuvRange::Limited,
        matrix.standard(),
        YuvConversionMode::Balanced,
    )
    .map_err(|e| anyhow::anyhow!("YUV conversion failed: {e:?}"))
}

/// Where a writer puts a file until it is complete: `name.partial.ext` next to `path`.
/// A failed or cancelled render then never replaces the last good output.
pub fn partial_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = match path.extension() {
        Some(e) => format!("{stem}.partial.{}", e.to_string_lossy()),
        None => format!("{stem}.partial"),
    };
    path.with_file_name(name)
}

/// Move a finished `partial` file over `path`.
fn publish(partial: &Path, path: &Path) -> Result<()> {
    fs::rename(partial, path).with_context(|| format!("replacing {} (is it open in another program?)", path.display()))
}

/// An output path ffmpeg cannot mistake for an option (`-out.mp4` → `./-out.mp4`).
pub(crate) fn ffmpeg_path_arg(path: &Path) -> PathBuf {
    if path.as_os_str().to_string_lossy().starts_with('-') {
        Path::new(".").join(path)
    } else {
        path.to_path_buf()
    }
}

/// Streams frames into an ffmpeg child process and publishes the MP4 on [`FrameWriter::finish`].
pub struct FfmpegWriter {
    child: Child,
    stdin: Option<BufWriter<ChildStdin>>,
    path: PathBuf,
    partial: PathBuf,
    done: bool,
    /// ffmpeg's stderr, collected so an encode failure can say why.
    stderr: Option<std::thread::JoinHandle<String>>,
    encoder: Encoder,
    /// Even dimensions: frames are converted to YUV (by the pipeline's threads, or by
    /// `write`). Odd: send RGBA and let ffmpeg pad + convert.
    yuv: Option<Arc<YuvPreparer>>,
    /// Scratch for [`FrameWriter::write`] called with RGBA directly.
    scratch: Vec<u8>,
    frames: u64,
}

/// RGBA8 to YUV 4:2:0 for even frame sizes.
pub(crate) struct YuvPreparer {
    width: usize,
    height: usize,
    matrix: YuvMatrix,
}

impl YuvPreparer {
    pub(crate) fn new(width: usize, height: usize, matrix: YuvMatrix) -> Self {
        YuvPreparer { width, height, matrix }
    }
}

impl FramePreparer for YuvPreparer {
    fn frame_len(&self) -> usize {
        self.width * self.height + 2 * (self.width / 2) * (self.height / 2)
    }

    fn prepare(&self, rgba: &[u8], out: &mut Vec<u8>) -> Result<()> {
        rgba_to_yuv420(rgba, self.width, self.height, self.matrix, out)
    }
}

impl FfmpegWriter {
    /// Start ffmpeg writing `path`, with the default x264 settings. Fails if ffmpeg is missing or the encoder is unusable.
    pub fn new(path: &Path, encoder: Encoder, width: u32, height: u32, fps: f64) -> Result<Self> {
        Self::with_options(path, encoder, &X264Options::default(), width, height, fps)
    }

    /// Like [`FfmpegWriter::new`] with explicit x264 settings (used when `encoder` resolves to x264).
    pub fn with_options(
        path: &Path,
        encoder: Encoder,
        x264: &X264Options,
        width: u32,
        height: u32,
        fps: f64,
    ) -> Result<Self> {
        let exe = find_ffmpeg().context("ffmpeg not found (set MANIM_FFMPEG or use --ffmpeg PATH)")?;
        if !matches!(encoder, Encoder::Auto | Encoder::X264) && !encoder.works(&exe) {
            bail!("encoder {} is not usable with this ffmpeg/GPU (try --encoder x264)", encoder.codec());
        }
        let encoder = encoder.resolve(&exe, width as f64 * height as f64 * fps);
        let even = width.is_multiple_of(2) && height.is_multiple_of(2);
        // ffmpeg's own RGBA→YUV conversion (odd sizes) uses BT.601.
        let matrix = if even { YuvMatrix::for_height(height) } else { YuvMatrix::Bt601 };
        let partial = partial_path(path);
        let mut cmd = tool_command(&exe);
        cmd.args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", if even { "yuv420p" } else { "rgba" }])
            .args(["-s", &format!("{width}x{height}"), "-r", &format!("{fps}"), "-i", "-"]);
        if !even {
            // H.264 4:2:0 needs even dimensions: pad by one black row/column.
            cmd.args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"]);
        }
        let mut child = cmd
            .args(["-c:v", encoder.codec(), "-pix_fmt", "yuv420p"])
            .args(encoder.quality_args(x264))
            .args(matrix.ffmpeg_tags())
            .args(["-movflags", "+faststart"])
            .arg(ffmpeg_path_arg(&partial))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to start ffmpeg")?;
        let stdin = child.stdin.take().map(|s| BufWriter::with_capacity(1 << 20, s));
        let stderr = child.stderr.take().map(|mut e| {
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = std::io::Read::read_to_string(&mut e, &mut text);
                text
            })
        });
        Ok(FfmpegWriter {
            child,
            stdin,
            path: path.to_path_buf(),
            partial,
            done: false,
            stderr,
            encoder,
            yuv: even.then(|| Arc::new(YuvPreparer::new(width as usize, height as usize, matrix))),
            scratch: Vec::new(),
            frames: 0,
        })
    }
}

impl FrameWriter for FfmpegWriter {
    fn write(&mut self, rgba: &[u8], repeat: u32) -> Result<()> {
        match self.yuv.clone() {
            Some(p) => {
                let mut buf = std::mem::take(&mut self.scratch);
                let r = p.prepare(rgba, &mut buf).and_then(|()| self.write_prepared(&buf, repeat));
                self.scratch = buf;
                r
            }
            None => self.write_prepared(rgba, repeat),
        }
    }

    fn preparer(&self) -> Option<Arc<dyn FramePreparer>> {
        self.yuv.clone().map(|p| p as Arc<dyn FramePreparer>)
    }

    fn write_prepared(&mut self, data: &[u8], repeat: u32) -> Result<()> {
        let stdin = self.stdin.as_mut().context("ffmpeg stdin closed")?;
        for _ in 0..repeat {
            if let Err(e) = stdin.write_all(data) {
                let why = self.ffmpeg_message();
                return Err(anyhow::Error::new(e).context(format!("ffmpeg stopped accepting frames{why}")));
            }
        }
        self.frames += repeat as u64;
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<()> {
        if self.frames == 0 {
            // Nothing to encode: Drop stops ffmpeg and removes the partial file.
            return Ok(());
        }
        if let Some(mut s) = self.stdin.take() {
            s.flush()?;
        }
        let status = self.child.wait()?;
        if !status.success() {
            bail!("ffmpeg exited with {status}{}", self.ffmpeg_message());
        }
        publish(&self.partial, &self.path)?;
        self.done = true;
        Ok(())
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn describe(&self) -> String {
        format!("mp4 ({})", self.encoder.codec())
    }
}

impl FfmpegWriter {
    /// ffmpeg's error output as `": ..."`, once it has exited (empty if it said nothing).
    fn ffmpeg_message(&mut self) -> String {
        drop(self.stdin.take());
        let _ = self.child.wait();
        let text = self.stderr.take().and_then(|h| h.join().ok()).unwrap_or_default();
        let text = text.trim();
        if text.is_empty() {
            String::new()
        } else {
            format!(": {text}")
        }
    }
}

impl Drop for FfmpegWriter {
    /// Not finished (error, cancel, or no frames): stop ffmpeg and remove the partial file.
    fn drop(&mut self) {
        if self.done {
            return;
        }
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.partial);
    }
}

/// Writes an animated GIF, quantizing each frame.
pub struct GifWriter {
    enc: Option<gif::Encoder<BufWriter<File>>>,
    centis_per_frame: f64,
    /// Frame waiting to be written (quantized, see [`GifPreparer`]), with its accumulated
    /// display time in centiseconds.
    held: Option<(Vec<u8>, f64)>,
    quantizer: Arc<GifPreparer>,
    /// Rounding error carried into the next delay so total duration stays exact.
    carry: f64,
    emitted: u64,
    path: PathBuf,
    partial: PathBuf,
    done: bool,
}

impl GifWriter {
    /// Open `path` for a GIF of the given size. Fails if a side exceeds 65535 pixels.
    pub fn new(path: &Path, width: u32, height: u32, fps: f64) -> Result<Self> {
        if width > u16::MAX as u32 || height > u16::MAX as u32 {
            bail!("frame too large for GIF");
        }
        let partial = partial_path(path);
        let file = BufWriter::new(File::create(&partial).with_context(|| format!("creating {}", partial.display()))?);
        let mut enc = gif::Encoder::new(file, width as u16, height as u16, &[])?;
        enc.set_repeat(gif::Repeat::Infinite)?;
        Ok(GifWriter {
            enc: Some(enc),
            centis_per_frame: 100.0 / fps,
            held: None,
            quantizer: Arc::new(GifPreparer { width: width as u16, height: height as u16 }),
            carry: 0.0,
            emitted: 0,
            path: path.to_path_buf(),
            partial,
            done: false,
        })
    }

    fn emit(&mut self, prepared: Vec<u8>, centis: f64) -> Result<()> {
        let mut frame = self.quantizer.unpack(&prepared)?;
        let exact = centis + self.carry;
        let delay = exact.round().clamp(2.0, u16::MAX as f64);
        self.carry = exact - delay;
        frame.delay = delay as u16;
        self.enc.as_mut().expect("encoder open").write_frame(&frame)?;
        self.emitted += 1;
        Ok(())
    }
}

/// Colour quantization for GIF (NeuQuant, speed 10): by far the slowest step of a GIF
/// export, so the pipeline runs it on its render threads, one frame per thread.
///
/// Prepared layout (fixed size): palette length (`u16`, LE), has-transparent flag, the
/// transparent index, the palette padded to 768 bytes, then one palette index per pixel.
pub(crate) struct GifPreparer {
    width: u16,
    height: u16,
}

const GIF_HEADER: usize = 4 + 768;

impl GifPreparer {
    fn unpack(&self, data: &[u8]) -> Result<gif::Frame<'static>> {
        let pixels = self.width as usize * self.height as usize;
        if data.len() != GIF_HEADER + pixels {
            bail!("malformed prepared GIF frame ({} bytes)", data.len());
        }
        let palette_len = u16::from_le_bytes([data[0], data[1]]) as usize;
        if palette_len > 768 {
            bail!("malformed prepared GIF frame (palette of {palette_len} bytes)");
        }
        Ok(gif::Frame {
            width: self.width,
            height: self.height,
            palette: Some(data[4..4 + palette_len].to_vec()),
            transparent: (data[2] != 0).then_some(data[3]),
            buffer: std::borrow::Cow::Owned(data[GIF_HEADER..].to_vec()),
            ..gif::Frame::default()
        })
    }
}

impl FramePreparer for GifPreparer {
    fn frame_len(&self) -> usize {
        GIF_HEADER + self.width as usize * self.height as usize
    }

    fn prepare(&self, rgba: &[u8], out: &mut Vec<u8>) -> Result<()> {
        if rgba.len() != self.width as usize * self.height as usize * 4 {
            bail!("GIF frame has {} bytes, expected {}", rgba.len(), self.width as usize * self.height as usize * 4);
        }
        let mut copy = rgba.to_vec(); // the quantizer normalizes alpha in place
        let frame = gif::Frame::from_rgba_speed(self.width, self.height, &mut copy, 10);
        let palette = frame.palette.as_deref().unwrap_or(&[]);
        out.clear();
        out.extend_from_slice(&(palette.len() as u16).to_le_bytes());
        out.push(frame.transparent.is_some() as u8);
        out.push(frame.transparent.unwrap_or(0));
        out.extend_from_slice(palette);
        out.resize(GIF_HEADER, 0);
        out.extend_from_slice(&frame.buffer);
        Ok(())
    }
}

impl FrameWriter for GifWriter {
    fn write(&mut self, rgba: &[u8], repeat: u32) -> Result<()> {
        let mut prepared = Vec::new();
        self.quantizer.prepare(rgba, &mut prepared)?;
        self.write_prepared(&prepared, repeat)
    }

    fn preparer(&self) -> Option<Arc<dyn FramePreparer>> {
        Some(self.quantizer.clone())
    }

    fn write_prepared(&mut self, prepared: &[u8], repeat: u32) -> Result<()> {
        // GIF delays are in centiseconds and players clamp tiny delays, so frames shorter
        // than 2cs are merged into the held frame instead of written individually.
        let t = self.centis_per_frame * repeat as f64;
        match self.held.take() {
            Some((prev, acc)) if acc + self.carry >= 2.0 => {
                self.emit(prev, acc)?;
                self.held = Some((prepared.to_vec(), t));
            }
            Some((prev, acc)) => self.held = Some((prev, acc + t)),
            None => self.held = Some((prepared.to_vec(), t)),
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<()> {
        if let Some((prev, acc)) = self.held.take() {
            self.emit(prev, acc)?;
        }
        drop(self.enc.take()); // writes the trailer and closes the file
        if self.emitted > 0 {
            publish(&self.partial, &self.path)?;
            self.done = true;
        }
        Ok(())
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn describe(&self) -> String {
        "gif".into()
    }
}

impl Drop for GifWriter {
    fn drop(&mut self) {
        if !self.done {
            drop(self.enc.take());
            let _ = fs::remove_file(&self.partial);
        }
    }
}

/// Writes frames as numbered PNG files into a directory.
pub struct PngSequence {
    dir: PathBuf,
    width: u32,
    height: u32,
    index: u64,
}

impl PngSequence {
    /// Use `base` as the output directory (created if missing; stale frames from an earlier render are removed).
    pub fn new(base: &Path, width: u32, height: u32) -> Result<Self> {
        fs::create_dir_all(base)?;
        // Frames from an earlier, longer render would otherwise trail this one.
        for e in fs::read_dir(base)?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_frame = name
                .strip_prefix("frame_")
                .and_then(|n| n.strip_suffix(".png"))
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            if is_frame {
                let _ = fs::remove_file(e.path());
            }
        }
        Ok(PngSequence { dir: base.to_path_buf(), width, height, index: 0 })
    }
}

impl FrameWriter for PngSequence {
    fn write(&mut self, rgba: &[u8], repeat: u32) -> Result<()> {
        let first = self.dir.join(format!("frame_{:05}.png", self.index));
        save_png(&first, self.width, self.height, rgba)?;
        self.index += 1;
        for _ in 1..repeat {
            fs::copy(&first, self.dir.join(format!("frame_{:05}.png", self.index)))?;
            self.index += 1;
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<()> {
        Ok(())
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn describe(&self) -> String {
        "png sequence".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hardware_probe_is_cached_per_ffmpeg() {
        let rate = Encoder::HW_MIN_PIXEL_RATE * 2.0;
        let (a, b) = (Path::new("probe-test-ffmpeg-a"), Path::new("probe-test-ffmpeg-b"));
        let probes = std::cell::Cell::new(0);
        let only_qsv = |e: Encoder, _: &Path| {
            probes.set(probes.get() + 1);
            e == Encoder::Qsv
        };
        assert_eq!(Encoder::Auto.resolve_with(a, rate, only_qsv), Encoder::Qsv);
        let n = probes.get();
        assert_eq!(Encoder::Auto.resolve_with(a, rate, only_qsv), Encoder::Qsv);
        assert_eq!(probes.get(), n, "second call for the same ffmpeg uses the cache");
        // A different ffmpeg is probed on its own.
        assert_eq!(Encoder::Auto.resolve_with(b, rate, |_, _| false), Encoder::X264);
    }

    #[test]
    fn yuv_matches_reference_colors() {
        // 2x2 frames of a single color → reference limited-range values.
        let check = |m: YuvMatrix, rgb: [u8; 3], want: [u8; 3]| {
            let px = [rgb[0], rgb[1], rgb[2], 255];
            let rgba: Vec<u8> = px.iter().copied().cycle().take(16).collect();
            let mut out = Vec::new();
            rgba_to_yuv420(&rgba, 2, 2, m, &mut out).unwrap();
            for (got, w) in [out[0], out[4], out[5]].iter().zip(want) {
                assert!((*got as i32 - w as i32).abs() <= 1, "{m:?} {rgb:?}: got {out:?}, want {want:?}");
            }
        };
        for m in [YuvMatrix::Bt601, YuvMatrix::Bt709] {
            check(m, [0, 0, 0], [16, 128, 128]);
            check(m, [255, 255, 255], [235, 128, 128]);
        }
        check(YuvMatrix::Bt709, [255, 0, 0], [63, 102, 240]);
        check(YuvMatrix::Bt709, [0, 0, 255], [32, 240, 118]);
        check(YuvMatrix::Bt601, [255, 0, 0], [82, 90, 240]);
        check(YuvMatrix::Bt601, [0, 0, 255], [41, 240, 110]);
        assert_eq!(YuvMatrix::for_height(480), YuvMatrix::Bt601);
        assert_eq!(YuvMatrix::for_height(1080), YuvMatrix::Bt709);
    }
}
