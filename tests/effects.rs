//! Camera frame animation, pinned and clipped mobjects, dash patterns, gradient fills,
//! drop shadows and background blur.

mod common;

use common::test_config;
use manim::animation::MoveCamera;
use manim::prelude::*;
use manim::renderer::cpu::{rasterize, CpuRenderer, Raster};
use manim::renderer::Renderer;

const W: u32 = 160;
const H: u32 = 90;

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

fn world_with(mobs: Vec<VMobject>) -> World {
    let mut w = World::new();
    for m in mobs {
        let id = w.insert(m);
        w.show(id);
    }
    w
}

fn light(c: &mut Camera) {
    c.background = WHITE;
}

fn solid(p: Point, size: f64, color: Color) -> VMobject {
    Square::new(size).set_fill(color, 1.0).set_stroke_width(0.0).move_to(p)
}

// ------------------------------------------------------------------ camera

#[test]
fn the_live_frame_zooms_and_moves_the_view() {
    let cam = Camera::new(W, H);
    let mut w = world_with(vec![solid(RIGHT * 2.0, 1.0, Color::rgb(1.0, 0.0, 0.0))]);
    // Unzoomed, the square is right of centre.
    let before = rasterize(&cam.display_list(&w));
    assert_eq!(px(&before, W / 2, H / 2), [0, 0, 0, 255]);
    // Centred on it and zoomed 2x: it fills the middle, twice as wide.
    w.set_frame(2.0, Some(RIGHT * 2.0));
    let after = rasterize(&cam.display_list(&w));
    assert_eq!(px(&after, W / 2, H / 2), [255, 0, 0, 255]);
    let width = |buf: &[u8]| (0..W).filter(|x| px(buf, *x, H / 2)[0] > 200).count();
    assert!(width(&after) + 2 >= 2 * width(&before), "twice as wide: {} vs {}", width(&after), width(&before));
}

#[test]
fn move_camera_eases_to_the_target_and_lands_exactly() {
    let mut s = Scene::new(&test_config("move_camera"), "Cam").unwrap();
    s.add(Dot::new());
    s.play(MoveCamera::to(Some(1.5), Some(UP)).run_time(1.0));
    let f = s.world().frame();
    assert_eq!(f.zoom, 1.5);
    assert_eq!(f.center, Some(UP));
    s.play(MoveCamera::reset().run_time(1.0));
    let f = s.world().frame();
    assert_eq!((f.zoom, f.center), (1.0, None), "back at the base frame exactly");
}

#[test]
fn zoom_on_keeps_a_point_under_a_gliding_screen_position() {
    let cam = Camera::new(W, H);
    let target = pt(10.0, 20.0); // far away: the point to push in on
    let anchor = pt(3.0, 0.0); // where it should end up on screen
    let mut w = World::new();
    w.set_base_center(ORIGIN);
    let mut push = MoveCamera::zoom_on(target, 1.22, anchor);
    push.begin(&mut w);
    // The point starts where an unmoved camera shows it.
    let on_screen = |w: &World| {
        let (c, h) = cam.live_frame(w);
        let z = FRAME_HEIGHT / h;
        (target - c) * z
    };
    let start = on_screen(&w);
    assert!(start.distance(target) < 1e-9);
    for k in [0.0, 0.25, 0.5, 1.0] {
        push.apply(&mut w, k);
        let want = target + (anchor - target) * k;
        assert!(on_screen(&w).distance(want) < 1e-9, "k = {k}: {:?} vs {want:?}", on_screen(&w));
        assert!((w.frame().zoom - (1.0 + 0.22 * k)).abs() < 1e-12);
    }
    // Pulling back with the point's original position as the anchor undoes the push exactly.
    let mut pull = MoveCamera::zoom_on(target, 1.0, target);
    pull.begin(&mut w);
    pull.apply(&mut w, 1.0);
    pull.clean_up(&mut w);
    assert_eq!((w.frame().zoom, w.frame().center), (1.0, None));
}

#[test]
fn pinned_mobjects_and_the_overlay_ignore_the_camera() {
    let cam = Camera::new(W, H);
    let mut w = world_with(vec![
        solid(LEFT * 3.0, 1.0, Color::rgb(1.0, 0.0, 0.0)),
        solid(LEFT * 3.0, 1.0, Color::rgb(0.0, 1.0, 0.0)).pin(true).shift(UP * 2.5),
    ]);
    let mark = w.insert(solid(RIGHT * 4.0, 0.6, Color::rgb(0.0, 0.0, 1.0)));
    w.add_overlay(mark);
    let at = |w: &World, p: Point| {
        let b = cam.to_pixel(p);
        px(&rasterize(&cam.display_list(w)), b[0] as u32, b[1] as u32)
    };
    assert_eq!(at(&w, LEFT * 3.0)[0], 255, "red is there before the move");
    w.set_frame(2.0, Some(RIGHT * 3.0));
    assert_eq!(at(&w, LEFT * 3.0), [0, 0, 0, 255], "the unpinned square moved away");
    assert_eq!(at(&w, LEFT * 3.0 + UP * 2.5), [0, 255, 0, 255], "pinned stays");
    assert_eq!(at(&w, RIGHT * 4.0), [0, 0, 255, 255], "the overlay stays");
}

#[test]
fn a_clip_cuts_zoomed_content_at_the_panel_edge() {
    let cam = Camera::new(W, H);
    let big = Square::new(6.0)
        .set_fill(Color::rgb(1.0, 0.0, 0.0), 1.0)
        .set_stroke_width(0.0)
        .clip_to(pt(-1.0, -1.0), pt(1.0, 1.0));
    let mut w = world_with(vec![big]);
    w.set_frame(2.0, None); // zoomed: the square would cover the whole frame
    let buf = rasterize(&cam.display_list(&w));
    let inside = cam.to_pixel(pt(0.5, 0.5));
    let outside = cam.to_pixel(pt(2.0, 0.0));
    assert_eq!(px(&buf, inside[0] as u32, inside[1] as u32), [255, 0, 0, 255]);
    assert_eq!(px(&buf, outside[0] as u32, outside[1] as u32), [0, 0, 0, 255], "clipped away");
    assert_eq!(px(&buf, 2, 2), [0, 0, 0, 255]);
}

#[test]
fn seeking_restores_the_camera_frame() {
    let mut c = test_config("seek_frame");
    c.record = true;
    let mut s = Scene::new(&c, "Seek").unwrap();
    s.add(Dot::new());
    s.play(MoveCamera::to(Some(2.0), Some(RIGHT)).run_time(1.0));
    s.wait(1.0);
    s.play(MoveCamera::reset().run_time(1.0));
    s.seek(1.5).unwrap();
    assert_eq!(s.world().frame().zoom, 2.0);
    s.seek(0.0).unwrap();
    assert!(s.world().frame().zoom < 1.5, "near the start of the push");
    s.seek(s.duration()).unwrap();
    assert_eq!(s.world().frame().zoom, 1.0);
}

// ------------------------------------------------------------------ dashes

fn dash_items(m: &VMobject) -> Vec<manim::renderer::DrawPath> {
    let w = world_with(vec![m.clone()]);
    Camera::new(W, H).display_list(&w).items
}

#[test]
fn a_dashed_stroke_is_cut_into_dashes_at_draw_time() {
    let plain = dash_items(&Line::new(LEFT * 4.0, RIGHT * 4.0));
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].subpaths.len(), 1);
    let dashed = dash_items(&Line::new(LEFT * 4.0, RIGHT * 4.0).set_dash(0.4, 0.4, 0.0));
    let n = dashed[0].subpaths.len();
    assert_eq!(n, 10, "8 units of line, 0.8 per period");
    // A phase of half a period moves every dash 0.4 (the width of the dash) along.
    let moved = dash_items(&Line::new(LEFT * 4.0, RIGHT * 4.0).set_dash(0.4, 0.4, 0.4));
    assert_ne!(dashed, moved, "phase changes the frame");
    assert_eq!(dashed[0].stroke, moved[0].stroke);
}

#[test]
fn dash_phase_animates() {
    let mut s = Scene::new(&test_config("dash_phase"), "Dash").unwrap();
    let id = s.add(Line::new(LEFT * 4.0, RIGHT * 4.0).set_dash(0.4, 0.4, 0.0));
    s.play(id.animate(|m| m.set_dash(0.4, 0.4, 0.8)).rate_func(linear).run_time(1.0));
    let d = s.world()[id].effects.dash.unwrap();
    assert!((d.phase - 0.8).abs() < 1e-9 && d.length == 0.4);
}

#[test]
fn create_draws_a_dashed_line_on() {
    let mut s = Scene::new(&test_config("dash_create"), "DashCreate").unwrap();
    let id = s.add(Line::new(LEFT * 4.0, RIGHT * 4.0).set_dash(0.4, 0.4, 0.0).set_stroke_width(6.0));
    s.remove(id);
    let mut p = s.begin_play(Create::new(id).run_time(1.0));
    let mut lengths = Vec::new();
    while let Some(dt) = s.step(&mut p) {
        let dl = s.display_list();
        lengths.push(dl.items.iter().map(|i| i.subpaths.len()).sum::<usize>());
        s.end_frame(&p, dt);
    }
    assert!(lengths.windows(2).all(|w| w[0] <= w[1]), "dashes only get added: {lengths:?}");
    assert_eq!(*lengths.last().unwrap(), 10);
}

// ------------------------------------------------------------------ gradient, shadow, blur

#[test]
fn a_gradient_runs_over_the_bounding_box() {
    let card = Rectangle::new(8.0, 4.0).set_fill(WHITE, 1.0).set_stroke_width(0.0).set_gradient(Gradient::new(
        0.0,
        Color::rgb(1.0, 0.0, 0.0),
        Color::rgb(0.0, 0.0, 1.0),
    ));
    let buf = rasterize(&Camera::new(W, H).display_list(&world_with(vec![card])));
    let cam = Camera::new(W, H);
    let at = |x: f64| {
        let p = cam.to_pixel(pt(x, 0.0));
        px(&buf, p[0] as u32, p[1] as u32)
    };
    let (l, m, r) = (at(-3.8), at(0.0), at(3.8));
    assert!(l[0] > 230 && l[2] < 30, "left is red: {l:?}");
    assert!(r[2] > 230 && r[0] < 30, "right is blue: {r:?}");
    assert!((m[0] as i32 - 128).abs() < 12 && (m[2] as i32 - 128).abs() < 12, "middle is mixed: {m:?}");
}

#[test]
fn a_gradient_follows_the_fill_opacity() {
    let card = Rectangle::new(4.0, 2.0)
        .set_fill(WHITE, 1.0)
        .set_stroke_width(0.0)
        .set_gradient(Gradient::new(-std::f64::consts::FRAC_PI_4, WHITE, WHITE))
        .set_fill_opacity(0.5);
    let buf = rasterize(&Camera::new(W, H).display_list(&world_with(vec![card])));
    assert!((px(&buf, W / 2, H / 2)[0] as i32 - 128).abs() <= 2);
}

#[test]
fn a_shadow_darkens_outside_the_card_only() {
    let mut cam = Camera::new(W, H);
    light(&mut cam);
    let card = Square::new(2.0).set_fill(WHITE, 1.0).set_stroke_width(0.0).set_shadow(Shadow {
        color: BLACK.with_alpha(0.6),
        blur: 0.25,
        dx: 0.0,
        dy: -0.3,
    });
    let buf = rasterize(&cam.display_list(&world_with(vec![card])));
    let at = |x: f64, y: f64| {
        let p = cam.to_pixel(pt(x, y));
        px(&buf, p[0] as u32, p[1] as u32)
    };
    assert_eq!(at(0.0, 0.0), [255, 255, 255, 255], "inside the card is untouched");
    let below = at(0.0, -1.15);
    assert!(below[0] < 235, "the shadow is below the card: {below:?}");
    assert!(at(0.0, 1.15)[0] > below[0], "and lighter above, where the offset leaves it");
    assert_eq!(at(0.0, -3.0), [255, 255, 255, 255], "far away there is none");
    assert!(at(0.0, -1.5)[0] > below[0], "it fades with distance");
}

#[test]
fn backdrop_blur_softens_what_is_behind_a_card() {
    let mut cam = Camera::new(W, H);
    light(&mut cam);
    // Black and white halves with a hard edge at x = 0, then a blurred panel over the middle.
    let w = world_with(vec![
        solid(LEFT * 3.5, 7.0, BLACK).stretch(1.0, 1),
        Rectangle::new(4.0, 2.0).set_fill(WHITE, 0.0).set_stroke_width(0.0).set_backdrop_blur(0.3),
    ]);
    let w = {
        let mut w = w;
        // The panel needs a visible fill to be kept; a clear one still blurs.
        let id = w.displayed()[1];
        w.get_mut(id).fill_opacity = 0.0;
        w
    };
    let buf = rasterize(&cam.display_list(&w));
    let at = |x: f64, y: f64| {
        let p = cam.to_pixel(pt(x, y));
        px(&buf, p[0] as u32, p[1] as u32)
    };
    let edge = at(0.0, 0.0);
    assert!(edge[0] > 90 && edge[0] < 165, "the edge is soft inside the panel: {edge:?}");
    assert!(at(-0.3, 2.0)[0] < 5, "outside the panel the edge stays sharp (black side)");
    assert!(at(0.3, 2.0)[0] > 250, "outside the panel the edge stays sharp (white side)");
    assert!(at(-1.9, 0.0)[0] < 30, "far from the edge, inside the panel, nothing changes");
}

#[test]
fn effect_frames_are_equal_only_when_the_effects_are() {
    let base = Square::new(2.0).set_fill(RED, 1.0);
    let cam = Camera::new(W, H);
    let a = cam.display_list(&world_with(vec![base.clone()]));
    let b = cam.display_list(&world_with(vec![base.clone()]));
    assert_eq!(a, b);
    let blur = cam.display_list(&world_with(vec![base.clone().set_backdrop_blur(0.2)]));
    let shadow = cam.display_list(&world_with(vec![base.clone().set_shadow(Shadow {
        color: BLACK,
        blur: 0.2,
        dx: 0.0,
        dy: 0.0,
    })]));
    let clip = cam.display_list(&world_with(vec![base.clone().clip_to(ORIGIN, UR)]));
    for (name, dl) in [("blur", &blur), ("shadow", &shadow), ("clip", &clip)] {
        assert_ne!(*dl, a, "{name}");
        assert!(dl.needs_reference(), "{name} goes to the reference rasterizer");
    }
    assert!(!a.needs_reference());
}

#[test]
fn every_cpu_rasterizer_draws_effect_frames_identically() {
    let mut cam = Camera::new(W, H);
    light(&mut cam);
    let card = Rectangle::new(4.0, 2.0)
        .set_fill(WHITE, 1.0)
        .set_gradient(Gradient::new(-0.8, TEAL, GOLD))
        .set_shadow(Shadow { color: BLACK.with_alpha(0.4), blur: 0.2, dx: 0.1, dy: -0.2 })
        .clip_to(pt(-3.0, -2.0), pt(3.0, 2.0));
    let dl = cam.display_list(&world_with(vec![solid(ORIGIN, 3.0, RED), card.set_backdrop_blur(0.1)]));
    let reference = CpuRenderer::with_raster(Raster::Skia).render_batch(&[&dl]).unwrap().remove(0);
    let mut others = vec![CpuRenderer::new()];
    #[cfg(feature = "vello")]
    others.push(CpuRenderer::with_raster(Raster::Vello));
    for mut r in others {
        assert!(r.render_batch(&[&dl]).unwrap().remove(0) == reference, "{}", r.name());
    }
    #[cfg(feature = "gpu")]
    if let Ok(mut g) = manim::renderer::gpu::GpuRenderer::new(W, H, false) {
        assert!(g.render_batch(&[&dl]).unwrap().remove(0) == reference, "GPU");
    }
}

// ------------------------------------------------------------------ scripts

#[cfg(feature = "script")]
mod script {
    use manim::config::{Config, Quality};
    use manim::script::{HostOptions, ScriptHost};

    fn run(name: &str, src: &str) -> ScriptHost {
        let d = std::env::temp_dir().join(format!("manim-effects-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("s.ts");
        std::fs::write(&p, src).unwrap();
        let host = ScriptHost::load(&p, HostOptions::default()).unwrap();
        let mut c = Config::default().quality(Quality::Low);
        c.dry_run = true;
        c.progress = manim::events::ProgressMode::None;
        c.record = true;
        host.run("S", &c).unwrap_or_else(|e| panic!("{e:#}"));
        host
    }

    #[test]
    fn scripts_can_move_the_camera_and_style_cards() {
        let host = run(
            "card",
            r##"
import { Scene, Rectangle, Line, Dot, FadeIn, Create, MoveCamera, LEFT, RIGHT, UP, vec } from "manim";
export default class S extends Scene {
  construct() {
    const card = Rectangle(4, 2, { fillColor: "#FFFFFF", fillOpacity: 1 })
      .setGradient(-Math.PI / 4, [[0, "#2EC4B6", 0.2], [1, "#2EC4B6", 0.05]])
      .setShadow("#000000", 0.35, 0.3, 0, -0.2)
      .pin();
    const glass = Rectangle(3, 1, { fillOpacity: 0 }).setBackdropBlur(0.2);
    const guide = Line([-2, 0, 0], [2, 0, 0]).setDash(0.2, 0.15).clipTo(-1, -1, 1, 1);
    this.add(card, glass, guide);
    this.play(MoveCamera({ focus: { point: [1, 1, 0], anchor: [3, 0, 0] }, zoom: 1.22, runTime: 2.2 }));
    this.play(guide.animate.setDash(0.2, 0.15, 0.35), { runTime: 0.5 });
    this.play(MoveCamera({ reset: true }));
    this.play(MoveCamera({ width: 7.1, center: [1, 0, 0] }));
  }
}
"##,
        );
        let (f, base_width) = host.with_scene(|s| (s.world().frame(), s.frame_size().0)).unwrap();
        assert!((f.zoom - base_width / 7.1).abs() < 1e-9, "zoom {}", f.zoom);
        assert_eq!(f.center.map(|c| c.x), Some(1.0));
        let items = host.with_scene(|s| s.display_list()).unwrap().items;
        assert!(items.iter().any(|i| i.gradient.is_some() && i.shadow.is_some()), "the card");
        assert!(items.iter().any(|i| i.backdrop_blur > 0.0), "the glass");
        assert!(items.iter().any(|i| i.clip.is_some() && i.subpaths.len() > 1), "the clipped dashes");
    }

    #[test]
    fn a_group_hands_out_the_same_member_every_time() {
        run(
            "same_child",
            r##"
import { Scene, VGroup, Circle, Square } from "manim";
export default class S extends Scene {
  construct() {
    const g = VGroup(Circle(), Square());
    if (g.get(0) !== g.get(0)) throw new Error("get(0) is a new object each time");
    if (g.get(-1) !== g.get(1)) throw new Error("negative and positive index differ");
    if (g.get(0) === g.get(1)) throw new Error("two members are one object");
    let threw = false;
    try { g.get(5); } catch { threw = true; }
    if (!threw) throw new Error("out of range did not throw");
    g.get(0).shift([1, 0, 0]);
    if (Math.abs(g.get(0).getCenter()[0] - 1) > 1e-9) throw new Error("the member does not write through");
  }
}
"##,
        );
    }

    #[test]
    fn axes_coordinates_follow_a_scaled_axes() {
        run(
            "axes_scale",
            r##"
import { Scene, Axes } from "manim";
export default class S extends Scene {
  construct() {
    const ax = Axes({ xRange: [0, 10, 1], yRange: [0, 20, 5], xLength: 6, yLength: 4 });
    const near = (a: number[], b: number[]) => Math.hypot(a[0] - b[0], a[1] - b[1]) < 1e-6;
    const c = ax.getCenter(), p = ax.c2p(4, 8);
    ax.scale(0.5);
    const want = [c[0] + (p[0] - c[0]) / 2, c[1] + (p[1] - c[1]) / 2];
    if (!near(ax.c2p(4, 8), want)) throw new Error("c2p ignores scale: " + ax.c2p(4, 8) + " vs " + want);
    const [x, y] = ax.p2c(ax.c2p(7, 15));
    if (Math.abs(x - 7) > 1e-6 || Math.abs(y - 15) > 1e-6) throw new Error("p2c is not the inverse: " + [x, y]);
    ax.shift([1, 2, 0]);
    if (!near(ax.c2p(4, 8), [want[0] + 1, want[1] + 2])) throw new Error("c2p ignores a shift after scaling");
  }
}
"##,
        );
    }
}

#[cfg(feature = "text")]
#[test]
fn the_layout_check_follows_the_camera_pins_and_clips() {
    if !common::fonts_available() {
        return;
    }
    let mut c = test_config("check_camera");
    c.check = true;
    let mut s = Scene::new(&c, "CheckCam").unwrap();
    // A heading and a label in the work column are pinned; a graph label near the right
    // edge is clipped to its panel (right edge at x = 6.5); one in the middle is neither.
    let pinned = s.insert(Text::new("pinned heading").shift(LEFT * 4.2).pin(true));
    let clipped = s.insert(Text::new("in the panel").shift(RIGHT * 6.0).clip_to(pt(-0.45, -3.3), pt(6.5, 3.2)));
    let loose = s.insert(Text::new("loose").shift(LEFT * 4.2 + DOWN * 2.0));
    s.play((FadeIn::new(pinned), FadeIn::new(clipped), FadeIn::new(loose)));
    assert!(s.problems().is_empty(), "{:?}", s.problems());
    // Push in on the right-hand label: unpinned text near the left edge leaves the frame, but
    // the pinned heading stays, and the clipped label is cut at its panel instead of overflowing.
    s.play(MoveCamera::zoom_on(RIGHT * 6.0, 1.5, RIGHT * 3.0));
    let found: Vec<String> = s.problems().iter().map(|p| p.message.clone()).collect();
    assert!(!found.iter().any(|m| m.contains("pinned heading")), "{found:?}");
    assert!(!found.iter().any(|m| m.contains("in the panel")), "{found:?}");
    assert!(
        found.iter().any(|m| m.contains("loose") && m.contains("off the frame")),
        "unpinned text left the frame: {found:?}"
    );
}
