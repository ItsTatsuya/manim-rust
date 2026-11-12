"""Python Manim CE equivalents of the Rust examples, for head-to-head benchmarking.

Each scene mirrors examples/<name>.rs animation-for-animation.
Run: manim -qh --disable_caching bench/scenes.py Showcase
"""

import numpy as np
from manim import *


class SquareToCircle(Scene):
    def construct(self):
        circle = Circle(radius=1.0).set_fill(PINK, 0.5)
        square = Square(side_length=2.0).rotate(PI / 4)
        self.add(square)
        self.play(Create(square))
        self.play(Transform(square, circle))
        self.play(square.animate.shift(RIGHT * 2).set_color(BLUE))
        self.play(FadeOut(square, shift=DOWN))
        self.wait(0.5)


class Showcase(Scene):
    def construct(self):
        title = Text("Manim, in Rust", color=BLUE).to_edge(UP)
        self.play(Write(title))

        shapes = VGroup(
            Circle(radius=0.7).set_fill(RED, 0.6),
            Square(side_length=1.3).set_fill(GREEN, 0.6),
            Triangle().set_fill(YELLOW, 0.6),
            Star(5, outer_radius=0.8, inner_radius=0.35).set_fill(PURPLE, 0.6),
            RoundedRectangle(width=1.5, height=1.1, corner_radius=0.3).set_fill(TEAL, 0.6),
        ).arrange(RIGHT, buff=0.5)
        self.play(LaggedStart(*[DrawBorderThenFill(m) for m in shapes], lag_ratio=0.2))

        self.play(Indicate(shapes[2]))
        self.play(Rotate(shapes[3], TAU, run_time=1.5), shapes[1].animate.rotate(PI / 4).scale(1.2))
        target = shapes[4].copy().set_color(ORANGE)
        first = shapes[0].copy().set_color(MAROON)
        self.play(
            Transform(shapes[0], target, path_arc=PI),
            Transform(shapes[4], first, path_arc=PI),
        )

        self.play(*[FadeOut(m, shift=DOWN) for m in shapes])
        dot = Dot(LEFT * 3, color=YELLOW).scale(2)
        label = Text("moving dot", font_size=28)
        label.add_updater(lambda l: l.next_to(dot.get_center(), UP))
        self.add(dot, label)
        arrow = Arrow(LEFT * 3 + DOWN * 1.5, RIGHT * 3 + DOWN * 1.5)
        self.play(GrowFromPoint(arrow, LEFT * 3 + DOWN * 1.5))
        self.play(dot.animate.shift(RIGHT * 6), run_time=2, rate_func=rate_functions.ease_in_out_cubic)
        self.play(MoveAlongPath(dot, ArcBetweenPoints(RIGHT * 3, LEFT * 3, angle=PI * 0.8)), run_time=2, rate_func=smooth)
        label.clear_updaters()

        self.play(FadeOut(dot), FadeOut(label), Uncreate(arrow), FadeOut(title, shift=UP))
        end = Text("Fast.", font_size=96, weight=BOLD, color=GOLD)
        self.play(SpinInFromNothing(end))
        self.wait(1)


class Graphing(Scene):
    def construct(self):
        axes = Axes(x_range=[-4, 4, 1], y_range=[-1.5, 1.5, 0.5], x_length=10, y_length=5)
        # Text labels (no LaTeX), matching the Rust Axes::with_numbers.
        labels = VGroup()
        for x in [-4, -3, -2, -1, 1, 2, 3, 4]:
            labels.add(Text(str(x), font_size=24).next_to(axes.c2p(x, 0), DOWN, buff=0.15))
        for y in [-1.5, -1, -0.5, 0.5, 1, 1.5]:
            s = str(int(y)) if y == int(y) else str(y)
            labels.add(Text(s, font_size=24).next_to(axes.c2p(0, y), LEFT, buff=0.15))
        ax = VGroup(axes, labels)
        self.play(Create(ax, run_time=1.5, lag_ratio=0.05))

        sin = axes.plot(np.sin, color=BLUE)
        cos = axes.plot(np.cos, color=RED)
        self.play(Create(sin, run_time=1.5))
        self.play(TransformFromCopy(sin, cos))

        x = ValueTracker(-4)
        dot = always_redraw(lambda: Dot(axes.c2p(x.get_value(), np.sin(x.get_value())), color=YELLOW).scale(1.5))
        line = always_redraw(
            lambda: DashedLine(axes.c2p(x.get_value(), 0), axes.c2p(x.get_value(), np.sin(x.get_value())), color=YELLOW)
        )
        self.add(dot, line)
        self.play(x.animate.set_value(4), run_time=4, rate_func=linear)
        self.wait(0.5)
        dot.clear_updaters()
        line.clear_updaters()

        self.play(FadeOut(ax), FadeOut(sin), FadeOut(cos), FadeOut(dot), FadeOut(line))
        plane = NumberPlane()
        plane.prepare_for_nonlinear_transform()
        self.play(Create(plane, run_time=1.5, lag_ratio=0.02))
        self.play(
            plane.animate.apply_function(lambda p: np.array([p[0] + np.sin(p[1]), p[1] + np.sin(p[0]) * 0.5, 0.0])),
            run_time=2,
        )
        self.wait(1)


class Benchmark(Scene):
    def construct(self):
        colors = [BLUE, RED, GREEN, YELLOW, PURPLE, TEAL]
        mobs = []
        for i in range(400):
            t = float(i)
            x, y = np.sin(t * 0.37) * 6.0, np.cos(t * 0.61) * 3.3
            m = Circle(radius=0.3) if i % 2 == 0 else RegularPolygon(5 + i % 4, radius=0.35)
            m.set_fill(colors[i % len(colors)], 0.5).move_to([x, y, 0])
            mobs.append(m)
        self.add(*mobs)
        self.play(*[Rotate(m, TAU if i % 2 == 0 else -TAU, about_point=ORIGIN) for i, m in enumerate(mobs)], run_time=3)
