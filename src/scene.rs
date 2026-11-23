//! The scene: owns the world, runs animations on a timeline and feeds frames to the
//! renderer.
//!
//! Performance notes:
//! - Scene logic runs on this thread while a worker rasterizes and encodes (see
//!   [`crate::pipeline`]); rasterizing runs in batches, in parallel on the CPU or
//!   pipelined on the GPU.
//! - Consecutive identical frames (waits, static moments) are rendered once and repeated.
//! - With `-s`, intermediate frames are never rasterized at all; with `--dry-run` no
//!   renderer is even created.
//!
//! Host applications can drive a scene frame by frame ([`Scene::begin_play`],
//! [`Scene::step`], [`Scene::end_frame`]) to run their own per-frame code, and with
//! [`Config::record`] jump to any time afterwards ([`Scene::seek`]).

use std::ops::{Index, IndexMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use serde_json::json;

use crate::animation::{Animation, AnimationGroup, IntoAnimations};
use crate::audio::{AudioSource, Sound, SoundInfo};
use crate::camera::Camera;
use crate::check::{self, Checker, Problem};
use crate::config::{Config, SnapshotAt};
use crate::events::Events;
use crate::mobject::{VMobject, MAX_NESTING};
use crate::output;
use crate::pipeline::Pipeline;
use crate::renderer::{self, DisplayList};
use crate::theme::Theme;
use crate::world::{MobjectId, TrackerId, World};

/// Handle to an updater registered with `Scene::add_updater`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UpdaterId(usize);

type Updater = Box<dyn FnMut(&mut World, f64)>;

struct UpdaterSlot {
    id: UpdaterId,
    f: Updater,
    active: bool,
}

/// A named moment on the timeline (`mark`) or the start of a chapter (`section`).
#[derive(Clone, Debug, Serialize)]
pub struct Marker {
    /// Marker or section name.
    pub name: String,
    /// Scene time in seconds.
    pub time: f64,
    /// `"marker"` or `"section"`.
    pub kind: String,
}

/// One `play` or `wait` as it happened.
#[derive(Clone, Debug, Serialize)]
pub struct PlayInfo {
    /// Position in the play log.
    pub index: usize,
    /// Animation name (`Wait` for waits).
    pub name: String,
    /// Scene time in seconds when it starts.
    pub start: f64,
    /// Duration in seconds.
    pub run_time: f64,
    /// Number of frames it spans.
    pub frames: usize,
    /// True for `wait`, false for `play`.
    pub wait: bool,
}

/// Everything a host needs to know about a finished (or running) scene's timeline.
#[derive(Clone, Debug, Serialize)]
pub struct TimelineInfo {
    /// Scene name.
    pub scene: String,
    /// Frames per second.
    pub fps: f64,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Total length in seconds.
    pub duration: f64,
    /// Every `play` and `wait`, in order.
    pub plays: Vec<PlayInfo>,
    /// Markers and sections, in order.
    pub markers: Vec<Marker>,
    /// Sounds placed on the timeline.
    pub sounds: Vec<SoundInfo>,
    /// Layout problems found (needs `Config::check`).
    pub problems: Vec<Problem>,
}

/// A recorded keyframe: the world before a play/wait and a fresh copy of its animation.
struct Segment {
    start: f64,
    frames: usize,
    anim: Option<AnimationGroup>,
    world: World,
    view: CameraView,
    updaters: Vec<UpdaterId>,
}

/// The parts of the camera a scene can change mid-timeline; recorded with each keyframe so
/// seeking shows the same framing and background as the exported video.
#[derive(Clone, Copy)]
struct CameraView {
    frame_height: f64,
    frame_center: crate::constants::Point,
    background: crate::color::Color,
}

impl CameraView {
    fn of(c: &Camera) -> Self {
        CameraView { frame_height: c.frame_height, frame_center: c.frame_center, background: c.background }
    }
    fn apply(self, c: &mut Camera) {
        c.frame_height = self.frame_height;
        c.frame_center = self.frame_center;
        c.background = self.background;
    }
}

/// A `play`/`wait` in progress (see [`Scene::begin_play`]).
pub struct Playback {
    anim: Option<AnimationGroup>,
    frames: usize,
    i: usize,
    /// Stop after this many frames (less than `frames` when seeking into the middle).
    stop: usize,
    dt: f64,
    replay: bool,
    updaters: Option<Vec<UpdaterId>>,
    play_index: usize,
    finished: bool,
}

impl Playback {
    /// Total frames of this play.
    pub fn frames(&self) -> usize {
        self.frames
    }
    /// Frames produced so far.
    pub fn frame(&self) -> usize {
        self.i
    }
    /// Whether every frame of this play has been produced.
    pub fn is_finished(&self) -> bool {
        self.finished
    }
    /// Index of the play/wait (and recorded segment) this playback belongs to.
    pub fn segment(&self) -> usize {
        self.play_index
    }
    /// Whether this is a seek replay (no frames are written).
    pub fn is_replay(&self) -> bool {
        self.replay
    }
}

/// A scene being built and rendered.
///
/// Not `Send`: updaters and `.animate` targets hold non-thread-safe closures and `Rc`s, so
/// build, play and seek a scene on one thread (a host keeps it on a dedicated worker and
/// talks to it over a channel). Default styles set by [`Scene::set_theme`] are
/// thread-local and reset by [`Scene::new`], so build mobjects on that same thread.
pub struct Scene {
    name: String,
    world: World,
    updaters: Vec<UpdaterSlot>,
    next_updater: usize,
    camera: Camera,
    pipeline: Option<Pipeline>,
    config: Config,
    time: f64,
    duration: f64,
    error: Option<anyhow::Error>,
    events: Events,
    theme: Option<Theme>,
    pattern: Option<MobjectId>,
    markers: Vec<Marker>,
    sounds: Vec<Sound>,
    play_log: Vec<PlayInfo>,
    segments: Vec<Segment>,
    end_state: Option<(World, f64, CameraView)>,
    checker: Option<Checker>,
    pending_snapshot_times: Vec<f64>,
    snapshots_taken: usize,
    frames_emitted: u64,
    finished: bool,
    created: Instant,
}

/// Render a scene using options from the command line (`-ql`, `--renderer gpu`, ...).
///
/// ```no_run
/// use manim::prelude::*;
/// fn main() -> anyhow::Result<()> {
///     manim::render("SquareToCircle", |s| {
///         let sq = s.add(Square::new(2.0));
///         s.play(Transform::new(sq, Circle::new(1.0).set_fill(PINK, 0.5)));
///         s.wait(1.0);
///     })
/// }
/// ```
pub fn render(name: &str, construct: impl FnOnce(&mut Scene)) -> Result<()> {
    render_with(&Config::from_args()?, name, construct)
}

/// Like [`render`], with an explicit `config` instead of command-line arguments.
pub fn render_with(config: &Config, name: &str, construct: impl FnOnce(&mut Scene)) -> Result<()> {
    if config.gpu_info {
        print!("{}", renderer::gpu_report());
        return Ok(());
    }
    if config.list_scenes {
        println!("{name}");
        return Ok(());
    }
    if config.scene_filter.as_deref().is_some_and(|f| f != name) {
        return Ok(());
    }
    let events = Events::new(config.progress);
    let run = || -> Result<()> {
        let mut scene = Scene::new(config, name)?;
        let started = Instant::now();
        construct(&mut scene);
        scene.finish(started)
    };
    let result = run();
    if let Err(e) = &result {
        events.emit("error", json!({ "scene": name, "message": format!("{e:#}") }), || None);
    }
    result
}

/// Run animation code so that a panic in it (a stale mobject id, an animation bug)
/// becomes the scene's error instead of unwinding into the host. `None` if it panicked.
fn contain<R>(error: &mut Option<anyhow::Error>, what: &str, f: impl FnOnce() -> R) -> Option<R> {
    match crate::guard::catch(f) {
        Ok(r) => Some(r),
        Err(msg) => {
            error.get_or_insert_with(|| anyhow!("{what}: internal error: {msg}"));
            None
        }
    }
}

impl Scene {
    /// Create a scene for `config` (validated first). Fails on an invalid config or when the renderer or writer cannot start.
    pub fn new(config: &Config, name: &str) -> Result<Self> {
        config.validate()?;
        if let Some(p) = &config.ffmpeg {
            output::set_ffmpeg_path(p);
        }
        let setup = Instant::now();
        let events = Events::new(config.progress);
        // GPU device creation, encoder probing (spawns ffmpeg) and system font loading are
        // each a few hundred ms; do them concurrently.
        #[cfg(feature = "text")]
        let _ = std::thread::Builder::new().name("manim-fonts".into()).spawn(crate::text::preload_fonts);
        let pipeline = if config.dry_run {
            None
        } else {
            let (renderer, writer) = std::thread::scope(|scope| {
                let writer = std::thread::Builder::new().spawn_scoped(scope, || -> Result<_> {
                    if !config.write_video {
                        return Ok(None);
                    }
                    let base = config.video_base(name);
                    Ok(Some(output::create_writer_with(
                        config.format,
                        config.encoder,
                        &config.x264,
                        &base,
                        config.pixel_width,
                        config.pixel_height,
                        config.fps,
                    )?))
                });
                let renderer = renderer::create(config.renderer, config.pixel_width, config.pixel_height, events);
                let writer = match writer {
                    Ok(handle) => handle.join().unwrap_or_else(|_| Err(anyhow!("output setup panicked"))),
                    Err(e) => Err(anyhow!("cannot start the output setup thread: {e}")),
                };
                (renderer, writer)
            });
            let mut pipeline =
                Pipeline::with_frame_size(renderer?, writer?, Some((config.pixel_width, config.pixel_height)))?;
            let cfg = config.clone();
            pipeline.set_interrupt(move || cfg.interrupted());
            Some(pipeline)
        };
        let describe = pipeline.as_ref().map(|p| (p.renderer_name(), p.output_description()));
        let (renderer_name, output_name) = describe.unwrap_or_else(|| ("none".into(), "dry run".into()));
        events.emit(
            "start",
            json!({ "scene": name, "width": config.pixel_width, "height": config.pixel_height,
                    "fps": config.fps, "renderer": renderer_name, "output": output_name }),
            || {
                Some(format!(
                    "manim: {name} at {}x{} {}fps, renderer: {renderer_name}, output: {output_name} (setup {:.2}s)",
                    config.pixel_width,
                    config.pixel_height,
                    config.fps,
                    setup.elapsed().as_secs_f64()
                ))
            },
        );
        let mut camera = Camera::new(config.pixel_width, config.pixel_height);
        camera.background = config.background;
        crate::defaults::reset();
        let checker = config.check.then(|| Checker::new(camera.frame_width(), camera.frame_height));
        let mut pending: Vec<f64> =
            config.snapshots.iter().filter_map(|s| if let SnapshotAt::Time(t) = s { Some(*t) } else { None }).collect();
        pending.sort_by(|a, b| a.total_cmp(b));
        let mut scene = Scene {
            name: name.to_string(),
            world: World::new(),
            updaters: Vec::new(),
            next_updater: 0,
            camera,
            pipeline,
            config: config.clone(),
            time: 0.0,
            duration: 0.0,
            error: None,
            events,
            theme: None,
            pattern: None,
            markers: Vec::new(),
            sounds: Vec::new(),
            play_log: Vec::new(),
            segments: Vec::new(),
            end_state: None,
            checker,
            pending_snapshot_times: pending,
            snapshots_taken: 0,
            frames_emitted: 0,
            finished: false,
            created: setup,
        };
        if let Some(t) = config.theme.clone() {
            scene.set_theme(t);
        }
        Ok(scene)
    }

    /// The scene name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// When the scene was created (for timing a whole render).
    pub fn created_at(&self) -> Instant {
        self.created
    }

    // ------------------------------------------------------------------ mobjects

    /// Add and display a mobject.
    pub fn add(&mut self, m: impl Into<VMobject>) -> MobjectId {
        let m = self.checked_nesting(m.into());
        let id = self.world.insert(m);
        self.world.show(id);
        id
    }

    /// Store a mobject without displaying it (e.g. a `ReplacementTransform` target).
    pub fn insert(&mut self, m: impl Into<VMobject>) -> MobjectId {
        let m = self.checked_nesting(m.into());
        self.world.insert(m)
    }

    /// A mobject nested deeper than [`MAX_NESTING`] would overflow the stack in the
    /// recursive code that clones, compares and drops it: refuse it, and fail the
    /// render with an error instead.
    fn checked_nesting(&mut self, m: VMobject) -> VMobject {
        if m.depth() <= MAX_NESTING {
            return m;
        }
        self.error.get_or_insert_with(|| anyhow!("a mobject is nested more than {MAX_NESTING} groups deep"));
        // Dropping it would recurse as deep as it is nested; leaking it is the safe way out.
        std::mem::forget(m);
        VMobject::group(Vec::new())
    }

    /// Re-display a stored mobject.
    pub fn show(&mut self, id: MobjectId) {
        self.world.show(id);
    }

    /// Remove `id` from every layer (scene, background and overlay).
    pub fn remove(&mut self, id: MobjectId) {
        self.world.remove_everywhere(id);
    }

    /// Remove everything from the scene layer (the background and overlay stay).
    pub fn clear(&mut self) {
        self.world.clear();
    }

    /// Move `id` to the top of the draw order.
    pub fn bring_to_front(&mut self, id: MobjectId) {
        self.world.bring_to_front(id);
    }

    /// Move `id` to the bottom of the draw order.
    pub fn bring_to_back(&mut self, id: MobjectId) {
        self.world.bring_to_back(id);
    }

    /// Draw below the scene; kept by `clear` and ignored by layout checks.
    pub fn add_background(&mut self, m: impl Into<VMobject>) -> MobjectId {
        let id = self.world.insert(m.into());
        self.world.add_background(id);
        id
    }

    /// Draw above the scene (watermarks, captions); kept by `clear`, ignored by checks.
    pub fn add_overlay(&mut self, m: impl Into<VMobject>) -> MobjectId {
        let id = self.world.insert(m.into());
        self.world.add_overlay(id);
        id
    }

    /// Displayed mobjects in the scene layer, in draw order.
    pub fn mobjects(&self) -> Vec<MobjectId> {
        self.world.displayed().to_vec()
    }

    /// The mobject stored under `id`. Panics if it does not exist.
    pub fn get(&self, id: MobjectId) -> &VMobject {
        &self.world[id]
    }

    /// Mutate in place with builder methods: `s.modify(id, |m| m.shift(UP))`.
    pub fn modify(&mut self, id: MobjectId, f: impl FnOnce(VMobject) -> VMobject) {
        self.world.modify(id, f);
    }

    /// The underlying world.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Mutable access to the underlying world.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The camera (frame size, center and background).
    pub fn camera(&mut self) -> &mut Camera {
        &mut self.camera
    }

    /// Whether the render was cancelled or passed its deadline (see [`Config::deadline`]).
    pub fn is_interrupted(&self) -> bool {
        self.config.interrupted()
    }

    /// Elapsed scene time in seconds.
    pub fn time(&self) -> f64 {
        self.time
    }

    /// Length of the scene so far (unchanged by seeking).
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// The configuration this scene was created with.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Frame size in scene units `(width, height)`.
    pub fn frame_size(&self) -> (f64, f64) {
        (self.camera.frame_width(), self.camera.frame_height)
    }

    // ------------------------------------------------------------------ theme

    /// Apply a theme: background color and pattern, and the default colors and fonts
    /// of new shapes and text.
    pub fn set_theme(&mut self, theme: Theme) {
        self.camera.background = theme.colors.background;
        crate::defaults::set(theme.defaults());
        if let Some(old) = self.pattern.take() {
            self.world.remove_everywhere(old);
        }
        let (w, h) = self.frame_size();
        if let Some(p) = theme.pattern_mobject(w, h) {
            self.pattern = Some(self.add_background(p));
        }
        self.theme = Some(theme);
    }

    /// The applied theme, if any.
    pub fn theme(&self) -> Option<&Theme> {
        self.theme.as_ref()
    }

    /// Set the frame background color (alpha is ignored).
    pub fn set_background_color(&mut self, c: crate::color::Color) {
        self.camera.background = c;
    }

    // ------------------------------------------------------------------ trackers & updaters

    /// Manim's `ValueTracker`.
    pub fn tracker(&mut self, v: f64) -> TrackerId {
        self.world.tracker(v)
    }

    /// Value of tracker `t`.
    pub fn value(&self, t: TrackerId) -> f64 {
        self.world.value(t)
    }

    /// Set the value of tracker `t`.
    pub fn set_value(&mut self, t: TrackerId, v: f64) {
        self.world.set_value(t, v);
    }

    /// Create a tracker and return its index (for hosts that address trackers by number).
    pub fn tracker_index(&mut self, v: f64) -> u32 {
        self.world.tracker(v).0 as u32
    }

    /// The tracker with index `i`.
    pub fn tracker_id(&self, i: usize) -> Result<TrackerId> {
        if i < self.world.trackers().len() {
            Ok(TrackerId(i))
        } else {
            bail!("tracker {i} does not exist")
        }
    }

    /// Run `f(world, dt)` every frame (after animations). Called once immediately with `dt = 0`.
    /// For seeking to reproduce frames, `f` should depend only on the world (not on
    /// state it keeps between calls).
    pub fn add_updater(&mut self, f: impl FnMut(&mut World, f64) + 'static) -> UpdaterId {
        let id = UpdaterId(self.next_updater);
        self.next_updater += 1;
        let mut f: Updater = Box::new(f);
        f(&mut self.world, 0.0);
        self.updaters.push(UpdaterSlot { id, f, active: true });
        id
    }

    /// Stop and drop updater `id` (kept inactive when recording so earlier segments replay correctly).
    pub fn remove_updater(&mut self, id: UpdaterId) {
        if self.config.record {
            // Keep it for replaying earlier segments.
            self.updaters.iter_mut().filter(|u| u.id == id).for_each(|u| u.active = false);
        } else {
            self.updaters.retain(|u| u.id != id);
        }
    }

    /// Stop and drop all updaters.
    pub fn clear_updaters(&mut self) {
        if self.config.record {
            self.updaters.iter_mut().for_each(|u| u.active = false);
        } else {
            self.updaters.clear();
        }
    }

    /// Rebuild a mobject from the world every frame (Manim's `always_redraw`).
    pub fn always_redraw(&mut self, f: impl Fn(&World) -> VMobject + 'static) -> MobjectId {
        let id = self.add(f(&self.world));
        self.add_updater(move |w, _| {
            let m = f(w);
            w.set(id, m);
        });
        id
    }

    fn active_updaters(&self) -> Vec<UpdaterId> {
        self.updaters.iter().filter(|u| u.active).map(|u| u.id).collect()
    }

    fn run_updaters(&mut self, dt: f64, only: Option<&[UpdaterId]>) {
        for u in &mut self.updaters {
            let run = match only {
                Some(ids) => ids.contains(&u.id),
                None => u.active,
            };
            if run {
                (u.f)(&mut self.world, dt);
            }
        }
    }

    // ------------------------------------------------------------------ markers & sound

    /// Name the current moment (e.g. where a narration line starts). Returns the time.
    pub fn mark(&mut self, name: &str) -> f64 {
        self.add_marker(name, "marker")
    }

    /// Start a chapter here. Returns the time.
    pub fn section(&mut self, name: &str) -> f64 {
        self.add_marker(name, "section")
    }

    fn add_marker(&mut self, name: &str, kind: &str) -> f64 {
        let t = self.time;
        self.markers.push(Marker { name: name.to_string(), time: t, kind: kind.to_string() });
        self.events.emit("marker", json!({ "name": name, "time": t, "kind": kind }), || None);
        if self.config.snapshots.contains(&SnapshotAt::Markers) {
            self.snapshot(name);
        }
        t
    }

    /// Markers and sections recorded so far.
    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// Play an audio file starting now.
    pub fn add_sound(&mut self, path: impl AsRef<Path>) {
        let t = self.time;
        self.add_sound_at(path, t, 1.0);
    }

    /// Play an audio file starting at scene time `at` with linear `gain`.
    pub fn add_sound_at(&mut self, path: impl AsRef<Path>, at: f64, gain: f64) {
        self.push_sound(Sound { source: AudioSource::File(path.as_ref().to_path_buf()), start: at, gain });
    }

    /// Play raw interleaved samples (-1..1) starting at `at` (e.g. a TTS voice line).
    pub fn add_samples(&mut self, data: Arc<[f32]>, sample_rate: u32, channels: u16, at: f64, gain: f64) {
        self.push_sound(Sound { source: AudioSource::Samples { data, sample_rate, channels }, start: at, gain });
    }

    fn push_sound(&mut self, s: Sound) {
        if !s.start.is_finite() || !s.gain.is_finite() || s.gain < 0.0 {
            self.error.get_or_insert_with(|| {
                anyhow!("sound needs a finite start time and a non-negative gain (got {} and {})", s.start, s.gain)
            });
            return;
        }
        if let AudioSource::Samples { sample_rate: 0, .. } = s.source {
            self.error.get_or_insert_with(|| anyhow!("sound samples need a sample rate above 0"));
            return;
        }
        let info = SoundInfo::from(&s);
        self.events.emit("sound", json!(info), || None);
        self.sounds.push(s);
    }

    /// Sounds added so far.
    pub fn sounds(&self) -> &[Sound] {
        &self.sounds
    }

    // ------------------------------------------------------------------ inspection

    /// What is on screen now (scene layer only).
    pub fn inspect(&self) -> Vec<check::Item> {
        check::inspect(&self.world)
    }

    /// Layout problems found so far (with `Config::check`).
    pub fn problems(&self) -> &[Problem] {
        self.checker.as_ref().map(|c| c.problems()).unwrap_or(&[])
    }

    /// Snapshot of the timeline: plays, markers, sounds and problems.
    pub fn timeline(&self) -> TimelineInfo {
        TimelineInfo {
            scene: self.name.clone(),
            fps: self.config.fps,
            width: self.config.pixel_width,
            height: self.config.pixel_height,
            duration: self.duration,
            plays: self.play_log.clone(),
            markers: self.markers.clone(),
            sounds: self.sounds.iter().map(SoundInfo::from).collect(),
            problems: self.problems().to_vec(),
        }
    }

    /// The current frame's display list.
    pub fn display_list(&self) -> DisplayList {
        self.camera.display_list(&self.world)
    }

    /// Rasterize the current frame on the CPU (RGBA8, `pixel_width * pixel_height * 4` bytes).
    pub fn render_frame(&self) -> Vec<u8> {
        renderer::cpu::rasterize(&self.display_list())
    }

    fn snapshot(&mut self, label: &str) {
        let dir = {
            let base = self.config.video_base(&self.name);
            let mut d = base.into_os_string();
            d.push("_snapshots");
            PathBuf::from(d)
        };
        self.snapshots_taken += 1;
        let safe: String =
            label.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).take(40).collect();
        let path = dir.join(format!("{:03}_{safe}.png", self.snapshots_taken));
        let frame = self.render_frame();
        match output::save_png(&path, self.config.pixel_width, self.config.pixel_height, &frame) {
            Ok(()) => {
                let t = self.time;
                self.events.emit(
                    "snapshot",
                    json!({ "path": path.display().to_string(), "time": t, "label": label }),
                    || Some(format!("manim: snapshot {} at {t:.2}s", path.display())),
                );
            }
            Err(e) => {
                if self.error.is_none() {
                    self.error = Some(e);
                }
            }
        }
    }

    // ------------------------------------------------------------------ timeline

    fn emit_frame(&mut self) {
        if self.error.is_some() {
            return;
        }
        if self.config.interrupted() {
            self.error = Some(anyhow!("render cancelled"));
            return;
        }
        self.frames_emitted += 1;
        let Some(pipeline) = &mut self.pipeline else { return };
        let dl = self.camera.display_list(&self.world);
        if let Err(e) = pipeline.push(dl) {
            self.error = Some(e);
        }
    }

    /// Play animations simultaneously. Each keeps its own run time; the call lasts as long
    /// as the longest. Accepts one animation, a tuple, or `anims![...]`.
    pub fn play(&mut self, anims: impl IntoAnimations) {
        let mut p = self.begin_play(anims);
        while let Some(dt) = self.step(&mut p) {
            self.end_frame(&p, dt);
        }
    }

    /// Hold for `seconds` (updaters keep running).
    pub fn wait(&mut self, seconds: f64) {
        let mut p = self.begin_wait(seconds);
        while let Some(dt) = self.step(&mut p) {
            self.end_frame(&p, dt);
        }
    }

    /// Start a `play` to drive frame by frame:
    /// `while let Some(dt) = s.step(&mut p) { /* own per-frame code */ s.end_frame(&p, dt) }`.
    pub fn begin_play(&mut self, anims: impl IntoAnimations) -> Playback {
        self.leave_seek();
        self.world.set_base_center(self.camera.frame_center);
        let mut group = AnimationGroup::new(anims);
        let world = &self.world;
        let g = &mut group;
        let mut usable = contain(&mut self.error, "play", || g.resolve(world)).is_some();
        if usable {
            if let Err(e) = group.validate(&self.world) {
                self.error.get_or_insert(e.context("play"));
                usable = false;
            }
        }
        let mut run_time = if usable { group.config().run_time.max(0.0) } else { 0.0 };
        if !run_time.is_finite() {
            self.error.get_or_insert_with(|| anyhow!("play: run_time must be finite (got {run_time})"));
            run_time = 0.0;
        }
        // Same count as Manim CE (frames at t = 0, dt, ... < run_time).
        let frames = (run_time * self.config.fps - 1e-6).ceil().max(0.0) as usize;
        let index = self.play_log.len();
        let name = group.name();
        if self.config.record {
            self.segments.push(Segment {
                start: self.time,
                frames,
                anim: Some(group.clone()),
                world: self.world.clone(),
                view: CameraView::of(&self.camera),
                updaters: self.active_updaters(),
            });
        }
        if usable {
            let world = &mut self.world;
            let g = &mut group;
            contain(&mut self.error, "play", || g.begin(world));
        }
        let start = self.time;
        let plays = self.play_log.iter().filter(|p| !p.wait).count();
        self.events.emit(
            "play",
            json!({ "index": index, "name": name, "start": start, "run_time": run_time, "frames": frames }),
            || Some(format!("  Animation {plays}: {name} ({run_time:.2}s, {frames} frames)")),
        );
        self.play_log.push(PlayInfo { index, name, start, run_time, frames, wait: false });
        Playback {
            anim: Some(group),
            frames,
            i: 0,
            stop: frames,
            dt: 1.0 / self.config.fps,
            replay: false,
            updaters: None,
            play_index: index,
            finished: false,
        }
    }

    /// Start a `wait` to drive frame by frame (see [`Scene::begin_play`]).
    pub fn begin_wait(&mut self, mut seconds: f64) -> Playback {
        self.leave_seek();
        if !seconds.is_finite() {
            self.error.get_or_insert_with(|| anyhow!("wait: duration must be finite (got {seconds})"));
            seconds = 0.0;
        }
        // Manim CE renders waits as floor(seconds · fps) frames.
        let frames = (seconds.max(0.0) * self.config.fps + 1e-6).floor() as usize;
        let index = self.play_log.len();
        if self.config.record {
            self.segments.push(Segment {
                start: self.time,
                frames,
                anim: None,
                world: self.world.clone(),
                view: CameraView::of(&self.camera),
                updaters: self.active_updaters(),
            });
        }
        let start = self.time;
        self.events.emit(
            "wait",
            json!({ "index": index, "start": start, "run_time": seconds, "frames": frames }),
            || None,
        );
        self.play_log.push(PlayInfo { index, name: "Wait".into(), start, run_time: seconds, frames, wait: true });
        Playback {
            anim: None,
            frames,
            i: 0,
            stop: frames,
            dt: 1.0 / self.config.fps,
            replay: false,
            updaters: None,
            play_index: index,
            finished: false,
        }
    }

    /// Apply the next frame of the animation. Returns the frame's `dt`, or `None` once
    /// the play has finished (the animation is then cleaned up).
    pub fn step(&mut self, p: &mut Playback) -> Option<f64> {
        if p.finished {
            return None;
        }
        if self.config.interrupted() {
            // Cancelled or out of time: stop here, even mid-replay.
            self.error.get_or_insert_with(|| anyhow!("render cancelled"));
            p.finished = true;
            return None;
        }
        let failed = self.error.is_some() && !p.replay;
        if p.i >= p.stop || failed {
            if p.stop == p.frames || failed {
                if let Some(a) = &mut p.anim {
                    let world = &mut self.world;
                    contain(&mut self.error, "play", || a.clean_up(world));
                }
                if !p.replay {
                    self.after_play(p);
                }
            }
            p.finished = true;
            return None;
        }
        p.i += 1;
        if let Some(a) = &mut p.anim {
            let (world, t) = (&mut self.world, p.i as f64 / p.frames as f64);
            contain(&mut self.error, "play", || a.interpolate(world, t));
        }
        Some(p.dt)
    }

    /// Finish a frame: run updaters, advance time and (unless replaying) emit it.
    pub fn end_frame(&mut self, p: &Playback, dt: f64) {
        self.run_updaters(dt, p.updaters.as_deref());
        // Count in whole frames so times stay exact multiples of 1/fps (no float drift).
        let fps = self.config.fps;
        self.time =
            if (dt * fps - 1.0).abs() < 1e-9 { ((self.time * fps).round() + 1.0) / fps } else { self.time + dt };
        if p.replay {
            return;
        }
        self.duration = self.duration.max(self.time);
        if p.anim.is_some() {
            if p.i == p.frames.div_ceil(2) {
                if let Some(c) = &mut self.checker {
                    let new = c.midway(&self.world, self.time, Some(p.play_index));
                    self.report_problems(new);
                }
            }
            let step = (p.frames / 10).max(1);
            if p.frames >= 10 && p.i.is_multiple_of(step) && p.i < p.frames && self.events.json() {
                let pct = (100 * p.i / p.frames) as u32;
                self.events.emit(
                    "progress",
                    json!({ "index": p.play_index, "percent": pct, "time": self.time }),
                    || None,
                );
            }
        }
        while self.pending_snapshot_times.first().is_some_and(|t| *t <= self.time + 1e-9) {
            let t = self.pending_snapshot_times.remove(0);
            self.snapshot(&format!("t{t:.2}"));
        }
        self.emit_frame();
    }

    fn after_play(&mut self, p: &Playback) {
        if p.anim.is_none() {
            return;
        }
        if let Some(c) = &mut self.checker {
            let cam = &self.camera;
            // Text boxes are mapped through the camera frame, so the check always uses the base frame.
            c.set_frame(cam.frame_width(), cam.frame_height, [cam.frame_center.x, cam.frame_center.y]);
            let new = c.settled(&self.world, self.time, Some(p.play_index));
            self.report_problems(new);
        }
        if self.config.snapshots.contains(&SnapshotAt::Plays) {
            self.snapshot(&format!("play{}", p.play_index));
        }
    }

    fn report_problems(&mut self, problems: Vec<Problem>) {
        for p in problems {
            let msg = p.message.clone();
            let t = p.time;
            self.events.emit("problem", json!(p), || Some(format!("manim: check: {msg} (at {t:.2}s)")));
        }
    }

    // ------------------------------------------------------------------ seeking

    /// Start replaying up to time `t` (needs [`Config::record`]). The world is reset to
    /// the keyframe before `t`; drive the returned playback with [`Scene::step`] and
    /// [`Scene::end_frame`] (nothing is written), or call [`Scene::seek`].
    pub fn seek_begin(&mut self, t: f64) -> Result<Playback> {
        if !self.config.record {
            bail!("seeking needs Config::record (--record)");
        }
        if self.end_state.is_none() {
            self.end_state = Some((self.world.clone(), self.time, CameraView::of(&self.camera)));
        }
        let t = t.clamp(0.0, self.duration);
        let dt = 1.0 / self.config.fps;
        let seg = self.segments.iter().rposition(|s| s.start <= t + 1e-9);
        let done = |frames| Playback {
            anim: None,
            frames,
            i: frames,
            stop: frames,
            dt,
            replay: true,
            updaters: Some(Vec::new()),
            play_index: 0,
            finished: true,
        };
        let Some(k) = seg else {
            // Before the first play: the first keyframe's world.
            if let Some(s) = self.segments.first() {
                self.world = s.world.clone();
                self.time = s.start;
                s.view.apply(&mut self.camera);
            }
            return Ok(done(0));
        };
        let s = &self.segments[k];
        let end = s.start + s.frames as f64 * dt;
        if t >= end - dt - 1e-9 && k + 1 == self.segments.len() {
            let (w, time, view) = self.end_state.clone().unwrap();
            self.world = w;
            self.time = time;
            view.apply(&mut self.camera);
            return Ok(done(0));
        }
        // Exported frame k shows the state after k + 1 steps (see `end_frame`), so the
        // frame at time t = k / fps is the one after floor(t / dt) + 1.
        let stop = ((((t - s.start) / dt) + 1e-6).floor() + 1.0).clamp(0.0, s.frames as f64) as usize;
        self.world = s.world.clone();
        self.time = s.start;
        s.view.apply(&mut self.camera);
        let mut anim = s.anim.clone();
        if stop > 0 {
            if let Some(a) = &mut anim {
                a.begin(&mut self.world);
            }
        } else {
            anim = None;
        }
        Ok(Playback {
            anim,
            frames: s.frames,
            i: 0,
            stop,
            dt,
            replay: true,
            updaters: Some(s.updaters.clone()),
            play_index: k,
            finished: false,
        })
    }

    /// Back to the end of the timeline after a seek, so further plays continue from where
    /// construction left off rather than from the seeked-to moment.
    fn leave_seek(&mut self) {
        if let Some((w, time, view)) = self.end_state.take() {
            self.world = w;
            self.time = time;
            view.apply(&mut self.camera);
        }
    }

    /// Jump to time `t` (needs [`Config::record`]): the world becomes what the exported
    /// frame at `t` shows. Frame `k` of the video (`k / fps` seconds in) is the state after
    /// `k + 1` frame steps, so `seek(k / fps)` and exported frame `k` match exactly.
    pub fn seek(&mut self, t: f64) -> Result<()> {
        let mut p = self.seek_begin(t)?;
        while let Some(dt) = self.step(&mut p) {
            self.end_frame(&p, dt);
        }
        Ok(())
    }

    /// The frame at time `t`, rasterized on the CPU (needs [`Config::record`]); identical to
    /// exported frame `floor(t * fps)`.
    pub fn frame_at(&mut self, t: f64) -> Result<Vec<u8>> {
        self.seek(t)?;
        Ok(self.render_frame())
    }

    // ------------------------------------------------------------------ finishing

    /// Flush frames, close the video, add audio and write the requested side files.
    pub fn finish(&mut self, started: Instant) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        if self.config.snapshots.contains(&SnapshotAt::End) {
            self.snapshot("end");
        }
        if let Some(e) = self.error.take() {
            // Failed or cancelled: discard the half-written output, keep any previous one.
            // The worker's error (e.g. ffmpeg died) is usually the real cause.
            let worker_err = self.pipeline.take().and_then(|p| p.abort());
            return Err(worker_err.unwrap_or(e));
        }
        let fin = self.pipeline.take().map(|p| p.finish());
        let mut outputs: Vec<PathBuf> = Vec::new();
        let (mut written, mut rasterized, mut renderer_name) = (0, 0, String::from("none"));
        if let Some(fin) = fin {
            let mut fin = fin?;
            let stats = &fin.stats;
            written = stats.frames_written;
            rasterized = stats.frames_rasterized;
            renderer_name = fin.renderer.name();
            if let Some(p) = &stats.output {
                if stats.frames_written > 0 && !p.as_os_str().is_empty() {
                    outputs.push(p.clone());
                }
            }
            if self.config.save_last_frame || stats.frames_written == 0 {
                let frame = match (fin.last_frame.take(), fin.last_list.take()) {
                    (Some(f), _) => f,
                    (None, Some(dl)) => fin.renderer.render_batch(&[&dl])?.remove(0),
                    (None, None) => fin.renderer.render_batch(&[&self.camera.display_list(&self.world)])?.remove(0),
                };
                let path = self.config.image_path(&self.name);
                output::save_png(&path, self.config.pixel_width, self.config.pixel_height, &frame)?;
                outputs.push(path);
            }
        }
        // Sound goes into the MP4; other formats can't carry it.
        if !self.sounds.is_empty() {
            match outputs.iter().find(|p| p.extension().is_some_and(|e| e == "mp4")) {
                Some(video) => {
                    let ffmpeg = output::find_ffmpeg().ok_or_else(|| anyhow!("adding sound needs ffmpeg"))?;
                    crate::audio::mux(&ffmpeg, video, &self.sounds)?;
                }
                None if !self.config.dry_run => self.events.emit(
                    "warning",
                    json!({ "message": "sounds were dropped: only MP4 output carries audio" }),
                    || Some("manim: sounds were dropped: only MP4 output carries audio".into()),
                ),
                None => {}
            }
        }
        if self.config.write_timeline {
            let base = self.config.video_base(&self.name);
            let mut p = base.into_os_string();
            p.push(".timeline.json");
            let path = PathBuf::from(p);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, serde_json::to_string_pretty(&self.timeline())?)?;
            outputs.push(path);
        }
        let secs = started.elapsed().as_secs_f64();
        let problems = self.problems().len();
        let outs: Vec<String> = outputs.iter().map(|o| o.display().to_string()).collect();
        let duration = self.duration;
        let check = self.config.check;
        self.events.emit(
            "done",
            json!({ "scene": self.name, "duration": duration, "frames": written, "rasterized": rasterized,
                    "elapsed": secs, "outputs": outs, "problems": problems, "renderer": renderer_name }),
            || {
                let mut s = format!(
                    "manim: {written} frames ({rasterized} rasterized) in {secs:.2}s = {:.1} fps [{renderer_name}], scene {duration:.2}s",
                    written as f64 / secs.max(1e-9)
                );
                if check {
                    s.push_str(&format!("\nmanim: check found {problems} problem(s)"));
                }
                for o in &outs {
                    s.push_str(&format!("\nmanim: wrote {o}"));
                }
                Some(s)
            },
        );
        Ok(())
    }
}

impl Index<MobjectId> for Scene {
    type Output = VMobject;
    fn index(&self, id: MobjectId) -> &VMobject {
        &self.world[id]
    }
}

impl IndexMut<MobjectId> for Scene {
    fn index_mut(&mut self, id: MobjectId) -> &mut VMobject {
        &mut self.world[id]
    }
}
