// A narrated-explainer style scene: theme, text roles, highlights, math, updaters, markers.
// Render: manim-rs examples/scripts/lesson.ts -qm   (fonts fall back to system ones if missing)
import {
  Scene, Text, MathTex, Circle, VGroup, RoundedRectangle, SurroundingRectangle, ValueTracker,
  Write, Create, FadeIn, FadeOut, GrowFromEdge, Transform, LaggedStart, Indicate, alwaysRedraw, DecimalNumber,
  UP, DOWN, LEFT, RIGHT, UL, ORIGIN, PI, vec,
} from "manim";
import { THEME, title, keyBox } from "./style";


export default class Lesson extends Scene {
  construct(): void {
    this.setTheme(THEME);
    const t = this.theme!;

    this.mark("line-1");
    const heading = title("Why circles have area πr²");
    this.play(Write(heading));
    const rings = VGroup(...[1, 2, 3, 4, 5].map((k) => Circle(0.3 * k, { color: t.colors!.primary })));
    this.play(Create(rings), { runTime: 1.5 });

    this.mark("line-2");
    const sentence = Text("Unroll each ring into a thin strip", { role: "body" });
    const card = keyBox(sentence, "Key idea");
    const group = VGroup(card).nextTo(heading, DOWN, 0.6, LEFT);
    this.play(FadeOut(rings), { runTime: 0.4 });
    this.play(FadeIn(group, { shift: UP.mul(0.15) }), { runTime: 0.6 });
    const strip = sentence.part("strip");
    const mark = SurroundingRectangle(strip, { buff: 0.07, cornerRadius: 0.06, color: t.colors!.secondary, fillOpacity: 0.35, strokeWidth: 0 });
    this.play(GrowFromEdge(mark, LEFT));

    this.mark("line-3");
    const eq = MathTex("A = pi r^2", { fontSize: 60, color: t.colors!.primary }).shift(DOWN.mul(1.5));
    this.play(Write(eq));
    this.play(Indicate(eq));

    // An updater-driven counter.
    const r = new ValueTracker(1);
    const label = alwaysRedraw(() => DecimalNumber(PI * r.getValue() ** 2, { numDecimalPlaces: 2, role: "caption" }).nextTo(eq, RIGHT, 0.5));
    this.add(label);
    this.play(r.animate.setValue(2), { runTime: 1 });
    this.play(eq.animate.shift(LEFT.mul(2)).setColor(t.colors!.accent));
    group.get(0).get(1).setColor(t.colors!.negative); // the card's label, inside the group
    this.wait(0.5);
    this.play(LaggedStart(...this.mobjects.map((m) => FadeOut(m))));
  }
}
