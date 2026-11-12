//! Features host applications (Manim Studio) rely on: themes, layers, timelines and
//! seeking, markers, layout checks, images, sound, cancellation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use manim::config::{Config, Quality};
use manim::prelude::*;

fn cfg() -> Config {
    let mut c = Config::default().quality(Quality::Low);
    c.dry_run = true;
    c.progress = manim::events::ProgressMode::None;
    c
}

fn recorded() -> Config {
    let mut c = cfg();
    c.record = true;
    c
}

#[test]
fn times_are_whole_frames() {
    let mut s = Scene::new(&cfg(), "T").unwrap();
    let d = s.add(Dot::new());
    for _ in 0..7 {
        s.play(d.animate(|m| m.shift(RIGHT * 0.1)).run_time(1.0 / 3.0));
    }
    s.wait(0.5);
    // 7 plays of ceil(15/3)=5 frames, then floor(7.5)=7 frames, at 15 fps.
    assert_eq!(s.time(), 42.0 / 15.0);
    let t = s.mark("here");
    assert_eq!(t, s.time());
    assert_eq!(s.timeline().plays.len(), 8);
}

#[test]
fn seeking_reproduces_frames() {
    let mut s = Scene::new(&recorded(), "Seek").unwrap();
    let mut expected: Vec<(f64, Vec<u8>)> = Vec::new();
    let sq = s.add(Square::new(1.5).set_fill(BLUE, 0.6));
    let tracker = s.tracker(0.0);
    // An updater that only depends on the world, so replays match.
    let dot = s.add(Dot::new());
    s.add_updater(move |w, _| {
        let x = w.value(tracker);
        w.modify(dot, |m| m.move_to(pt(x, -2.0)));
    });
    expected.push((s.time(), s.render_frame()));
    s.play((sq.animate(|m| m.shift(RIGHT * 2.0).rotate(0.7)), ChangeValue::new(tracker, 3.0)));
    expected.push((s.time(), s.render_frame()));
    // Capture a frame in the middle of a play by driving it frame by frame.
    let c = s.insert(Circle::new(1.0).set_fill(RED, 0.5));
    let mut p = s.begin_play(LaggedStart::new(anims![FadeIn::new(c), Create::new(sq)]).run_time(2.0));
    let mut k = 0;
    while let Some(dt) = s.step(&mut p) {
        s.end_frame(&p, dt);
        k += 1;
        if k == 11 {
            expected.push((s.time(), s.render_frame()));
        }
    }
    s.wait(0.4);
    s.play(FadeOut::new(c));
    expected.push((s.time(), s.render_frame()));
    // Jump around out of order.
    for (t, frame) in expected.iter().rev().chain(expected.iter()) {
        s.seek(*t).unwrap();
        assert!(s.render_frame() == *frame, "frame at t={t} differs after seeking");
    }
}

#[test]
fn seeking_needs_recording() {
    let mut s = Scene::new(&cfg(), "NoRec").unwrap();
    s.wait(1.0);
    assert!(s.seek(0.5).is_err());
}

#[test]
fn check_finds_layout_problems() {
    let mut c = cfg();
    c.check = true;
    let mut s = Scene::new(&c, "Check").unwrap();
    let a = s.insert(Text::new("first sentence"));
    let b = s.insert(Text::new("second sentence").shift(RIGHT * 0.3));
    s.play(FadeIn::new(a));
    // Crossfade on top of each other.
    s.play((FadeOut::new(a), FadeIn::new(b)));
    // Two texts left on top of each other.
    s.play(FadeIn::new(a));
    // Text running off the frame.
    let far = s.insert(Text::new("off the edge").shift(RIGHT * 7.5));
    s.play(FadeIn::new(far));
    let kinds: Vec<&str> = s.problems().iter().map(|p| p.kind.as_str()).collect();
    assert!(kinds.contains(&"ghosting"), "{kinds:?}");
    assert!(kinds.contains(&"overlap"), "{kinds:?}");
    assert!(kinds.contains(&"offscreen"), "{kinds:?}");
    // A clean layout reports nothing.
    let mut s = Scene::new(&c, "Clean").unwrap();
    let a = s.insert(Text::new("top").shift(UP * 2.0));
    let b = s.insert(Text::new("bottom").shift(DOWN * 2.0));
    s.play((FadeIn::new(a), FadeIn::new(b)));
    s.play(FadeOut::new(a));
    s.play(FadeIn::new(a));
    assert!(s.problems().is_empty(), "{:?}", s.problems());
}

#[test]
fn theme_sets_background_pattern_and_defaults() {
    let theme = Theme::from_json(
        r##"{"colors":{"background":"#FFFFFF","text":"#112233","primary":"#4F46E5"},
             "layout":{"pattern":"grid"}}"##,
    )
    .unwrap();
    let mut s = Scene::new(&cfg(), "Theme").unwrap();
    s.set_theme(theme.clone());
    assert_eq!(s.world().background().len(), 1, "pattern in the background layer");
    let t = Text::new("x");
    assert_eq!(t.children[0].fill_color, Color::hex("#112233").unwrap());
    assert_eq!(Square::new(1.0).stroke_color, Color::hex("#112233").unwrap());
    let h = theme.text(TextRole::Heading, "Heading");
    assert_eq!(h.children[0].fill_color, Color::hex("#4F46E5").unwrap());
    s.add(Circle::new(1.0));
    s.clear();
    assert!(s.world().displayed().is_empty());
    assert_eq!(s.world().background().len(), 1, "clear keeps the background");
    let frame = s.render_frame();
    // Top-left pixel is paper (white-ish), not the default black.
    assert!(frame[0] > 200 && frame[1] > 200 && frame[2] > 200);
    // A new scene starts from default style again.
    drop(s);
    let _s2 = Scene::new(&cfg(), "Fresh").unwrap();
    assert_eq!(Square::new(1.0).stroke_color, WHITE);
}

#[test]
fn overlay_draws_on_top_and_survives_clear() {
    let mut s = Scene::new(&cfg(), "Layers").unwrap();
    s.add_overlay(Square::new(1.0).set_fill(RED, 1.0).set_z_index(-100));
    s.add(Square::new(2.0).set_fill(BLUE, 1.0).set_z_index(100));
    let (w, h) = (s.config().pixel_width as usize, s.config().pixel_height as usize);
    let center = |f: &[u8]| {
        let i = ((h / 2) * w + w / 2) * 4;
        [f[i], f[i + 1], f[i + 2]]
    };
    let f = s.render_frame();
    let c = center(&f);
    assert!(c[0] > 200 && c[2] < 120, "overlay is on top: {c:?}");
    s.clear();
    let c = center(&s.render_frame());
    assert!(c[0] > 200, "overlay kept by clear: {c:?}");
    assert!(s.inspect().is_empty(), "layers are not inspected");
}

#[test]
fn images_render_and_transform() {
    let (w, h) = (4u32, 4u32);
    let px: Vec<u8> = (0..w * h).flat_map(|_| [0u8, 200, 0, 255]).collect();
    let img = ImageMobject::from_rgba(w, h, px).set_height(4.0);
    assert!((img.height() - 4.0).abs() < 1e-9);
    let mut s = Scene::new(&cfg(), "Img").unwrap();
    let id = s.add(img);
    let (pw, ph) = (s.config().pixel_width as usize, s.config().pixel_height as usize);
    let at = |f: &[u8], x: usize, y: usize| {
        let i = (y * pw + x) * 4;
        [f[i], f[i + 1], f[i + 2]]
    };
    let f = s.render_frame();
    assert!(at(&f, pw / 2, ph / 2)[1] > 150, "green at the center");
    s.modify(id, |m| m.shift(RIGHT * 5.0));
    let f = s.render_frame();
    assert!(at(&f, pw / 2, ph / 2)[1] < 50, "moved away");
    s.modify(id, |m| m.set_opacity(0.0));
    let f = s.render_frame();
    assert!(at(&f, pw * 3 / 4 + 40, ph / 2)[1] < 50, "transparent when faded");
    // Images inspect like other mobjects.
    assert_eq!(s.inspect()[0].kind, "Image");
}

#[test]
fn cancellation_stops_a_render() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut c = cfg();
    c.cancel = Some(flag.clone());
    let f2 = flag.clone();
    let r = manim::render_with(&c, "Cancel", move |s| {
        let d = s.add(Dot::new());
        s.play(d.animate(|m| m.shift(RIGHT)));
        f2.store(true, Ordering::Relaxed);
        s.play(d.animate(|m| m.shift(LEFT)));
    });
    let e = r.expect_err("cancelled render fails");
    assert!(format!("{e:#}").contains("cancelled"));
}

#[test]
fn text_selection_and_wrapping() {
    let t = Text::new("Unroll each ring into a thin strip");
    assert_eq!(t.find_text("strip"), Some((29, 34)));
    let part = t.part("strip");
    assert_eq!(part.children.len(), 5);
    assert!(part.get_left().x > t.get_center().x, "the last word is on the right");
    let red = t.clone().set_color_text("ring", RED);
    let reds = red.family().iter().filter(|m| m.meta.char_index.is_some() && m.fill_color == RED).count();
    assert_eq!(reds, 4);
    let wrapped = Text::styled(&"word ".repeat(30), TextStyle::default().max_width(4.0));
    assert!(wrapped.width() <= 4.0 + 1e-6, "width {}", wrapped.width());
    assert!(wrapped.height() > Text::new("word").height() * 3.0);
    assert!(Text::markup("<b>unclosed", TextStyle::default()).is_err());
}

#[test]
fn timeline_json_and_sounds() {
    let dir = std::env::temp_dir().join(format!("manim-timeline-{}", std::process::id()));
    let mut c = cfg();
    c.dry_run = false;
    c.write_video = false;
    c.save_last_frame = false;
    c.write_timeline = true;
    c.output = Some(dir.clone());
    manim::render_with(&c, "Timed", |s| {
        s.section("intro");
        s.wait(0.6); // 9 frames at 15 fps
        s.mark("line-1");
        s.add_samples(vec![0.0f32; 24000].into(), 24000, 1, s.time(), 1.0);
        s.wait(1.0);
    })
    .unwrap();
    let json = std::fs::read_to_string(dir.join("Timed.timeline.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["markers"][1]["name"], "line-1");
    assert_eq!(v["markers"][1]["time"], 0.6);
    assert_eq!(v["markers"][0]["kind"], "section");
    assert_eq!(v["sounds"][0]["duration"], 1.0);
    assert_eq!(v["duration"], 1.6);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn sound_is_muxed_into_mp4() {
    let Some(_) = manim::output::find_ffmpeg() else {
        eprintln!("skipping: ffmpeg not found");
        return;
    };
    let dir = std::env::temp_dir().join(format!("manim-sound-{}", std::process::id()));
    let mut c = Config::default().quality(Quality::Low);
    c.progress = manim::events::ProgressMode::None;
    c.output = Some(dir.clone());
    c.format = OutputFormat::Mp4;
    c.encoder = Encoder::X264;
    manim::render_with(&c, "Voice", |s| {
        let tone: Vec<f32> = (0..24000).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        s.add_samples(tone.into(), 24000, 1, 0.2, 1.0);
        let d = s.add(Dot::new());
        s.play(d.animate(|m| m.shift(RIGHT)));
        s.wait(0.5);
    })
    .unwrap();
    let mp4 = dir.join("Voice.mp4");
    let out = std::process::Command::new(manim::output::find_ffmpeg().unwrap()).arg("-i").arg(&mp4).output().unwrap();
    let info = String::from_utf8_lossy(&out.stderr);
    assert!(info.contains("Audio: aac"), "{info}");
    assert!(info.contains("Video: h264"), "{info}");
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(feature = "math")]
#[test]
fn math_and_svg_are_tagged() {
    let m = MathTex::new("e^(i pi) + 1 = 0").unwrap();
    assert_eq!(m.meta.kind(), "Math");
    assert_eq!(m.text(), Some("e^(i pi) + 1 = 0"));
    let svg = SVGMobject::from_str(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="5"/></svg>"#,
    )
    .unwrap();
    assert_eq!(svg.meta.kind(), "SVG");
    assert!((svg.height() - 2.0).abs() < 1e-6);
}
