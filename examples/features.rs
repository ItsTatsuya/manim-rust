//! Visual check of Studio-facing features: theme + pattern, roles, math, SVG, image.
use manim::prelude::*;
use manim::text::register_font_dir;

const THEME: &str = r##"{"name":"Notebook",
 "fonts":{"heading":"Plus Jakarta Sans","body":"Inter","code":"JetBrains Mono","hand":"Caveat"},
 "colors":{"background":"#FDFBF6","text":"#1F2937","muted":"#6B7280","primary":"#2563EB","secondary":"#EA580C",
           "accent":"#DB2777","positive":"#16A34A","negative":"#DC2626","key_fill":"#E8F0FE","example_fill":"#FFEDDF"},
 "sizes":{"title":54,"heading":40,"body":30,"small":22},
 "layout":{"margin":0.6,"title":"top-left","boxes":"card","accent":true,"pattern":"grid"}}"##;

fn main() -> anyhow::Result<()> {
    if let Ok(dir) = std::env::var("FONTS") {
        register_font_dir(&dir)?;
    }
    let theme = Theme::from_json(THEME)?;
    manim::render("Features", move |s| {
        s.set_theme(theme.clone());
        let t = theme.clone();
        let title = t.text(TextRole::Title, "Why circles have area πr²").to_corner(UL);
        s.add(title);
        let eq = MathTex::styled("A = pi r^2 = integral_0^r 2 pi rho dif rho", 52.0, t.colors.primary).unwrap();
        let card = RoundedRectangle::new(eq.width() + 0.8, eq.height() + 0.7, 0.22)
            .set_fill(t.colors.key_fill, 1.0)
            .set_stroke_width(0.0)
            .move_to(eq.get_center());
        let block = vgroup![card, eq].move_to(pt(-2.2, 0.6));
        s.add(block);
        let svg = SVGMobject::from_str(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
            <circle cx="50" cy="50" r="40" fill="#EA580C" fill-opacity="0.25" stroke="#EA580C" stroke-width="3"/>
            <path d="M50 50 L90 50" stroke="#1F2937" stroke-width="3"/>
            <text x="62" y="45" font-size="14" fill="#1F2937">r</text></svg>"##,
        )
        .unwrap()
        .move_to(pt(4.2, 0.6));
        s.add(svg);
        // A gradient test image.
        let (w, h) = (240u32, 120u32);
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, 200, 255]);
            }
        }
        let img = ImageMobject::from_rgba(w, h, px)
            .expect("pixel buffer matches its size")
            .set_height(1.4)
            .rotate(0.15)
            .move_to(pt(-4.0, -2.5));
        s.add(img);
        let note = t.text(TextRole::Hand, "the area grows with r²").move_to(pt(2.6, -2.4));
        let arrow = CurvedArrow::with_tip_length(note.get_top() + UP * 0.1, pt(3.4, -0.3), -0.6, 0.18)
            .set_color(t.colors.accent);
        s.add(note);
        s.add(arrow);
        let cap = t.text(TextRole::Caption, "Caption text in the muted color").to_edge(DOWN);
        let brace = Brace::new(&cap, UP).set_color(t.colors.secondary);
        s.add(cap);
        s.add(brace);
    })
}
