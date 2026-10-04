// FxIR: what an effect is, said once. An effect is a list of layers; a layer
// is one of four shapes whose every vertex is a closed-form function of the
// effect's age. Nothing here is simulated per particle at play time: `lower`
// turns a layer into a fixed vertex template and eight rows of constants, and
// a renderer draws all live instances of an effect's layer in one call.
//
//   particles  quads flung out, falling, growing, fading
//   ring       a band about a centre: a shock on the ground, the arc of a sweep
//   ribbon     strips along a direction that face the eye: a beam, lightning
//   shell      a small mesh scaled about a point: a circle, a column, a dome
//
// The reference (render/fx.ts) and the device compilers read the same lowered
// layers; the vertex programs on each side evaluate the same formulas.

import { FX_ATLAS, Rect } from "./atlas";

export type Rgba = readonly [number, number, number, number];
/** A value at the start and at the end of a layer's life. */
export type Span = readonly [number, number];

interface Common {
  sprite: Rect;
  /** Colour at the start and at the end; alpha is its strength. */
  color: readonly [Rgba, Rgba];
  /** `add` lights what is behind it; `over` covers it (smoke, dust). */
  blend?: "add" | "over";
  /** Alpha falls as `(1 - t)^fade`. */
  fade?: number;
  /** Sizes, speeds and reaches are multiplied by the effect's number (its reach or radius). */
  byA?: boolean;
}

export interface Particles extends Common {
  type: "particles";
  count: number;
  /** Where they go, in the effect's frame (z along its direction): all round, a cone of `angle` about z, or a disc across z. */
  shape: "sphere" | "cone" | "disc";
  angle?: number;
  speed: Span;
  gravity?: number;
  /** Speed lost over a particle's life, 0 to 1. */
  drag?: number;
  /** Upward speed added to every particle. */
  rise?: number;
  /** Distance from the centre at birth. */
  radius?: number;
  /** Births are spread over this fraction of the effect's life. */
  birth?: number;
  /** A particle's life in seconds: least and most. */
  life: Span;
  size: Span;
  /** Sizes vary downward by up to this fraction. */
  vary?: number;
  /** Above 0 the quad lies along its velocity, longer the faster it goes. */
  stretch?: number;
  /** Fraction of its life a particle takes to appear. */
  appear?: number;
}

export interface Ring extends Common {
  type: "ring";
  /** `ground`: flat, turned to the effect's direction. `facing`: across the direction. `swing`: through the direction, tilted by `tilt`. */
  plane: "ground" | "facing" | "swing";
  tilt?: number;
  inner: Span;
  outer: Span;
  /** Half the angle the band covers, to each side of the direction; PI is a full ring. */
  half?: number;
  /** Turn at the start, and turn per life. */
  turn?: Span;
  /** Radii move as `t^ease`: below 1 they start fast. */
  ease?: number;
  height?: number;
  /** A sweep's side: mirror the band when the effect's number is negative. */
  signed?: boolean;
  /** Fade toward both ends of the arc. */
  ends?: boolean;
  segments?: number;
}

export interface Ribbon extends Common {
  type: "ribbon";
  length: Span;
  width: Span;
  /** Fraction of its life the strip takes to reach its length. */
  grow?: number;
  /** Sideways wander as a fraction of the length (lightning), and how often it changes per second. */
  jag?: number;
  flicker?: number;
  /** Several strips fanned to each side of the direction, over `spread` radians. */
  fan?: number;
  spread?: number;
  lift?: number;
  segments?: number;
  /** The strip stands up from the point instead of following the direction. */
  upright?: boolean;
}

export interface Shell extends Common {
  type: "shell";
  mesh: "quad" | "cylinder" | "dome" | "panel";
  /** `ground`: upright on the ground, turned to the direction. `facing`: its axis along the direction. */
  plane: "ground" | "facing";
  radius: Span;
  height?: Span;
  turn?: Span;
  ease?: number;
  lift?: number;
  /** Distance along the direction from the effect's point. */
  ahead?: number;
  /** Brightness at the silhouette: power and gain. `base` is the brightness face on. */
  rim?: readonly [number, number];
  base?: number;
}

export type Layer = Particles | Ring | Ribbon | Shell;

export const PROGRAM = { particles: 0, ring: 1, ribbon: 2, shell: 3 } as const;

/** Floats per template vertex: three vec4 attributes. */
export const TEMPLATE_STRIDE = 12;

export interface Lowered {
  program: number;
  /** 0 add, 1 over. */
  blend: number;
  /** `TEMPLATE_STRIDE` floats per vertex, every component in [-1, 1]. */
  vertices: Float32Array;
  indices: Uint16Array;
  /** Eight rows of four constants (`uP`). */
  rows: Float32Array;
}

/** A fixed sequence of numbers in [0, 1) for a layer: the same template on every machine. */
function sequence(seed: number): () => number {
  let s = (seed * 2654435761) >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s >>>= 0;
    s ^= s >>> 17;
    s ^= s << 5;
    s >>>= 0;
    return s / 4294967296;
  };
}

function rect(r: Rect): [number, number, number, number] {
  return [r[0] / FX_ATLAS, r[1] / FX_ATLAS, (r[0] + r[2]) / FX_ATLAS, (r[1] + r[3]) / FX_ATLAS];
}

/** `life` is the effect's life in seconds; `seed` makes each layer's template its own. */
export function lower(layer: Layer, life: number, seed: number): Lowered {
  const rows = new Float32Array(32);
  const row = (k: number, v: readonly number[]) => rows.set(v, k * 4);
  row(3, layer.color[0]);
  row(4, layer.color[1]);
  row(5, rect(layer.sprite));
  const byA = layer.byA ? 1 : 0;
  const fade = layer.fade ?? 1;
  const v: number[] = [];
  const idx: number[] = [];
  const put = (a: number[], b: number[], c: number[]) => v.push(...a, ...b, ...c);
  const rnd = sequence(seed);

  if (layer.type === "particles") {
    row(0, [layer.speed[0], layer.speed[1], layer.gravity ?? 0, layer.drag ?? 0]);
    row(1, [layer.birth ?? 0, layer.life[0], layer.life[1], life]);
    row(2, [layer.size[0], layer.size[1], layer.vary ?? 0, layer.stretch ?? 0]);
    row(6, [byA, layer.radius ?? 0, layer.rise ?? 0, fade]);
    row(7, [layer.appear ?? 0.08, 0, 0, 0]);
    for (let p = 0; p < layer.count; p++) {
      let d: [number, number, number];
      if (layer.shape === "disc") {
        const a = rnd() * Math.PI * 2;
        d = [Math.cos(a), Math.sin(a), 0];
      } else {
        // Uniform over a cap of the sphere about z.
        const cap = layer.shape === "cone" ? Math.cos(layer.angle ?? 0.5) : -1;
        const z = cap + (1 - cap) * rnd();
        const a = rnd() * Math.PI * 2;
        const r = Math.sqrt(Math.max(0, 1 - z * z));
        d = [Math.cos(a) * r, Math.sin(a) * r, z];
      }
      const who = [rnd(), rnd(), rnd(), rnd()];
      const speed = rnd();
      const first = v.length / TEMPLATE_STRIDE;
      for (const [cx, cy] of [
        [-1, -1],
        [1, -1],
        [1, 1],
        [-1, 1],
      ])
        put([d[0], d[1], d[2], speed], who, [cx, cy, 0, 0]);
      idx.push(first, first + 1, first + 2, first, first + 2, first + 3);
    }
  } else if (layer.type === "ring") {
    const mode = layer.plane === "ground" ? 0 : layer.plane === "facing" ? 1 : 2;
    row(0, [layer.inner[0], layer.inner[1], layer.outer[0], layer.outer[1]]);
    row(1, [layer.half ?? Math.PI, layer.turn?.[0] ?? 0, layer.turn?.[1] ?? 0, layer.ease ?? 1]);
    row(2, [0, fade, layer.height ?? 0, byA]);
    row(6, [mode, layer.tilt ?? 0, layer.signed ? 1 : 0, 0]);
    row(7, [0, 0, 0, layer.ends ? 1 : 0]);
    const n = layer.segments ?? 32;
    for (let k = 0; k <= n; k++) {
      const a = (k / n) * 2 - 1;
      put([a, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]);
      put([a, 1, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]);
    }
    for (let k = 0; k < n; k++) idx.push(k * 2, k * 2 + 2, k * 2 + 3, k * 2, k * 2 + 3, k * 2 + 1);
  } else if (layer.type === "ribbon") {
    row(0, [layer.length[0], layer.length[1], layer.width[0], layer.width[1]]);
    row(1, [layer.jag ?? 0, layer.flicker ?? 0, layer.spread ?? 0, byA]);
    row(2, [layer.grow ?? 0.1, fade, 0.15, 0.2]);
    row(6, [layer.lift ?? 0, layer.upright ? 1 : 0, 0, 0]);
    const n = layer.segments ?? 12;
    const fan = layer.fan ?? 1;
    for (let f = 0; f < fan; f++) {
      const side = fan > 1 ? (f / (fan - 1)) * 2 - 1 : 0;
      const who = rnd();
      const first = v.length / TEMPLATE_STRIDE;
      for (let k = 0; k <= n; k++) {
        const u = k / n;
        const ox = rnd() * 2 - 1;
        const oy = rnd() * 2 - 1;
        put([u, -1, ox, oy], [side, who, 0, 0], [0, 0, 0, 0]);
        put([u, 1, ox, oy], [side, who, 0, 0], [0, 0, 0, 0]);
      }
      for (let k = 0; k < n; k++) idx.push(first + k * 2, first + k * 2 + 2, first + k * 2 + 3, first + k * 2, first + k * 2 + 3, first + k * 2 + 1);
    }
  } else {
    const h = layer.height ?? [0, 0];
    row(0, [layer.radius[0], layer.radius[1], h[0], h[1]]);
    row(1, [layer.turn?.[0] ?? 0, layer.turn?.[1] ?? 0, layer.rim?.[0] ?? 1, layer.rim?.[1] ?? 0]);
    row(2, [layer.ease ?? 1, fade, layer.lift ?? 0, byA]);
    row(6, [layer.plane === "facing" ? 1 : 0, layer.ahead ?? 0, layer.base ?? 1, 0]);
    if (layer.mesh === "quad") {
      // In the shell's XZ plane, its normal up the axis.
      for (const [x, z] of [
        [-1, -1],
        [1, -1],
        [1, 1],
        [-1, 1],
      ])
        put([x, 0, z, 0], [0, 1, 0, 0], [x * 0.5 + 0.5, z * 0.5 + 0.5, 0, 0]);
      idx.push(0, 2, 1, 0, 3, 2, 0, 1, 2, 0, 2, 3);
    } else if (layer.mesh === "cylinder") {
      const n = 20;
      for (let k = 0; k <= n; k++) {
        const a = (k / n) * Math.PI * 2;
        for (const y of [0, 1]) put([Math.cos(a), y, Math.sin(a), 0], [Math.cos(a), 0, Math.sin(a), 0], [k / n, 1 - y, 0, 0]);
      }
      for (let k = 0; k < n; k++) idx.push(k * 2, k * 2 + 1, k * 2 + 3, k * 2, k * 2 + 3, k * 2 + 2, k * 2, k * 2 + 3, k * 2 + 1, k * 2, k * 2 + 2, k * 2 + 3);
    } else if (layer.mesh === "dome") {
      const [nu, nv] = [20, 7];
      for (let j = 0; j <= nv; j++) {
        const el = (j / nv) * Math.PI * 0.5;
        for (let k = 0; k <= nu; k++) {
          const a = (k / nu) * Math.PI * 2;
          const p = [Math.cos(a) * Math.cos(el), Math.sin(el), Math.sin(a) * Math.cos(el)];
          put([p[0], p[1], p[2], 0], [p[0], p[1], p[2], 0], [k / nu, 1 - j / nv, 0, 0]);
        }
      }
      for (let j = 0; j < nv; j++) {
        for (let k = 0; k < nu; k++) {
          const a = j * (nu + 1) + k;
          const b = a + nu + 1;
          idx.push(a, b, b + 1, a, b + 1, a + 1, a, b + 1, b, a, a + 1, b + 1);
        }
      }
    } else {
      // A panel: a slice of a cylinder's wall ahead of the point, 110° wide.
      const n = 10;
      for (let k = 0; k <= n; k++) {
        const a = ((k / n) * 2 - 1) * 0.96;
        // z is toward the front (the shell's -z is ahead on the ground plane).
        for (const y of [0, 1]) put([Math.sin(a), y, -Math.cos(a), 0], [Math.sin(a), 0, -Math.cos(a), 0], [k / n, 1 - y, 0, 0]);
      }
      for (let k = 0; k < n; k++) idx.push(k * 2, k * 2 + 1, k * 2 + 3, k * 2, k * 2 + 3, k * 2 + 2, k * 2, k * 2 + 3, k * 2 + 1, k * 2, k * 2 + 2, k * 2 + 3);
    }
  }
  return { program: PROGRAM[layer.type], blend: layer.blend === "over" ? 1 : 0, vertices: new Float32Array(v), indices: new Uint16Array(idx), rows };
}
