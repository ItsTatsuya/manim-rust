# Porting a Python Manim scene to manim-rust

Most of a Manim scene carries over line by line. The main change is that you hold
**ids** instead of objects.

## The classic quickstart

<table>
<tr><th>Python Manim</th><th>manim-rust</th></tr>
<tr><td>

```python
from manim import *

class SquareToCircle(Scene):
    def construct(self):
        circle = Circle().set_fill(PINK, 0.5)
        square = Square().rotate(PI / 4)

        self.play(Create(square))
        self.play(Transform(square, circle))
        self.play(square.animate.shift(2 * RIGHT))
        self.play(FadeOut(square, shift=DOWN))
        self.wait(0.5)
```

</td><td>

```rust
use manim::prelude::*;

fn main() -> anyhow::Result<()> {
    manim::render("SquareToCircle", |s| {
        let circle = Circle::new(1.0).set_fill(PINK, 0.5);
        let square = s.insert(Square::new(2.0).rotate(PI / 4.0));

        s.play(Create::new(square));
        s.play(Transform::new(square, circle));
        s.play(square.animate(|m| m.shift(RIGHT * 2.0)));
        s.play(FadeOut::new(square).shift(DOWN));
        s.wait(0.5);
    })
}
```

</td></tr>
</table>

Save it as `square_to_circle.rs` and run `manim-rs square_to_circle.rs -ql`. The
command-line flags are the same as Manim's, and `SquareToCircle.mp4` appears next to the
script. Install the command once with `cargo install --path <manim-rust clone>`.

## Cheat sheet

| Python | Rust |
|---|---|
| `Circle()` | `Circle::new(1.0)` (defaults become explicit arguments) |
| `Square()` | `Square::new(2.0)` |
| `Dot(point)` | `Dot::at(point)` or `Dot::new()` |
| `Text("hi", font_size=36, color=RED)` | `Text::styled("hi", TextStyle::default().size(36.0).color(RED))` |
| `VGroup(a, b, c)` | `vgroup![a, b, c]` or `VGroup::new(vec![a, b, c])` |
| `group.arrange(DOWN, buff=0.5)` | `group.arrange_buff(DOWN, 0.5)` |
| `m.next_to(other, UP)` | `m.next_to(&other, UP)`. `other` is a `VMobject`; for an id use `s.get(id)` |
| `self.add(m)` | `let id = s.add(m);` |
| `self.remove(m)` | `s.remove(id);` |
| `self.play(Create(m))` | `s.play(Create::new(id));` |
| `self.play(A, B)` | `s.play((a, b));`, or `s.play(anims![a, b, c])` for many |
| `self.play(A, run_time=3, rate_func=linear)` | `s.play(a.run_time(3.0).rate_func(linear));` |
| `LaggedStart(*anims, lag_ratio=0.2)` | `LaggedStart::new(anims).lag_ratio(0.2)` |
| `m.animate.shift(UP).scale(2)` | `id.animate(\|m\| m.shift(UP).scale(2.0))` |
| `Transform(a, b)` (both on screen) | `Transform::new(a_id, s.get(b_id).clone())` |
| `ReplacementTransform(a, b)` | `ReplacementTransform::new(a_id, b_id)` where `b_id = s.insert(...)` |
| `FadeIn(m, shift=UP)` | `FadeIn::new(id).shift(UP)` |
| `Rotate(m, PI, about_point=ORIGIN)` | `Rotate::new(id, PI).about(ORIGIN)` |
| `t = ValueTracker(0)` | `let t = s.tracker(0.0);` |
| `self.play(t.animate.set_value(5))` | `s.play(ChangeValue::new(t, 5.0));` |
| `t.get_value()` | `w.value(t)` in closures, `s.value(t)` elsewhere |
| `always_redraw(lambda: Dot(ax.c2p(t.get_value(), 0)))` | `s.always_redraw(move \|w\| Dot::at(ax.c2p(w.value(t), 0.0)))` |
| `m.add_updater(lambda m, dt: m.rotate(dt))` | `s.add_updater(move \|w, dt\| w.modify(id, \|m\| m.rotate(dt)));` |
| `axes.plot(lambda x: x**2, color=BLUE)` | `axes.plot(\|x\| x * x).set_stroke(BLUE, 4.0)` |
| `axes.add_coordinates()` | `Axes::new(...).with_numbers()` |
| `config.background_color = WHITE` | `Config { background: WHITE, ..Config::default() }` with `render_with` |

## Things to watch for

1. **Clone before reuse.** Builder methods consume the mobject. To use a shape twice, call
   `.clone()`: `let a = Square::new(1.0); let b = a.clone().shift(RIGHT);`
2. **`insert` vs `add`.** `add` shows the mobject immediately. Animations that introduce
   something (`Create`, `Write`, `FadeIn`, `GrowFromCenter`) display it themselves, so
   `insert` it first to avoid a one-frame flash.
3. **Updaters and animations on the same mobject.** As in Python, updaters run after the
   animation each frame, so an updater that rebuilds a mobject overrides an animation on it.
   Call `s.clear_updaters()` (or `remove_updater`) first.
4. **Closures need `move`.** Ids, trackers and `Axes` values captured by updaters must be
   moved in. `Axes` is `Clone`, so clone it per closure.
5. **Typst, not LaTeX.** Equations use Typst math syntax: write `MathTex::new("theta^2")` (feature `math`) instead of
   `MathTex(r"	heta^2")`. Without that feature, use `Text("θ²")`.
