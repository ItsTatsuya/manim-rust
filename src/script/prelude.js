// The `manim` module for TypeScript/JavaScript scene scripts.
// Mobjects are handles into the Rust engine; animations are plain specs played by it.
const N = globalThis.__manim_native;

// Every mobject is a handle into the engine's table. When its JS object is garbage
// collected the handle goes back to the engine (temporaries like `alwaysRedraw`'s
// rebuilt mobject would otherwise pile up). The epoch tells the engine which run a
// handle belongs to, so a late release never frees another run's object.
const FREE = typeof FinalizationRegistry === "function" ? new FinalizationRegistry((r) => N.release(r[0], r[1])) : null;
let EPOCH = N.epoch(), ALLOCS = 0;
function own(m) {
  if (FREE) {
    FREE.register(m, [m._h, EPOCH]);
    // QuickJS queues finalizers as jobs; run them now and then, also in scripts that never play.
    if ((++ALLOCS & 255) === 0) N.collect();
  }
  return m;
}

// ------------------------------------------------------------------ vectors

/** A 3D vector with arithmetic helpers. Plain `[x, y]` / `[x, y, z]` arrays work anywhere too. */
export class Vec3 extends Array {
  constructor(x = 0, y = 0, z = 0) {
    super(3);
    this[0] = x; this[1] = y; this[2] = z;
  }
  static from(v) { return new Vec3(v[0] ?? 0, v[1] ?? 0, v[2] ?? 0); }
  get x() { return this[0]; }
  get y() { return this[1]; }
  get z() { return this[2]; }
  add(o) { o = Vec3.from(o); return new Vec3(this[0] + o[0], this[1] + o[1], this[2] + o[2]); }
  sub(o) { o = Vec3.from(o); return new Vec3(this[0] - o[0], this[1] - o[1], this[2] - o[2]); }
  mul(k) { return new Vec3(this[0] * k, this[1] * k, this[2] * k); }
  scale(k) { return this.mul(k); }
  times(k) { return this.mul(k); }
  div(k) { return this.mul(1 / k); }
  neg() { return this.mul(-1); }
  dot(o) { o = Vec3.from(o); return this[0] * o[0] + this[1] * o[1] + this[2] * o[2]; }
  norm() { return Math.hypot(this[0], this[1], this[2]); }
  normalize() { const n = this.norm(); return n ? this.div(n) : new Vec3(); }
  rotate(a) { const c = Math.cos(a), s = Math.sin(a); return new Vec3(this[0] * c - this[1] * s, this[0] * s + this[1] * c, this[2]); }
  lerp(o, t) { o = Vec3.from(o); return this.add(o.sub(this).mul(t)); }
}
export const vec = (x = 0, y = 0, z = 0) => new Vec3(x, y, z);
export const ORIGIN = vec(0, 0), UP = vec(0, 1), DOWN = vec(0, -1), LEFT = vec(-1, 0), RIGHT = vec(1, 0);
export const OUT = vec(0, 0, 1), IN = vec(0, 0, -1);
export const UL = vec(-1, 1), UR = vec(1, 1), DL = vec(-1, -1), DR = vec(1, -1);
export const PI = Math.PI, TAU = 2 * Math.PI, DEGREES = Math.PI / 180;
export const SMALL_BUFF = 0.1, MED_SMALL_BUFF = 0.25, MED_LARGE_BUFF = 0.5, LARGE_BUFF = 1;
export const DEFAULT_FONT_SIZE = 48, DEFAULT_STROKE_WIDTH = 4;
export const FRAME_HEIGHT = 8, FRAME_WIDTH = 8 * 16 / 9;
export const THIN = 100, LIGHT = 300, NORMAL = 400, MEDIUM = 500, SEMIBOLD = 600, BOLD = 700, HEAVY = 900;

// ------------------------------------------------------------------ colors
export const WHITE = "#FFFFFF",
  BLACK = "#000000",
  GRAY_A = "#DDDDDD",
  GRAY_B = "#BBBBBB",
  GRAY_C = "#888888",
  GRAY_D = "#444444",
  GRAY_E = "#222222",
  BLUE_A = "#C7E9F1",
  BLUE_B = "#9CDCEB",
  BLUE_C = "#58C4DD",
  BLUE_D = "#29ABCA",
  BLUE_E = "#236B8E",
  TEAL_A = "#ACEAD7",
  TEAL_B = "#76DDC0",
  TEAL_C = "#5CD0B3",
  TEAL_D = "#55C1A7",
  TEAL_E = "#49A88F",
  GREEN_A = "#C9E2AE",
  GREEN_B = "#A6CF8C",
  GREEN_C = "#83C167",
  GREEN_D = "#77B05D",
  GREEN_E = "#699C52",
  YELLOW_A = "#FFF1B6",
  YELLOW_B = "#FFEA94",
  YELLOW_C = "#F7D96F",
  YELLOW_D = "#F4D345",
  YELLOW_E = "#E8C11C",
  GOLD_A = "#F7C797",
  GOLD_B = "#F9B775",
  GOLD_C = "#F0AC5F",
  GOLD_D = "#E1A158",
  GOLD_E = "#C78D46",
  RED_A = "#F7A1A3",
  RED_B = "#FF8080",
  RED_C = "#FC6255",
  RED_D = "#E65A4C",
  RED_E = "#CF5044",
  MAROON_A = "#ECABC1",
  MAROON_B = "#EC92AB",
  MAROON_C = "#C55F73",
  MAROON_D = "#A24D61",
  MAROON_E = "#94424F",
  PURPLE_A = "#CAA3E8",
  PURPLE_B = "#B189C6",
  PURPLE_C = "#9A72AC",
  PURPLE_D = "#715582",
  PURPLE_E = "#644172",
  PINK = "#D147BD",
  LIGHT_PINK = "#DC75CD",
  ORANGE = "#FF862F",
  LIGHT_BROWN = "#CD853F",
  DARK_BROWN = "#8B4513",
  GRAY_BROWN = "#736357",
  PURE_RED = "#FF0000",
  PURE_GREEN = "#00FF00",
  PURE_BLUE = "#0000FF",
  PURE_CYAN = "#00FFFF",
  PURE_MAGENTA = "#FF00FF",
  PURE_YELLOW = "#FFFF00",
  GRAY = "#888888",
  GREY = "#888888",
  LIGHT_GRAY = "#BBBBBB",
  DARK_GRAY = "#444444",
  BLUE = "#58C4DD",
  DARK_BLUE = "#236B8E",
  TEAL = "#5CD0B3",
  GREEN = "#83C167",
  YELLOW = "#F7D96F",
  GOLD = "#F0AC5F",
  RED = "#FC6255",
  MAROON = "#C55F73",
  PURPLE = "#9A72AC";
/** Blend two "#RRGGBB" colors. */
export function interpolateColor(a, b, t) {
  const p = (c) => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16));
  const [x, y] = [p(a), p(b)];
  return "#" + x.map((v, i) => Math.round(v + (y[i] - v) * t).toString(16).padStart(2, "0")).join("").toUpperCase();
}

// ------------------------------------------------------------------ encoding

function enc(v) {
  if (v instanceof Mobject) return { $m: v._h };
  if (v instanceof ValueTracker) return v._id;
  if (Array.isArray(v)) return Array.from(v, enc);
  if (v && typeof v === "object") {
    const o = {};
    for (const k of Object.keys(v)) if (v[k] !== undefined && typeof v[k] !== "function") o[k] = enc(v[k]);
    return o;
  }
  return v;
}
const js = (v) => JSON.stringify(enc(v));
const camel = (s) => s.replace(/_([a-z])/g, (_, c) => c.toUpperCase());
/** Accept snake_case option names too (`font_size` → `fontSize`). */
function opts(o) {
  if (!o || typeof o !== "object" || Array.isArray(o)) return o ?? {};
  const out = {};
  for (const k of Object.keys(o)) out[camel(k)] = o[k];
  return out;
}

// ------------------------------------------------------------------ mobjects

/** Base class of everything drawn. Methods mutate the mobject and return it (chainable). */
export class Mobject {
  constructor(h) { this._h = h; own(this); }
  _op(name, ...args) { N.op(this._h, name, js(args)); return this; }
  _q(name, ...args) { return JSON.parse(N.op(this._h, name, js(args))); }
  // geometry
  shift(...vectors) { return this._op("shift", ...vectors); }
  moveTo(target, alignedEdge) { return this._op("moveTo", target, alignedEdge); }
  nextTo(target, direction = RIGHT, buff = MED_SMALL_BUFF, alignedEdge) {
    if (direction && !Array.isArray(direction) && typeof direction === "object") { const o = opts(direction); return this._op("nextTo", target, o.direction ?? RIGHT, o.buff ?? MED_SMALL_BUFF, o.alignedEdge); }
    if (buff && typeof buff === "object" && !Array.isArray(buff)) { const o = opts(buff); return this._op("nextTo", target, direction, o.buff ?? MED_SMALL_BUFF, o.alignedEdge); }
    return this._op("nextTo", target, direction, buff, alignedEdge);
  }
  toEdge(edge = LEFT, buff = MED_LARGE_BUFF) { return this._op("toEdge", edge, buff); }
  toCorner(corner = DL, buff = MED_LARGE_BUFF) { return this._op("toCorner", corner, buff); }
  alignTo(target, direction = UP) { return this._op("alignTo", target, direction); }
  scale(factor, aboutPoint) { if (aboutPoint && !Array.isArray(aboutPoint)) aboutPoint = opts(aboutPoint).aboutPoint; return this._op("scale", factor, aboutPoint); }
  stretch(factor, dim) { return this._op("stretch", factor, dim); }
  rotate(angle, aboutPoint) { if (aboutPoint && !Array.isArray(aboutPoint)) aboutPoint = opts(aboutPoint).aboutPoint; return this._op("rotate", angle, aboutPoint); }
  flip() { return this._op("flip"); }
  setWidth(w) { return this._op("setWidth", w); }
  setHeight(h) { return this._op("setHeight", h); }
  scaleToFitWidth(w) { return this._op("setWidth", w); }
  scaleToFitHeight(h) { return this._op("setHeight", h); }
  matchWidth(m) { return this._op("matchWidth", m); }
  matchHeight(m) { return this._op("matchHeight", m); }
  setX(x) { return this._op("setX", x); }
  setY(y) { return this._op("setY", y); }
  center() { return this._op("center"); }
  arrange(direction = RIGHT, buff = MED_SMALL_BUFF, alignedEdge) {
    if (buff && typeof buff === "object" && !Array.isArray(buff)) { const o = opts(buff); return this._op("arrange", direction, o.buff ?? MED_SMALL_BUFF, o.alignedEdge); }
    return this._op("arrange", direction, buff, alignedEdge);
  }
  arrangeInGrid(cols = 3, buff = MED_SMALL_BUFF) { return this._op("arrangeInGrid", cols, buff); }
  /** Shrink (or with grow, enlarge up to maxGrow×) to fit width × height. */
  fitWithin(width, height, grow = false, maxGrow = 1.5) { return this._op("fitWithin", width, height, grow, maxGrow); }
  applyFunction(fn) {
    const fam = this._q("points");
    const mapped = fam.map((sps) => sps.map((pts) => pts.map((p) => { const r = fn(Vec3.from(p)); return [r[0], r[1] ?? 0, r[2] ?? 0]; })));
    return this._op("setPoints", mapped);
  }
  // style
  setColor(c) { return this._op("setColor", c); }
  setFill(color, opacity) { if (color && typeof color === "object" && !Array.isArray(color)) { const o = opts(color); return this._op("setFill", o.color, o.opacity); } return this._op("setFill", color, opacity); }
  setFillColor(c) { return this._op("setFillColor", c); }
  setFillOpacity(o) { return this._op("setFillOpacity", o); }
  setStroke(color, width, opacity) { if (color && typeof color === "object" && !Array.isArray(color)) { const o = opts(color); return this._op("setStroke", o.color, o.width, o.opacity); } return this._op("setStroke", color, width, opacity); }
  setStrokeColor(c) { return this._op("setStrokeColor", c); }
  setStrokeWidth(w) { return this._op("setStrokeWidth", w); }
  setStrokeOpacity(o) { return this._op("setStrokeOpacity", o); }
  setOpacity(o) { return this._op("setOpacity", o); }
  fade(darkness = 0.5) { return this._op("fade", darkness); }
  setColorByGradient(...colors) { return this._op("setColorByGradient", colors.flat()); }
  setZIndex(z) { return this._op("setZIndex", z); }
  /** Dash the stroke: `length` long dashes with `gap` between (scene units); animate `phase` to make them march. */
  setDash(length = 0.1, gap = length, phase = 0) { return this._op("setDash", length, gap, phase); }
  clearDash() { return this._op("clearDash"); }
  /** Linear gradient fill over the bounding box. `angle` in radians (0 = left to right); stops are `[position, color, opacity?]`. */
  setGradient(angle, stops) { return this._op("setGradient", angle, stops); }
  /** Soft drop shadow outside the outline. */
  setShadow(color = "#000000", opacity = 0.35, blur = 0.3, dx = 0, dy = -0.2) { return this._op("setShadow", color, opacity, blur, dx, dy); }
  clearShadow() { return this._op("clearShadow"); }
  /** Blur everything drawn below this mobject, inside its outline, by `sigma` scene units. */
  setBackdropBlur(sigma) { return this._op("setBackdropBlur", sigma); }
  /** Keep this mobject (and its children) in the base frame when the camera moves. */
  pin(on = true) { return this._op("pin", on); }
  /** Clip this mobject (and its children) to a rectangle of the base frame. */
  clipTo(left, bottom, right, top) { return this._op("clipTo", left, bottom, right, top); }
  clearClip() { return this._op("clearClip"); }
  /** Recolor every occurrence of `needle` in a text. */
  setColorText(needle, color) { return this._op("setColorText", needle, color); }
  setColorChars(start, end, color) { return this._op("setColorChars", start, end, color); }
  become(other, keepZ = false) { return this._op("become", other, keepZ); }
  // queries
  getCenter() { return Vec3.from(this._q("getCenter")); }
  getTop() { return Vec3.from(this._q("getTop")); }
  getBottom() { return Vec3.from(this._q("getBottom")); }
  getLeft() { return Vec3.from(this._q("getLeft")); }
  getRight() { return Vec3.from(this._q("getRight")); }
  getCorner(dir) { return Vec3.from(this._q("getCorner", dir)); }
  getCriticalPoint(dir) { return Vec3.from(this._q("getCriticalPoint", dir)); }
  getEdgeCenter(dir) { return Vec3.from(this._q("getCriticalPoint", dir)); }
  getStart() { return Vec3.from(this._q("getStart")); }
  getEnd() { return Vec3.from(this._q("getEnd")); }
  pointFromProportion(a) { return Vec3.from(this._q("pointFromProportion", a)); }
  get width() { return this._q("width"); }
  get height() { return this._q("height"); }
  getWidth() { return this.width; }
  getHeight() { return this.height; }
  /** [left, bottom, right, top] */
  get bbox() { return this._q("bbox"); }
  get zIndex() { return this._q("zIndex"); }
  getColor() { return this._q("getColor"); }
  getFillOpacity() { return this._q("getFillOpacity"); }
  get kind() { return this._q("kind"); }
  // structure
  copy() { return wrap(N.copy(this._h), Object.getPrototypeOf(this)); }
  /** Member `i` of a group (negative counts from the end). Changes to it change the group. */
  get(i) { return childOf(this, i, Mobject.prototype); }
  get length() { return this._q("length"); }
  get submobjects() { const n = this.length; const out = []; for (let i = 0; i < n; i++) out.push(this.get(i)); return out; }
  [Symbol.iterator]() { return this.submobjects[Symbol.iterator](); }
  add(...mobs) { N.addChildren(this._h, JSON.stringify(mobs.flat().map((m) => m._h))); return this; }
  get animate() { return new AnimateBuilder(this); }
  addUpdater(fn) { currentScene()._addUpdater(this, fn); return this; }
  removeUpdater(fn) { currentScene()._removeUpdater(this, fn); return this; }
  clearUpdaters() { currentScene()._removeUpdater(this, null); return this; }
}

function wrap(h, proto) { const m = Object.create(proto ?? Mobject.prototype); m._h = h; return own(m); }
/** The same wrapper (and handle) every time member `i` of `parent` is asked for, so `g.get(0) === g.get(0)` and a loop over a group does not make a handle per access. */
function childOf(parent, i, proto) {
  const n = parent.length;
  const idx = i < 0 ? n + i : i;
  if (!(idx >= 0 && idx < n)) return wrap(N.child(parent._h, i), proto); // throws the usual out-of-range error
  const cache = parent._kids ?? (parent._kids = new Map());
  let w = cache.get(idx);
  if (!w || Object.getPrototypeOf(w) !== proto) { w = wrap(N.child(parent._h, idx), proto); cache.set(idx, w); }
  return w;
}
// A wrapper for a handle that other wrappers may share (scene mobjects): never released.
function wrapShared(h) { const m = Object.create(Mobject.prototype); m._h = h; return m; }

/** Text helpers shared by Text, MarkupText, MathTex... */
class TextMobject extends Mobject {
  /** The source string. */
  get text() { return this._q("text"); }
  /** Glyphs of characters start..end (string indices; negatives count from the end). A copy, for measuring and framing. */
  slice(start = 0, end) { return wrap(N.chars(this._h, start, end ?? 1e9), Mobject.prototype); }
  chars(start, end) { return this.slice(start, end); }
  /** Glyphs of the first occurrence of `needle` (a copy). */
  part(needle) { return wrap(N.part(this._h, needle), Mobject.prototype); }
  findText(needle) { return this._q("findText", needle); }
}

// A constructor usable with or without `new` (Manim's Python habits carry over).
function defineMobject(name, Base, build, init) {
  const C = function (...args) {
    const m = Object.create(C.prototype);
    m._h = build(...args);
    own(m);
    if (init) init(m, ...args);
    return m;
  };
  Object.defineProperty(C, "name", { value: name });
  C.prototype = Object.create(Base.prototype, { constructor: { value: C } });
  return C;
}
const mk = (kind, o) => N.mk(kind, js(opts(o)));
const num = (a, key, o) => (typeof a === "number" ? { ...opts(o), [key]: a } : opts(a));

export const VMobject = defineMobject("VMobject", Mobject, (o) => mk("VMobject", o));
export const VGroup = defineMobject("VGroup", Mobject, (...mobs) => {
  const list = mobs.flat().filter((m) => m instanceof Mobject);
  return list.length ? N.group(JSON.stringify(list.map((m) => m._h))) : mk("VGroup", {});
});
export const Group = VGroup;
export const Circle = defineMobject("Circle", Mobject, (a, o) => mk("Circle", num(a, "radius", o)));
export const Square = defineMobject("Square", Mobject, (a, o) => mk("Square", num(a, "sideLength", o)));
export const Rectangle = defineMobject("Rectangle", Mobject, (a, b, o) =>
  typeof a === "number" ? mk("Rectangle", { ...opts(o), width: a, height: b }) : mk("Rectangle", a));
export const RoundedRectangle = defineMobject("RoundedRectangle", Mobject, (o) => mk("RoundedRectangle", o));
export const Ellipse = defineMobject("Ellipse", Mobject, (o) => mk("Ellipse", o));
export const Dot = defineMobject("Dot", Mobject, (a, o) => (Array.isArray(a) ? mk("Dot", { ...opts(o), point: a }) : mk("Dot", a)));
const twoPoint = (kind) => (a, b, o) => (Array.isArray(a) ? mk(kind, { ...opts(o), start: a, end: b }) : mk(kind, a));
export const Line = defineMobject("Line", Mobject, twoPoint("Line"));
export const DashedLine = defineMobject("DashedLine", Mobject, twoPoint("DashedLine"));
export const Arrow = defineMobject("Arrow", Mobject, twoPoint("Arrow"));
export const DoubleArrow = defineMobject("DoubleArrow", Mobject, twoPoint("DoubleArrow"));
export const CurvedArrow = defineMobject("CurvedArrow", Mobject, twoPoint("CurvedArrow"));
export const ArcBetweenPoints = defineMobject("ArcBetweenPoints", Mobject, twoPoint("ArcBetweenPoints"));
export const Vector = defineMobject("Vector", Mobject, (a, o) => (Array.isArray(a) ? mk("Vector", { ...opts(o), direction: a }) : mk("Vector", a)));
export const Arc = defineMobject("Arc", Mobject, (o) => mk("Arc", o));
export const Annulus = defineMobject("Annulus", Mobject, (o) => mk("Annulus", o));
export const Sector = defineMobject("Sector", Mobject, (o) => mk("Sector", o));
export const Polygon = defineMobject("Polygon", Mobject, (...pts) => {
  const o = pts.length && !Array.isArray(pts[pts.length - 1]) ? pts.pop() : {};
  return mk("Polygon", { ...opts(o), points: pts });
});
export const Polyline = defineMobject("Polyline", Mobject, (...pts) => {
  const o = pts.length && !Array.isArray(pts[pts.length - 1]) ? pts.pop() : {};
  return mk("Polyline", { ...opts(o), points: pts });
});
export const RegularPolygon = defineMobject("RegularPolygon", Mobject, (a, o) => mk("RegularPolygon", num(a, "n", o)));
export const Triangle = defineMobject("Triangle", Mobject, (o) => mk("Triangle", o));
export const Star = defineMobject("Star", Mobject, (a, o) => mk("Star", num(a, "n", o)));
export const SurroundingRectangle = defineMobject("SurroundingRectangle", Mobject, (t, o) => mk("SurroundingRectangle", { ...opts(o), target: t }));
export const BackgroundRectangle = defineMobject("BackgroundRectangle", Mobject, (t, o) => mk("BackgroundRectangle", { ...opts(o), target: t }));
export const Brace = defineMobject("Brace", Mobject, (t, d, o) =>
  Array.isArray(d) ? mk("Brace", { ...opts(o), target: t, direction: d }) : mk("Brace", { ...opts(d), target: t }));
export const Text = defineMobject("Text", TextMobject, (s, o) => mk("Text", { ...opts(o), text: String(s) }));
export const Paragraph = defineMobject("Paragraph", TextMobject, (...lines) => {
  const o = lines.length && typeof lines[lines.length - 1] === "object" ? lines.pop() : {};
  return mk("Text", { ...opts(o), text: lines.join("\n") });
});
export const MarkupText = defineMobject("MarkupText", TextMobject, (s, o) => mk("MarkupText", { ...opts(o), text: String(s) }));
export const MathTex = defineMobject("MathTex", TextMobject, (...parts) => {
  const o = parts.length > 1 && typeof parts[parts.length - 1] === "object" ? parts.pop() : {};
  return mk("MathTex", { ...opts(o), source: parts.join(" ") });
});
export const Tex = defineMobject("Tex", TextMobject, (s, o) => mk("Typst", { ...opts(o), source: String(s) }));
export const Typst = Tex;
export const MathTypst = MathTex;
export const DecimalNumber = defineMobject("DecimalNumber", TextMobject, (v, o) => {
  const p = opts(o);
  return mk("DecimalNumber", { ...p, value: v, decimals: p.numDecimalPlaces ?? p.decimals ?? 2 });
});
export const Integer = defineMobject("Integer", TextMobject, (v, o) => mk("Integer", { ...opts(o), value: v }));
export const ImageMobject = defineMobject("ImageMobject", Mobject, (path, o) => mk("ImageMobject", { ...opts(o), path }));
export const SVGMobject = defineMobject("SVGMobject", Mobject, (src, o) =>
  String(src).trimStart().startsWith("<") ? mk("SVGMobject", { ...opts(o), svg: src }) : mk("SVGMobject", { ...opts(o), path: src }));
export const NumberPlane = defineMobject("NumberPlane", Mobject, (o) => mk("NumberPlane", o));

/** A smooth curve through sampled points; non-finite samples break it. */
function curve(points, o) {
  const runs = [[]];
  for (const p of points) {
    if (p && p.every((v) => Number.isFinite(v))) runs[runs.length - 1].push(p);
    else if (runs[runs.length - 1].length) runs.push([]);
  }
  return mk("SmoothCurve", { ...opts(o), runs });
}
export const ParametricFunction = defineMobject("ParametricFunction", Mobject, (fn, o) => {
  const p = opts(o);
  const [t0, t1] = p.tRange ?? [0, 1];
  const n = p.samples ?? 200;
  const pts = [];
  for (let i = 0; i <= n; i++) { const t = t0 + ((t1 - t0) * i) / n; const v = fn(t); pts.push([v[0], v[1], v[2] ?? 0]); }
  return curve(pts, p);
});
export const FunctionGraph = defineMobject("FunctionGraph", Mobject, (fn, o) => {
  const p = opts(o);
  const [x0, x1] = p.xRange ?? [-FRAME_WIDTH / 2, FRAME_WIDTH / 2];
  const n = p.samples ?? 200;
  const pts = [];
  for (let i = 0; i <= n; i++) { const x = x0 + ((x1 - x0) * i) / n; pts.push([x, fn(x), 0]); }
  return curve(pts, p);
});

/** Coordinate axes. `c2p(x, y)` maps graph coordinates to scene points. */
export const Axes = defineMobject("Axes", Mobject, (o) => mk("Axes", o), (m, o) => { m._axesOpts = opts(o); m.__center = m.getCenter(); m.__size = [m.getWidth(), m.getHeight()]; });
Object.assign(Axes.prototype, {
  _info() {
    if (!this.__info) this.__info = JSON.parse(N.axesInfo(js(this._axesOpts ?? {})));
    return this.__info;
  },
  // How much the axes were scaled since creation (per axis), from their current size.
  _scale() {
    const [w0, h0] = this.__size ?? [0, 0];
    return [w0 > 1e-9 ? this.getWidth() / w0 : 1, h0 > 1e-9 ? this.getHeight() / h0 : 1];
  },
  c2p(x, y) {
    // Follows the axes if they were moved or scaled after creation (not rotated).
    const i = this._info(), base = Vec3.from(i.origin).add(vec(x * i.xUnit, y * i.yUnit));
    const now = this.getCenter(), was = Vec3.from(this.__center ?? now), [kx, ky] = this._scale();
    const d = base.sub(was);
    return now.add(vec(d[0] * kx, d[1] * ky, d[2]));
  },
  coordsToPoint(x, y) { return this.c2p(x, y); },
  p2c(p) { const i = this._info(), [kx, ky] = this._scale(), d = Vec3.from(p).sub(this.c2p(0, 0)); return [d[0] / (i.xUnit * kx), d[1] / (i.yUnit * ky)]; },
  pointToCoords(p) { return this.p2c(p); },
  plot(fn, o) {
    const p = opts(o);
    const [x0, x1] = p.xRange ?? this._info().xRange;
    const n = p.samples ?? 200;
    const pts = [];
    for (let k = 0; k <= n; k++) { const x = x0 + ((x1 - x0) * k) / n; const y = fn(x); pts.push(Number.isFinite(y) ? this.c2p(x, y) : [NaN, NaN, 0]); }
    return wrap(curve(pts, p), Mobject.prototype);
  },
  plotParametricCurve(fn, o) {
    const p = opts(o);
    const [t0, t1] = p.tRange ?? [0, 1];
    const n = p.samples ?? 200;
    const pts = [];
    for (let k = 0; k <= n; k++) { const t = t0 + ((t1 - t0) * k) / n; const [x, y] = fn(t); pts.push(this.c2p(x, y)); }
    return wrap(curve(pts, p), Mobject.prototype);
  },
  getVerticalLine(point, o) { const [x] = this.p2c(point); return DashedLine(this.c2p(x, 0), point, o); },
  getHorizontalLine(point, o) { const [, y] = this.p2c(point); return DashedLine(this.c2p(0, y), point, o); },
});

// ------------------------------------------------------------------ tables, matrices, code

/** camelCase the keys of nested config objects too (`line_config: { stroke_width }`). */
function nested(p, ...keys) {
  for (const k of keys) if (p[k] && typeof p[k] === "object" && !Array.isArray(p[k])) p[k] = opts(p[k]);
  return p;
}
function cellPos(pos) {
  if (!Array.isArray(pos) || pos.length !== 2) throw new TypeError("a cell position is [row, col], 1-based");
  return [pos[0], pos[1]];
}

/** A row or column of a table or matrix: a copy for measuring and framing, whose style changes also reach the entries. */
class GridSlice extends Mobject {}
for (const name of ["setColor", "setFill", "setFillColor", "setFillOpacity", "setStroke", "setStrokeColor", "setStrokeWidth", "setStrokeOpacity", "setOpacity", "fade"]) {
  const f = function (...a) {
    Mobject.prototype[name].apply(this, a);
    for (const v of this._members) Mobject.prototype[name].apply(v, a);
    return this;
  };
  GridSlice.prototype[name] = f;
  GridSlice.prototype[name.replace(/[A-Z]/g, (c) => "_" + c.toLowerCase())] = f;
}
function gridSlice(members) {
  const copies = members.map((v) => v.copy());
  const s = wrap(copies.length ? N.group(JSON.stringify(copies.map((c) => c._h))) : mk("VGroup", {}), GridSlice.prototype);
  s._members = members;
  return s;
}

/** A group whose members are text-like (table entries, code lines): `get(i)` gives text helpers. */
class TextGroup extends Mobject {
  get(i) { return childOf(this, i, TextMobject.prototype); }
}

/** Shared by Table and Matrix. Positions are 1-based [row, col], labels included. */
class GridMobject extends Mobject {
  _grid() {
    const g = this._q("gridInfo");
    if (!g) throw new Error("this mobject is not a table or a matrix");
    return g;
  }
  /** Every entry (a live group, row by row), or the entry at [row, col]. */
  getEntries(pos) {
    const g = this._grid(), e = wrap(N.child(this._h, g.entries), TextGroup.prototype);
    if (pos == null) return e;
    const [r, c] = cellPos(pos);
    if (r < 1 || c < 1 || r > g.rows || c > g.cols) throw new RangeError(`no cell [${r}, ${c}] in a ${g.rows}×${g.cols} grid`);
    return e.get((r - 1) * g.cols + (c - 1));
  }
  /** Entries as rows of live mobjects. */
  getMobMatrix() {
    const g = this._grid(), e = this.getEntries(), out = [];
    for (let r = 0; r < g.rows; r++) {
      const row = [];
      for (let c = 0; c < g.cols; c++) row.push(e.get(r * g.cols + c));
      out.push(row);
    }
    return out;
  }
  getRows() { return this.getMobMatrix().map(gridSlice); }
  getColumns() { const m = this.getMobMatrix(); return (m[0] ?? []).map((_, c) => gridSlice(m.map((row) => row[c]))); }
  getEntriesWithoutLabels() { const g = this._grid(); return this.getMobMatrix().slice(g.colLabels ? 1 : 0).flatMap((r) => r.slice(g.rowLabels ? 1 : 0)); }
  getRowLabels() { const g = this._grid(); return g.rowLabels ? this.getMobMatrix().slice(g.colLabels ? 1 : 0).map((r) => r[0]) : []; }
  getColLabels() { const g = this._grid(); return g.colLabels ? this.getMobMatrix()[0].slice(g.rowLabels ? 1 : 0) : []; }
  getLabels() { return [...this.getRowLabels(), ...this.getColLabels()]; }
  /** One color per row (labels included). */
  setRowColors(...colors) { return this._op("setRowColors", colors); }
  /** One color per column (labels included). */
  setColumnColors(...colors) { return this._op("setColumnColors", colors); }
  /** The outline of a cell, as a new polygon. */
  getCell(pos, o) { return wrap(mk("TableCell", { ...opts(o), target: this, pos: cellPos(pos) }), Mobject.prototype); }
  /** A filled rectangle covering a cell, as a new mobject. */
  getHighlightedCell(pos, o) {
    const p = typeof o === "string" ? { color: o } : opts(o);
    return wrap(mk("TableHighlightedCell", { ...p, target: this, pos: cellPos(pos) }), Mobject.prototype);
  }
  /** Put a highlight (default PURE_YELLOW) behind a cell. */
  addHighlightedCell(pos, o) {
    const p = typeof o === "string" ? { color: o } : opts(o);
    return this._op("addHighlightedCell", cellPos(pos), p.color ?? PURE_YELLOW);
  }
  addBackgroundToEntries(color = BLACK) {
    if (color && typeof color === "object" && !Array.isArray(color)) color = opts(color).color ?? BLACK;
    return this._op("addBackgroundToEntries", color);
  }
  _part(i) { return i == null ? VGroup() : this.get(i); }
  getHorizontalLines() { return this._part(this._grid().hlines); }
  getVerticalLines() { return this._part(this._grid().vlines); }
  /** A matrix's [left, right] brackets (a live group). */
  getBrackets() { return this._part(this._grid().brackets); }
}

class TableMobject extends GridMobject {
  get horizontalLines() { return this.getHorizontalLines(); }
  get verticalLines() { return this.getVerticalLines(); }
  /** Manim's `table.create()`: lines, then entries, then labels. Adds the table to the scene. */
  create(o) {
    const p = opts(o);
    const lag = p.lagRatio ?? 1;
    delete p.lagRatio;
    currentScene().add(this);
    const parts = [AnimationGroup(Create(this.getVerticalLines(), p), Create(this.getHorizontalLines(), p))];
    const entries = this.getEntriesWithoutLabels();
    if (entries.length) parts.push(AnimationGroup(...entries.map((e) => Create(e, p))));
    const labels = this.getLabels();
    if (labels.length) parts.push(AnimationGroup(...labels.map((l) => Write(l, p))));
    return LaggedStart(...parts, { lagRatio: lag });
  }
}

const tableKind = (kind) => (rows, o) => mk(kind, nested({ ...opts(o), table: rows }, "elementToMobjectConfig", "lineConfig"));
/** A table of text entries (`\n` breaks lines). Options: rowLabels, colLabels, topLeftEntry, vBuff, hBuff, includeOuterLines, ... */
export const Table = defineMobject("Table", TableMobject, tableKind("Table"));
/** A table of equations (Typst math syntax). */
export const MathTable = defineMobject("MathTable", TableMobject, tableKind("MathTable"));
export const IntegerTable = defineMobject("IntegerTable", TableMobject, tableKind("IntegerTable"));
/** Decimal entries; `elementToMobjectConfig: { numDecimalPlaces }` (default 1). */
export const DecimalTable = defineMobject("DecimalTable", TableMobject, tableKind("DecimalTable"));
export const MobjectTable = defineMobject("MobjectTable", TableMobject, tableKind("MobjectTable"));

const matrixKind = (kind) => (rows, o) => mk(kind, nested({ ...opts(o), matrix: rows }, "elementToMobjectConfig", "bracketConfig"));
/** A matrix of equations (Typst math syntax: "pi", "x^2") in brackets. Options: leftBracket, rightBracket, vBuff, hBuff, ... */
export const Matrix = defineMobject("Matrix", GridMobject, matrixKind("Matrix"));
export const IntegerMatrix = defineMobject("IntegerMatrix", GridMobject, matrixKind("IntegerMatrix"));
/** Decimal entries; `elementToMobjectConfig: { numDecimalPlaces }` (default 1). */
export const DecimalMatrix = defineMobject("DecimalMatrix", GridMobject, matrixKind("DecimalMatrix"));
export const MobjectMatrix = defineMobject("MobjectMatrix", GridMobject, matrixKind("MobjectMatrix"));
/** "det" left of a matrix, and "= determinant" right of it when given. */
export function getDetText(matrix, o) {
  const p = typeof o === "object" && o !== null ? opts(o) : { determinant: o };
  return wrap(mk("DetText", { target: matrix, determinant: p.determinant }), Mobject.prototype);
}
export const get_det_text = getDetText;

class CodeMobject extends Mobject {
  _kindPart(kind, proto) { const i = this._q("childIndexOfKind", kind); return i == null ? null : wrap(N.child(this._h, i), proto); }
  /** One live text group per line: `code.codeLines.get(2)` is the third line. */
  get codeLines() { return this._kindPart("CodeLines", TextGroup.prototype); }
  /** One live group per line number (null without line numbers). */
  get lineNumbers() { return this._kindPart("CodeLineNumbers", TextGroup.prototype); }
  /** The background (with the window buttons as members). */
  get background() { return this._kindPart("CodeBackground", Mobject.prototype); }
}
/**
 * A syntax-highlighted listing. `Code({ codeString, language, formatterStyle, background: "window" })`,
 * `Code("path/to/file.py")`, or `Code(sourceText, options)`.
 */
export const Code = defineMobject("Code", CodeMobject, (a, o) => {
  let p;
  if (typeof a === "string") {
    // A lone path-looking string is a file; anything else is source code.
    const isFile = /^[^\s()"'=;{}<>]+\.[A-Za-z0-9]+$/.test(a);
    p = { ...opts(o), [isFile ? "codeFile" : "codeString"]: a };
  } else p = opts(a);
  return mk("Code", nested(p, "backgroundConfig", "paragraphConfig"));
});
/** Names of the color styles (Pygments' names: "vim", "monokai", "dracula", ...). */
Code.getStylesList = () => JSON.parse(N.codeStyles());
Code.get_styles_list = Code.getStylesList;
/** Languages the highlighter knows. */
Code.getLanguages = () => JSON.parse(N.codeLanguages());
Code.get_languages = Code.getLanguages;

/** A number that animations can change (`tracker.animate.setValue(5)`). */
export class ValueTracker {
  constructor(value = 0) { this._id = N.trackerNew(value); }
  getValue() { return N.trackerGet(this._id); }
  setValue(v) { N.trackerSet(this._id, v); return this; }
  incrementValue(d) { return this.setValue(this.getValue() + d); }
  get value() { return this.getValue(); }
  set value(v) { this.setValue(v); }
  get animate() {
    const t = this;
    return {
      setValue: (v, o) => anim("ChangeValue", { tracker: t._id, value: v, ...opts(o) }),
      incrementValue: (d, o) => anim("ChangeValue", { tracker: t._id, value: t.getValue() + d, ...opts(o) }),
    };
  }
}

/** Rebuild a mobject from `fn` every frame. */
export function alwaysRedraw(fn) {
  const m = fn();
  m.addUpdater((mm) => mm.become(fn()));
  return m;
}
export const always_redraw = alwaysRedraw;

// ------------------------------------------------------------------ animations

class Animation {
  constructor(spec) { this._spec = spec; }
  /** Override timing: `.set({ runTime: 2, rateFunc: "linear" })`. */
  set(o) { Object.assign(this._spec, opts(o)); return this; }
}
function anim(type, o) { return new Animation({ type, ...opts(o) }); }
function specOf(a) {
  if (a instanceof Animation) return enc(a._spec);
  if (a instanceof AnimateBuilder) return enc(a._toSpec());
  throw new TypeError("play() takes animations (FadeIn(m), m.animate.shift(UP), ...), got " + (a && a.constructor ? a.constructor.name : typeof a));
}
// Factories work with or without `new`.
const A1 = (type) => function (target, o) { return anim(type, { ...opts(o), target }); };
export const FadeIn = A1("FadeIn");
export const FadeOut = A1("FadeOut");
export const Create = A1("Create");
export const Uncreate = A1("Uncreate");
export const Write = A1("Write");
export const Unwrite = A1("Unwrite");
export const AddTextLetterByLetter = A1("AddTextLetterByLetter");
export const RemoveTextLetterByLetter = A1("RemoveTextLetterByLetter");
export const TypeWithCursor = function (target, cursor, o) { return anim("TypeWithCursor", { ...opts(o), target, cursor }); };
export const UntypeWithCursor = function (target, cursor, o) { return anim("UntypeWithCursor", { ...opts(o), target, cursor }); };
export const DrawBorderThenFill = A1("DrawBorderThenFill");
export const ShowPassingFlash = A1("ShowPassingFlash");
export const GrowFromCenter = A1("GrowFromCenter");
export const GrowArrow = A1("GrowArrow");
export const SpinInFromNothing = A1("SpinInFromNothing");
export const ShrinkToCenter = A1("ShrinkToCenter");
export const Indicate = A1("Indicate");
export const GrowFromPoint = function (target, point, o) { return anim("GrowFromPoint", { ...opts(o), target, point }); };
export const GrowFromEdge = function (target, edge, o) { return anim("GrowFromEdge", { ...opts(o), target, edge }); };
export const Transform = function (a, b, o) { return anim("Transform", { ...opts(o), target: a, to: b }); };
export const ReplacementTransform = function (a, b, o) { return anim("ReplacementTransform", { ...opts(o), target: a, to: b }); };
export const TransformFromCopy = function (a, b, o) { return anim("TransformFromCopy", { ...opts(o), target: a, to: b }); };
export const FadeTransform = function (a, b, o) { return anim("FadeTransform", { ...opts(o), target: a, to: b }); };
export const TransformMatchingShapes = function (a, b, o) { return anim("TransformMatchingShapes", { ...opts(o), target: a, to: b }); };
export const TransformMatchingTex = function (a, b, o) { return anim("TransformMatchingTex", { ...opts(o), target: a, to: b }); };
export const Rotate = function (target, angle = PI, o) { return anim("Rotate", { ...opts(o), target, angle }); };
export const Rotating = Rotate;
export const MoveAlongPath = function (target, path, o) { return anim("MoveAlongPath", { ...opts(o), target, path }); };
/** Move and zoom the camera frame. `{zoom|width, center}`, `{focus: {point, anchor}, zoom}` to push in on a point while it glides to `anchor`, or `{reset: true}`. */
export const MoveCamera = function (o) { return anim("MoveCamera", o); };
export const Wait = function (runTime = 1) { return anim("Wait", { runTime }); };
const G = (type) => function (...items) {
  const o = items.length && !(items[items.length - 1] instanceof Animation) && !(items[items.length - 1] instanceof AnimateBuilder) && !Array.isArray(items[items.length - 1]) ? opts(items.pop()) : {};
  return anim(type, { ...o, anims: items.flat().map(specOf) });
};
export const AnimationGroup = G("AnimationGroup");
export const LaggedStart = G("LaggedStart");
export const Succession = G("Succession");

/** `m.animate.shift(UP).setColor(RED)`: records calls, played as one smooth change. */
class AnimateBuilder {
  constructor(m) { this._m = m; this._ops = []; this._opts = {}; }
  set(o) { Object.assign(this._opts, opts(o)); return this; }
  _toSpec() { return { type: "Animate", target: this._m, ops: this._ops, ...this._opts }; }
}
for (const name of Object.getOwnPropertyNames(Mobject.prototype)) {
  const d = Object.getOwnPropertyDescriptor(Mobject.prototype, name);
  if (typeof d.value !== "function" || name.startsWith("_") || name === "constructor" || name.startsWith("get") || name === "copy" || name === "add" || name.endsWith("Updater") || name === "clearUpdaters" || name === "applyFunction") continue;
  AnimateBuilder.prototype[name] = function (...args) {
    // Reuse the method's argument handling by recording what it would send.
    const rec = Object.create(Mobject.prototype);
    rec._ops = this._ops;
    rec._op = function (n, ...a) { this._ops.push([n, a]); return this; };
    Mobject.prototype[name].apply(rec, args);
    return this;
  };
}

// ------------------------------------------------------------------ scene

let CURRENT = null;
function currentScene() {
  if (!CURRENT) throw new Error("no scene is running: create updaters and trackers inside construct()");
  return CURRENT;
}

export class Scene {
  constructor() {
    this._updaters = [];
    // The updaters active at the start of each play/wait, by segment index, so a seek
    // replays a segment with exactly the updaters it had (not ones added later).
    this._segments = [];
  }
  /** Override this: build and animate the scene. */
  construct() {}
  add(...mobs) { N.add(JSON.stringify(mobs.flat().map((m) => m._h))); return this; }
  remove(...mobs) { N.remove(JSON.stringify(mobs.flat().map((m) => m._h))); return this; }
  clear() { N.clear(); return this; }
  bringToFront(...mobs) { for (const m of mobs.flat()) N.layer(m._h, "front"); return this; }
  bringToBack(...mobs) { for (const m of mobs.flat()) N.layer(m._h, "back"); return this; }
  /** Draw below everything; kept by clear() and ignored by layout checks. */
  addBackground(...mobs) { for (const m of mobs.flat()) N.layer(m._h, "background"); return this; }
  /** Draw above everything (watermarks, captions); kept by clear(). */
  addOverlay(...mobs) { for (const m of mobs.flat()) N.layer(m._h, "overlay"); return this; }
  /** Mobjects currently in the scene, in draw order. */
  get mobjects() { return JSON.parse(N.mobjects()).map((h) => wrapShared(h)); }
  isOnScreen(m) { return N.isShown(m._h); }
  /** Scene time in seconds. */
  get time() { return N.time(); }
  get renderer() { return { time: N.time() }; }
  get frameWidth() { return JSON.parse(N.frameSize())[0]; }
  get frameHeight() { return JSON.parse(N.frameSize())[1]; }
  get fps() { return N.fps(); }
  play(...items) {
    let o = {};
    const last = items[items.length - 1];
    if (last && typeof last === "object" && !(last instanceof Animation) && !(last instanceof AnimateBuilder) && !Array.isArray(last)) o = opts(items.pop());
    const specs = items.flat().map(specOf).map((s) => ({ ...s, ...enc(o) }));
    this._run(this._begin(N.beginPlay(JSON.stringify(specs))));
    return this;
  }
  wait(seconds = 1) { this._run(this._begin(N.beginWait(seconds))); return this; }
  /** Name this moment (e.g. where a narration line starts). Returns the time. */
  mark(name) { return N.mark(String(name), "marker"); }
  /** Start a chapter here. Returns the time. */
  nextSection(name = "section") { return N.mark(String(name), "section"); }
  section(name) { return this.nextSection(name); }
  /** Play an audio file from the project folder, starting now (or at `timeOffset` seconds from now). */
  addSound(path, o) { const p = opts(typeof o === "number" ? { timeOffset: o } : o); N.addSound(String(path), p.at ?? (p.timeOffset != null ? N.time() + p.timeOffset : -1), p.gain ?? 1); return this; }
  /** Apply a theme: an object, JSON text, or a theme file path in the project. */
  setTheme(theme) { N.setTheme(typeof theme === "string" ? theme : JSON.stringify(theme)); return this; }
  get theme() { const t = N.theme(); return t ? JSON.parse(t) : null; }
  get camera() { return { set backgroundColor(c) { N.setBackgroundColor(c); }, get frameWidth() { return JSON.parse(N.frameSize())[0]; }, get frameHeight() { return JSON.parse(N.frameSize())[1]; } }; }
  /** Run `fn(dt)` every frame. */
  addUpdater(fn) { this._updaters.push({ m: null, fn }); return this; }
  removeUpdater(fn) { this._updaters = this._updaters.filter((u) => u.fn !== fn); return this; }
  /** What is on screen: kind, label, bbox, opacity for each mobject. */
  inspect() { return JSON.parse(N.inspect()); }
  _addUpdater(m, fn) { this._updaters.push({ m, fn }); fn(m, 0); }
  _removeUpdater(m, fn) { this._updaters = this._updaters.filter((u) => !(u.m === m && (fn == null || u.fn === fn))); }
  _begin(pid) { this._segments[N.playbackSegment(pid)] = this._updaters.slice(); return pid; }
  _tick(dt, updaters) {
    for (const u of updaters) {
      if (u.m) u.fn(u.m, dt); else u.fn(dt);
    }
  }
  _run(pid, updaters = this._updaters) {
    if (!updaters.length) { N.playAll(pid); return; }
    for (;;) {
      const dt = N.step(pid);
      if (dt < 0) break;
      this._tick(dt, updaters);
      N.endFrame(pid, dt);
      N.collect(); // give back the handles of this frame's temporaries
    }
  }
}

/** Define a scene from a function: `export default scene("Intro", (s) => { ... })`. */
export function scene(name, construct) {
  const C = class extends Scene { construct() { construct(this); } };
  Object.defineProperty(C, "name", { value: name });
  return C;
}

export function log(...a) { N.log(a.map((x) => (typeof x === "string" ? x : JSON.stringify(x))).join(" ")); }
export function registerFonts(dir) { return N.registerFonts(dir); }
export function registerFont(file) { return N.registerFont(file); }
export function hasFont(name) { return N.hasFont(name); }

// Rate functions are passed by name: { rateFunc: "linear" }.
export const linear = "linear", smooth = "smooth", rushInto = "rush_into", rushFrom = "rush_from", slowInto = "slow_into",
  doubleSmooth = "double_smooth", thereAndBack = "there_and_back", thereAndBackWithPause = "there_and_back_with_pause",
  wiggle = "wiggle", lingering = "lingering", exponentialDecay = "exponential_decay", easeInSine = "ease_in_sine",
  easeOutSine = "ease_out_sine", easeInOutSine = "ease_in_out_sine", easeInQuad = "ease_in_quad", easeOutQuad = "ease_out_quad",
  easeInOutQuad = "ease_in_out_quad", easeInCubic = "ease_in_cubic", easeOutCubic = "ease_out_cubic",
  easeInOutCubic = "ease_in_out_cubic", easeOutBack = "ease_out_back", easeOutBounce = "ease_out_bounce";
export const rate_functions = { linear, smooth, rush_into: rushInto, rush_from: rushFrom, slow_into: slowInto, double_smooth: doubleSmooth, there_and_back: thereAndBack, wiggle, lingering, ease_in_out_sine: easeInOutSine, ease_out_back: easeOutBack, ease_out_bounce: easeOutBounce };

// snake_case aliases for Python habits (move_to, next_to, set_color, ...).
for (const proto of [Mobject.prototype, TextMobject.prototype, Scene.prototype, GridMobject.prototype, TableMobject.prototype, CodeMobject.prototype]) {
  for (const name of Object.getOwnPropertyNames(proto)) {
    if (name.startsWith("_") || name === "constructor") continue;
    const snake = name.replace(/[A-Z]/g, (c) => "_" + c.toLowerCase());
    if (snake !== name && !(snake in proto)) Object.defineProperty(proto, snake, Object.getOwnPropertyDescriptor(proto, name));
  }
}
for (const name of Object.getOwnPropertyNames(Mobject.prototype)) {
  const snake = name.replace(/[A-Z]/g, (c) => "_" + c.toLowerCase());
  if (snake !== name && AnimateBuilder.prototype[name]) AnimateBuilder.prototype[snake] = AnimateBuilder.prototype[name];
}

// Host hooks.
globalThis.__manim_run = (SceneClass) => {
  EPOCH = N.epoch();
  const s = new SceneClass();
  CURRENT = s;
  try { s.construct(); } finally { /* keep CURRENT for seeking */ }
};
globalThis.__manim_seek = (t) => {
  const s = currentScene();
  const pid = N.seekBegin(t);
  s._run(pid, s._segments[N.playbackSegment(pid)] ?? []);
};
globalThis.__manim_isScene = (v) => typeof v === "function" && (v === Scene || v.prototype instanceof Scene);
