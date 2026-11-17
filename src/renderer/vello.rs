//! Second CPU rasterizer, built on Linebender's vello_cpu (sparse strips, SIMD).
//!
//! It has a higher fixed cost per frame than tiny-skia but scales much better with path
//! count: on a 400-shape scene at 1080p it is ~3× faster, while on light scenes it is
//! ~15% slower. [`super::cpu::CpuRenderer`] measures both and picks per frame.
//! Frames with raster images are left to tiny-skia ([`rasterize`] returns `None`).

use std::cell::RefCell;

use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{BezPath, Cap, Join, Stroke};
use vello_cpu::peniko::Fill;
use vello_cpu::{PixmapMut, RasterizerSettings, RenderContext, Resources, TargetInit};

use super::{DisplayList, DrawPath, FramePool};

thread_local! {
    /// One context per rayon worker, reused across frames (its buffers are the expensive part).
    static CTX: RefCell<Option<(RenderContext, Resources)>> = const { RefCell::new(None) };
}

fn color(c: [f32; 4]) -> AlphaColor<Srgb> {
    AlphaColor::new(c)
}

fn draw_items(ctx: &mut RenderContext, items: &[DrawPath]) {
    let mut path = BezPath::new();
    for item in items {
        path.truncate(0);
        for sp in &item.subpaths {
            path.move_to((sp.start[0] as f64, sp.start[1] as f64));
            for c in &sp.curves {
                path.curve_to(
                    (c[0][0] as f64, c[0][1] as f64),
                    (c[1][0] as f64, c[1][1] as f64),
                    (c[2][0] as f64, c[2][1] as f64),
                );
            }
            if sp.closed {
                path.close_path();
            }
        }
        if let Some(fill) = item.fill {
            ctx.set_paint(color(fill));
            ctx.set_fill_rule(if item.nonzero { Fill::NonZero } else { Fill::EvenOdd });
            ctx.fill_path(&path);
        }
        if let Some((c, width)) = item.stroke {
            ctx.set_paint(color(c));
            ctx.set_stroke(Stroke::new(width as f64).with_caps(Cap::Round).with_join(Join::Round));
            ctx.stroke_path(&path);
        }
    }
}

/// Rasterize one display list into RGBA8 in a buffer from `pool`, or `None` if it contains images.
pub fn rasterize_pooled(dl: &DisplayList, pool: &FramePool) -> Option<Vec<u8>> {
    if dl.has_images() {
        return None;
    }
    let (w, h) = (dl.width.max(1), dl.height.max(1));
    let (Ok(w16), Ok(h16)) = (u16::try_from(w), u16::try_from(h)) else { return None };
    // A background layer is rasterized once and cached by the tiny-skia path; start from
    // that so both rasterizers produce the same base and vello only draws the scene.
    let (mut buf, init) = match &dl.background_layer {
        Some(_) => (super::cpu::base_pixmap(dl, pool).take(), TargetInit::SrcOver),
        None => (pool.take(w as usize * h as usize * 4), TargetInit::Clear(color(dl.background))),
    };
    CTX.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.as_ref().is_none_or(|(c, _)| c.width() != w16 || c.height() != h16) {
            *cell = Some((RenderContext::new(w16, h16), Resources::new()));
        }
        let (ctx, res) = cell.as_mut().unwrap();
        ctx.reset();
        draw_items(ctx, &dl.items);
        ctx.flush();
        let target = PixmapMut::new(w16, h16, &mut buf).expect("buffer matches frame size");
        ctx.render_with(target, res, RasterizerSettings { target_init: init, ..Default::default() });
    });
    // The background is opaque, so premultiplied == straight alpha here.
    Some(buf)
}
