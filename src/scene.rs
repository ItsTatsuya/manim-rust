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
use std::sync::atomic::Ordering;
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
use crate::mobject::VMobject;
use crate::output;
use crate::pipeline::Pipeline;
use crate::renderer::{self, DisplayList};
use crate::theme::Theme;
use crate::world::{MobjectId, TrackerId, World};

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
    pub name: String,
    pub time: f64,
    /// `"marker"` or `"section"`.
    pub kind: String,
}

/// One `play` or `wait` as it happened.
#[derive(Clone, Debug, Serialize)]
pub struct PlayInfo {
    pub index: usize,
    pub name: String,
    pub start: f64,
    pub run_time: f64,
    pub frames: usize,
    pub wait: bool,
}

/// Everything a host needs to know about a finished (or running) scene's timeline.
#[derive(Clone, Debug, Serialize)]
pub struct TimelineInfo {
    pub scene: String,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    pub plays: Vec<PlayInfo>,
    pub markers: Vec<Marker>,
    pub sounds: Vec<SoundInfo>,
    pub problems: Vec<Problem>,
}

/// A recorded keyframe: the world before a play/wait and a fresh copy of its animation.
struct Segment {
    start: f64,
    frames: usize,
    anim: Option<AnimationGroup>,
    world: World,
    updaters: Vec<UpdaterId>,
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
    end_state: Option<(World, f64)>,
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

impl Scene {
    pub fn new(config: &Config, name: &str) -> Result<Self> {
        let setup = Instant::now();
        let events = Events::new(config.progress);
        // GPU device creation, encoder probing (spawns ffmpeg) and system font loading are
        // each a few hundred ms; do them concurrently.
        #[cfg(feature = "text")]
        std::thread::spawn(crate::text::preload_fonts);
        let pipeline = if config.dry_run {
            None
        } else {
            let (renderer, writer) = std::thread::scope(|scope| {
                let writer = scope.spawn(|| -> Result<_> {
                    if !config.write_video {
                        return Ok(None);
                    }
                    let base = config.video_base(name);
                    Ok(Some(output::create_writer(
                        config.format,
                        config.encoder,
                        &base,
                        config.pixel_width,
                        config.pixel_height,
                        config.fps,
                    )?))
                });
                let renderer = renderer::create(config.renderer, config.pixel_width, config.pixel_height, events);
                (renderer, writer.join().expect("writer setup panicked"))
            });
            Some(Pipeline::new(renderer?, writer?))
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
        let id = self.world.insert(m.into());
        self.world.show(id);
        id
    }

    /// Store a mobject without displaying it (e.g. a `ReplacementTransform` target).
    pub fn insert(&mut self, m: impl Into<VMobject>) -> MobjectId {
        self.world.insert(m.into())
    }

    /// Re-display a stored mobject.
    pub fn show(&mut self, id: MobjectId) {
        self.world.show(id);
    }

    pub fn remove(&mut self, id: MobjectId) {
        self.world.remove_everywhere(id);
    }

    /// Remove everything from the scene layer (the background and overlay stay).
    pub fn clear(&mut self) {
        self.world.clear();
    }

    pub fn bring_to_front(&mut self, id: MobjectId) {
        self.world.bring_to_front(id);
    }

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

    pub fn get(&self, id: MobjectId) -> &VMobject {
        &self.world[id]
    }

    /// Mutate in place with builder methods: `s.modify(id, |m| m.shift(UP))`.
    pub fn modify(&mut self, id: MobjectId, f: impl FnOnce(VMobject) -> VMobject) {
        self.world.modify(id, f);
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    pub fn camera(&mut self) -> &mut Camera {
        &mut self.camera
    }

    /// Elapsed scene time in seconds.
    pub fn time(&self) -> f64 {
        self.time
    }

    /// Length of the scene so far (unchanged by seeking).
    pub fn duration(&self) -> f64 {
        self.duration
    }

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

    pub fn theme(&self) -> Option<&Theme> {
        self.theme.as_ref()
    }

    pub fn set_background_color(&mut self, c: crate::color::Color) {
        self.camera.background = c;
    }

    // ------------------------------------------------------------------ trackers & updaters

    /// Manim's `ValueTracker`.
    pub fn tracker(&mut self, v: f64) -> TrackerId {
        self.world.tracker(v)
    }

    pub fn value(&self, t: TrackerId) -> f64 {
        self.world.value(t)
    }

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

    pub fn remove_updater(&mut self, id: UpdaterId) {
        if self.config.record {
            // Keep it for replaying earlier segments.
            self.updaters.iter_mut().filter(|u| u.id == id).for_each(|u| u.active = false);
        } else {
            self.updaters.retain(|u| u.id != id);
        }
    }

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
        let info = SoundInfo::from(&s);
        self.events.emit("sound", json!(info), || None);
        self.sounds.push(s);
    }

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
        if self.config.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
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
        let mut group = AnimationGroup::new(anims);
        group.resolve(&self.world);
        let run_time = group.config().run_time.max(0.0);
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
                updaters: self.active_updaters(),
            });
        }
        group.begin(&mut self.world);
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
    pub fn begin_wait(&mut self, seconds: f64) -> Playback {
        // Manim CE renders waits as floor(seconds · fps) frames.
        let frames = (seconds.max(0.0) * self.config.fps + 1e-6).floor() as usize;
        let index = self.play_log.len();
        if self.config.record {
            self.segments.push(Segment {
                start: self.time,
                frames,
                anim: None,
                world: self.world.clone(),
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
        let failed = self.error.is_some() && !p.replay;
        if p.i >= p.stop || failed {
            if p.stop == p.frames || failed {
                if let Some(a) = &mut p.anim {
                    a.clean_up(&mut self.world);
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
            a.apply(&mut self.world, p.i as f64 / p.frames as f64);
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
            self.end_state = Some((self.world.clone(), self.time));
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
            }
            return Ok(done(0));
        };
        let s = &self.segments[k];
        let end = s.start + s.frames as f64 * dt;
        if t >= end - 1e-9 && k + 1 == self.segments.len() {
            let (w, time) = self.end_state.clone().unwrap();
            self.world = w;
            self.time = time;
            return Ok(done(0));
        }
        let stop = (((t - s.start) / dt) + 1e-6).floor().clamp(0.0, s.frames as f64) as usize;
        self.world = s.world.clone();
        self.time = s.start;
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

    /// Jump to time `t` (needs [`Config::record`]): the world becomes what frame `t` shows.
    pub fn seek(&mut self, t: f64) -> Result<()> {
        let mut p = self.seek_begin(t)?;
        while let Some(dt) = self.step(&mut p) {
            self.end_frame(&p, dt);
        }
        Ok(())
    }

    /// The frame at time `t`, rasterized on the CPU (needs [`Config::record`]).
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
        let fin = self.pipeline.take().map(|p| p.finish());
        if let Some(e) = self.error.take() {
            // The worker's error (e.g. ffmpeg died) is usually the real cause.
            return Err(fin.and_then(|r| r.err()).unwrap_or(e));
        }
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
