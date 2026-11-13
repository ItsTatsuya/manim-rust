# manim-rust vs Manim Community (Python)

manim-rust is a **rewrite**, not a port or a binding. It follows Manim Community's concepts,
names, defaults and visual style, but the engine underneath is new. This page lists every
difference that matters when you use it.

## 1. Language and scene model

| | Python Manim | manim-rust |
|---|---|---|
| Scene | Subclass `Scene`, override `construct(self)` | A closure: `manim::render("Name", \|s: &mut Scene\| { ... })` |
| Rendering a file | `manim -qh scene.py MyScene` (interpreted) | `manim-rs scene.rs -qh`: compiled in a hidden cache, run, then the executable is deleted. Only the script and video stay in your folder. ~1 s overhead once the engine is cached |
| Several scenes per file | `manim file.py SceneA SceneB` | Call `manim::render` several times in `main` |
| Mobject identity | Python object references, freely shared and mutated | The scene owns all mobjects; you hold a `MobjectId` (`Copy`) |
| Adding | `self.add(m)` | `let id = s.add(m);` (shown) or `s.insert(m)` (stored, hidden) |
| Mutating outside animations | `m.shift(UP)` | `s.modify(id, \|m\| m.shift(UP))` or `s[id].stroke_width = 2.0` |
| Building mobjects | Methods mutate in place and return `self` | Builder methods take and return the value: `Circle::new(1.0).set_fill(RED, 0.5).shift(UP)` |
| Optional arguments | Keyword arguments | Separate constructors (`Text::sized`, `Arrow::with_buff`) or builder methods (`.run_time(2.0)`) |
| `.animate` | `m.animate.shift(RIGHT).set_color(RED)` | `id.animate(\|m\| m.shift(RIGHT).set_color(RED))` |
| `self.play(a, b, run_time=2)` | Keyword applies to all | Per animation: `s.play((a.run_time(2.0), b.run_time(2.0)))`, or wrap them in `AnimationGroup::new(...).run_time(2.0)` |
| `ValueTracker` | Mobject with `get_value` / `set_value` | `let t = s.tracker(0.0)`, `w.value(t)`, animate it with `ChangeValue::new(t, 5.0)` |
| Updaters | `m.add_updater(lambda m, dt: ...)` | `s.add_updater(move \|w, dt\| ...)`: one closure with access to the whole world |
| `always_redraw` | `always_redraw(lambda: ...)` | `s.always_redraw(move \|w\| ...)` returns the new mobject's id |
| Errors | Exceptions at runtime | Type errors at compile time; `render` returns `anyhow::Result` |

## 2. Rendering

| | Python Manim | manim-rust |
|---|---|---|
| CPU backend | Cairo, sequential | tiny-skia, batches of frames rasterized in parallel (rayon) |
| GPU backend | OpenGL renderer, a separate code path with its own feature gaps | wgpu (Vulkan / DX12 / Metal): lyon tessellation, 4× MSAA, pipelined readback. Same display list as the CPU path, so identical features |
| Choosing a backend | `--renderer cairo\|opengl` | `--renderer auto\|cpu\|gpu\|skia\|vello`. `auto` rejects software adapters, then at ≥1080p times both backends on the scene's first frames and keeps the faster. The CPU backend itself times tiny-skia against vello_cpu per path-complexity bucket |
| Fill rule | Cairo winding | Even-odd for shapes; non-zero for glyphs and SVG paths (variable-font contours overlap) |
| Anti-aliasing | Cairo analytic AA | CPU: analytic AA. GPU: 4× MSAA. Mean difference between the two is under 2/255 per channel (tested) |
| Translucent strokes | Single pass | Single pass on CPU. On GPU, overlapping stroke joins can double-blend |
| Frame dedup | No | Identical consecutive frames are rasterized once |
| `-s` (last frame only) | Skips animations | Runs scene logic but never rasterizes intermediate frames |

## 3. Text

| | Python Manim | manim-rust |
|---|---|---|
| `Text` | Pango layout, Cairo outlines | fontdb finds the font (system, or files/folders you register), rustybuzz shapes it, ttf-parser gives the outlines |
| Default font | Pango's default (often a serif) | The theme's body font, else the system sans-serif (Arial on Windows) |
| Weights | Named weights | 100–900; variable fonts get their `wght` axis set |
| Per-glyph submobjects | Yes | Yes: `Write` and `LaggedStart` stagger per letter |
| Indexing | `text[6:11]` counts non-space glyphs | `part("word")` / `chars(a, b)` count characters of the string (spaces included) |
| Complex scripts (Arabic, Devanagari, ligatures) | Yes (via Pango/HarfBuzz) | Yes (rustybuzz, a HarfBuzz port), with per-character font fallback |
| `MarkupText` | Yes | Yes: `<b>`, `<i>`, `<span color= weight= font= size=>` |
| `MathTex` / `Tex` | LaTeX (needs a TeX distribution) | Embedded Typst compiler: same idea, Typst math syntax (`x^2`, `integral_0^1`, `sum_(k=1)^n`), no install |

## 4. Video output

| | Python Manim | manim-rust |
|---|---|---|
| Encoder | PyAV + libx264, CRF 23 | ffmpeg subprocess. `auto` uses NVENC / QSV / AMF at ≥1080p30, else libx264 CRF 18 |
| Color conversion | swscale in-process, single-threaded, untagged | RGBA→YUV 4:2:0 in Rust across all cores. BT.601 for SD and BT.709 for HD, tagged in the file |
| Partial movie files | One file per `play`, concatenated at the end | Single pass, streamed straight to the encoder |
| Caching | Hash-based partial-movie cache | None. Rendering is fast enough that it hasn't been needed |
| Formats | MP4, MOV, WebM, GIF, PNG | MP4, GIF (pure Rust, no ffmpeg needed), PNG sequence |
| Sound | `add_sound` | `add_sound` / `add_sound_at` (files) and `add_samples` (raw audio), mixed and muxed with ffmpeg |
| Frame counts | `play`: ⌈t·fps⌉, `wait`: ⌊t·fps⌋ | Same rule; identical counts verified on all benchmark scenes |
| Odd resolutions | Handled by the encoder | Padded to even dimensions for H.264 |

## 5. Behaviour differences inside animations

- **During** an animation, a mobject is temporarily flattened into its leaf paths. When
  the animation finishes, the original hierarchy (or the target's, for transforms) comes
  back. Code that reads `s[id].children` from an updater *during* a `play` sees the
  flattened version.
- `FadeOut`, `Uncreate`, `ShrinkToCenter` and `ShowPassingFlash` hide the mobject and
  restore its original geometry, so `s.show(id)` brings it back intact.
- Updaters always receive `dt`. Python distinguishes `lambda m:` from `lambda m, dt:`.
- `to_edge` and `to_corner` assume a 16:9 frame, like Manim's defaults.

## 6. What's missing

3D (`ThreeDScene`, surfaces, camera orientation), `MovingCameraScene`, `Table`, `Matrix`,
`Code`, LaTeX syntax (equations use Typst syntax), `--preview`, configuration files
(`manim.cfg`), and Python bindings. Scenes can be written in TypeScript/JavaScript (see
[SCRIPTING.md](SCRIPTING.md)) as well as Rust.
