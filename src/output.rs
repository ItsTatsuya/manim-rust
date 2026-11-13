//! Frame sinks: MP4 via an ffmpeg pipe, GIF and PNG sequences in pure Rust.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use anyhow::{bail, Context, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    /// MP4 if ffmpeg is on PATH, otherwise GIF.
    Auto,
    Mp4,
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

fn runs(exe: &Path) -> bool {
    Command::new(exe).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Find an ffmpeg executable: `MANIM_FFMPEG`, then PATH, then (on Windows) winget's
/// package folder, where installs land without being on PATH until a new shell starts.
pub fn find_ffmpeg() -> Option<PathBuf> {
    static FOUND: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| {
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
        })
        .clone()
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

pub fn ffmpeg_available() -> bool {
    find_ffmpeg().is_some()
}

/// H.264 encoder used for MP4 output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoder {
    /// First working hardware encoder (NVENC, Quick Sync, AMF), else x264.
    Auto,
    X264,
    Nvenc,
    Qsv,
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

    /// Visually near-lossless settings for each encoder.
    fn quality_args(self) -> &'static [&'static str] {
        match self {
            Encoder::Auto | Encoder::X264 => &["-preset", "fast", "-crf", "18"],
            Encoder::Nvenc => &["-preset", "p3", "-rc", "vbr", "-cq", "19", "-b:v", "0"],
            Encoder::Qsv => &["-preset", "faster", "-global_quality", "20"],
            Encoder::Amf => &["-quality", "speed", "-rc", "cqp", "-qp_i", "20", "-qp_p", "20"],
        }
    }

    /// Does this ffmpeg + driver actually encode with it? (Builds often list hardware
    /// encoders that fail at runtime without the matching GPU.)
    fn works(self, ffmpeg: &Path) -> bool {
        Command::new(ffmpeg)
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

    /// Resolve `Auto` (hardware probe cached per process).
    pub fn resolve(self, ffmpeg: &Path, pixel_rate: f64) -> Encoder {
        if self != Encoder::Auto {
            return self;
        }
        if pixel_rate < Self::HW_MIN_PIXEL_RATE {
            return Encoder::X264;
        }
        static CHOSEN: std::sync::OnceLock<Encoder> = std::sync::OnceLock::new();
        *CHOSEN.get_or_init(|| {
            [Encoder::Nvenc, Encoder::Qsv, Encoder::Amf].into_iter().find(|e| e.works(ffmpeg)).unwrap_or(Encoder::X264)
        })
    }
}

pub trait FrameWriter: Send {
    /// Write one RGBA8 frame, shown for `repeat` consecutive frame slots.
    fn write(&mut self, rgba: &[u8], repeat: u32) -> Result<()>;
    fn finish(self: Box<Self>) -> Result<()>;
    fn path(&self) -> &Path;
    /// Short description for logs, e.g. "mp4 (h264_nvenc)".
    fn describe(&self) -> String;
}

/// Resolve `Auto` and open a writer at `base` (extension added here).
pub fn create_writer(
    format: OutputFormat,
    encoder: Encoder,
    base: &Path,
    width: u32,
    height: u32,
    fps: f64,
) -> Result<Box<dyn FrameWriter>> {
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
        OutputFormat::Mp4 => Box::new(FfmpegWriter::new(&base.with_extension("mp4"), encoder, width, height, fps)?),
        OutputFormat::Gif => Box::new(GifWriter::new(&base.with_extension("gif"), width, height, fps)?),
        OutputFormat::Png => Box::new(PngSequence::new(base, width, height)?),
        OutputFormat::None => Box::new(NullWriter(PathBuf::new())),
        OutputFormat::Auto => unreachable!(),
    })
}

pub struct NullWriter(PathBuf);

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

/// RGBA8 → planar YUV 4:2:0 (limited range), via yuvutils-rs (SIMD, rows split across
/// rayon). Doing this here instead of in ffmpeg's single-threaded swscale cuts encode time
/// and the pipe traffic by 2.7×. It is ~16× cheaper per frame than the scalar loop it
/// replaced, which matters because it shares the CPU with x264. `width` and `height` must
/// be even.
pub fn rgba_to_yuv420(rgba: &[u8], width: usize, height: usize, matrix: YuvMatrix, out: &mut Vec<u8>) {
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
    .expect("even frame size with matching buffers");
}

pub struct FfmpegWriter {
    child: Child,
    stdin: Option<BufWriter<ChildStdin>>,
    path: PathBuf,
    pub encoder: Encoder,
    /// Even dimensions: convert to YUV here. Odd: send RGBA and let ffmpeg pad + convert.
    yuv: Option<(usize, usize, YuvMatrix, Vec<u8>)>,
    frames: u64,
}

impl FfmpegWriter {
    pub fn new(path: &Path, encoder: Encoder, width: u32, height: u32, fps: f64) -> Result<Self> {
        let exe = find_ffmpeg().context("ffmpeg not found (set MANIM_FFMPEG or use --ffmpeg PATH)")?;
        if !matches!(encoder, Encoder::Auto | Encoder::X264) && !encoder.works(&exe) {
            bail!("encoder {} is not usable with this ffmpeg/GPU (try --encoder x264)", encoder.codec());
        }
        let encoder = encoder.resolve(&exe, width as f64 * height as f64 * fps);
        let even = width.is_multiple_of(2) && height.is_multiple_of(2);
        // ffmpeg's own RGBA→YUV conversion (odd sizes) uses BT.601.
        let matrix = if even { YuvMatrix::for_height(height) } else { YuvMatrix::Bt601 };
        let mut cmd = Command::new(&exe);
        cmd.args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", if even { "yuv420p" } else { "rgba" }])
            .args(["-s", &format!("{width}x{height}"), "-r", &format!("{fps}"), "-i", "-"]);
        if !even {
            // H.264 4:2:0 needs even dimensions: pad by one black row/column.
            cmd.args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"]);
        }
        let mut child = cmd
            .args(["-c:v", encoder.codec(), "-pix_fmt", "yuv420p"])
            .args(encoder.quality_args())
            .args(matrix.ffmpeg_tags())
            .args(["-movflags", "+faststart"])
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .context("failed to start ffmpeg")?;
        let stdin = child.stdin.take().map(|s| BufWriter::with_capacity(1 << 20, s));
        Ok(FfmpegWriter {
            child,
            stdin,
            path: path.to_path_buf(),
            encoder,
            yuv: even.then(|| (width as usize, height as usize, matrix, Vec::new())),
            frames: 0,
        })
    }
}

impl FrameWriter for FfmpegWriter {
    fn write(&mut self, rgba: &[u8], repeat: u32) -> Result<()> {
        let stdin = self.stdin.as_mut().context("ffmpeg stdin closed")?;
        let data: &[u8] = match &mut self.yuv {
            Some((w, h, m, buf)) => {
                rgba_to_yuv420(rgba, *w, *h, *m, buf);
                buf
            }
            None => rgba,
        };
        for _ in 0..repeat {
            stdin.write_all(data).context("ffmpeg pipe closed")?;
        }
        self.frames += repeat as u64;
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<()> {
        if self.frames == 0 {
            // Nothing to encode: don't leave an empty/corrupt file behind.
            drop(self.stdin.take());
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = fs::remove_file(&self.path);
            return Ok(());
        }
        if let Some(mut s) = self.stdin.take() {
            s.flush()?;
        }
        let status = self.child.wait()?;
        if !status.success() {
            bail!("ffmpeg exited with {status}");
        }
        Ok(())
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn describe(&self) -> String {
        format!("mp4 ({})", self.encoder.codec())
    }
}

pub struct GifWriter {
    enc: Option<gif::Encoder<BufWriter<File>>>,
    width: u16,
    height: u16,
    centis_per_frame: f64,
    /// Frame waiting to be written, with its accumulated display time in centiseconds.
    held: Option<(Vec<u8>, f64)>,
    /// Rounding error carried into the next delay so total duration stays exact.
    carry: f64,
    emitted: u64,
    path: PathBuf,
}

impl GifWriter {
    pub fn new(path: &Path, width: u32, height: u32, fps: f64) -> Result<Self> {
        if width > u16::MAX as u32 || height > u16::MAX as u32 {
            bail!("frame too large for GIF");
        }
        let file = BufWriter::new(File::create(path).with_context(|| format!("creating {}", path.display()))?);
        let mut enc = gif::Encoder::new(file, width as u16, height as u16, &[])?;
        enc.set_repeat(gif::Repeat::Infinite)?;
        Ok(GifWriter {
            enc: Some(enc),
            width: width as u16,
            height: height as u16,
            centis_per_frame: 100.0 / fps,
            held: None,
            carry: 0.0,
            emitted: 0,
            path: path.to_path_buf(),
        })
    }

    fn emit(&mut self, mut rgba: Vec<u8>, centis: f64) -> Result<()> {
        let mut frame = gif::Frame::from_rgba_speed(self.width, self.height, &mut rgba, 10);
        let exact = centis + self.carry;
        let delay = exact.round().clamp(2.0, u16::MAX as f64);
        self.carry = exact - delay;
        frame.delay = delay as u16;
        self.enc.as_mut().expect("encoder open").write_frame(&frame)?;
        self.emitted += 1;
        Ok(())
    }
}

impl FrameWriter for GifWriter {
    fn write(&mut self, rgba: &[u8], repeat: u32) -> Result<()> {
        // GIF delays are in centiseconds and players clamp tiny delays, so frames shorter
        // than 2cs are merged into the held frame instead of written individually.
        let t = self.centis_per_frame * repeat as f64;
        match self.held.take() {
            Some((prev, acc)) if acc + self.carry >= 2.0 => {
                self.emit(prev, acc)?;
                self.held = Some((rgba.to_vec(), t));
            }
            Some((prev, acc)) => self.held = Some((prev, acc + t)),
            None => self.held = Some((rgba.to_vec(), t)),
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<()> {
        if let Some((prev, acc)) = self.held.take() {
            self.emit(prev, acc)?;
        }
        drop(self.enc.take()); // writes the trailer and closes the file
        if self.emitted == 0 {
            let _ = fs::remove_file(&self.path);
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

pub struct PngSequence {
    dir: PathBuf,
    width: u32,
    height: u32,
    index: u64,
}

impl PngSequence {
    pub fn new(base: &Path, width: u32, height: u32) -> Result<Self> {
        fs::create_dir_all(base)?;
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
    fn yuv_matches_reference_colors() {
        // 2x2 frames of a single color → reference limited-range values.
        let check = |m: YuvMatrix, rgb: [u8; 3], want: [u8; 3]| {
            let px = [rgb[0], rgb[1], rgb[2], 255];
            let rgba: Vec<u8> = px.iter().copied().cycle().take(16).collect();
            let mut out = Vec::new();
            rgba_to_yuv420(&rgba, 2, 2, m, &mut out);
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
