// Text and morph animations, rendered to contact sheets by
// `cargo run --release --example anim_gallery -- <out-dir>` for checking by eye.
import {
  Scene, Text, MathTex, Square, Circle, Triangle, RegularPolygon, Star, Line, Rectangle,
  Write, Unwrite, FadeIn, AddTextLetterByLetter, TypeWithCursor, Create, Transform, ReplacementTransform,
  TransformMatchingShapes, TransformMatchingTex, FadeTransform, UP, LEFT, RIGHT,
} from "manim";

export class WriteText extends Scene {
  construct() { this.play(Write(Text("Area of a circle", { fontSize: 64 }))); }
}
export class WriteMath extends Scene {
  construct() { this.play(Write(MathTex(`"Area" = 1/2 dot "base" dot "height"`, { fontSize: 60 })), { runTime: 2 }); }
}
export class FadeInText extends Scene {
  construct() { this.play(FadeIn(Text("Fade in, drifting up", { fontSize: 56 }), { shift: UP.mul(0.4) })); }
}
export class LetterByLetter extends Scene {
  construct() { this.play(AddTextLetterByLetter(Text("typewriter reveal", { fontSize: 56 }))); }
}
export class TypeCursor extends Scene {
  construct() {
    const t = Text("typed with a cursor", { fontSize: 56 });
    const cursor = Rectangle(0.06, 0.6, { color: "#58C4DD", fillOpacity: 1 }).moveTo(t.getLeft());
    this.play(TypeWithCursor(t, cursor));
  }
}
export class UnwriteText extends Scene {
  construct() {
    const t = Text("Unwrite me", { fontSize: 64 });
    this.add(t);
    this.play(Unwrite(t));
  }
}
export class CreateText extends Scene {
  construct() { this.play(Create(Text("Create on text", { fontSize: 64 }))); }
}
export class SquareToCircle extends Scene {
  construct() {
    const s = Square(3, { color: "#58C4DD", fillOpacity: 0.8 });
    this.add(s);
    this.play(Transform(s, Circle(1.6, { color: "#FC6255", fillOpacity: 0.8 })), { runTime: 1.5 });
  }
}
export class ShapeMorphs extends Scene {
  construct() {
    const a = Triangle({ color: "#83C167", fillOpacity: 0.8 }).scale(1.4).shift(LEFT.mul(3.5));
    const b = Circle(1.2, { color: "#FFFF00" }); // outline only
    const c = Star(5, { color: "#9A72AC", fillOpacity: 0.8 }).scale(1.2).shift(RIGHT.mul(3.5));
    this.add(a, b, c);
    this.play(
      Transform(a, RegularPolygon(6, { color: "#58C4DD", fillOpacity: 0.8 }).scale(1.4).shift(LEFT.mul(3.5))),
      Transform(b, Square(2.2, { color: "#FC6255", fillOpacity: 0.9 })),
      Transform(c, Circle(1.3, { color: "#FFFF00", fillOpacity: 0.8 }).shift(RIGHT.mul(3.5))),
      { runTime: 1.5 },
    );
  }
}
export class RingToStrip extends Scene {
  construct() {
    const ring = Circle(1.2, { color: "#58C4DD", strokeWidth: 26 }).shift(UP);
    this.add(ring);
    this.play(Transform(ring, Line([-3.7, -2, 0], [3.7, -2, 0], { color: "#58C4DD", strokeWidth: 26 })), { runTime: 1.6 });
  }
}
export class TextMorph extends Scene {
  construct() {
    const a = Text("hello", { fontSize: 96 });
    this.add(a);
    this.play(Transform(a, Text("world", { fontSize: 96, color: "#FFFF00" })), { runTime: 1.5 });
  }
}
export class FormulaSteps extends Scene {
  construct() {
    const s2 = MathTex("A = 1/2 dot 2 pi r dot r", { fontSize: 60 });
    this.add(s2);
    const s3 = MathTex("A = pi r dot r", { fontSize: 60 });
    this.play(ReplacementTransform(s2, s3), { runTime: 1.5 });
    const s4 = MathTex("A = pi r^2", { fontSize: 84, color: "#58C4DD" });
    this.play(ReplacementTransform(s3, s4), { runTime: 1.5 });
  }
}
export class MatchingShapes extends Scene {
  construct() {
    const a = MathTex("a^2 + b^2 = c^2", { fontSize: 72 });
    this.add(a);
    this.play(TransformMatchingShapes(a, MathTex("c^2 - b^2 = a^2", { fontSize: 72 })), { runTime: 1.5 });
  }
}
export class MatchingTex extends Scene {
  construct() {
    const a = MathTex("A = pi r dot r", { fontSize: 72 });
    this.add(a);
    this.play(TransformMatchingTex(a, MathTex("A = pi r^2", { fontSize: 72 })), { runTime: 1.5 });
  }
}
export class FadeTransformText extends Scene {
  construct() {
    const a = Text("before", { fontSize: 80 });
    this.add(a);
    this.play(FadeTransform(a, Text("after it", { fontSize: 80, color: "#83C167" })), { runTime: 1.5 });
  }
}
