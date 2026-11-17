//! Contact sheets of text and morph animations, for comparing engine versions by eye.
//!
//! `cargo run --release --example anim_gallery -- OUT_DIR` renders every scene of
//! `examples/scripts/anim_gallery.ts`: one row per play, frames at 0..100% of it.
//! `cargo run --release --example anim_gallery -- --compare BEFORE AFTER OUT_DIR`
//! stacks two such runs per scene (before on top with a red bar, after below with a green one).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use manim::config::{Config, Quality};
use manim::script::{HostOptions, ScriptHost};

const ALPHAS: [f64; 9] = [0.0, 0.1, 0.25, 0.4, 0.5, 0.6, 0.75, 0.9, 1.0];
const SHRINK: usize = 2;

struct Image {
    w: usize,
    h: usize,
    rgba: Vec<u8>,
}

impl Image {
    fn new(w: usize, h: usize, fill: [u8; 4]) -> Self {
        Image { w, h, rgba: fill.repeat(w * h) }
    }

    fn blit(&mut self, src: &Image, x0: usize, y0: usize) {
        for y in 0..src.h.min(self.h.saturating_sub(y0)) {
            let n = src.w.min(self.w.saturating_sub(x0)) * 4;
            let d = ((y0 + y) * self.w + x0) * 4;
            self.rgba[d..d + n].copy_from_slice(&src.rgba[y * src.w * 4..y * src.w * 4 + n]);
        }
    }

    fn shrink(&self, k: usize) -> Image {
        let (w, h) = (self.w / k, self.h / k);
        let mut out = Image::new(w, h, [0, 0, 0, 255]);
        for y in 0..h {
            for x in 0..w {
                for c in 0..4 {
                    let mut s = 0u32;
                    for dy in 0..k {
                        for dx in 0..k {
                            s += self.rgba[((y * k + dy) * self.w + x * k + dx) * 4 + c] as u32;
                        }
                    }
                    out.rgba[(y * w + x) * 4 + c] = (s / (k * k) as u32) as u8;
                }
            }
        }
        out
    }

    fn save(&self, p: &Path) -> Result<()> {
        manim::output::save_png(p, self.w as u32, self.h as u32, &self.rgba)
    }

    fn load(p: &Path) -> Result<Image> {
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(p).with_context(|| p.display().to_string())?));
        let mut r = dec.read_info()?;
        let mut buf = vec![0; r.output_buffer_size().context("PNG too large")?];
        let info = r.next_frame(&mut buf)?;
        buf.truncate(info.buffer_size());
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => buf.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
            t => anyhow::bail!("unsupported PNG color type {t:?}"),
        };
        Ok(Image { w: info.width as usize, h: info.height as usize, rgba })
    }
}

fn sheet(host: &ScriptHost, scene: &str, cfg: &Config) -> Result<Image> {
    host.run(scene, cfg)?;
    let tl = host.timeline().context("no timeline")?;
    let plays: Vec<_> = tl.plays.iter().filter(|p| !p.wait).collect();
    let (fw, fh) = (cfg.pixel_width as usize / SHRINK, cfg.pixel_height as usize / SHRINK);
    let gap = 4;
    let mut out = Image::new(ALPHAS.len() * (fw + gap), plays.len() * (fh + gap), [40, 40, 40, 255]);
    let dt = 1.0 / cfg.fps;
    for (row, p) in plays.iter().enumerate() {
        for (col, a) in ALPHAS.iter().enumerate() {
            // The frame `a` of the way through the play.
            let t = (p.start + (a * p.frames as f64).round() * dt).min(tl.duration);
            let f = host.frame_at(t)?;
            let img = Image { w: cfg.pixel_width as usize, h: cfg.pixel_height as usize, rgba: f }.shrink(SHRINK);
            out.blit(&img, col * (fw + gap), row * (fh + gap));
        }
    }
    Ok(out)
}

fn render(out: &Path) -> Result<()> {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/scripts/anim_gallery.ts");
    let host = ScriptHost::load(&script, HostOptions::default())?;
    let mut cfg = Config::default().quality(Quality::Low);
    cfg.dry_run = true;
    cfg.record = true;
    cfg.progress = manim::events::ProgressMode::None;
    for scene in host.scenes() {
        match sheet(&host, &scene, &cfg) {
            Ok(img) => img.save(&out.join(format!("{scene}.png")))?,
            Err(e) => eprintln!("{scene}: {e:#}"),
        }
    }
    Ok(())
}

fn compare(before: &Path, after: &Path, out: &Path) -> Result<()> {
    for entry in std::fs::read_dir(after)? {
        let p = entry?.path();
        if p.extension().is_none_or(|e| e != "png") {
            continue;
        }
        let name = p.file_name().context("file name")?;
        let a = Image::load(&p)?;
        // A scene the old engine couldn't run gets a dark red placeholder.
        let b = Image::load(&before.join(name)).unwrap_or_else(|_| Image::new(a.w, a.h, [70, 0, 0, 255]));
        let bar = 10;
        let mut img = Image::new(a.w.max(b.w) + bar, a.h + b.h + 8, [20, 20, 20, 255]);
        img.blit(&Image::new(bar - 3, b.h, [220, 60, 60, 255]), 0, 0);
        img.blit(&b, bar, 0);
        img.blit(&Image::new(bar - 3, a.h, [60, 200, 90, 255]), 0, b.h + 8);
        img.blit(&a, bar, b.h + 8);
        img.save(&out.join(name))?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag, b, a, o] if flag == "--compare" => compare(Path::new(b), Path::new(a), Path::new(o)),
        [o] => render(Path::new(o)),
        _ => anyhow::bail!("usage: anim_gallery OUT_DIR | anim_gallery --compare BEFORE AFTER OUT_DIR"),
    }
}
