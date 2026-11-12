import { Text, VGroup, SurroundingRectangle, Mobject, UL, UP, LEFT, type Theme } from "manim";

export const THEME: Theme = {
  name: "Studio",
  fonts: { heading: "Plus Jakarta Sans", body: "Inter", code: "JetBrains Mono", hand: "Caveat" },
  colors: {
    background: "#FFFFFF", text: "#0F172A", muted: "#64748B", primary: "#4F46E5", secondary: "#0284C7",
    accent: "#E11D48", positive: "#059669", negative: "#DC2626", key_fill: "#EEF0FF", example_fill: "#E6F4FB",
  },
  sizes: { title: 54, heading: 40, body: 30, small: 22 },
  layout: { margin: 0.65, title: "top-left", boxes: "card", accent: true, pattern: "dots" },
};

export function title(text: string): Mobject {
  return Text(text, { role: "title" }).toCorner(UL, 0.65);
}

export function keyBox(content: Mobject, label: string): Mobject {
  const tag = Text(label.toUpperCase(), { role: "caption", weight: "bold", color: THEME.colors!.primary });
  tag.nextTo(content, UP, 0.18, LEFT);
  const inner = VGroup(content, tag);
  const frame = SurroundingRectangle(inner, { buff: 0.34, cornerRadius: 0.22, strokeWidth: 0, fillColor: THEME.colors!.key_fill, fillOpacity: 1, color: THEME.colors!.key_fill });
  return VGroup(frame, inner);
}
