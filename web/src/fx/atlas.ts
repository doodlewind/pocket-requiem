// The effects' texture: one channel of light, 512 × 512. Every sprite, strip
// and pattern an effect samples is a rectangle of it; an effect's colour comes
// from its layer, so the atlas holds only how bright each texel is.
//
// Painted by plain loops over a byte array: the same in the browser and in Bun.

import { fbm, hash01 } from "../world/rng";

export const FX_ATLAS = 512;

/** A rectangle of the atlas in texels: x, y, width, height. */
export type Rect = readonly [number, number, number, number];

export const SPRITE = {
  /** A soft round glow. */
  DOT: [0, 0, 128, 128],
  /** A glow drawn out along U. */
  STREAK: [128, 0, 128, 128],
  /** A ragged puff. */
  SMOKE: [256, 0, 128, 128],
  /** A tongue of flame, its root at the bottom. */
  FLAME: [384, 0, 128, 128],
  /** A four-pointed glint. */
  STAR: [0, 128, 128, 128],
  /** A line of light with a glow either side, across V. */
  BOLT: [128, 128, 128, 64],
  /** Soft across V, even along U. */
  SOFT: [128, 192, 128, 64],
  /** A stroke that is bright at its leading edge and trails away across V. */
  ARC: [256, 128, 256, 64],
  /** Streaks along U, bright in the middle of V: a beam's body. */
  BEAM: [256, 192, 256, 64],
  /** The six-petalled circle of a spell. */
  CIRCLE: [0, 256, 256, 256],
  /** The honeycomb of a barrier. */
  HEX: [256, 256, 256, 256],
} as const satisfies Record<string, Rect>;

export function paintFxAtlas(seed: number): Uint8Array {
  const out = new Uint8Array(FX_ATLAS * FX_ATLAS);
  const put = (r: Rect, f: (u: number, v: number, x: number, y: number) => number) => {
    for (let y = 0; y < r[3]; y++) {
      for (let x = 0; x < r[2]; x++) {
        // A texel of margin all round is dark, so a sprite's edge never shows its neighbour.
        const edge = x < 1 || y < 1 || x >= r[2] - 1 || y >= r[3] - 1;
        const v = edge ? 0 : f((x + 0.5) / r[2], (y + 0.5) / r[3], x, y);
        out[(r[1] + y) * FX_ATLAS + r[0] + x] = Math.max(0, Math.min(255, Math.round(v * 255)));
      }
    }
  };
  const round = (u: number, v: number) => Math.hypot(u - 0.5, v - 0.5) * 2;
  const soft = (d: number, k: number) => Math.exp(-d * d * k);
  put(SPRITE.DOT, (u, v) => soft(round(u, v), 4.5) * Math.max(0, 1 - round(u, v)) ** 0.5);
  put(SPRITE.STREAK, (u, v) => soft(Math.abs(v - 0.5) * 2, 14) * Math.max(0, 1 - Math.abs(u - 0.5) * 2) ** 1.5);
  put(SPRITE.SMOKE, (u, v, x, y) => {
    const d = round(u, v);
    const n = fbm(x / 21, y / 21, seed + 1, 4);
    return Math.max(0, 1 - d * (0.9 + 0.7 * (n - 0.5))) ** 1.4 * (0.55 + 0.6 * n);
  });
  put(SPRITE.FLAME, (u, v, x, y) => {
    // Wide at the root, drawn to a ragged point at the top.
    const up = 1 - v;
    const width = (1 - up) ** 0.7 * 0.42 + 0.03;
    const sway = (fbm(x / 17, y / 29, seed + 2, 3) - 0.5) * 0.3 * up;
    const d = Math.abs(u - 0.5 - sway) / width;
    return Math.max(0, 1 - d) ** 1.2 * (0.5 + 0.7 * fbm(x / 9, y / 15, seed + 3, 3)) * Math.min(1, v * 6);
  });
  put(SPRITE.STAR, (u, v) => {
    const x = Math.abs(u - 0.5) * 2;
    const y = Math.abs(v - 0.5) * 2;
    const ray = Math.max(soft(y, 600) * (1 - x) ** 2, soft(x, 600) * (1 - y) ** 2);
    return Math.min(1, ray + soft(Math.hypot(x, y), 22));
  });
  put(SPRITE.BOLT, (_u, v) => {
    const d = Math.abs(v - 0.5) * 2;
    return Math.min(1, soft(d, 160) + 0.35 * soft(d, 6));
  });
  put(SPRITE.SOFT, (_u, v) => Math.sin(Math.PI * v) ** 1.5);
  put(SPRITE.ARC, (u, v, x) => {
    // V runs from the stroke's inner edge to its leading edge.
    const lead = v ** 3;
    const grain = 0.75 + 0.5 * hash01(Math.floor(x / 3), 0, seed + 4) * (1 - v);
    return lead * grain * Math.min(1, (1 - v) * 30) * Math.sin(Math.PI * u) ** 0.5;
  });
  put(SPRITE.BEAM, (u, v, x, y) => {
    const d = Math.abs(v - 0.5) * 2;
    const streak = 0.6 + 0.6 * fbm(x / 37, y / 3, seed + 5, 3);
    return Math.min(1, soft(d, 26) * 1.2 + soft(d, 3) * 0.4 * streak) * Math.min(1, u * 12);
  });
  put(SPRITE.CIRCLE, (u, v) => {
    const x = u * 2 - 1;
    const y = v * 2 - 1;
    const r = Math.hypot(x, y);
    const a = Math.atan2(y, x);
    const line = (d: number, w: number) => Math.max(0, 1 - Math.abs(d) / w) ** 1.5;
    // Two outer rings with ticks between them, a flower of six petals, a ring at the heart.
    let c = line(r - 0.94, 0.022) + line(r - 0.82, 0.016) + line(r - 0.3, 0.018);
    if (r > 0.83 && r < 0.93) c += line(((a / (Math.PI * 2)) * 36) % 1 - 0.5, 0.08) * 0.8;
    const petal = Math.abs(Math.cos(a * 3));
    c += line(r - (0.32 + 0.47 * petal), 0.02);
    c += line(r - (0.32 + 0.47 * Math.abs(Math.sin(a * 3))), 0.014) * 0.7;
    // A hexagram's lines through the petals.
    for (let k = 0; k < 6; k++) {
      const n = (k * Math.PI) / 3;
      c += line(x * Math.cos(n) + y * Math.sin(n) - 0.41, 0.012) * (r < 0.82 ? 0.55 : 0);
    }
    return Math.min(1, c + 0.1 * soft(r, 3)) * (r < 0.97 ? 1 : 0);
  });
  put(SPRITE.HEX, (u, v) => {
    // Distance to the nearest edge of a honeycomb.
    const s = 7;
    const px = u * s;
    const py = (v * s) / 0.866;
    const row = Math.floor(py);
    const fx = px + (row % 2 === 0 ? 0 : 0.5);
    const cx = Math.floor(fx) + 0.5;
    let best = 9;
    for (const [ox, oy] of [
      [0, 0],
      [1, 0],
      [-1, 0],
      [0.5, 1],
      [-0.5, 1],
      [0.5, -1],
      [-0.5, -1],
    ]) {
      const dx = fx - (cx + ox);
      const dy = (py - (row + 0.5 + oy)) * 0.866;
      best = Math.min(best, Math.hypot(dx, dy));
    }
    // `best` is the distance to the nearest cell centre: edges lie where two centres are equally near.
    const edge = Math.abs(best - 0.5) * 2;
    const fade = Math.max(0, 1 - round(u, v)) ** 0.6;
    return (Math.max(0, 1 - edge * 5) ** 1.4 * 0.9 + 0.08) * fade;
  });
  return out;
}
