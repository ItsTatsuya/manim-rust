# Benchmarks

## Setup

- **Machine:** AMD Ryzen 7 5700X (8 cores / 16 threads), NVIDIA RTX 4060 Ti, Windows 11.
- **Python:** Manim Community v0.22.1 (Cairo renderer) in a clean virtualenv, Python 3.12.
  Run with `--disable_caching --progress_bar none` so neither the cache nor the progress
  bar affects the timing.
- **Rust:** `cargo build --release`, default settings (`--renderer auto --encoder auto`)
  unless noted.
- **Encoding:** both write H.264 MP4. Python uses PyAV + libx264. Rust uses ffmpeg 9.0.2
  (libx264, or NVENC at ≥1080p30).
- **Timing:** wall-clock for the whole process (startup + render + encode), median of 3
  runs. Python's `import manim` alone takes ~0.9 s and is included, because users pay it
  on every render.
- **Scenes:** [`bench/scenes.py`](../bench/scenes.py) mirrors `examples/*.rs` animation for
  animation. Frame counts are checked with ffprobe and match exactly.

## Results

| Quality | Scene | Python | Rust (default) | Speedup | Rust (`--encoder x264`) | Speedup |
|---|---|---|---|---|---|---|
| 480p15 | SquareToCircle | 1.17 s | 0.23 s | 5.0× | — | — |
| 480p15 | Showcase | 2.18 s | 0.44 s | 5.0× | — | — |
| 480p15 | Graphing | 2.37 s | 0.44 s | 5.4× | — | — |
| 480p15 | Benchmark | 5.52 s | 0.41 s | 13.4× | — | — |
| 1080p60 | SquareToCircle | 3.97 s | 1.43 s | 2.8× | 1.91 s | 2.0× |
| 1080p60 | Showcase | 12.62 s | 3.66 s | 3.4× | 5.50 s | 2.2× |
| 1080p60 | Graphing | 11.50 s | 3.07 s | 3.7× | 4.84 s | 2.4× |
| 1080p60 | Benchmark | 22.41 s | 1.48 s | 15.1× | 2.76 s | 8.2× |

At 480p, x264 is already the default (hardware encoders only kick in at ≥1080p30), so
there's no separate x264 column.

## Where the time goes (Rust, Showcase, 1080p60)

`MANIM_TRACE=1` prints per-stage timings:

| Stage | Time |
|---|---|
| Rasterizing 966 unique frames (CPU, 16 threads) | ~0.8 s |
| Encoding 1026 frames (x264 `fast`, sharing the CPU) | ~4.2 s |
| Encoding 1026 frames (NVENC) | ~2.5 s, mostly the ffmpeg pipe and upload |

So at 1080p manim-rust is encoder-bound. The renderer comparison without encoding
(`--format none`) shows how much headroom is left:

| Scene (1080p60, frames discarded) | CPU | GPU | `auto` picks |
|---|---|---|---|
| Showcase | ~1000 fps | ~570 fps | CPU |
| Graphing | ~1080 fps | ~580 fps | CPU |
| Benchmark (400 shapes) | ~235 fps | ~305 fps | GPU |

On this machine the CPU wins for typical 2D scenes: copying 8 MB per frame back from the
GPU costs more than rasterizing on 16 threads. The GPU wins on heavy scenes. That's why
`auto` measures instead of assuming.

## Reproduce

```powershell
python -m venv .venv
.venv\Scripts\pip install manim
pwsh bench/run.ps1 -Manim .venv\Scripts\manim.exe -Runs 3
pwsh bench/run.ps1 -Manim .venv\Scripts\manim.exe -Qualities qh -RustArgs "--encoder","x264"
```
