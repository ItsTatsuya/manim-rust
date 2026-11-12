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
