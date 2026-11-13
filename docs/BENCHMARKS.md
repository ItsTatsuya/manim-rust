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

## Community crates evaluated (2026-11-12)

Each candidate was measured on its own pipeline stage, then end to end with an interleaved
A/B (old and new binaries alternate run by run, so background load hits both; median of
4–6 runs, 1080p60 unless noted).

| Crate | Stage | Verdict | Isolated result |
|---|---|---|---|
| [yuvutils-rs](https://github.com/awxkee/yuvutils-rs) | RGBA → YUV 4:2:0 before ffmpeg | **Adopted** | 0.67 → 0.06 ms/frame wall, 5.0 → 0.3 ms single-thread (16× less CPU); max ±1 level vs the old loop |
| [vello_cpu](https://github.com/linebender/vello) | CPU rasterizer | **Adopted, adaptive** | Benchmark (400 shapes): 257 → 663 fps (2.6×, beats the GPU's ~305). Light scenes: ~15% slower, so `cpu` picks per complexity bucket |
| [mimalloc](https://github.com/purpleprotocol/mimalloc_rust) | Global allocator | Rejected | Within ±2% on every scene (noise) |
| [blaze](https://github.com/indexable-inc/blaze) | Build tool (incremental rebuilds) | Not applicable | Speeds up `cargo build`, not renders; Linux/macOS only, no Windows installer |

End to end, before → after:

| Output | SquareToCircle | Showcase | Graphing | Benchmark |
|---|---|---|---|---|
| MP4, default (NVENC) | 1.33 → 1.23 s (−7%) | 3.35 → 2.98 s (−11%) | 2.92 → 2.57 s (−12%) | 1.46 → 1.23 s (−16%) |
| MP4, `--encoder x264` | 1.97 → 1.89 s (−4%) | 5.42 → 5.14 s (−5%) | 4.61 → 4.44 s (−4%) | 2.77 → 2.59 s (−7%) |
| MP4, 480p15 | 0.22 → 0.22 s (−2%) | 0.40 → 0.39 s (−4%) | 0.41 → 0.39 s (−6%) | 0.37 → 0.33 s (−9%) |
| `--format none` (render only) | 0.70 → 0.69 s | 1.28 → 1.27 s | 1.14 → 1.16 s | 0.99 → 0.79 s (−21%) |

The YUV conversion gain is largest with NVENC, where ffmpeg no longer competes with our
conversion for CPU; with x264 the encoder itself is still the bottleneck. With vello the
CPU now beats the GPU on the Benchmark scene too, so `auto` picks the CPU there.


## Also evaluated, not adopted (2026-11-13)

Measured for low- to mid-range laptops (4–8 cores simulated with process affinity) and
kept out of the engine. ffmpeg stays the MP4 encoder and the audio muxer.

| Candidate | Result | Why not |
|---|---|---|
| Windows Media Foundation (H.264 + AAC, in-process, no ffmpeg) | On 4 cores: 60 s narrated scene 4.3 s vs 12.0 s (ffmpeg NVENC); Showcase 3.3 s vs 4.3 s | Windows-only second code path, untested on Intel/AMD iGPUs, MF quirks (its faststart option writes corrupt files), files 1.5–3× larger |
| openh264 + muxide | 10–50× slower than x264 (no nasm → no SIMD), Baseline profile only | Too slow |
| vello (GPU compute) | Readback-bound: 474–520 fps on light scenes vs 641–670 for the wgpu renderer; ≤16% faster on the 400-shape scene | Not faster; needs wgpu 24 → 30 |
| vello_hybrid | 139–154 fps on the 400-shape scene | Slower everywhere |

## Reproduce

```powershell
python -m venv .venv
.venv\Scripts\pip install manim
pwsh bench/run.ps1 -Manim .venv\Scripts\manim.exe -Runs 3
pwsh bench/run.ps1 -Manim .venv\Scripts\manim.exe -Qualities qh -RustArgs "--encoder","x264"
```
