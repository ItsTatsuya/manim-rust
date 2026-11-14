//! TypeScript / JavaScript scene scripts on the embedded QuickJS host.
#![cfg(feature = "script")]

use std::path::{Path, PathBuf};
use std::time::Duration;

use manim::config::{Config, Quality};
use manim::script::{HostOptions, ScriptHost};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("manim-script-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(d: &Path, file: &str, src: &str) -> PathBuf {
    let p = d.join(file);
    std::fs::write(&p, src).unwrap();
    p
}

fn cfg() -> Config {
    let mut c = Config::default().quality(Quality::Low);
    c.dry_run = true;
    c.progress = manim::events::ProgressMode::None;
    c
}

#[test]
fn finds_scenes_and_runs_them() {
    let d = dir("scenes");
    let p = write(
        &d,
        "scenes.ts",
        r##"
import { Scene, scene, Circle, Square, Create, FadeOut, Transform, RIGHT, UP } from "manim";
type N = number;
export default class Intro extends Scene {
  construct(): void {
    const c = Circle(1, { color: "#FF0000" });
    this.play(Create(c));
    this.play(c.animate.shift(RIGHT.mul(2)).setColor("#00FF00"), { runTime: 0.5 });
    const n: N = 3;
    this.mark("after-move");
    this.wait(n / 3);
  }
}
export const Outro = scene("Outro", (s) => {
  const sq = Square(2);
  s.add(sq);
  s.play(Transform(sq, Circle()));
  s.play(FadeOut(sq, { shift: UP }));
});
export const notAScene = 42;
"##,
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    assert_eq!(host.scenes(), vec!["Intro".to_string(), "Outro".to_string()]);
    host.run("Intro", &cfg()).unwrap();
    let tl = host.timeline().unwrap();
    assert_eq!(tl.plays.len(), 3);
    assert_eq!(tl.markers[0].name, "after-move");
    assert!((tl.markers[0].time - 1.5333333).abs() < 1e-6, "{}", tl.markers[0].time);
    // The circle moved right and turned green.
    let center = host.with_scene(|s| s.inspect()[0].bbox).unwrap();
    assert!(((center[0] + center[2]) / 2.0 - 2.0).abs() < 1e-6);
    host.run("Outro", &cfg()).unwrap();
    assert!(host.with_scene(|s| s.inspect().is_empty()).unwrap(), "faded out");
    assert!(host.run("Nope", &cfg()).is_err());
}

#[test]
fn errors_point_at_typescript_lines() {
    let d = dir("errors");
    let p = write(
        &d,
        "broken.ts",
        "import { Scene, Circle } from \"manim\";\n\
         interface Opts { r: number }\n\
         \n\
         export default class Broken extends Scene {\n\
           construct(): void {\n\
             const o: Opts = { r: 1 };\n\
             const c = Circle(o.r);\n\
             c.notAMethod();\n\
           }\n\
         }\n",
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    let e = format!("{:#}", host.run("Broken", &cfg()).unwrap_err());
    assert!(e.contains("broken.ts:8"), "{e}");
    assert!(e.contains("8 | ") && e.contains("c.notAMethod();"), "the failing line is quoted: {e}");
    // Engine errors surface as JS exceptions with a readable message.
    let p = write(&d, "bad_anim.ts", "import { Scene, Text } from 'manim';\nexport default class B extends Scene { construct() { Text('x').setColor('blue-ish'); } }\n");
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    let e = format!("{:#}", host.run("B", &cfg()).unwrap_err());
    assert!(e.contains("setColor"), "{e}");
    // Syntax errors are reported at load with a position.
    let p = write(&d, "syntax.ts", "const x: = 1;\n");
    let e = format!("{:#}", ScriptHost::load(&p, HostOptions::default()).err().unwrap());
    assert!(e.contains("syntax.ts:1:"), "{e}");
}

#[test]
fn imports_relative_modules_but_nothing_else() {
    let d = dir("imports");
    write(
        &d,
        "helpers.ts",
        "import { Text } from 'manim';\nexport const label = (s: string) => Text(s, { role: 'caption' });\n",
    );
    let p = write(
        &d,
        "main.ts",
        "import { Scene, FadeIn } from 'manim';\nimport { label } from './helpers';\nexport default class M extends Scene { construct() { this.play(FadeIn(label('hi'))); } }\n",
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    host.run("M", &cfg()).unwrap();
    // (An unused import would be elided by the TypeScript transform, as tsc does.)
    let p = write(&d, "fs.ts", "import * as fs from 'fs';\nexport const read = () => fs.readFileSync('x');\n");
    let e = format!("{:#}", ScriptHost::load(&p, HostOptions::default()).err().unwrap());
    assert!(e.contains("cannot import 'fs'"), "{e}");
}

#[test]
fn sandbox_keeps_assets_inside_the_project() {
    let root = dir("sandbox");
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        root.join("secret.svg"),
        "<svg xmlns='http://www.w3.org/2000/svg'><rect width='1' height='1'/></svg>",
    )
    .unwrap();
    std::fs::write(
        project.join("ok.svg"),
        "<svg xmlns='http://www.w3.org/2000/svg'><rect width='1' height='1'/></svg>",
    )
    .unwrap();
    let p = write(
        &project,
        "s.ts",
        "import { Scene, SVGMobject } from 'manim';\n\
         export class Inside extends Scene { construct() { this.add(SVGMobject('ok.svg')); } }\n\
         export class Outside extends Scene { construct() { this.add(SVGMobject('../secret.svg')); } }\n",
    );
    let host = ScriptHost::load(&p, HostOptions { sandbox: Some(project.clone()), ..Default::default() }).unwrap();
    host.run("Inside", &cfg()).unwrap();
    let e = format!("{:#}", host.run("Outside", &cfg()).unwrap_err());
    assert!(e.contains("outside the project"), "{e}");
}

#[test]
fn groups_members_and_updaters() {
    let d = dir("groups");
    let p = write(
        &d,
        "g.ts",
        r##"
import { Scene, VGroup, Square, Circle, Dot, ValueTracker, alwaysRedraw, FadeIn, Indicate, RIGHT, DOWN } from "manim";
export default class G extends Scene {
  construct() {
    const a = Square(1), b = Circle(0.5);
    this.add(a, b);
    const g = VGroup(a, b).arrange(RIGHT, 1);
    if (this.mobjects.length !== 1) throw new Error("group replaces its members on screen: " + this.mobjects.length);
    b.set_color("#FF0000");                       // snake_case alias, edits the group's member
    if (g.get(1).getColor() !== "#FF0000") throw new Error("member edit did not reach the group");
    this.play(Indicate(b));                        // animate a member, then it returns to the group
    if (this.mobjects.length !== 1) throw new Error("member popped back: " + this.mobjects.length);
    g.shift(DOWN);
    if (Math.abs(b.getCenter().y + 1) > 1e-6) throw new Error("member follows the group");
    const t = new ValueTracker(0);
    const d = alwaysRedraw(() => Dot([t.getValue(), 2]));
    this.add(d);
    this.play(t.animate.setValue(3));
    if (Math.abs(d.getCenter().x - 3) > 1e-6) throw new Error("updater ran: " + d.getCenter().x);
  }
}
"##,
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    host.run("G", &cfg()).unwrap();
}

#[test]
fn host_can_seek_a_recorded_script() {
    let d = dir("seek");
    let p = write(
        &d,
        "seek.ts",
        r##"
import { Scene, Square, Dot, ValueTracker, Create, RIGHT } from "manim";
export default class S extends Scene {
  construct() {
    const sq = Square(2, { fillColor: "#3366FF", fillOpacity: 0.7 });
    this.play(Create(sq));
    const t = new ValueTracker(-3);
    const dot = Dot([-3, -2]);
    dot.addUpdater((m) => m.moveTo([t.getValue(), -2]));
    this.add(dot);
    this.play(t.animate.setValue(3), sq.animate.shift(RIGHT).rotate(1), { runTime: 2 });
    this.wait(1);
  }
}
"##,
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    let mut c = cfg();
    c.record = true;
    host.run("S", &c).unwrap();
    let end = host.timeline().unwrap().duration;
    assert!((end - 4.0).abs() < 1e-9);
    // Seek forwards and backwards; the same time gives the same frame.
    let a = host.frame_at(1.8).unwrap();
    let b = host.frame_at(3.5).unwrap();
    let a2 = host.frame_at(1.8).unwrap();
    assert!(a == a2, "same frame after seeking back");
    assert!(a != b);
    // At 1.8 s the dot is 40% through a smooth move from x = -3 to 3: left of center.
    let dot_x = host
        .with_scene(|s| {
            let items = s.inspect();
            let dot = items.iter().min_by(|a, b| (a.bbox[2] - a.bbox[0]).total_cmp(&(b.bbox[2] - b.bbox[0]))).unwrap();
            (dot.bbox[0] + dot.bbox[2]) / 2.0
        })
        .unwrap();
    assert!(dot_x > -3.0 && dot_x < 0.0, "dot at {dot_x}");
}

#[test]
fn timeout_stops_runaway_scripts() {
    let d = dir("timeout");
    let p = write(
        &d,
        "loop.js",
        "import { Scene } from 'manim';\nexport default class L extends Scene { construct() { for (;;) {} } }\n",
    );
    let host =
        ScriptHost::load(&p, HostOptions { timeout: Some(Duration::from_millis(300)), ..Default::default() }).unwrap();
    let t = std::time::Instant::now();
    assert!(host.run("L", &cfg()).is_err());
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
fn globals_from_the_host() {
    let d = dir("globals");
    let p = write(
        &d,
        "n.ts",
        "import { Scene } from 'manim';\ndeclare const NARRATION: { seconds: number[] };\nexport default class N extends Scene { construct() { this.wait(NARRATION.seconds[1]); } }\n",
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    host.set_global_json("NARRATION", r#"{"seconds":[1.0, 2.0]}"#).unwrap();
    host.run("N", &cfg()).unwrap();
    assert_eq!(host.timeline().unwrap().duration, 2.0);
}

#[test]
fn types_cover_the_exports() {
    // Every value the JS module exports should be declared for editors and agents.
    let d = dir("types");
    let p = write(&d, "t.js", "import * as m from 'manim';\nimport { Scene } from 'manim';\nglobalThis.EXPORTS = Object.keys(m);\nexport default class T extends Scene {}\n");
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    let names: Vec<String> = host.eval_json("EXPORTS").unwrap();
    let missing: Vec<&String> = names.iter().filter(|n| !manim::script::TYPES.contains(&format!(" {n}"))).collect();
    assert!(missing.is_empty(), "undeclared exports: {missing:?}");
}

#[test]
fn tables_matrices_and_code() {
    let d = dir("tmc");
    write(&d, "snippet.py", "def f(x):\n    return x\n");
    let p = write(
        &d,
        "tmc.ts",
        r##"
import { Scene, Table, Matrix, IntegerMatrix, DecimalMatrix, MathTable, Code, Text, SurroundingRectangle,
  getDetText, Create, GREEN, RED, YELLOW, LEFT } from "manim";
const check = (ok: boolean, what: string) => { if (!ok) throw new Error(what); };
export default class T extends Scene {
  construct() {
    const t = Table([["First", "Second"], ["Third", "Fourth"]], {
      row_labels: [Text("R1"), Text("R2")], col_labels: ["C1", "C2"], include_outer_lines: true,
      line_config: { stroke_width: 2 },
    });
    check(t.getHorizontalLines().length === 4 && t.get_vertical_lines().length === 4, "outer + inner lines");
    check(t.getEntries([2, 2]).text === "First", "entry [2, 2]");
    check(t.getRows().length === 3 && t.getColumns().length === 3, "rows and columns with labels");
    check(t.getLabels().length === 4 && t.getEntriesWithoutLabels().length === 4, "labels");
    t.add_highlighted_cell([2, 2], { color: GREEN });
    t.getEntries([2, 2]).setColor(RED);                      // a live view, still right after the highlight
    check(t.getEntries([2, 2]).getColor() === "#FC6255", "entry color: " + t.getEntries([2, 2]).getColor());
    t.getColumns()[2].setColor(YELLOW);                      // a slice: style reaches the entries
    check(t.getEntries([3, 3]).getColor() === "#F7D96F", "column color");
    const cell = t.getCell([1, 1]);
    check(cell.width > 1 && cell.height > 0.5, "cell size");
    this.play(t.create());
    check(this.mobjects.length === 1, "create() leaves one table on screen: " + this.mobjects.length);
    t.shift(LEFT);

    const m = Matrix([[2, "pi"], [-1, 1]], { left_bracket: "(", right_bracket: ")" });
    check(m.getBrackets().length === 2, "brackets");
    check(Math.abs(m.getEntries([2, 2]).height - 0.333) < 0.01, "CE-sized entries");
    m.set_column_colors(RED, GREEN);
    check(m.getEntries([2, 1]).getColor() === "#FC6255", "column colors");
    const box = SurroundingRectangle(m.getColumns()[1]);
    check(box.height > m.getEntries().height, "frame a column");
    const im = IntegerMatrix([[1, 2], [3, 4]]);
    check(im.getEntries([1, 2]).text === "2", "integer entry");
    const dm = DecimalMatrix([[1.256]], { element_to_mobject_config: { num_decimal_places: 2 } });
    check(dm.getEntries([1, 1]).text === "1.26", "decimal places: " + dm.getEntries([1, 1]).text);
    check(getDetText(m, { determinant: 2 }).length === 5, "det text");
    this.add(m, box, MathTable([["x", "x^2"]]));

    const c = Code({ code_string: "let x = 1;\nconsole.log(x);\n", language: "js", background: "window", formatter_style: "monokai" });
    check(c.codeLines.length === 2 && c.code_lines.get(1).text === "console.log(x);", "code lines");
    check(c.lineNumbers.length === 2, "line numbers");
    check(Code.getStylesList().includes("vim"), "styles");
    const f = Code("snippet.py");
    check(f.codeLines.get(0).text === "def f(x):", "code from a file");
    this.play(Create(c));
  }
}
"##,
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    host.run("T", &cfg()).unwrap();
}

#[test]
fn axes_plot_returns_a_mobject_and_themes_load_by_path() {
    let d = dir("plot");
    std::fs::create_dir_all(d.join("themes")).unwrap();
    write(&d, "themes/night.json", r##"{ "name": "Night", "colors": { "background": "#101820" } }"##);
    let p = write(
        &d,
        "plot.ts",
        r##"
import { Scene, Axes, Create, Mobject } from "manim";
const check = (ok: boolean, what: string) => { if (!ok) throw new Error(what); };
export default class P extends Scene {
  construct() {
    this.setTheme("themes/night");
    check(this.theme?.name === "Night", "theme from a file path");
    const ax = Axes({ xRange: [-3, 3, 1], yRange: [-1, 9, 1] });
    const g = ax.plot((x) => x * x);
    const c = ax.plotParametricCurve((t) => [Math.cos(t), Math.sin(t)], { tRange: [0, 6.28] });
    check(g instanceof Mobject && c instanceof Mobject, "plot returns a Mobject");
    this.play(Create(ax), Create(g), Create(c));
    let threw = false;
    try { this.setTheme("no-such-theme"); } catch (e) { threw = String(e).includes("no built-in named themes"); }
    check(threw, "unknown theme name explains itself");
  }
}
"##,
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    host.run("P", &cfg()).unwrap();
}

/// Seeking into a segment that predates a tracker, updater or redrawn mobject must
/// replay only the updaters active then (regression: "tracker 0 does not exist" and a
/// `World::get` index panic when seeking before them).
#[test]
fn seeking_before_later_updaters() {
    let d = dir("seek-late");
    let p = write(
        &d,
        "late.ts",
        r##"
import { Scene, Axes, ValueTracker, DecimalNumber, Create, alwaysRedraw } from "manim";
export class TrackerLater extends Scene {
  construct() {
    const axes = Axes({ xRange: [0, 4, 1], yRange: [0, 50, 10] });
    this.play(Create(axes), { runTime: 1.5 });
    const r = new ValueTracker(1);
    const n = alwaysRedraw(() => DecimalNumber(r.getValue()));
    this.add(n);
    this.play(r.animate.setValue(3), { runTime: 2 });
    this.wait(2);
  }
}
export class MobjectLater extends Scene {
  construct() {
    const r = new ValueTracker(1);
    const axes = Axes({ xRange: [0, 4, 1], yRange: [0, 50, 10] });
    this.play(Create(axes), { runTime: 1.5 });
    const n = alwaysRedraw(() => DecimalNumber(r.getValue()));
    this.add(n);
    this.play(r.animate.setValue(3), { runTime: 2 });
    this.wait(2);
  }
}
"##,
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    let mut c = cfg();
    c.record = true;
    for name in ["TrackerLater", "MobjectLater"] {
        host.run(name, &c).unwrap();
        let shown = |host: &ScriptHost| host.with_scene(|s| s.inspect().len()).unwrap();
        let late = host.frame_at(4.0).unwrap();
        let after = shown(&host);
        let early = host.frame_at(0.5).unwrap_or_else(|e| panic!("{name}: seek to 0.5 failed: {e:#}"));
        assert!(shown(&host) < after, "{name}: the number isn't on screen yet at 0.5 s");
        let mid = host.frame_at(2.5).unwrap();
        assert_eq!(shown(&host), after, "{name}: the number is back at 2.5 s");
        // Every time gives the same frame whatever was seeked before it.
        assert!(host.frame_at(0.5).unwrap() == early, "{name}: 0.5 s reproducible");
        assert!(host.frame_at(4.0).unwrap() == late, "{name}: 4.0 s reproducible");
        assert!(host.frame_at(2.5).unwrap() == mid, "{name}: 2.5 s reproducible");
        assert!(early != mid && mid != late);
    }
}

/// The text and morph animations keep the host contract Studio's retiming relies on:
/// one play per `play()`, one wait per `wait()`, and a play lasts exactly its `runTime`
/// (per-glyph lag stays inside it). Defaults follow Manim CE.
#[test]
fn text_and_morph_animations_keep_play_timing() {
    let d = dir("text-morph");
    let p = write(
        &d,
        "tm.ts",
        r##"
import { Scene, Text, MathTex, Rectangle, Write, Unwrite, AddTextLetterByLetter, RemoveTextLetterByLetter,
  TypeWithCursor, UntypeWithCursor, Transform, ReplacementTransform, TransformMatchingShapes,
  TransformMatchingTex, FadeTransform, FadeIn } from "manim";
export default class T extends Scene {
  construct() {
    const t = Text("hello there");
    this.play(Write(t), { runTime: 1.3 });
    this.play(Unwrite(t), { runTime: 0.7 });
    const u = Text("abc");
    this.play(AddTextLetterByLetter(u));                        // 3 letters: 0.3 s
    this.play(RemoveTextLetterByLetter(u, { runTime: 0.4 }));
    const cursor = Rectangle(0.05, 0.5);
    const v = Text("xy");
    this.play(TypeWithCursor(v, cursor, { timePerChar: 0.2 }));  // 0.4 s
    this.play(UntypeWithCursor(v, cursor, { leaveCursorOn: false }));
    const a = MathTex("a + b"), b = MathTex("b + a"), c = MathTex("b - a");
    this.add(a);
    this.play(TransformMatchingShapes(a, b), { runTime: 1.1 });
    this.play(TransformMatchingTex(b, c, { transformMismatches: true }));
    const w = Text("one");
    this.play(FadeIn(w));
    this.play(FadeTransform(w, Text("two")), { runTime: 0.9 });
    this.wait(0.5);
    this.play(Transform(c, Text("done")), ReplacementTransform(cursor, Text("!")), { runTime: 0.6 });
  }
}
"##,
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    let mut c = cfg();
    c.record = true;
    host.run("T", &c).unwrap();
    let tl = host.timeline().unwrap();
    let got: Vec<(String, f64, bool)> = tl.plays.iter().map(|p| (p.name.clone(), p.run_time, p.wait)).collect();
    let want = [
        ("Write", 1.3, false),
        ("Unwrite", 0.7, false),
        ("AddTextLetterByLetter", 0.3, false),
        ("RemoveTextLetterByLetter", 0.4, false),
        ("TypeWithCursor", 0.4, false),
        ("UntypeWithCursor", 0.2, false),
        ("TransformMatchingShapes", 1.1, false),
        ("TransformMatchingTex", 1.0, false),
        ("FadeIn", 1.0, false),
        ("FadeTransform", 0.9, false),
        ("Wait", 0.5, true),
        ("Transform, ReplacementTransform", 0.6, false),
    ];
    assert_eq!(got.len(), want.len(), "{got:?}");
    for ((name, rt, wait), (wn, wrt, ww)) in got.iter().zip(want) {
        assert_eq!((name.as_str(), *wait), (wn, ww));
        assert!((rt - wrt).abs() < 1e-9, "{name}: run time {rt}, expected {wrt}");
    }
    let fps = c.fps;
    for p in &tl.plays {
        let expect = if p.wait { (p.run_time * fps + 1e-6).floor() } else { (p.run_time * fps - 1e-6).ceil() };
        assert_eq!(p.frames as f64, expect, "{}: frames follow the run time", p.name);
    }
    // Every moment can be seeked to.
    for k in 0..=20 {
        host.frame_at(tl.duration * k as f64 / 20.0).unwrap();
    }
    let shown = host.with_scene(|s| s.inspect().len()).unwrap();
    assert_eq!(shown, 3, "\"two\", \"done\" and \"!\" remain; the unwritten, removed and untyped text is gone");
}

#[test]
fn group_cycles_are_errors_not_stack_overflows() {
    let d = dir("cycle");
    let p = write(
        &d,
        "cycle.ts",
        r##"
import { Scene, VGroup, Circle, Square, RIGHT } from "manim";
export class SelfAdd extends Scene {
  construct(): void {
    const g = VGroup(Circle());
    g.add(g);
    g.shift(RIGHT);
  }
}
export class Mutual extends Scene {
  construct(): void {
    const a = VGroup(Circle());
    const b = VGroup(Square());
    a.add(b);
    b.add(a);
  }
}
"##,
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    for scene in ["SelfAdd", "Mutual"] {
        let e = host.run(scene, &cfg()).unwrap_err();
        assert!(format!("{e:#}").contains("itself"), "{scene}: {e:#}");
    }
}

#[test]
fn native_panics_and_huge_counts_become_script_errors() {
    let d = dir("panics");
    let p = write(
        &d,
        "panics.ts",
        r##"
import { Scene, Circle, RegularPolygon } from "manim";
export default class P extends Scene {
  construct(): void {
    this.add(Circle().stretch(2, 5));
    this.add(RegularPolygon({ n: 1e10 }));
  }
}
"##,
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    host.run("P", &cfg()).unwrap();
}

#[test]
fn runs_and_hosts_do_not_share_handles_or_scenes() {
    let d = dir("isolation");
    let p = write(
        &d,
        "iso.ts",
        r##"
import { Scene, Circle, Square, FadeIn } from "manim";
const shared = Circle();
export class A extends Scene {
  construct(): void { this.play(FadeIn(shared)); }
}
export class B extends Scene {
  construct(): void { this.add(Square()); this.play(FadeIn(shared)); }
}
"##,
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    host.run("A", &cfg()).unwrap();
    host.run("B", &cfg()).unwrap();
    assert_eq!(host.with_scene(|s| s.mobjects().len()), Some(2));

    let other = ScriptHost::load(&p, HostOptions::default()).unwrap();
    other.run("A", &cfg()).unwrap();
    // Each host keeps its own scene.
    assert_eq!(host.with_scene(|s| s.mobjects().len()), Some(2));
    assert_eq!(other.with_scene(|s| s.mobjects().len()), Some(1));
}

#[test]
fn timeout_stops_long_native_waits() {
    let d = dir("timeout");
    let p = write(
        &d,
        "slow.ts",
        r##"
import { Scene, Dot } from "manim";
export default class Slow extends Scene {
  construct(): void { this.add(Dot()); this.wait(1e7); }
}
"##,
    );
    let host =
        ScriptHost::load(&p, HostOptions { timeout: Some(Duration::from_millis(300)), ..Default::default() }).unwrap();
    let t = std::time::Instant::now();
    assert!(host.run("Slow", &cfg()).is_err());
    assert!(t.elapsed() < Duration::from_secs(10), "took {:?}", t.elapsed());
}

#[test]
fn animating_a_member_of_an_offscreen_group_keeps_it_on_screen() {
    let d = dir("member");
    let p = write(
        &d,
        "member.ts",
        r##"
import { Scene, Text, VGroup, FadeIn, FadeOut, DOWN, ORIGIN } from "manim";
export class Member extends Scene {
  construct() {
    const a = Text("Plain").shift(DOWN);
    const b = Text("Grouped");
    const c = Text("Other");
    VGroup(b, c).arrange(DOWN).moveTo(ORIGIN).shift([0, 2, 0]);
    this.play(FadeIn(a), FadeIn(b));
    this.wait(0.5);
  }
}
export class Rejoin extends Scene {
  construct() {
    const b = Text("Grouped");
    const c = Text("Other");
    const g = VGroup(b, c).arrange(DOWN);
    this.play(FadeIn(b));
    this.add(g);
    this.wait(0.2);
  }
}
"##,
    );
    let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
    let mut c = cfg();
    c.record = true;
    let labels = |host: &ScriptHost| -> Vec<String> {
        host.with_scene(|s| s.inspect().into_iter().map(|i| i.label).collect()).unwrap()
    };

    host.run("Member", &c).unwrap();
    let l = labels(&host);
    assert!(l.iter().any(|x| x == "Grouped"), "after the play: {l:?}");
    host.seek(1.2).unwrap();
    assert!(labels(&host).iter().any(|x| x == "Grouped"), "during the wait after seeking");

    // When the group itself joins, the member goes back into it: drawn once, not twice.
    host.run("Rejoin", &c).unwrap();
    let (tops, members) = host
        .with_scene(|s| {
            let ids = s.mobjects();
            let g = &s[ids[0]];
            (ids.len(), g.children.iter().map(|m| m.text().unwrap_or("").to_string()).collect::<Vec<_>>())
        })
        .unwrap();
    assert_eq!(tops, 1, "the member was drawn twice");
    assert_eq!(members, ["Grouped", "Other"]);
}
