//! `manim-rs`: render a scene script directly, like `manim scene.py`.
//!
//! Rust has no interpreter, so the script is still compiled. This runner does it out of
//! sight: it builds the script against the engine in a private cache directory, runs it
//! with the output pointed next to the script, then deletes the compiled executable.
//! Your folder ends up with only `scene.rs` and the video.
//!
//! The compiled engine library stays cached, so the first render takes a couple of
//! minutes and later renders only recompile your script (a few seconds).
//!
//! ```text
//! manim-rs scene.rs -qh            render (any manim flag works: -s, --format gif, ...)
//! manim-rs new intro.rs            create a scene script from a template
//! manim-rs clean                   delete the build cache
//! ```

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const HELP: &str = "\
manim-rs: render manim-rust scene scripts without leaving build artifacts behind

USAGE:
  manim-rs <script.ts> [manim options]   Render a TypeScript/JavaScript scene (no compiler needed)
  manim-rs <script.rs> [manim options]   Render; the video is written next to the script
  manim-rs render <script.rs> [options]  Same as above
  manim-rs new <script.ts|.rs>           Create a new scene script from a template
  manim-rs types                         Print the TypeScript declarations of the manim module
  manim-rs clean                         Delete the build cache
  manim-rs cache                         Print the cache location

MANIM OPTIONS (passed to the scene):
  -ql -qm -qh -qp -qk, -s, --renderer auto|cpu|gpu, --format auto|mp4|gif|png,
  --encoder auto|x264|nvenc, --fps N, -r WxH, -o PATH, --media_dir DIR

ENVIRONMENT:
  MANIM_RS_CACHE    Build cache location (default: your OS cache directory)
  MANIM_RS_SOURCE   Path to a manim-rust checkout to build scenes against. Default: the
                    source this runner was built from; if that directory no longer exists
                    (e.g. after `cargo install --git`), the GitHub repository is used instead.
";

/// The engine source this runner was built from (may be gone by run time).
const BUILT_FROM: &str = env!("CARGO_MANIFEST_DIR");

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("manim-rs: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(mut args: Vec<String>) -> Result<ExitCode, String> {
    match args.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => {
            print!("{HELP}");
            Ok(ExitCode::SUCCESS)
        }
        Some("clean") => {
            let dir = cache_dir();
            if dir.exists() {
                fs::remove_dir_all(&dir).map_err(|e| format!("could not delete {}: {e}", dir.display()))?;
            }
            println!("manim-rs: removed {}", dir.display());
            Ok(ExitCode::SUCCESS)
        }
        Some("cache") => {
            println!("{}", cache_dir().display());
            Ok(ExitCode::SUCCESS)
        }
        Some("new") => {
            let path = args.get(1).ok_or("usage: manim-rs new <script.ts|script.rs>")?;
            new_script(Path::new(path))?;
            Ok(ExitCode::SUCCESS)
        }
        Some("types") => {
            #[cfg(feature = "script")]
            print!("{}", manim::script::TYPES);
            Ok(ExitCode::SUCCESS)
        }
        Some("render") => {
            args.remove(0);
            render(args)
        }
        Some(a) if a.ends_with(".rs") => render(args),
        Some(a) if is_script(a) => render_script(args),
        Some(other) => Err(format!("unknown command or not a .ts/.js/.rs file: '{other}' (see manim-rs --help)")),
    }
}

fn is_script(a: &str) -> bool {
    [".ts", ".mts", ".js", ".mjs"].iter().any(|e| a.ends_with(e))
}

/// TypeScript / JavaScript scenes run in-process on the embedded engine: no compiler.
#[cfg(feature = "script")]
fn render_script(args: Vec<String>) -> Result<ExitCode, String> {
    let script = canonical(Path::new(&args[0])).map_err(|e| format!("cannot open {}: {e}", args[0]))?;
    let script_dir = script.parent().ok_or("script has no parent directory")?.to_path_buf();
    let mut rest: Vec<String> = args[1..].to_vec();
    let mut options = manim::script::HostOptions::default();
    if let Some(i) = rest.iter().position(|a| a == "--sandbox") {
        let dir = rest.get(i + 1).cloned().ok_or("--sandbox needs a folder")?;
        options.sandbox = Some(PathBuf::from(dir));
        rest.drain(i..=i + 1);
    }
    if let Some(i) = rest.iter().position(|a| a == "--timeout") {
        let secs: f64 = rest.get(i + 1).and_then(|s| s.parse().ok()).ok_or("--timeout needs seconds")?;
        let t = std::time::Duration::try_from_secs_f64(secs)
            .map_err(|_| "--timeout needs a non-negative number of seconds")?;
        options.timeout = Some(t);
        rest.drain(i..=i + 1);
    }
    if !rest.iter().any(|a| matches!(a.as_str(), "-o" | "--output" | "--media_dir")) {
        rest.push("-o".into());
        rest.push(script_dir.to_string_lossy().into_owned());
    }
    let config = manim::config::Config::parse(rest).map_err(|e| format!("{e:#}"))?;
    if config.help {
        println!("{}", manim::config::HELP);
        return Ok(ExitCode::SUCCESS);
    }
    match manim::script::render_script(&script, &config, options) {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(e) => {
            if config.progress != manim::events::ProgressMode::Json {
                eprintln!("manim-rs: {e:#}");
            }
            Ok(ExitCode::FAILURE)
        }
    }
}

#[cfg(not(feature = "script"))]
fn render_script(_args: Vec<String>) -> Result<ExitCode, String> {
    Err("this manim-rs was built without the `script` feature".into())
}

fn render(args: Vec<String>) -> Result<ExitCode, String> {
    let script = args.first().ok_or("usage: manim-rs <script.rs> [options]")?;
    let script = canonical(Path::new(script)).map_err(|e| format!("cannot open {script}: {e}"))?;
    let script_dir = script.parent().ok_or("script has no parent directory")?.to_path_buf();
    let engine = engine_source()?;

    let cache = cache_dir();
    let id = format!("{:016x}", fnv1a(script.to_string_lossy().as_bytes()));
    let project = cache.join("scenes").join(&id);
    let target = cache.join("target");
    let bin = format!("scene-{id}");
    fs::create_dir_all(&project).map_err(|e| format!("cannot create {}: {e}", project.display()))?;

    // A small cargo project whose binary *is* the user's script (errors point at their file).
    let manifest = format!(
        r#"# Generated by manim-rs for {script}
[package]
name = "{bin}"
version = "0.0.0"
edition = "2021"
publish = false

[[bin]]
name = "{bin}"
path = "{script_toml}"

[dependencies]
manim = {{ {engine_dep} }}

[profile.release]
incremental = true

[workspace]
"#,
        script = script.display(),
        script_toml = toml_path(&script),
        engine_dep = engine.dependency(),
    );
    write_if_changed(&project.join("Cargo.toml"), &manifest)?;
    let lock = project.join("Cargo.lock");
    if let (false, Engine::Path(dir)) = (lock.exists(), &engine) {
        // Start from the engine's tested dependency versions.
        let _ = fs::copy(dir.join("Cargo.lock"), &lock);
    }

    remove_executables(&target, &id); // leftovers from an interrupted run
    if !target.join("release").exists() {
        eprintln!("manim-rs: first run, so compiling the engine into {} (one-time, ~2 min)", cache.display());
    }
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(&cargo)
        .args(["build", "--release", "--quiet", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target)
        .status()
        .map_err(|e| format!("could not run cargo ({e}); is Rust installed?"))?;
    if !status.success() {
        remove_executables(&target, &id);
        return Ok(ExitCode::FAILURE);
    }

    let exe = target.join("release").join(format!("{bin}{}", env::consts::EXE_SUFFIX));
    let mut scene_args: Vec<String> = args[1..].to_vec();
    let has_output = scene_args.iter().any(|a| matches!(a.as_str(), "-o" | "--output" | "--media_dir"));
    if !has_output {
        scene_args.push("-o".into());
        scene_args.push(script_dir.to_string_lossy().into_owned());
    }
    let result = Command::new(&exe).args(&scene_args).current_dir(&script_dir).status();
    remove_executables(&target, &id);
    let status = result.map_err(|e| format!("could not run the compiled scene: {e}"))?;
    Ok(match status.code() {
        Some(0) => ExitCode::SUCCESS,
        Some(c) => ExitCode::from(c.clamp(1, 255) as u8),
        None => ExitCode::FAILURE,
    })
}

/// Delete the compiled scene executable (and debug info) for script `id`. The engine's
/// cached library artifacts are kept so the next build is fast.
fn remove_executables(target: &Path, id: &str) {
    let release = target.join("release");
    let bin = format!("scene-{id}");
    let crate_name = format!("scene_{id}");
    for name in [format!("{bin}{}", env::consts::EXE_SUFFIX), format!("{crate_name}.pdb"), format!("{bin}.d")] {
        let _ = fs::remove_file(release.join(name));
    }
    // cargo also keeps an intermediate copy, and where depends on its version: deps/
    // (`scene_<id>.exe` on Windows, `scene_<id>-<hash>` elsewhere) up to cargo 1.99, and
    // build/scene-<id>/<hash>/out/ from 1.100 on. Sweep both; the id is unique per script,
    // so the engine's cached library artifacts are never touched.
    let is_ours = |path: &Path| {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let ours = [&crate_name, &bin].iter().any(|n| stem == n.as_str() || stem.starts_with(&format!("{n}-")));
        ours && matches!(path.extension().and_then(|x| x.to_str()), None | Some("exe" | "pdb"))
    };
    remove_files(&release.join("deps"), false, &is_ours);
    remove_files(&release.join("build").join(&bin), true, &is_ours);
}

/// Delete the files in `dir` (and, with `recursive`, its subfolders) that `pick` selects.
fn remove_files(dir: &Path, recursive: bool, pick: &dyn Fn(&Path) -> bool) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            if recursive {
                remove_files(&path, true, pick);
            }
        } else if pick(&path) {
            let _ = fs::remove_file(&path);
        }
    }
}

fn new_script(path: &Path) -> Result<(), String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !matches!(ext, "rs" | "ts" | "js") {
        return Err("the script name must end in .ts, .js or .rs".into());
    }
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Scene");
    let scene: String = stem
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect();
    let scene = if scene.is_empty() { "Scene".to_string() } else { scene };
    let template = if ext == "rs" { TEMPLATE } else { TS_TEMPLATE };
    let body = template.replace("{SCENE}", &scene).replace("{FILE}", &path.display().to_string());
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(path, body).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    println!("manim-rs: created {}. Render it with: manim-rs {} -ql", path.display(), path.display());
    Ok(())
}

const TEMPLATE: &str = r#"//! Render with: manim-rs {FILE} -ql   (use -qh for 1080p60)
use manim::prelude::*;

fn main() -> manim::anyhow::Result<()> {
    manim::render("{SCENE}", |s| {
        let title = s.insert(Text::new("Hello, manim-rs!").set_color(BLUE));
        s.play(Write::new(title));

        let circle = s.insert(Circle::new(1.5).set_fill(PINK, 0.5).next_to(s.get(title), DOWN));
        s.play(Create::new(circle));
        s.play(circle.animate(|m| m.shift(RIGHT * 3.0).set_color(YELLOW)));
        s.wait(1.0);
    })
}
"#;

const TS_TEMPLATE: &str = r#"// Render with: manim-rs {FILE} -ql   (use -qh for 1080p60)
// Editor types: manim-rs types > manim.d.ts
import { Scene, Text, Circle, Write, Create, FadeOut, BLUE, PINK, YELLOW, DOWN, RIGHT } from "manim";

export default class {SCENE} extends Scene {
  construct() {
    const title = Text("Hello, manim-rs!", { color: BLUE });
    this.play(Write(title));

    const circle = Circle(1.5, { fillColor: PINK, fillOpacity: 0.5 }).nextTo(title, DOWN);
    this.play(Create(circle));
    this.play(circle.animate.shift(RIGHT.mul(3)).setColor(YELLOW));
    this.wait(1);
    this.play(FadeOut(title), FadeOut(circle));
  }
}
"#;

/// Where the scene project gets the `manim` crate from.
enum Engine {
    /// A source checkout (this runner's own, or `MANIM_RS_SOURCE`).
    Path(PathBuf),
    /// The GitHub repository (the crate is not published to crates.io).
    Git(&'static str),
}

impl Engine {
    /// The right-hand side of the `manim = { ... }` dependency entry.
    fn dependency(&self) -> String {
        match self {
            Engine::Path(dir) => format!(r#"path = "{}""#, toml_path(dir)),
            Engine::Git(url) => format!(r#"git = "{url}""#),
        }
    }
}

fn engine_source() -> Result<Engine, String> {
    if let Some(over) = env::var_os("MANIM_RS_SOURCE") {
        let src = PathBuf::from(over);
        if !src.join("Cargo.toml").is_file() {
            return Err(format!(
                "MANIM_RS_SOURCE={} has no Cargo.toml; point it at a manim-rust checkout or unset it",
                src.display()
            ));
        }
        return canonical(&src).map(Engine::Path).map_err(|e| e.to_string());
    }
    let built = Path::new(BUILT_FROM);
    if built.join("Cargo.toml").is_file() {
        return canonical(built).map(Engine::Path).map_err(|e| e.to_string());
    }
    // Installed with `cargo install --git` (or the build tree was deleted): use the repository.
    Ok(Engine::Git(env!("CARGO_PKG_REPOSITORY")))
}

fn cache_dir() -> PathBuf {
    if let Some(p) = env::var_os("MANIM_RS_CACHE") {
        return PathBuf::from(p);
    }
    let base = if cfg!(windows) {
        env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        env::var_os("HOME").map(|h| PathBuf::from(h).join("Library").join("Caches"))
    } else {
        env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    };
    base.unwrap_or_else(env::temp_dir).join("manim-rs")
}

/// Canonical path without Windows' `\\?\` prefix (cargo and TOML don't need it).
fn canonical(p: &Path) -> std::io::Result<PathBuf> {
    let c = fs::canonicalize(p)?;
    let s = c.to_string_lossy();
    Ok(match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => c,
    })
}

fn toml_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").replace('"', "\\\"")
}

fn write_if_changed(path: &Path, contents: &str) -> Result<(), String> {
    if fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(()); // keep the mtime so cargo doesn't rebuild
    }
    fs::write(path, contents).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}
