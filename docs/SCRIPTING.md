# Scene scripts and host integration

manim-rust runs scenes written in **TypeScript or JavaScript** on an embedded QuickJS
engine. There is no Node, no TypeScript compiler, no Rust toolchain and no Python at
runtime: the script is type-stripped by [oxc](https://oxc.rs) and evaluated in-process, and
every frame is produced by the Rust engine. This is the path meant for apps such as
Manim Studio, where scripts are written by people or agents and rendered on users'
machines.

```bash
manim-rs new intro.ts         # template
manim-rs intro.ts -qh         # render: Intro.mp4 next to the script
manim-rs types > manim.d.ts   # declarations for editors and agents
```

## Writing scenes

```ts
import { Scene, Text, MathTex, Circle, VGroup, Write, Create, FadeIn, FadeOut,
         GrowFromEdge, SurroundingRectangle, DOWN, LEFT, UP } from "manim";

export default class Lesson extends Scene {
  construct() {
    this.setTheme({ colors: { background: "#FFFFFF", text: "#0F172A", primary: "#4F46E5" },
                    fonts: { heading: "Plus Jakarta Sans", body: "Inter" },
                    layout: { pattern: "dots" } });
    const title = Text("Why circles have area πr²", { role: "title" }).toCorner(UL);
    this.play(Write(title));

    this.mark("line-2");                                   // a named moment on the timeline
    const sentence = Text("Unroll each ring into a thin strip", { role: "body" });
    this.play(FadeIn(sentence, { shift: UP.mul(0.15) }));
    const word = SurroundingRectangle(sentence.part("strip"), { buff: 0.07, fillOpacity: 0.35, strokeWidth: 0 });
    this.play(GrowFromEdge(word, LEFT));

    const eq = MathTex("A = pi r^2", { fontSize: 60 }).nextTo(sentence, DOWN, 0.8);
    this.play(Write(eq));
    this.play(eq.animate.shift(LEFT.mul(2)).setColor("#E11D48"), { runTime: 0.8 });
  }
}
```

- The API follows Manim Community Edition in camelCase (`moveTo`, `nextTo`,
  `setColor`); snake_case method and option names (`move_to`, `font_size`) work too.
  Constructors and animations work with or without `new`.
- Mobjects are handles into the engine. Methods change the mobject and return it.
  `VGroup(a, b)` makes `a` and `b` members of the group: changing or animating them
  changes the group, as in Python Manim.
- Vectors are `[x, y]` arrays or `Vec3` (`UP.mul(2).add(LEFT)`).
- Colors are `"#RRGGBB"`; Manim's palette (`BLUE`, `RED_C`, …) is exported.
- Equations use [Typst math](https://typst.app/docs/reference/math/) syntax:
  `MathTex("integral_0^1 x^2 dif x = 1/3")`. No TeX installation is needed.
- `Text` options: `font`, `fontSize`, `weight` (100–900 or names), `italic`, `color`,
  `maxWidth` (wraps), `align`, `lineSpacing`, `role` (`title`, `heading`, `body`,
  `caption`, `code`, `hand` from the theme). `MarkupText` takes `<b>`, `<i>`,
  `<span color= weight= font= size=>`.
- `text.part("word")` / `text.slice(a, b)` select glyphs by position in the string (to
  frame or highlight words); `setColorText("word", color)` recolors in place.
- Scripts may import `"manim"` and relative files (`import { title } from "./style"`).
  They have no file system, network or process access.

### Camera, cards and dashes

```ts
const card = Rectangle(4, 2, { fillColor: "#FFFFFF", fillOpacity: 1 })
  .setGradient(-PI / 4, [[0, "#2EC4B6", 0.2], [1, "#2EC4B6", 0.05]]) // CSS 135deg
  .setShadow("#000000", 0.35, 0.3, 0, -0.2)                          // colour, opacity, blur, dx, dy
  .pin();                                                            // ignores the camera
const guide = Line(a, b).setDash(0.2, 0.15).clipTo(-1, -1, 1, 1);    // dashes cut at draw time

this.play(MoveCamera({ focus: { point: [10, 20], anchor: [3, 0] }, zoom: 1.22, runTime: 2.2 }));
this.play(guide.animate.setDash(0.2, 0.15, 0.35));                   // dashes march
this.play(MoveCamera({ focus: { point: [10, 20], anchor: [10, 20] }, zoom: 1 }));
```

- `MoveCamera` takes `zoom` (2 shows half the width) or `width`, and `center`; `{focus:
  {point, anchor}}` keeps `point` under a screen position that glides to `anchor` (a position in
  the base frame), so a panel inside a larger layout can be pushed in on one spot, and the same
  call with the point's own position as the anchor and `zoom: 1` undoes it exactly;
  `{reset: true}` returns to the base frame. It moves everything except pinned mobjects, the
  overlay (`addOverlay`) and the background. The live frame is part of the timeline, so seeking
  restores it.
- `pin()` and `clipTo(left, bottom, right, top)` mark the mobject and the children it has *now*.
  The clip rectangle is in the base frame, in whole pixels, so zoomed content stays inside its
  panel.
- `setDash(length, gap, phase)` dashes the stroke in scene units. Dashes are cut from the path
  when a frame is drawn, so `Create` and partial paths draw a dashed line on, it works on curves,
  and `phase` can be animated. `clearDash()` removes it.
- `setGradient(angle, stops)` fills over the bounding box (angle in radians, 0 = left to right,
  counter-clockwise). Stops are `[position, color, opacity?]`; the fill opacity still multiplies
  them (set `fillOpacity: 1`), so fades work.
- `setShadow(color, opacity, blur, dx, dy)` draws a soft shadow outside the outline (never
  through a see-through card). `setBackdropBlur(sigma)` blurs everything drawn below the
  mobject, inside its outline; the mobject needs a shape but not a fill.
- Frames that use gradients, shadows, blur or clips are drawn by the tiny-skia rasterizer on every
  renderer (the GPU renderer and vello hand them over, as they do for images), so all renderers
  give identical pixels for them.

### Tables, matrices and code

```ts
import { Scene, Table, Matrix, IntegerMatrix, Code, Text, SurroundingRectangle, GREEN, RED, YELLOW } from "manim";

export default class Data extends Scene {
  construct() {
    const t = Table([["First", "Second"], ["Third", "Fourth"]], {
      rowLabels: [Text("R1"), Text("R2")], colLabels: [Text("C1"), Text("C2")], includeOuterLines: true,
    });
    t.addHighlightedCell([2, 2], { color: GREEN });   // [row, col], 1-based, labels included
    t.getEntries([3, 3]).setColor(RED);               // a live entry
    this.play(t.create());                            // lines, then entries, then labels

    const m = Matrix([[2, "pi"], [-1, 1]], { leftBracket: "(", rightBracket: ")" });
    m.setColumnColors(RED, YELLOW);
    const box = SurroundingRectangle(m.getColumns()[1]);

    const code = Code({ codeString: "def f(x):\n    return 2 * x\n", language: "python",
                        formatterStyle: "monokai", background: "window" });
    const line2 = SurroundingRectangle(code.codeLines.get(1));
  }
}
```

- `Table`, `MathTable`, `IntegerTable`, `DecimalTable` and `MobjectTable` take rows of
  strings, numbers or mobjects. Options follow Manim: `rowLabels`, `colLabels`,
  `topLeftEntry`, `vBuff` (0.8), `hBuff` (1.3), `includeOuterLines`, `includeInnerLines`,
  `lineConfig`, `addBackgroundRectanglesToEntries`, `includeBackgroundRectangle`,
  `elementToMobjectConfig` (text options for the entries).
- `Matrix` entries are equations in Typst math syntax (`"pi"`, `"x^2"`; numbers work),
  sized and spaced like Manim's (digits 0.333 units tall at the default size).
  `IntegerMatrix`, `DecimalMatrix` (`elementToMobjectConfig: { numDecimalPlaces }`) and
  `MobjectMatrix` too. Brackets: `leftBracket`/`rightBracket` `"["`, `"("`, `"{"`, `"|"`,
  `"||"`, `"<"` (or `"\\langle"`), `"."` for none; `getDetText(m, { determinant })`.
- Both have `getEntries()` (a live group, row by row) and `getEntries([r, c])`,
  `getRows()`, `getColumns()`, `getMobMatrix()`, `setRowColors(...)`,
  `setColumnColors(...)`, `getCell([r, c])`, `getHighlightedCell`, `addHighlightedCell`,
  `getHorizontalLines()`, `getVerticalLines()`, `getBrackets()`, `getLabels()`.
  Rows and columns are copies for framing and measuring; style changes on them
  (`setColor`, `setFill`, …) also reach the entries.
- `Code` highlights `codeString` or `codeFile` (a project file; the language follows the
  extension) like Manim with Pygments: `language` (Python, Rust, TypeScript, JavaScript, C,
  C++, C#, Java, Go, Kotlin, Swift, JSON, Bash, SQL; guessed when omitted),
  `formatterStyle` (`"vim"` by default; `"monokai"`, `"dracula"`, `"github-dark"`,
  `"one-dark"`, `"nord"`, … in `Code.getStylesList()`), `addLineNumbers`,
  `lineNumbersFrom`, `tabWidth`, `background: "rectangle" | "window"`,
  `backgroundConfig`, `paragraphConfig: { font, fontSize, lineSpacing }`. The font is
  the theme's code font. `code.codeLines.get(i)` is line `i` (0-based) as live text;
  `code.lineNumbers` and `code.background` are the other parts.

The full API is in [`src/script/manim.d.ts`](../src/script/manim.d.ts) (`manim-rs types`).

## Command line for hosts

All Manim flags work, plus:

| Flag | Meaning |
|---|---|
| `--progress json` | One JSON event per line on stderr (below) |
| `--dry-run` | Run the scene logic only: no rasterizing, no files. Milliseconds for most scenes |
| `--check` | Report overlapping text, text off the frame, and crossfade ghosting (implies `--dry-run` unless snapshots are requested) |
| `--snapshot markers,plays,end,2.5` | Save PNGs at each `mark()`, after each play, at the end, or at given seconds, in `<Scene>_snapshots/` |
| `--timeline` | Write `<Scene>.timeline.json`: plays, markers, sections, sounds, duration, problems |
| `--theme FILE` | Apply a theme JSON (Manim Studio's `styles.json` entry format) |
| `--fonts DIR` | Register a folder of font files (repeatable) |
| `--scene NAME`, `--list-scenes` | Pick one scene / list them |
| `--sandbox DIR` | Asset paths and imports must stay inside `DIR` |
| `--timeout SECONDS` | Stop a script that runs too long (infinite loops); a per-call limit that does not cover finishing the video |
| `--x264-preset NAME`, `--crf N` | x264 quality settings (see the README) |

### Progress events (`--progress json`)

| event | fields |
|---|---|
| `start` | `scene`, `width`, `height`, `fps`, `renderer`, `output` |
| `play` | `index`, `name`, `start`, `run_time`, `frames` |
| `wait` | `index`, `start`, `run_time`, `frames` |
| `progress` | `index`, `percent`, `time` |
| `marker` | `name`, `time`, `kind` (`marker` / `section`) |
| `sound` | `source`, `start`, `gain`, `duration` |
| `problem` | `kind` (`overlap` / `offscreen` / `ghosting`), `time`, `play`, `a`, `b`, `message` |
| `snapshot` | `path`, `time`, `label` |
| `done` | `scene`, `duration`, `frames`, `rasterized`, `elapsed`, `outputs`, `problems` |
| `error` | `scene`, `message` (TypeScript file and line, plus the offending source line) |

Scene times are exact multiples of `1/fps`.

## Embedding (Rust)

```rust
use manim::config::Config;
use manim::script::{HostOptions, ScriptHost};

let host = ScriptHost::load("project/script.ts", HostOptions {
    sandbox: Some("project".into()),
    timeout: Some(std::time::Duration::from_secs(60)),
    ..Default::default()
})?;
println!("{:?}", host.scenes());

// Data for the script (e.g. measured narration lengths): globalThis.NARRATION
host.set_global_json("NARRATION", r#"{"seconds":[3.2, 5.8]}"#)?;

// Final render.
host.render("Lesson", &Config::default())?;

// Preview: run once without rendering, then show any time.
let mut cfg = Config::default();
cfg.dry_run = true;
cfg.record = true;
host.run("Lesson", &cfg)?;
let rgba = host.frame_at(4.25)?;                     // scrub
let timeline = host.timeline().unwrap();             // plays, markers, duration
let on_screen = host.with_scene(|s| s.inspect());    // layout boxes
```

- `ScriptHost` is single-threaded (keep it on one thread); `cancel_handle()` returns an
  `Arc<AtomicBool>` other threads can set to stop a render.
- Seeking replays from the keyframe before `t` (one per play), so a jump costs at most one
  play's worth of scene logic, not rasterizing. Updaters run again while replaying, so they
  should depend on the scene, not on state they keep themselves.
- `seek(t)` and `frame_at(t)` show exactly what exported frame `floor(t * fps)` shows
  (frame `k` is `k / fps` seconds in), so a preview scrubbed to a time matches the video.
- Sound: `scene.addSound("assets/x.wav")` from scripts, or
  `Scene::add_samples(samples, rate, channels, at, gain)` from the host (e.g. a TTS voice
  line); sounds are mixed and muxed into the MP4 with ffmpeg in one pass.
- From Rust scenes the same features are on `Scene`: `set_theme`, `add_background`,
  `add_overlay`, `mark`, `section`, `add_sound_at`, `inspect`, `problems`, `timeline`,
  `seek`, `frame_at`, and `begin_play` / `step` / `end_frame` for driving a play frame by
  frame.

### Limits

| Limit | Value |
|---|---|
| `HostOptions::timeout` | Per call, restarting each time: loading the script, `run`/`render` (the whole `construct()`, including the frames it rasterizes and writes), `seek`, `frame_at`. It does not cover `finish` (encoder flush, audio mux); stop that with `cancel_handle()` |
| `HostOptions::memory_limit` | JS heap, default 512 MiB (`None` removes it). Mobject geometry lives in Rust and is bounded by the next rows |
| Group nesting | 256 levels (`MAX_NESTING`) |
| Curves per mobject | 1,000,000 when built by a script |
| Typst markup (`MathTex`, `Typst`) | 64 KiB of markup, 30 s per compile; a compile stops when the script is cancelled or times out |
| Frame size | 16384 pixels per side |
| Frames in flight | about 512 MiB shared by the batch being drawn and the encoder queue |

Exceeding a limit is an error from the call, not a panic. Panics from scene code are caught
and returned as errors too, which relies on unwinding: a host must not be built with
`panic = "abort"` (the `dist` profile no longer sets it). A script gets its own
`script_cancel` / `script_deadline` next to `Config::cancel` / `Config::deadline`, so a
host's flags and the script's never overwrite each other.

## Themes

A theme is the JSON of a Manim Studio style (extra fields are ignored):

```json
{ "name": "Studio",
  "fonts":  { "heading": "Plus Jakarta Sans", "body": "Inter", "code": "JetBrains Mono", "hand": "Caveat", "fallback": ["Nirmala UI"] },
  "colors": { "background": "#FFFFFF", "text": "#0F172A", "muted": "#64748B", "primary": "#4F46E5",
              "secondary": "#0284C7", "accent": "#E11D48", "positive": "#059669", "negative": "#DC2626",
              "key_fill": "#EEF0FF", "example_fill": "#E6F4FB" },
  "sizes":  { "title": 54, "heading": 40, "body": 30, "small": 22 },
  "layout": { "margin": 0.65, "title": "top-left", "boxes": "card", "accent": true, "pattern": "grid" },
  "pacing": { "run_time": 1.0, "wait": 1.0, "words_per_second": 2.4 } }
```

Applying it sets the background color, draws the pattern once into the background layer
(cached by the rasterizer, kept by `clear()`, ignored by layout checks), and makes the
text color and body font the defaults for new shapes and text. Text roles pick the role's
font, size, color and weight.
