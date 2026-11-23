//! Rasterizer correctness, robustness and CPU/GPU parity.

mod common;

use manim::prelude::*;
use manim::renderer::cpu::{rasterize, CpuRenderer, Raster};
use manim::renderer::{DisplayList, DrawPath, Layer, PxSubpath, Renderer};

const W: u32 = 160;
const H: u32 = 90;

fn frame(mobs: Vec<VMobject>) -> DisplayList {
    let mut w = World::new();
    for m in mobs {
        let id = w.insert(m);
        w.show(id);
    }
    Camera::new(W, H).display_list(&w)
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

fn to_u8(c: Color) -> [u8; 3] {
    [(c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8]
}

fn near(a: [u8; 4], b: [u8; 3], tol: i32) -> bool {
    (0..3).all(|i| (a[i] as i32 - b[i] as i32).abs() <= tol) && a[3] == 255
}

#[test]
fn camera_maps_frame_to_pixels() {
    let cam = Camera::new(W, H);
    assert_eq!(cam.to_pixel(ORIGIN), [W as f32 / 2.0, H as f32 / 2.0]);
    let tl = cam.to_pixel(pt(-cam.frame_width() / 2.0, FRAME_HEIGHT / 2.0));
    assert!(tl[0].abs() < 1e-3 && tl[1].abs() < 1e-3);
}

#[test]
fn background_only() {
    let buf = rasterize(&frame(vec![]));
    assert_eq!(buf.len(), (W * H * 4) as usize);
    assert!(buf.chunks(4).all(|p| p == [0, 0, 0, 255]));
}

#[test]
fn solid_fill_color_is_exact() {
    let buf = rasterize(&frame(vec![Square::new(4.0).set_fill(RED, 1.0).set_stroke_width(0.0)]));
    assert!(near(px(&buf, W / 2, H / 2), to_u8(RED), 0));
    assert!(near(px(&buf, 2, 2), [0, 0, 0], 0));
}

#[test]
fn translucent_fill_blends() {
    let buf = rasterize(&frame(vec![Square::new(4.0).set_fill(WHITE, 0.5).set_stroke_width(0.0)]));
    assert!(near(px(&buf, W / 2, H / 2), [128, 128, 128], 1));
}

#[test]
fn even_odd_fill_leaves_hole() {
    let buf = rasterize(&frame(vec![Annulus::new(1.5, 3.5).set_color(GREEN)]));
    assert!(near(px(&buf, W / 2, H / 2), [0, 0, 0], 0), "inside the hole");
    let ring_x = W / 2 + (2.5 * W as f64 / Camera::new(W, H).frame_width()) as u32;
    assert!(near(px(&buf, ring_x, H / 2), to_u8(GREEN), 0), "on the ring");
}

#[test]
fn stroke_only_shape_is_hollow() {
    // 40 stroke units ≈ 4.5 px at this tiny resolution, so the sampled pixel is fully covered.
    let buf = rasterize(&frame(vec![Circle::new(3.0).set_stroke(YELLOW, 40.0)]));
    assert!(near(px(&buf, W / 2, H / 2), [0, 0, 0], 0));
    let edge = W / 2 + (3.0 * W as f64 / Camera::new(W, H).frame_width()).round() as u32;
    assert!(px(&buf, edge, H / 2)[0] > 100, "stroke drawn at the radius");
}

#[test]
fn z_order_paints_last_on_top() {
    let buf = rasterize(&frame(vec![
        Square::new(3.0).set_fill(BLUE, 1.0).set_z_index(1),
        Square::new(3.0).set_fill(RED, 1.0),
    ]));
    assert!(near(px(&buf, W / 2, H / 2), to_u8(BLUE), 0));
}

#[test]
fn rendering_is_deterministic() {
    let dl = frame(vec![Circle::new(2.0).set_fill(PINK, 0.4), Text::new("abc"), Star::new(5, 1.0, 0.4)]);
    assert_eq!(rasterize(&dl), rasterize(&dl));
    for raster in [Raster::Skia, Raster::Vello] {
        let mut r = CpuRenderer::with_raster(raster);
        let batch = r.render_batch(&[&dl, &dl, &dl]).unwrap();
        assert!(batch.iter().all(|f| *f == batch[0]), "{raster:?}");
    }
    // Adaptive alternates rasterizers while calibrating, then sticks to one per bucket.
    let mut r = CpuRenderer::with_raster(Raster::Adaptive);
    r.render_batch(&[&dl; 8]).unwrap();
    let batch = r.render_batch(&[&dl, &dl, &dl]).unwrap();
    assert!(batch.iter().all(|f| *f == batch[0]), "adaptive after calibration");
}

#[test]
fn vello_matches_tiny_skia() {
    // Same geometry and colors; only anti-aliasing along edges may differ.
    let dl = frame(vec![
        Circle::new(2.0).set_fill(PINK, 0.4),
        Square::new(3.0).set_stroke(YELLOW, 8.0),
        Star::new(5, 1.0, 0.4).set_fill(BLUE, 1.0),
    ]);
    let a = rasterize(&dl);
    let b = CpuRenderer::with_raster(Raster::Vello).render_batch(&[&dl]).unwrap().remove(0);
    let off = a.iter().zip(&b).filter(|(x, y)| x.abs_diff(**y) > 8).count();
    assert!(off * 100 < a.len(), "{off} of {} channels differ", a.len());
    let [r, g, bl, _] = px(&a, W / 2, H / 2);
    assert!(near(px(&b, W / 2, H / 2), [r, g, bl], 2), "interior color matches");
}

#[test]
fn hostile_geometry_does_not_panic() {
    let nan = Square::new(1.0).apply_function(|p| if p.x > 0.0 { pt(f64::NAN, p.y) } else { p });
    let inf = Line::new(ORIGIN, pt(f64::INFINITY, 0.0));
    let huge = Circle::new(1e9).set_fill(RED, 1.0);
    let tiny = Circle::new(1e-9).set_fill(RED, 1.0);
    let thick = Line::new(LEFT, RIGHT).set_stroke(WHITE, 1e6);
    let degenerate = Square::new(0.0).set_fill(BLUE, 1.0);
    let dl = frame(vec![nan, inf, huge, tiny, thick, degenerate]);
    for item in &dl.items {
        for sp in &item.subpaths {
            let finite =
                sp.start.iter().all(|v| v.is_finite()) && sp.curves.iter().flatten().flatten().all(|v| v.is_finite());
            assert!(finite, "non-finite subpaths must be dropped by the camera");
        }
    }
    rasterize(&dl);
    if let Some(mut gpu) = gpu() {
        gpu.render_batch(&[&dl]).unwrap();
    }
}

#[test]
fn odd_resolutions() {
    for (w, h) in [(1, 1), (3, 7), (101, 57), (854, 480)] {
        let mut world = World::new();
        let id = world.insert(Circle::new(2.0).set_fill(RED, 1.0));
        world.show(id);
        let dl = Camera::new(w, h).display_list(&world);
        assert_eq!(rasterize(&dl).len(), (w * h * 4) as usize);
    }
}

fn gpu() -> Option<Box<dyn Renderer>> {
    #[cfg(feature = "gpu")]
    match manim::renderer::gpu::GpuRenderer::new(W, H, false) {
        Ok(g) => return Some(Box::new(g)),
        Err(e) => eprintln!("skipping GPU checks: {e:#}"),
    }
    None
}

#[test]
fn gpu_matches_cpu() {
    let Some(mut gpu) = gpu() else { return };
    let scenes = [
        frame(vec![]),
        frame(vec![Square::new(4.0).set_fill(RED, 1.0).set_stroke_width(0.0)]),
        frame(vec![
            Circle::new(2.0).set_fill(BLUE, 0.5),
            Annulus::new(1.0, 2.5).set_color(GREEN).shift(RIGHT * 3.0),
            Star::new(5, 1.5, 0.6).set_fill(YELLOW, 0.8).shift(LEFT * 3.0),
            Text::new("GPU"),
        ]),
    ];
    let refs: Vec<&DisplayList> = scenes.iter().collect();
    let g = gpu.render_batch(&refs).unwrap();
    for (dl, gf) in scenes.iter().zip(&g) {
        let c = rasterize(dl);
        assert_eq!(c.len(), gf.len());
        let diff: u64 = c.iter().zip(gf).map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64).sum();
        let mean = diff as f64 / c.len() as f64;
        assert!(mean < 2.0, "CPU/GPU mean channel difference {mean:.3}");
        // Interiors must agree exactly-ish (edges differ by anti-aliasing method).
        assert!(near(
            px(gf, W / 2, H / 2),
            [px(&c, W / 2, H / 2)[0], px(&c, W / 2, H / 2)[1], px(&c, W / 2, H / 2)[2]],
            2
        ));
    }
    // Batches larger than one GPU chunk exercise the pipelined readback.
    let many: Vec<&DisplayList> = (0..37).map(|i| &scenes[i % 3]).collect();
    let out = gpu.render_batch(&many).unwrap();
    assert_eq!(out.len(), 37);
    for (i, f) in out.iter().enumerate() {
        assert_eq!(f, &g[i % 3], "frame {i} differs from the single-batch render");
    }
}

#[test]
fn gpu_odd_size() {
    #[cfg(feature = "gpu")]
    {
        let Ok(mut g) = manim::renderer::gpu::GpuRenderer::new(101, 57, false) else { return };
        let mut world = World::new();
        let id = world.insert(Circle::new(2.0).set_fill(RED, 1.0));
        world.show(id);
        let dl = Camera::new(101, 57).display_list(&world);
        assert_eq!(g.render_batch(&[&dl]).unwrap()[0].len(), 101 * 57 * 4);
        let wrong = Camera::new(50, 50).display_list(&world);
        assert!(g.render_batch(&[&wrong]).is_err(), "size mismatch is an error, not a panic");
    }
}

#[test]
fn frames_are_opaque_even_with_a_translucent_background() {
    let mut cam = Camera::new(W, H);
    cam.background = Color::rgba(0.2, 0.4, 0.6, 0.3);
    let dl = cam.display_list(&World::new());
    assert_eq!(dl.background[3], 1.0);
    for buf in [rasterize(&dl), CpuRenderer::with_raster(Raster::Vello).render_batch(&[&dl]).unwrap().remove(0)] {
        assert!(buf.chunks(4).all(|p| p[3] == 255 && p[2] > 140 && p[2] < 165), "opaque, unmultiplied bg");
    }
}

#[test]
fn non_finite_colors_draw_nothing_in_both_rasterizers() {
    let bad = Circle::new(2.0).set_fill(Color::rgba(f32::NAN, 0.5, 0.5, 1.0), 1.0).set_stroke_width(0.0);
    let mut worse = Square::new(2.0).set_fill(RED, f64::NAN);
    worse.stroke_opacity = f64::INFINITY;
    let mut dl = frame(vec![bad, worse]);
    dl.background = [0.2, 0.4, 0.6, 1.0]; // not black, so a black fill would show
    let a = rasterize(&dl);
    let bg = [51, 102, 153, 255];
    let b = CpuRenderer::with_raster(Raster::Vello).render_batch(&[&dl]).unwrap().remove(0);
    for (name, buf) in [("tiny-skia", a), ("vello", b)] {
        assert!(
            buf.chunks(4).all(|p| p.iter().zip(bg).all(|(a, b)| a.abs_diff(b) <= 1)),
            "{name}: broken colors must not paint (not even black)"
        );
    }
}

#[test]
fn pooled_buffers_are_fully_overwritten() {
    use manim::renderer::FramePool;
    // A recycled buffer full of junk must produce the same frame as a fresh one, for both
    // rasterizers and with or without a background layer.
    let mut with_layer = frame(vec![Circle::new(2.0).set_fill(RED, 0.5)]);
    let layer = DrawPath {
        subpaths: vec![PxSubpath {
            start: [0.0, 0.0],
            curves: vec![[[10.0, 0.0], [20.0, 5.0], [30.0, 30.0]]],
            closed: true,
        }],
        fill: Some([0.0, 0.5, 0.0, 1.0]),
        stroke: None,
        image: None,
        nonzero: true,
        ..Default::default()
    };
    with_layer.background_layer = Some(Layer(std::sync::Arc::new(vec![layer])));
    for dl in [frame(vec![Circle::new(2.0).set_fill(RED, 0.5)]), with_layer] {
        let mut kinds = vec![Raster::Skia];
        #[cfg(feature = "vello")]
        kinds.push(Raster::Vello);
        for raster in kinds {
            let want = CpuRenderer::with_raster(raster).render_batch(&[&dl]).unwrap().remove(0);
            let pool = FramePool::new();
            pool.put(vec![0xAB; (W * H * 4) as usize]);
            let mut r = CpuRenderer::with_raster(raster);
            r.use_pool(pool.clone());
            assert_eq!(pool.idle(), 1);
            let got = r.render_batch(&[&dl]).unwrap().remove(0);
            assert_eq!(pool.idle(), 0, "the dirty buffer was reused");
            assert!(got == want, "{raster:?}: pooled frame differs from a fresh one");
        }
    }
}

#[test]
fn gpu_background_layer_is_cached_and_matches_cpu() {
    #[cfg(feature = "gpu")]
    {
        // 64 is a multiple of 64 pixels: rows are not padded, which takes the single-copy readback.
        for (w, h) in [(64u32, 36u32), (W, H)] {
            let Ok(mut g) = manim::renderer::gpu::GpuRenderer::new(w, h, false) else { return };
            let layer = std::sync::Arc::new(vec![DrawPath {
                subpaths: vec![PxSubpath {
                    start: [4.0, 4.0],
                    curves: vec![[[30.0, 4.0], [30.0, 4.0], [30.0, 30.0]], [[30.0, 30.0], [4.0, 30.0], [4.0, 4.0]]],
                    closed: true,
                }],
                fill: Some([0.1, 0.6, 0.2, 1.0]),
                stroke: None,
                image: None,
                nonzero: true,
                ..Default::default()
            }]);
            let mut world = World::new();
            let id = world.insert(Circle::new(2.0).set_fill(RED, 0.7));
            world.show(id);
            let mut dl = Camera::new(w, h).display_list(&world);
            dl.background_layer = Some(Layer(layer.clone()));
            let moving: Vec<DisplayList> = (0..40)
                .map(|i| {
                    let mut d = dl.clone();
                    for item in &mut d.items {
                        for sp in &mut item.subpaths {
                            sp.start[0] += i as f32 * 0.5;
                            for c in &mut sp.curves {
                                for p in c.iter_mut() {
                                    p[0] += i as f32 * 0.5;
                                }
                            }
                        }
                    }
                    d
                })
                .collect();
            let refs: Vec<&DisplayList> = moving.iter().collect();
            let out = g.render_batch(&refs).unwrap(); // several chunks reuse the cached layer
            for (i, (dl, gf)) in moving.iter().zip(&out).enumerate() {
                let c = rasterize(dl);
                let diff: u64 = c.iter().zip(gf).map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64).sum();
                assert!((diff as f64 / c.len() as f64) < 2.0, "{w}x{h} frame {i}: GPU differs from CPU");
                // The layer's interior is drawn.
                assert!(near(px_at(gf, w, 12, 12), [26, 153, 51], 3), "{w}x{h} frame {i}: layer missing");
            }
        }
    }
}

fn px_at(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}
