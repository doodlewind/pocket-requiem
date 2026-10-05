// The stage's one texture: horizontal strips that repeat along U.
//
// Every static surface samples this atlas, so a cell is one draw. A strip is a
// band of rows; U wraps across the full width, V stays inside the band. The
// ground runs V up one row of the terrain grid and back down the next, so the
// band repeats across the field without a seam.
//
// The strips are near white: a surface's colour is its vertex tint, and the
// strip gives it grain. The painter is a software rasterizer over a byte
// array: it runs the same in the browser and in Bun, and its output does not
// depend on a canvas.

import { fbm, hash01, Rng } from "./rng";

export const ATLAS_W = 1024;
export const ATLAS_H = 1024;

export interface Strip {
  /** First pixel row. */
  y: number;
  /** Rows. */
  h: number;
  /** Metres of world for one full U repeat. */
  mPerU: number;
  /** Metres of world the strip's height covers. */
  mPerV: number;
}

export const STRIP = {
  /** Dry grass and earth. */
  GROUND: { y: 0, h: 256, mPerU: 16, mPerV: 4 },
  ROCK: { y: 256, h: 256, mPerU: 16, mPerV: 4 },
  /** A conifer's boughs: needles hanging in rows. */
  NEEDLE: { y: 512, h: 128, mPerU: 8, mPerV: 2 },
  BARK: { y: 640, h: 128, mPerU: 4, mPerV: 2 },
  /** Bare, weathered wood. */
  DEAD: { y: 768, h: 64, mPerU: 4, mPerV: 1 },
  SNOW: { y: 832, h: 64, mPerU: 64, mPerV: 16 },
  FLAT: { y: 896, h: 16, mPerU: 16, mPerV: 1 },
} as const satisfies Record<string, Strip>;

/** V at the bottom and at the top of a strip, half a texel inside it. */
export function stripV(s: Strip): [number, number] {
  return [(s.y + s.h - 0.5) / ATLAS_H, (s.y + 0.5) / ATLAS_H];
}

type C = readonly [number, number, number];

class Painter {
  d = new Uint8ClampedArray(ATLAS_W * ATLAS_H * 4);

  set(x: number, y: number, c: C, a = 1) {
    if (y < 0 || y >= ATLAS_H) return;
    const o = (y * ATLAS_W + (((x % ATLAS_W) + ATLAS_W) % ATLAS_W)) * 4;
    const d = this.d;
    d[o] = d[o] + (c[0] - d[o]) * a;
    d[o + 1] = d[o + 1] + (c[1] - d[o + 1]) * a;
    d[o + 2] = d[o + 2] + (c[2] - d[o + 2]) * a;
    d[o + 3] = 255;
  }
  rect(x: number, y: number, w: number, h: number, c: C, a = 1) {
    for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) this.set(x + i, y + j, c, a);
  }
  /** Multiplies a region by `f(x, y)`. */
  shade(x: number, y: number, w: number, h: number, f: (x: number, y: number) => number) {
    const d = this.d;
    for (let j = 0; j < h; j++) {
      const yy = y + j;
      if (yy < 0 || yy >= ATLAS_H) continue;
      for (let i = 0; i < w; i++) {
        const xx = (((x + i) % ATLAS_W) + ATLAS_W) % ATLAS_W;
        const o = (yy * ATLAS_W + xx) * 4;
        const k = f(x + i, yy);
        d[o] *= k;
        d[o + 1] *= k;
        d[o + 2] *= k;
      }
    }
  }
  /** A line of thickness `t` with square ends, clipped to rows `[y0c, y1c)`. */
  line(x0: number, y0: number, x1: number, y1: number, t: number, c: C, y0c = 0, y1c = ATLAS_H) {
    const dx = x1 - x0;
    const dy = y1 - y0;
    const l2 = dx * dx + dy * dy || 1;
    const r = t / 2;
    for (let y = Math.floor(Math.min(y0, y1) - r); y <= Math.ceil(Math.max(y0, y1) + r); y++) {
      if (y < y0c || y >= y1c) continue;
      for (let x = Math.floor(Math.min(x0, x1) - r); x <= Math.ceil(Math.max(x0, x1) + r); x++) {
        const k = Math.max(0, Math.min(1, ((x + 0.5 - x0) * dx + (y + 0.5 - y0) * dy) / l2));
        const d = Math.hypot(x + 0.5 - x0 - dx * k, y + 0.5 - y0 - dy * k);
        if (d <= r + 0.5) this.set(x, y, c, Math.min(1, r + 0.5 - d));
      }
    }
  }
}

function ground(p: Painter, s: Strip, seed: number) {
  p.rect(0, s.y, ATLAS_W, s.h, [206, 204, 196]);
  // Broad patches, then finer mottling. The noise wraps in U; in V the band is mirrored, so it need not.
  p.shade(0, s.y, ATLAS_W, s.h, (px, py) => 0.66 + 0.42 * fbm(px / 97, py / 97, seed, 5, ATLAS_W / 97));
  p.shade(0, s.y, ATLAS_W, s.h, (px, py) => 0.86 + 0.22 * fbm(px / 9, py / 9, seed + 3, 3, ATLAS_W / 9));
  const r = new Rng(seed);
  // Blades of dry grass: short strokes leaning either way, light over dark.
  for (let i = 0; i < 5200; i++) {
    const x = r.int(0, ATLAS_W - 1);
    const y = s.y + r.int(2, s.h - 12);
    const len = r.int(4, 11);
    const lean = r.range(-0.6, 0.6);
    const light = r.chance(0.6);
    const c: C = light ? [248, 246, 230] : [108, 104, 96];
    for (let k = 0; k < len; k++) p.set(Math.round(x + lean * k), y + len - k, c, (light ? 0.5 : 0.4) * (1 - k / (len * 1.4)));
  }
  // Pebbles.
  for (let i = 0; i < 700; i++) {
    const x = r.int(0, ATLAS_W - 1);
    const y = s.y + r.int(1, s.h - 4);
    const w = r.int(1, 3);
    p.rect(x, y, w, w, [150, 150, 152], r.range(0.3, 0.7));
    p.rect(x, y + w, w, 1, [70, 70, 74], 0.5);
  }
}

function rock(p: Painter, s: Strip, seed: number) {
  p.rect(0, s.y, ATLAS_W, s.h, [214, 214, 216]);
  p.shade(0, s.y, ATLAS_W, s.h, (px, py) => 0.6 + 0.5 * fbm(px / 61, py / 43, seed, 5, ATLAS_W / 61));
  // Cracks: dark lines that wander.
  const r = new Rng(seed);
  for (let i = 0; i < 46; i++) {
    let x = r.int(0, ATLAS_W - 1);
    let y = s.y + r.int(4, s.h - 5);
    let a = r.range(0, Math.PI * 2);
    const n = r.int(30, 120);
    for (let k = 0; k < n; k++) {
      p.set(Math.round(x), Math.round(y), [70, 70, 76], 0.7);
      a += r.range(-0.5, 0.5);
      x += Math.cos(a);
      y += Math.sin(a) * 0.7;
      if (y < s.y + 1 || y > s.y + s.h - 2) break;
    }
  }
  // Lichen.
  for (let i = 0; i < 300; i++) p.rect(r.int(0, ATLAS_W - 1), s.y + r.int(1, s.h - 5), r.int(2, 5), r.int(2, 4), [232, 236, 222], r.range(0.15, 0.4));
}

function needle(p: Painter, s: Strip, seed: number) {
  p.rect(0, s.y, ATLAS_W, s.h, [96, 100, 100]);
  const r = new Rng(seed);
  // Rows of hanging sprays: each a fan of short strokes, lighter at the tips.
  const rows = 5;
  for (let row = 0; row < rows; row++) {
    const y0 = s.y + (row * s.h) / rows;
    for (let x = 0; x < ATLAS_W; x += 5) {
      const len = r.int(10, 24);
      const lean = r.range(-0.5, 0.5);
      const k0 = r.range(0.7, 1.0);
      for (let k = 0; k < len; k++) {
        const t = k / len;
        const c = (150 + 100 * t) * k0;
        p.set(Math.round(x + lean * k + r.range(-0.6, 0.6)), Math.round(y0 + k), [c, c + 4, c], 0.75);
      }
    }
    // The shadow under the row above.
    p.rect(0, Math.round(y0), ATLAS_W, 3, [40, 44, 46], 0.6);
  }
  p.shade(0, s.y, ATLAS_W, s.h, (px, py) => 0.78 + 0.34 * fbm(px / 37, py / 23, seed + 5, 3, ATLAS_W / 37));
}

function bark(p: Painter, s: Strip, seed: number, base: C, depth: number) {
  p.rect(0, s.y, ATLAS_W, s.h, base);
  // Furrows run up the trunk: columns of varying darkness, broken by noise.
  p.shade(0, s.y, ATLAS_W, s.h, (px, py) => {
    const col = hash01(Math.floor(px / 6), 0, seed);
    const groove = px % 6 === 0 ? 1 - depth : 1;
    return groove * (1 - depth * 0.5 + depth * col) * (0.84 + 0.3 * fbm(px / 11, py / 61, seed + 1, 3, ATLAS_W / 11));
  });
  const r = new Rng(seed);
  for (let i = 0; i < 500; i++) p.rect(r.int(0, ATLAS_W - 1), s.y + r.int(1, s.h - 8), 1, r.int(3, 7), [60, 56, 54], r.range(0.2, 0.5));
}

/** Paints the atlas: RGBA, `ATLAS_W × ATLAS_H`, row 0 first. */
export function paintAtlas(seed: number): Uint8ClampedArray {
  const p = new Painter();
  p.rect(0, 0, ATLAS_W, ATLAS_H, [255, 255, 255]);
  ground(p, STRIP.GROUND, seed + 10);
  rock(p, STRIP.ROCK, seed + 20);
  needle(p, STRIP.NEEDLE, seed + 30);
  bark(p, STRIP.BARK, seed + 40, [196, 190, 184], 0.5);
  bark(p, STRIP.DEAD, seed + 50, [224, 222, 218], 0.3);
  p.rect(0, STRIP.SNOW.y, ATLAS_W, STRIP.SNOW.h, [244, 246, 250]);
  p.shade(0, STRIP.SNOW.y, ATLAS_W, STRIP.SNOW.h, (px, py) => 0.82 + 0.22 * fbm(px / 53, py / 17, seed + 60, 4, ATLAS_W / 53));
  p.rect(0, STRIP.FLAT.y, ATLAS_W, STRIP.FLAT.h, [255, 255, 255]);
  return p.d;
}
