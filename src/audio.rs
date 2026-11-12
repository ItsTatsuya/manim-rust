//! Sound on the timeline: audio files or raw samples placed at scene times, mixed and
//! muxed into the MP4 when the scene finishes.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::Serialize;

#[derive(Clone, Debug)]
pub enum AudioSource {
    /// Any format ffmpeg reads (wav, mp3, ogg, flac, m4a...).
    File(PathBuf),
    /// Interleaved float samples in -1..1 (e.g. straight from a TTS model).
    Samples { data: Arc<[f32]>, sample_rate: u32, channels: u16 },
}

#[derive(Clone, Debug)]
pub struct Sound {
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
    pub source: String,
    pub start: f64,
    pub gain: f64,
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
    let data_len = (samples.len() * 2) as u32;
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
    let tmp_dir = video.with_extension("audio-tmp");
    std::fs::create_dir_all(&tmp_dir)?;
    let result = (|| -> Result<()> {
        let mut cmd = Command::new(ffmpeg);
        cmd.args(["-y", "-loglevel", "error", "-i"]).arg(video);
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
            cmd.arg("-i").arg(&input);
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
        cmd.arg("-filter_complex")
            .arg(filters.join(";"))
            .args(["-map", "0:v", "-map", "[aout]", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest"])
            .args(["-movflags", "+faststart"])
            .arg(&out)
            .stdin(Stdio::null());
        let res = cmd.output().with_context(|| format!("running {}", ffmpeg.display()))?;
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
