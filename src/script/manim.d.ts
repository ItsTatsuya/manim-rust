// Type declarations for the `manim` module of manim-rs scene scripts.
//
// Scripts are TypeScript (types are stripped, not checked, at load time) or JavaScript.
// The API follows Manim Community Edition with camelCase names; snake_case aliases of
// methods (move_to, next_to, set_color...) and options (font_size...) also work.
// Constructors and animations can be called with or without `new`.
//
// Units: the frame is FRAME_WIDTH (≈14.2) × FRAME_HEIGHT (8) scene units, origin at the
// center, y up. Font sizes follow Manim (48 = default). Colors are "#RRGGBB" strings.

declare module "manim" {
  // ------------------------------------------------------------------ vectors
  /** A point or direction: `[x, y]` or `[x, y, z]`, or a Vec3. */
  export type Point = Vec3 | [number, number] | [number, number, number] | number[];
  export type Color = string;

  export class Vec3 extends Array<number> {
    constructor(x?: number, y?: number, z?: number);
    static from(v: Point): Vec3;
    readonly x: number;
    readonly y: number;
    readonly z: number;
    add(o: Point): Vec3;
    sub(o: Point): Vec3;
    /** Multiply by a number (also `scale`, `times`). */
    mul(k: number): Vec3;
    scale(k: number): Vec3;
    times(k: number): Vec3;
    div(k: number): Vec3;
    neg(): Vec3;
    dot(o: Point): number;
    norm(): number;
    normalize(): Vec3;
    /** Rotate counter-clockwise about the origin. */
    rotate(angle: number): Vec3;
    lerp(o: Point, t: number): Vec3;
  }
  export function vec(x?: number, y?: number, z?: number): Vec3;

  export const ORIGIN: Vec3, UP: Vec3, DOWN: Vec3, LEFT: Vec3, RIGHT: Vec3, OUT: Vec3, IN: Vec3;
  export const UL: Vec3, UR: Vec3, DL: Vec3, DR: Vec3;
  export const PI: number, TAU: number, DEGREES: number;
  export const SMALL_BUFF: number, MED_SMALL_BUFF: number, MED_LARGE_BUFF: number, LARGE_BUFF: number;
  export const DEFAULT_FONT_SIZE: number, DEFAULT_STROKE_WIDTH: number, FRAME_WIDTH: number, FRAME_HEIGHT: number;
  /** Font weights. */
  export const THIN: number, LIGHT: number, NORMAL: number, MEDIUM: number, SEMIBOLD: number, BOLD: number, HEAVY: number;

  // Manim's palette.
  export const WHITE: Color, BLACK: Color, GRAY: Color, GREY: Color, LIGHT_GRAY: Color, DARK_GRAY: Color;
  export const GRAY_A: Color, GRAY_B: Color, GRAY_C: Color, GRAY_D: Color, GRAY_E: Color;
  export const BLUE: Color, DARK_BLUE: Color, BLUE_A: Color, BLUE_B: Color, BLUE_C: Color, BLUE_D: Color, BLUE_E: Color;
  export const TEAL: Color, TEAL_A: Color, TEAL_B: Color, TEAL_C: Color, TEAL_D: Color, TEAL_E: Color;
  export const GREEN: Color, GREEN_A: Color, GREEN_B: Color, GREEN_C: Color, GREEN_D: Color, GREEN_E: Color;
  export const YELLOW: Color, YELLOW_A: Color, YELLOW_B: Color, YELLOW_C: Color, YELLOW_D: Color, YELLOW_E: Color;
  export const GOLD: Color, GOLD_A: Color, GOLD_B: Color, GOLD_C: Color, GOLD_D: Color, GOLD_E: Color;
  export const RED: Color, RED_A: Color, RED_B: Color, RED_C: Color, RED_D: Color, RED_E: Color;
  export const MAROON: Color, MAROON_A: Color, MAROON_B: Color, MAROON_C: Color, MAROON_D: Color, MAROON_E: Color;
  export const PURPLE: Color, PURPLE_A: Color, PURPLE_B: Color, PURPLE_C: Color, PURPLE_D: Color, PURPLE_E: Color;
  export const PINK: Color, LIGHT_PINK: Color, ORANGE: Color, LIGHT_BROWN: Color, DARK_BROWN: Color, GRAY_BROWN: Color;
  export const PURE_RED: Color, PURE_GREEN: Color, PURE_BLUE: Color, PURE_CYAN: Color, PURE_MAGENTA: Color, PURE_YELLOW: Color;
  export function interpolateColor(a: Color, b: Color, t: number): Color;

  // ------------------------------------------------------------------ style options
  export interface StyleOptions {
    color?: Color;
    fillColor?: Color;
    fillOpacity?: number;
    strokeColor?: Color;
    /** Manim units: 4 is the default line. */
    strokeWidth?: number;
    strokeOpacity?: number;
    opacity?: number;
    zIndex?: number;
  }

  export type TextRole = "title" | "heading" | "body" | "caption" | "code" | "hand";

  export interface TextOptions extends StyleOptions {
    /** Font family; the theme's body font by default. Registered and system fonts work. */
    font?: string;
    fontSize?: number;
    /** 100–900 or "light" | "normal" | "medium" | "semibold" | "bold" | "heavy". */
    weight?: number | string;
    italic?: boolean;
    /** Line height as a multiple of the font size (default 1.3). */
    lineSpacing?: number;
    /** Wrap at spaces so no line is wider than this (scene units). */
    maxWidth?: number;
    align?: "left" | "center" | "right";
    /** Use the current theme's style for this role (font, size, color, weight). */
    role?: TextRole;
    /** Extra fonts for characters the main font lacks. */
    fallbackFonts?: string[];
  }

  // ------------------------------------------------------------------ mobjects
  export class Mobject {
    // placement — all return this
    shift(...vectors: Point[]): this;
    /** Center on a point or mobject; with `alignedEdge`, put that edge there instead. */
    moveTo(target: Point | Mobject, alignedEdge?: Point): this;
    /** Place beside `target` in `direction`, `buff` apart, optionally lining up `alignedEdge`. */
    nextTo(target: Point | Mobject, direction?: Point, buff?: number, alignedEdge?: Point): this;
    nextTo(target: Point | Mobject, options: { direction?: Point; buff?: number; alignedEdge?: Point }): this;
    toEdge(edge?: Point, buff?: number): this;
    toCorner(corner?: Point, buff?: number): this;
    alignTo(target: Point | Mobject, direction?: Point): this;
    scale(factor: number, aboutPoint?: Point): this;
    stretch(factor: number, dim: 0 | 1): this;
    /** Counter-clockwise, in radians, about the center or `aboutPoint`. */
    rotate(angle: number, aboutPoint?: Point): this;
    flip(): this;
    setWidth(width: number): this;
    setHeight(height: number): this;
    scaleToFitWidth(width: number): this;
    scaleToFitHeight(height: number): this;
    matchWidth(other: Mobject): this;
    matchHeight(other: Mobject): this;
    setX(x: number): this;
    setY(y: number): this;
    center(): this;
    /** Lay members out in a row/column `buff` apart, then center the group. */
    arrange(direction?: Point, buff?: number, alignedEdge?: Point): this;
    arrange(direction: Point, options: { buff?: number; alignedEdge?: Point }): this;
    arrangeInGrid(cols?: number, buff?: number): this;
    /** Shrink (with grow, also enlarge up to maxGrow×) to fit width × height. */
    fitWithin(width: number, height: number, grow?: boolean, maxGrow?: number): this;
    /** Move every point through `fn` (call `insertCurves` first for smooth warps). */
    applyFunction(fn: (p: Vec3) => Point): this;
    // style
    setColor(color: Color): this;
    setFill(color?: Color, opacity?: number): this;
    setFillColor(color: Color): this;
    setFillOpacity(opacity: number): this;
    setStroke(color?: Color, width?: number, opacity?: number): this;
    setStrokeColor(color: Color): this;
    setStrokeWidth(width: number): this;
    setStrokeOpacity(opacity: number): this;
    setOpacity(opacity: number): this;
    fade(darkness?: number): this;
    setColorByGradient(...colors: Color[]): this;
    setZIndex(z: number): this;
    /** Recolor every occurrence of `needle` in a text. */
    setColorText(needle: string, color: Color): this;
    setColorChars(start: number, end: number, color: Color): this;
    /** Take another mobject's shape and style. */
    become(other: Mobject, keepZIndex?: boolean): this;
    // queries
    getCenter(): Vec3;
    getTop(): Vec3;
    getBottom(): Vec3;
    getLeft(): Vec3;
    getRight(): Vec3;
    getCorner(direction: Point): Vec3;
    getCriticalPoint(direction: Point): Vec3;
    getEdgeCenter(direction: Point): Vec3;
    getStart(): Vec3;
    getEnd(): Vec3;
    pointFromProportion(alpha: number): Vec3;
    readonly width: number;
    readonly height: number;
    getWidth(): number;
    getHeight(): number;
    /** [left, bottom, right, top] */
    readonly bbox: [number, number, number, number] | null;
    readonly zIndex: number;
    readonly kind: string;
    getColor(): Color;
    getFillOpacity(): number;
    // structure
    copy(): this;
    /** Member `i` of a group (negative counts from the end); changing it changes the group. */
    get(i: number): Mobject;
    readonly length: number;
    readonly submobjects: Mobject[];
    [Symbol.iterator](): Iterator<Mobject>;
    /** Append members (they move into this group). */
    add(...mobjects: Mobject[]): this;
    /** Record changes to play as one smooth animation: `this.play(m.animate.shift(UP).setColor(RED))`. */
    readonly animate: AnimateBuilder<this>;
    /** Run `fn(mobject, dt)` every frame while the scene plays. */
    addUpdater(fn: (m: this, dt: number) => void): this;
    removeUpdater(fn: (m: this, dt: number) => void): this;
    clearUpdaters(): this;
  }

  /** Text-like mobjects (Text, MarkupText, MathTex, DecimalNumber...). */
  export class TextMobject extends Mobject {
    /** The source string. */
    readonly text: string;
    /** A copy of the glyphs of characters start..end (string indices, spaces count; negatives from the end). Use it to frame or highlight words. */
    slice(start?: number, end?: number): Mobject;
    chars(start: number, end: number): Mobject;
    /** A copy of the glyphs of the first occurrence of `needle`. */
    part(needle: string): Mobject;
    /** Char range [start, end) of `needle`, or null. */
    findText(needle: string): [number, number] | null;
  }

  type Ctor<A extends any[], T> = { new (...args: A): T; (...args: A): T };

  export const VMobject: Ctor<[options?: StyleOptions], Mobject>;
  /** A group; its members become views into it (changing a member changes the group). */
  export const VGroup: Ctor<Mobject[], Mobject>;
  export const Group: typeof VGroup;
  export const Circle: Ctor<[radius?: number, options?: StyleOptions] | [options?: StyleOptions & { radius?: number }], Mobject>;
  export const Square: Ctor<[sideLength?: number, options?: StyleOptions] | [options?: StyleOptions & { sideLength?: number }], Mobject>;
  export const Rectangle: Ctor<[width?: number, height?: number, options?: StyleOptions] | [options?: StyleOptions & { width?: number; height?: number }], Mobject>;
  export const RoundedRectangle: Ctor<[options?: StyleOptions & { width?: number; height?: number; cornerRadius?: number }], Mobject>;
  export const Ellipse: Ctor<[options?: StyleOptions & { width?: number; height?: number }], Mobject>;
  export const Dot: Ctor<[point?: Point, options?: StyleOptions & { radius?: number }] | [options?: StyleOptions & { point?: Point; radius?: number }], Mobject>;
  export const Line: Ctor<[start: Point, end: Point, options?: StyleOptions], Mobject>;
  export const DashedLine: Ctor<[start: Point, end: Point, options?: StyleOptions & { dashLength?: number; dashedRatio?: number }], Mobject>;
  export const Arrow: Ctor<[start: Point, end: Point, options?: StyleOptions & { buff?: number }], Mobject>;
  export const DoubleArrow: Ctor<[start: Point, end: Point, options?: StyleOptions], Mobject>;
  /** An arrow along an arc; `angle` bends it (negative bends clockwise). */
  export const CurvedArrow: Ctor<[start: Point, end: Point, options?: StyleOptions & { angle?: number; tipLength?: number }], Mobject>;
  export const ArcBetweenPoints: Ctor<[start: Point, end: Point, options?: StyleOptions & { angle?: number }], Mobject>;
  export const Vector: Ctor<[direction: Point, options?: StyleOptions], Mobject>;
  export const Arc: Ctor<[options?: StyleOptions & { radius?: number; startAngle?: number; angle?: number }], Mobject>;
  export const Annulus: Ctor<[options?: StyleOptions & { innerRadius?: number; outerRadius?: number }], Mobject>;
  export const Sector: Ctor<[options?: StyleOptions & { radius?: number; startAngle?: number; angle?: number }], Mobject>;
  export const Polygon: Ctor<Point[], Mobject>;
  export const Polyline: Ctor<Point[], Mobject>;
  export const RegularPolygon: Ctor<[n?: number, options?: StyleOptions & { radius?: number }], Mobject>;
  export const Triangle: Ctor<[options?: StyleOptions], Mobject>;
  export const Star: Ctor<[n?: number, options?: StyleOptions & { outerRadius?: number; innerRadius?: number }], Mobject>;
  export const SurroundingRectangle: Ctor<[target: Mobject, options?: StyleOptions & { buff?: number; cornerRadius?: number }], Mobject>;
  export const BackgroundRectangle: Ctor<[target: Mobject, options?: StyleOptions], Mobject>;
  export const Brace: Ctor<[target: Mobject, direction?: Point, options?: StyleOptions], Mobject>;

  export const Text: Ctor<[text: string, options?: TextOptions], TextMobject>;
  export const Paragraph: Ctor<string[], TextMobject>;
  /** Pango-style markup: `<b>`, `<i>`, `<span color="#E11D48" weight="600" font="Inter" size="36">`, `<color=#hex>`. */
  export const MarkupText: Ctor<[markup: string, options?: TextOptions], TextMobject>;
  /** An equation in Typst math syntax: `MathTex("x^2 + y^2 = r^2")`, `"integral_0^1 f(x) dif x"`, `"sum_(k=1)^n k"`. No LaTeX needed. */
  export const MathTex: Ctor<[math: string, options?: { fontSize?: number; color?: Color } & StyleOptions], TextMobject>;
  export const MathTypst: typeof MathTex;
  /** Typst markup (text with `$inline math$`, `*bold*`...). */
  export const Tex: Ctor<[markup: string, options?: { fontSize?: number; color?: Color } & StyleOptions], TextMobject>;
  export const Typst: typeof Tex;
  export const DecimalNumber: Ctor<[value: number, options?: TextOptions & { numDecimalPlaces?: number }], TextMobject>;
  export const Integer: Ctor<[value: number, options?: TextOptions], TextMobject>;
  /** PNG or JPEG from the project folder. */
  export const ImageMobject: Ctor<[path: string, options?: { height?: number } & StyleOptions], Mobject>;
  /** An SVG file path or SVG text, scaled to `height` (default 2). */
  export const SVGMobject: Ctor<[pathOrSvg: string, options?: { height?: number } & StyleOptions], Mobject>;
  export const NumberPlane: Ctor<[options?: { xRange?: [number, number, number]; yRange?: [number, number, number] } & StyleOptions], Mobject>;
  export const ParametricFunction: Ctor<[fn: (t: number) => Point, options?: StyleOptions & { tRange?: [number, number]; samples?: number }], Mobject>;
  export const FunctionGraph: Ctor<[fn: (x: number) => number, options?: StyleOptions & { xRange?: [number, number]; samples?: number }], Mobject>;

  export interface AxesOptions {
    xRange?: [number, number, number];
    yRange?: [number, number, number];
    xLength?: number;
    yLength?: number;
    center?: Point;
    axisColor?: Color;
    tips?: boolean;
    numbers?: boolean;
  }
  export interface AxesMobject extends Mobject {
    /** Graph coordinates → scene point. */
    c2p(x: number, y: number): Vec3;
    coordsToPoint(x: number, y: number): Vec3;
    p2c(point: Point): [number, number];
    pointToCoords(point: Point): [number, number];
    plot(fn: (x: number) => number, options?: StyleOptions & { xRange?: [number, number]; samples?: number }): Mobject;
    plotParametricCurve(fn: (t: number) => [number, number], options?: StyleOptions & { tRange?: [number, number]; samples?: number }): Mobject;
    getVerticalLine(point: Point, options?: StyleOptions): Mobject;
    getHorizontalLine(point: Point, options?: StyleOptions): Mobject;
  }
  export const Axes: Ctor<[options?: AxesOptions], AxesMobject>;

  export class ValueTracker {
    constructor(value?: number);
    getValue(): number;
    setValue(v: number): this;
    incrementValue(d: number): this;
    value: number;
    readonly animate: { setValue(v: number, options?: AnimationOptions): Animation; incrementValue(d: number, options?: AnimationOptions): Animation };
  }

  /** Rebuild a mobject from `fn` every frame (add the result to the scene). */
  export function alwaysRedraw<T extends Mobject>(fn: () => T): T;
  export const always_redraw: typeof alwaysRedraw;

  // ------------------------------------------------------------------ animations
  export type RateFunc =
    | "linear" | "smooth" | "smoothstep" | "rush_into" | "rush_from" | "slow_into" | "double_smooth"
    | "there_and_back" | "there_and_back_with_pause" | "wiggle" | "lingering" | "exponential_decay"
    | "ease_in_sine" | "ease_out_sine" | "ease_in_out_sine" | "ease_in_quad" | "ease_out_quad" | "ease_in_out_quad"
    | "ease_in_cubic" | "ease_out_cubic" | "ease_in_out_cubic" | "ease_out_back" | "ease_out_bounce";
  export const linear: RateFunc, smooth: RateFunc, rushInto: RateFunc, rushFrom: RateFunc, slowInto: RateFunc,
    doubleSmooth: RateFunc, thereAndBack: RateFunc, thereAndBackWithPause: RateFunc, wiggle: RateFunc,
    lingering: RateFunc, exponentialDecay: RateFunc, easeInSine: RateFunc, easeOutSine: RateFunc,
    easeInOutSine: RateFunc, easeInQuad: RateFunc, easeOutQuad: RateFunc, easeInOutQuad: RateFunc,
    easeInCubic: RateFunc, easeOutCubic: RateFunc, easeInOutCubic: RateFunc, easeOutBack: RateFunc, easeOutBounce: RateFunc;
  /** Rate function names by their Manim (snake_case) names. */
  export const rate_functions: Record<string, RateFunc>;

  export interface AnimationOptions {
    /** Seconds (default 1). */
    runTime?: number;
    rateFunc?: RateFunc;
    lagRatio?: number;
  }
  export interface FadeOptions extends AnimationOptions {
    /** Travel by this vector while fading. */
    shift?: Point;
    /** Scale from/to this factor while fading. */
    scale?: number;
  }
  export class Animation {
    /** Change timing afterwards: `.set({ runTime: 2 })`. */
    set(options: AnimationOptions): this;
  }
  export interface AnimateBuilder<T> extends Omit<Mobject, "animate"> {
    set(options: AnimationOptions & { pathArc?: number }): this;
  }
  type A<Args extends any[]> = { new (...args: Args): Animation; (...args: Args): Animation };

  export const FadeIn: A<[m: Mobject, options?: FadeOptions]>;
  export const FadeOut: A<[m: Mobject, options?: FadeOptions]>;
  export const Create: A<[m: Mobject, options?: AnimationOptions]>;
  export const Uncreate: A<[m: Mobject, options?: AnimationOptions]>;
  /** Handwriting reveal; timing scales with the number of glyphs. */
  export const Write: A<[m: Mobject, options?: AnimationOptions]>;
  export const Unwrite: A<[m: Mobject, options?: AnimationOptions]>;
  export const AddTextLetterByLetter: A<[m: Mobject, options?: AnimationOptions]>;
  export const DrawBorderThenFill: A<[m: Mobject, options?: AnimationOptions]>;
  export const ShowPassingFlash: A<[m: Mobject, options?: AnimationOptions & { timeWidth?: number }]>;
  export const GrowFromCenter: A<[m: Mobject, options?: AnimationOptions]>;
  export const GrowFromPoint: A<[m: Mobject, point: Point, options?: AnimationOptions]>;
  /** Sweep in from one side (`LEFT` for a highlighter stroke). */
  export const GrowFromEdge: A<[m: Mobject, edge: Point, options?: AnimationOptions]>;
  export const GrowArrow: A<[m: Mobject, options?: AnimationOptions]>;
  export const SpinInFromNothing: A<[m: Mobject, options?: AnimationOptions]>;
  export const ShrinkToCenter: A<[m: Mobject, options?: AnimationOptions]>;
  export const Indicate: A<[m: Mobject, options?: AnimationOptions & { color?: Color; scaleFactor?: number }]>;
  /** `a` morphs into `b`'s shape (b is not added). */
  export const Transform: A<[a: Mobject, b: Mobject, options?: AnimationOptions & { pathArc?: number }]>;
  /** `a` morphs into `b`, then `b` replaces `a` in the scene. */
  export const ReplacementTransform: A<[a: Mobject, b: Mobject, options?: AnimationOptions & { pathArc?: number }]>;
  export const TransformFromCopy: A<[a: Mobject, b: Mobject, options?: AnimationOptions & { pathArc?: number }]>;
  export const FadeTransform: A<[a: Mobject, b: Mobject, options?: AnimationOptions]>;
  export const Rotate: A<[m: Mobject, angle?: number, options?: AnimationOptions & { about?: Point }]>;
  export const Rotating: typeof Rotate;
  export const MoveAlongPath: A<[m: Mobject, path: Mobject, options?: AnimationOptions]>;
  export const Wait: A<[runTime?: number]>;
  export const AnimationGroup: A<(Animation | AnimateBuilder<Mobject> | AnimationOptions)[]>;
  /** Start each animation a little after the previous (lagRatio 0.05 unless set). */
  export const LaggedStart: A<(Animation | AnimateBuilder<Mobject> | AnimationOptions)[]>;
  /** One after another. */
  export const Succession: A<(Animation | AnimateBuilder<Mobject> | AnimationOptions)[]>;

  // ------------------------------------------------------------------ themes
  export interface Theme {
    id?: string;
    name?: string;
    colors?: Partial<Record<"background" | "text" | "muted" | "primary" | "secondary" | "accent" | "positive" | "negative" | "key_fill" | "example_fill", Color>>;
    fonts?: { heading?: string; body?: string; code?: string; hand?: string; math?: string; fallback?: string[] };
    sizes?: { title?: number; heading?: number; body?: number; small?: number };
    layout?: { margin?: number; title?: "top-left" | "top-center" | "center"; accent?: boolean; boxes?: "outline" | "card"; pattern?: "none" | "grid" | "dots"; pattern_spacing?: number };
    pacing?: { run_time?: number; wait?: number; words_per_second?: number };
  }

  // ------------------------------------------------------------------ scenes
  export class Scene {
    /** Build and animate the scene here. */
    construct(): void;
    /** Show mobjects (on top of everything shown so far). */
    add(...mobjects: Mobject[]): this;
    remove(...mobjects: Mobject[]): this;
    /** Remove everything except the background and overlay layers. */
    clear(): this;
    bringToFront(...mobjects: Mobject[]): this;
    bringToBack(...mobjects: Mobject[]): this;
    /** Draw below everything; kept by clear() and ignored by layout checks. */
    addBackground(...mobjects: Mobject[]): this;
    /** Draw above everything (watermarks, captions); kept by clear(). */
    addOverlay(...mobjects: Mobject[]): this;
    /** What is shown now, in draw order. */
    readonly mobjects: Mobject[];
    isOnScreen(m: Mobject): boolean;
    /** Play animations together; a final options object applies to all of them. */
    play(...animations: (Animation | AnimateBuilder<Mobject> | AnimationOptions)[]): this;
    wait(seconds?: number): this;
    /** Scene time in seconds. */
    readonly time: number;
    readonly renderer: { time: number };
    readonly frameWidth: number;
    readonly frameHeight: number;
    readonly fps: number;
    /** Name this moment (e.g. where a narration line starts); returns the time. Hosts get it in the timeline. */
    mark(name: string): number;
    /** Start a chapter; returns the time. */
    nextSection(name?: string): number;
    section(name: string): number;
    /** Play an audio file from the project folder, now or `timeOffset` seconds from now. */
    addSound(path: string, options?: number | { timeOffset?: number; at?: number; gain?: number }): this;
    /** Apply a theme: background, pattern, default colors and fonts, text roles. */
    setTheme(theme: Theme | string): this;
    readonly theme: Theme | null;
    readonly camera: { backgroundColor: Color; readonly frameWidth: number; readonly frameHeight: number };
    /** Run `fn(dt)` every frame. */
    addUpdater(fn: (dt: number) => void): this;
    removeUpdater(fn: (dt: number) => void): this;
    /** What is on screen: id, kind, label, bbox [l, b, r, t], opacity, z, text. */
    inspect(): { id: number; kind: string; label: string; bbox: [number, number, number, number]; opacity: number; z_index: number; text: boolean }[];
  }

  /** Define a scene from a function: `export default scene("Intro", (s) => { s.play(...) })`. */
  export function scene(name: string, construct: (s: Scene) => void): typeof Scene;

  /** Print to the host's log. */
  export function log(...values: unknown[]): void;
  /** Register the fonts in a project folder (e.g. "assets/fonts"); returns how many faces were added. */
  export function registerFonts(dir: string): number;
  /** Register one font file; returns its family names. */
  export function registerFont(file: string): string;
  export function hasFont(family: string): boolean;
}
