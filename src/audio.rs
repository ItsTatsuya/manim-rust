//! Sound on the timeline: audio files or raw samples placed at scene times, mixed and
//! muxed into the MP4 when the scene finishes.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::Serialize;

/// Where a sound's audio comes from.
#[derive(Clone, Debug)]
pub enum AudioSource {
    /// Any format ffmpeg reads (wav, mp3, ogg, flac, m4a...).
    File(PathBuf),
    /// Interleaved float samples in -1..1 (e.g. straight from a TTS model).
    Samples {
        /// Interleaved samples.
        data: Arc<[f32]>,
        /// Samples per second, per channel.
        sample_rate: u32,
        /// Number of interleaved channels.
        channels: u16,
    },
}

/// An audio clip placed on the scene timeline.
#[derive(Clone, Debug)]
pub struct Sound {
    /// The audio to play.
    pub source: AudioSource,
    /// Scene time in seconds when it starts.
    pub start: f64,
    /// Linear gain (1 = unchanged).
    pub gain: f64,
}

impl Sound {
    /// Length in seconds, when known without decoding (raw samples only).
    pub fn duration(&self) -> Option<f64> {
        match &self.source {
            AudioSource::Samples { data, sample_rate, channels } => {
                Some(data.len() as f64 / (*sample_rate as f64 * (*channels).max(1) as f64))
            }
            AudioSource::File(_) => None,
        }
    }

    /// Short description of the source, for logs and events.
    pub fn describe(&self) -> String {
        match &self.source {
            AudioSource::File(p) => p.display().to_string(),
            AudioSource::Samples { sample_rate, channels, .. } => format!("samples ({sample_rate} Hz, {channels} ch)"),
        }
    }
}

/// Serializable summary of a sound (for timelines and events).
#[derive(Clone, Debug, Serialize)]
pub struct SoundInfo {
    /// File path, or a short description for raw samples.
    pub source: String,
    /// Scene time in seconds when the sound starts.
    pub start: f64,
    /// Linear gain (1 = unchanged).
    pub gain: f64,
    /// Length in seconds, when known without decoding (raw samples only).
    pub duration: Option<f64>,
}

impl From<&Sound> for SoundInfo {
    fn from(s: &Sound) -> Self {
        SoundInfo { source: s.describe(), start: s.start, gain: s.gain, duration: s.duration() }
    }
}

/// Write 16-bit PCM WAV.
pub fn write_wav(path: &Path, samples: &[f32], sample_rate: u32, channels: u16) -> Result<()> {
    let channels = channels.max(1);
    if sample_rate == 0 {
        bail!("WAV needs a sample rate above 0");
    }
    let data_len = samples
        .len()
        .checked_mul(2)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n <= u32::MAX - 36)
        .context("sound is too long for a WAV file (over 2 GB)")?;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))
}

/// Mix `sounds` into the audio track of `video` (an MP4 written by the scene), in place.
/// The audio is padded or cut to the video's length; the video stream is copied untouched.
pub fn mux(ffmpeg: &Path, video: &Path, sounds: &[Sound]) -> Result<()> {
    if sounds.is_empty() {
        return Ok(());
    }
    // Unique per call: never collides with a user folder or a concurrent render.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp_dir = video.with_extension(format!("audio-tmp-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;
    let result = (|| -> Result<()> {
        let mut input_files = Vec::new();
        let mut filters = Vec::new();
        for (i, s) in sounds.iter().enumerate() {
            let input = match &s.source {
                AudioSource::File(p) => {
                    if !p.is_file() {
                        bail!("sound file {} does not exist", p.display());
                    }
                    p.clone()
                }
                AudioSource::Samples { data, sample_rate, channels } => {
                    let p = tmp_dir.join(format!("{i}.wav"));
                    write_wav(&p, data, *sample_rate, *channels)?;
                    p
                }
            };
            input_files.push(input);
            let ms = (s.start.max(0.0) * 1000.0).round() as u64;
            filters.push(format!(
                "[{}:a]aformat=sample_rates=48000:channel_layouts=stereo,adelay={ms}|{ms},volume={:.4}[a{i}]",
                i + 1,
                s.gain
            ));
        }
        let inputs: String = (0..sounds.len()).map(|i| format!("[a{i}]")).collect();
        filters.push(format!("{inputs}amix=inputs={}:normalize=0:dropout_transition=0,apad[aout]", sounds.len()));
        let out = video.with_extension("muxing.mp4");
        // The graph grows with every sound; a file keeps the command line short (Windows
        // refuses ~32k characters).
        let script = tmp_dir.join("filters.txt");
        std::fs::write(&script, filters.join(";\n")).with_context(|| format!("writing {}", script.display()))?;
        // `-/filter_complex FILE` is the current spelling (ffmpeg 7+); older builds only
        // know `-filter_complex_script`.
        let run = |flag: &str| {
            let mut cmd = crate::output::tool_command(ffmpeg);
            cmd.args(["-y", "-loglevel", "error", "-i"]).arg(crate::output::ffmpeg_path_arg(video));
            for f in &input_files {
                cmd.arg("-i").arg(crate::output::ffmpeg_path_arg(f));
            }
            cmd.arg(flag)
                .arg(&script)
                .args(["-map", "0:v", "-map", "[aout]", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest"])
                .args(["-movflags", "+faststart"])
                .arg(crate::output::ffmpeg_path_arg(&out))
                .stdin(Stdio::null());
            cmd.output().with_context(|| format!("running {}", ffmpeg.display()))
        };
        let mut res = run("-/filter_complex")?;
        if !res.status.success() && String::from_utf8_lossy(&res.stderr).contains("Unrecognized option") {
            res = run("-filter_complex_script")?;
        }
        if !res.status.success() {
            let _ = std::fs::remove_file(&out);
            bail!("ffmpeg could not add the audio: {}", String::from_utf8_lossy(&res.stderr).trim());
        }
        std::fs::rename(&out, video).with_context(|| format!("replacing {}", video.display()))?;
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp_dir);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header() {
        let dir = std::env::temp_dir().join(format!("manim-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.wav");
        write_wav(&p, &[0.0, 0.5, -0.5, 1.0], 24000, 1).unwrap();
        let b = std::fs::read(&p).unwrap();
        assert_eq!(&b[..4], b"RIFF");
        assert_eq!(b.len(), 44 + 8);
        assert_eq!(u32::from_le_bytes(b[24..28].try_into().unwrap()), 24000);
        let _ = std::fs::remove_dir_all(dir);
        let s = Sound {
            source: AudioSource::Samples { data: vec![0.0; 48000].into(), sample_rate: 24000, channels: 2 },
            start: 0.0,
            gain: 1.0,
        };
        assert_eq!(s.duration(), Some(1.0));
    }
}
