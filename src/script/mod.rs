//! TypeScript / JavaScript scene scripts on an embedded QuickJS engine.
//!
//! No Node, no Rust toolchain, no Python: a script is type-stripped (oxc) and run
//! in-process, and every frame is produced by the Rust engine.
//!
//! ```ts
//! import { Scene, Circle, Square, Create, Transform, FadeOut, PINK } from "manim";
//!
//! export default class SquareToCircle extends Scene {
//!   construct() {
//!     const sq = Square().rotate(Math.PI / 4);
//!     this.play(Create(sq));
//!     this.play(Transform(sq, Circle({ fillColor: PINK, fillOpacity: 0.5 })));
//!     this.play(FadeOut(sq));
//!   }
//! }
//! ```
//!
//! Scripts can import other files of the project (`import { title } from "./style"`), and
//! see only the `manim` module: no file system, network or process access. With
//! [`HostOptions::sandbox`], asset paths (images, SVGs, sounds, fonts) must stay inside a
//! folder.
//!
//! Hosts (an editor app, a preview pane) keep a [`ScriptHost`] to list scenes, render
//! them, or run one with [`Config::record`] and then [`ScriptHost::seek`] to any time.

mod native;
mod transpile;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context as _, Result};
use rquickjs::loader::{Loader, Resolver};
use rquickjs::{CatchResultExt, CaughtError, Context, Ctx, Function, Module, Object, Persistent, Runtime, Value};

use crate::config::Config;
use crate::scene::{Scene, TimelineInfo};

pub use transpile::{transpile, Transpiled};

/// The JS source of the `manim` module.
pub const PRELUDE: &str = include_str!("prelude.js");
/// TypeScript declarations for the `manim` module (give these to editors and agents).
pub const TYPES: &str = include_str!("manim.d.ts");

/// JS heap limit applied by default (the heap, not Rust-side mobject data).
pub const DEFAULT_MEMORY_LIMIT: usize = 512 << 20;

/// Limits and sandboxing for a [`ScriptHost`].
#[derive(Clone, Debug)]
pub struct HostOptions {
    /// Asset paths (images, SVGs, sounds, fonts, imported scripts) must stay in here.
    pub sandbox: Option<PathBuf>,
    /// JS heap limit in bytes ([`DEFAULT_MEMORY_LIMIT`] by default; `None` removes it).
    /// Mobject geometry lives on the Rust side and is bounded separately.
    pub memory_limit: Option<usize>,
    /// Time limit for each call into the script: loading it, [`ScriptHost::run`] (the whole
    /// `construct()`, including the frames it rasterizes and writes), `seek`, `frame_at`.
    /// The clock restarts on every call. [`ScriptHost::finish`] (encoder flush, audio
    /// mux) is not covered; stop that with [`ScriptHost::cancel_handle`].
    pub timeout: Option<Duration>,
}

impl Default for HostOptions {
    fn default() -> Self {
        HostOptions { sandbox: None, memory_limit: Some(DEFAULT_MEMORY_LIMIT), timeout: None }
    }
}

/// Let Typst compiles (equations) started inside `f` stop when the script is interrupted.
fn with_math_interrupt<R>(interrupts: &Arc<Interrupts>, f: impl FnOnce() -> R) -> R {
    #[cfg(feature = "math")]
    {
        let i = interrupts.clone();
        crate::math::with_interrupt(move || i.fired(), f)
    }
    #[cfg(not(feature = "math"))]
    {
        let _ = interrupts;
        f()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Everything that can stop a script: its own cancel flag and deadline, plus those the
/// host put on the [`Config`] it passed to `run`. Any of them stops it.
struct Interrupts {
    cancel: Arc<AtomicBool>,
    deadline: crate::config::Deadline,
    host: Mutex<(Option<Arc<AtomicBool>>, Option<crate::config::Deadline>)>,
}

impl Interrupts {
    fn fired(&self) -> bool {
        if self.cancel.load(Ordering::Relaxed) || lock(&self.deadline).is_some_and(|t| Instant::now() > t) {
            return true;
        }
        let host = lock(&self.host);
        host.0.as_ref().is_some_and(|c| c.load(Ordering::Relaxed))
            || host.1.as_ref().is_some_and(|d| lock(d).is_some_and(|t| Instant::now() > t))
    }
}

type Maps = Rc<RefCell<HashMap<String, Transpiled>>>;

struct ScriptResolver {
    sandbox: Option<PathBuf>,
    /// QuickJS drops resolver exceptions; keep the message to report it.
    last_error: Rc<RefCell<Option<String>>>,
}

impl ScriptResolver {
    fn fail(&self, ctx: &Ctx<'_>, msg: String) -> rquickjs::Error {
        *self.last_error.borrow_mut() = Some(msg.clone());
        rquickjs::Exception::throw_message(ctx, &msg)
    }
}

/// Canonical paths without the Windows verbatim prefix (nicer in messages and stack traces).
pub(crate) fn plain(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p,
    }
}

fn candidates(p: &Path) -> Vec<PathBuf> {
    let mut v = vec![p.to_path_buf()];
    for ext in ["ts", "js", "mjs"] {
        let mut s = p.as_os_str().to_owned();
        s.push(format!(".{ext}"));
        v.push(PathBuf::from(s));
    }
    v.push(p.join("index.ts"));
    v.push(p.join("index.js"));
    v
}

impl Resolver for ScriptResolver {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<rquickjs::loader::ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        if name == "manim" || name == "manim-rs" {
            return Ok("manim".into());
        }
        if !(name.starts_with("./") || name.starts_with("../")) {
            return Err(
                self.fail(ctx, format!("cannot import '{name}': scripts may import \"manim\" and relative files only"))
            );
        }
        let dir = Path::new(base).parent().unwrap_or(Path::new("."));
        let target = dir.join(name);
        let Some(found) = candidates(&target).into_iter().find(|p| p.is_file()) else {
            return Err(self.fail(ctx, format!("cannot find module '{name}' (from {base})")));
        };
        let found = plain(found.canonicalize().unwrap_or(found));
        if let Some(root) = &self.sandbox {
            let root = plain(root.canonicalize().unwrap_or_else(|_| root.clone()));
            if !found.starts_with(&root) {
                return Err(self.fail(ctx, format!("'{name}' is outside the project folder")));
            }
        }
        Ok(found.to_string_lossy().into_owned())
    }
}

struct ScriptLoader {
    maps: Maps,
}

impl Loader for ScriptLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<rquickjs::loader::ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, rquickjs::module::Declared>> {
        if name == "manim" {
            return Module::declare(ctx.clone(), "manim", PRELUDE);
        }
        let src = std::fs::read_to_string(name)
            .map_err(|e| rquickjs::Exception::throw_message(ctx, &format!("reading {name}: {e}")))?;
        let t =
            transpile(&src, Path::new(name)).map_err(|e| rquickjs::Exception::throw_message(ctx, &format!("{e:#}")))?;
        let code = t.code.clone();
        self.maps.borrow_mut().insert(name.to_string(), t);
        Module::declare(ctx.clone(), name, code)
    }
}

/// Rewrite `file:line` references in a JS error to point at the TypeScript source.
fn map_lines(text: &str, maps: &Maps) -> String {
    let maps = maps.borrow();
    let mut out = text.to_string();
    for (file, t) in maps.iter() {
        if t.lines.is_empty() {
            continue;
        }
        let mut result = String::new();
        let mut rest = out.as_str();
        let pat = format!("{file}:");
        while let Some(i) = rest.find(&pat) {
            result.push_str(&rest[..i + pat.len()]);
            rest = &rest[i + pat.len()..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(line) = digits.parse::<u32>() {
                result.push_str(&t.source_line(line).to_string());
                rest = &rest[digits.len()..];
                // Columns refer to the generated code; drop them.
                if rest.starts_with(':') {
                    let col: String = rest[1..].chars().take_while(|c| c.is_ascii_digit()).collect();
                    if !col.is_empty() {
                        rest = &rest[1 + col.len()..];
                    }
                }
            }
        }
        result.push_str(rest);
        out = result;
    }
    out
}

fn caught(err: CaughtError<'_>, maps: &Maps) -> anyhow::Error {
    let text = match err {
        CaughtError::Exception(e) => {
            let msg = e.message().unwrap_or_default();
            let stack = e.stack().unwrap_or_default();
            if stack.is_empty() {
                msg
            } else {
                format!("{msg}\n{}", stack.trim_end())
            }
        }
        CaughtError::Value(v) => format!("thrown: {v:?}"),
        CaughtError::Error(e) => e.to_string(),
    };
    let mapped = map_lines(&text, maps);
    match source_excerpt(&mapped, maps) {
        Some(ex) => anyhow!("{mapped}\n{ex}"),
        None => anyhow!("{mapped}"),
    }
}

/// The source line of the first `file:line` in an error, so a reader sees the code at once.
fn source_excerpt(text: &str, maps: &Maps) -> Option<String> {
    let maps = maps.borrow();
    let mut best: Option<(usize, &str, u32)> = None;
    for file in maps.keys() {
        let pat = format!("{file}:");
        if let Some(i) = text.find(&pat) {
            let digits: String = text[i + pat.len()..].chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(line) = digits.parse::<u32>() {
                if best.is_none_or(|b| i < b.0) {
                    best = Some((i, file.as_str(), line));
                }
            }
        }
    }
    let (_, file, line) = best?;
    let src = std::fs::read_to_string(file).ok()?;
    let code = src.lines().nth(line.checked_sub(1)? as usize)?;
    Some(format!("  {line} | {}", code.trim_end()))
}

const CANCELLED: &str = "cancelled";

/// One loaded script.
pub struct ScriptHost {
    // Fields drop in order: JS values first, then the context, then the runtime.
    namespace: Persistent<Object<'static>>,
    context: Context,
    _runtime: Runtime,
    path: PathBuf,
    /// (scene name, export key)
    scenes: Vec<(String, String)>,
    maps: Maps,
    interrupts: Arc<Interrupts>,
    options: HostOptions,
    /// This script's scene, handle table and paths (swapped in for every call).
    state: RefCell<native::State>,
    /// Handle table after the top level ran; each run starts from it.
    initial_handles: Vec<native::Slot>,
}

impl ScriptHost {
    /// Load (type-check-free compile and evaluate) a `.ts` or `.js` scene script.
    pub fn load(path: impl AsRef<Path>, options: HostOptions) -> Result<Self> {
        let path = path.as_ref();
        let path = plain(path.canonicalize().with_context(|| format!("{} does not exist", path.display()))?);
        let source = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let name = path.to_string_lossy().into_owned();
        let t = transpile(&source, &path)?;
        let code = t.code.clone();
        let maps: Maps = Rc::new(RefCell::new(HashMap::new()));
        maps.borrow_mut().insert(name.clone(), t);

        let runtime = Runtime::new()?;
        if let Some(limit) = options.memory_limit {
            runtime.set_memory_limit(limit);
        }
        runtime.set_max_stack_size(512 * 1024);
        let interrupts = Arc::new(Interrupts {
            cancel: Arc::new(AtomicBool::new(false)),
            deadline: Default::default(),
            host: Mutex::new((None, None)),
        });
        {
            let i = interrupts.clone();
            runtime.set_interrupt_handler(Some(Box::new(move || i.fired())));
        }
        let import_error: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        runtime.set_loader(
            ScriptResolver { sandbox: options.sandbox.clone(), last_error: import_error.clone() },
            ScriptLoader { maps: maps.clone() },
        );
        let context = Context::full(&runtime)?;
        let base_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let state = RefCell::new(native::State::default());
        {
            let mut st = state.borrow_mut();
            st.base_dir = base_dir;
            st.sandbox = options.sandbox.clone();
        }

        if let Some(t) = options.timeout {
            *lock(&interrupts.deadline) = Some(Instant::now() + t);
        }
        let loaded = with_math_interrupt(&interrupts, || {
            native::with_host_state(&state, || {
                context.with(|ctx| -> Result<_> {
                    native::install(&ctx).map_err(|e| anyhow!("installing natives: {e}"))?;
                    let module =
                        Module::declare(ctx.clone(), name.clone(), code).catch(&ctx).map_err(|e| caught(e, &maps))?;
                    let (module, promise) = module.eval().catch(&ctx).map_err(|e| caught(e, &maps))?;
                    promise.finish::<()>().catch(&ctx).map_err(|e| caught(e, &maps))?;
                    // A failed import can leave the exception pending without rejecting the promise.
                    let pending = ctx.catch();
                    if !matches!(
                        pending.type_of(),
                        rquickjs::Type::Uninitialized | rquickjs::Type::Undefined | rquickjs::Type::Null
                    ) {
                        let err = match pending.as_exception() {
                            Some(e) => CaughtError::Exception(e.clone()),
                            None => CaughtError::Value(pending),
                        };
                        return Err(caught(err, &maps));
                    }
                    let ns = module.namespace()?;
                    let is_scene: Option<Function> = ctx.globals().get("__manim_isScene")?;
                    let Some(is_scene) = is_scene else {
                        bail!("{} does not import from \"manim\"", name);
                    };
                    let mut scenes = Vec::new();
                    for key in ns.keys::<String>() {
                        let key = key?;
                        let v: Value = ns.get(&key)?;
                        if is_scene.call::<_, bool>((v.clone(),))? {
                            let f: Function = v.get()?;
                            let fname: String = f.get::<_, Option<String>>("name")?.unwrap_or_default();
                            let scene_name = if key == "default" && !fname.is_empty() { fname } else { key.clone() };
                            scenes.push((scene_name, key));
                        }
                    }
                    // Namespace keys come sorted; list scenes in source order instead.
                    let pos = |key: &str, name: &str| -> usize {
                        let pats = if key == "default" {
                            vec!["export default".to_string()]
                        } else {
                            vec![
                                format!("class {key}"),
                                format!("const {key}"),
                                format!("let {key}"),
                                format!("function {key}"),
                            ]
                        };
                        pats.iter()
                            .chain([&format!("class {name}")])
                            .filter_map(|p| source.find(p.as_str()))
                            .min()
                            .unwrap_or(usize::MAX)
                    };
                    scenes.sort_by_key(|(name, key)| pos(key, name));
                    Ok((scenes, Persistent::save(&ctx, ns)))
                })
            })
        });
        let (scenes, namespace) = match (loaded, import_error.borrow_mut().take()) {
            (Ok(v), _) => v,
            (Err(_), Some(import)) => bail!("{}: {import}", path.display()),
            (Err(e), None) => return Err(e),
        };
        *lock(&interrupts.deadline) = None;
        if scenes.is_empty() {
            bail!(
                "{} exports no scenes (export a class extending Scene, or scene(\"Name\", s => ...))",
                path.display()
            );
        }
        let initial_handles = state.borrow().handles();
        Ok(ScriptHost {
            namespace,
            context,
            _runtime: runtime,
            path,
            scenes,
            maps,
            interrupts,
            options,
            state,
            initial_handles,
        })
    }

    /// Run `f` with this script's state active and its deadline armed. A cancel that stopped
    /// the call is consumed here (the next call starts clean); one set before the call
    /// stops it immediately.
    fn enter<R>(&self, f: impl FnOnce() -> Result<R>) -> Result<R> {
        *lock(&self.interrupts.deadline) = self.options.timeout.map(|t| Instant::now() + t);
        let r = with_math_interrupt(&self.interrupts, || native::with_host_state(&self.state, f));
        *lock(&self.interrupts.deadline) = None;
        match r {
            Err(_) if self.interrupts.cancel.swap(false, Ordering::Relaxed) => Err(anyhow!(CANCELLED)),
            r => r,
        }
    }

    /// Path of the script this host loaded.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Scene names, in export order.
    pub fn scenes(&self) -> Vec<String> {
        self.scenes.iter().map(|s| s.0.clone()).collect()
    }

    /// Set from any thread to stop a running script.
    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        self.interrupts.cancel.clone()
    }

    /// Expose JSON data to scripts as `globalThis[name]` (e.g. measured narration lengths).
    pub fn set_global_json(&self, name: &str, json: &str) -> Result<()> {
        let v: serde_json::Value = serde_json::from_str(json).context("set_global_json: invalid JSON")?;
        let src = format!("globalThis[{}] = {};", serde_json::to_string(name)?, v);
        self.enter(|| self.context.with(|ctx| ctx.eval::<(), _>(src).catch(&ctx).map_err(|e| caught(e, &self.maps))))
    }

    /// Evaluate a JS expression and return its JSON value (for hosts and tests).
    pub fn eval_json<T: serde::de::DeserializeOwned>(&self, expr: &str) -> Result<T> {
        let src = format!("JSON.stringify({expr})");
        let s: String = self.enter(|| {
            self.context.with(|ctx| ctx.eval::<String, _>(src).catch(&ctx).map_err(|e| caught(e, &self.maps)))
        })?;
        serde_json::from_str(&s).context("eval_json: result is not JSON")
    }

    fn key_for(&self, scene: &str) -> Result<String> {
        self.scenes
            .iter()
            .find(|s| s.0 == scene)
            .map(|s| s.1.clone())
            .ok_or_else(|| anyhow!("no scene named '{scene}' (scenes: {})", self.scenes().join(", ")))
    }

    /// Run a scene's `construct` against a fresh [`Scene`]. The scene stays loaded for
    /// [`seek`](Self::seek), [`timeline`](Self::timeline) and [`finish`](Self::finish).
    pub fn run(&self, scene: &str, config: &Config) -> Result<()> {
        let key = self.key_for(scene)?;
        let mut config = config.clone();
        // The host's own `cancel` / `deadline` stay in force; this script's join them.
        *lock(&self.interrupts.host) = (config.cancel.clone(), config.deadline.clone());
        config.script_cancel = Some(self.interrupts.cancel.clone());
        config.script_deadline = Some(self.interrupts.deadline.clone());
        // Drop the previous scene (and its video writer) before starting the next.
        self.state.borrow_mut().scene = None;
        let result = self.enter(|| {
            let s = Scene::new(&config, scene)?;
            native::with_state(|st| {
                st.reset_for_run(&self.initial_handles);
                st.scene = Some(s);
            });
            self.context.with(|ctx| -> Result<()> {
                let ns = self.namespace.clone().restore(&ctx)?;
                let cls: Value = ns.get(&key)?;
                let run: Function = ctx.globals().get("__manim_run")?;
                run.call::<_, ()>((cls,)).catch(&ctx).map_err(|e| caught(e, &self.maps))
            })
        });
        match result {
            Err(e) if e.to_string() == CANCELLED => Err(anyhow!("render cancelled")),
            Err(e) => Err(e.context(format!("in scene {scene}"))),
            Ok(()) => Ok(()),
        }
    }

    /// Finish the loaded scene: flush video, add audio, write side files.
    pub fn finish(&self) -> Result<()> {
        let mut s = self.state.borrow_mut().scene.take().ok_or_else(|| anyhow!("no scene has run"))?;
        let started = s.created_at();
        let r = with_math_interrupt(&self.interrupts, || s.finish(started));
        self.state.borrow_mut().scene = Some(s);
        if r.is_err() {
            // A cancel that stopped the render is spent; don't cancel the next one too.
            self.interrupts.cancel.store(false, Ordering::Relaxed);
        }
        r
    }

    /// Run and finish a scene (a complete render).
    pub fn render(&self, scene: &str, config: &Config) -> Result<()> {
        let events = crate::events::Events::new(config.progress);
        let r = self.run(scene, config).and_then(|_| self.finish());
        if let Err(e) = &r {
            events.emit("error", serde_json::json!({ "scene": scene, "message": format!("{e:#}") }), || None);
        }
        r
    }

    /// Jump the loaded scene to time `t` (it must have run with `Config::record`): the state
    /// shown is exported frame `floor(t * fps)`.
    /// Script updaters run again for the frames replayed.
    pub fn seek(&self, t: f64) -> Result<()> {
        self.enter(|| {
            self.context.with(|ctx| -> Result<()> {
                let f: Function = ctx.globals().get("__manim_seek")?;
                f.call::<_, ()>((t,)).catch(&ctx).map_err(|e| caught(e, &self.maps))
            })
        })
    }

    /// The loaded scene's frame at `t`, rasterized on the CPU (RGBA8).
    pub fn frame_at(&self, t: f64) -> Result<Vec<u8>> {
        self.seek(t)?;
        self.with_scene(|s| s.render_frame()).ok_or_else(|| anyhow!("no scene has run"))
    }

    /// Plays, markers and sounds of the scene that last ran, or `None` before the first run.
    pub fn timeline(&self) -> Option<TimelineInfo> {
        self.with_scene(|s| s.timeline())
    }

    /// Access the loaded scene (inspect, markers, problems, ...).
    pub fn with_scene<R>(&self, f: impl FnOnce(&mut Scene) -> R) -> Option<R> {
        self.state.borrow_mut().scene.as_mut().map(f)
    }
}

/// Render every scene of a script with command-line style options (used by `manim-rs`).
pub fn render_script(path: &Path, config: &Config, options: HostOptions) -> Result<()> {
    let host = ScriptHost::load(path, options)?;
    if config.list_scenes {
        for s in host.scenes() {
            println!("{s}");
        }
        return Ok(());
    }
    let mut ran = false;
    for name in host.scenes() {
        if config.scene_filter.as_deref().is_some_and(|f| f != name) {
            continue;
        }
        host.render(&name, config)?;
        ran = true;
    }
    if !ran {
        bail!(
            "no scene named '{}' (scenes: {})",
            config.scene_filter.clone().unwrap_or_default(),
            host.scenes().join(", ")
        );
    }
    Ok(())
}
