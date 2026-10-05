// The stage, from one seed: a barren field at night,
// about two kilometres square. The mage comes onto it from the south; the
// army stands across the middle in cohorts, facing her; the demon waits on a
// rise behind it. Conifers close the field to the east and west, low hills
// ring it, and far mountains carry snow.
//
// No Math.random, wall time or canvas APIs here: the same code runs in the
// browser and in Bun for the export.

import { STRIP, Strip, stripV } from "./atlas";
import { CELL, Collision, cross, Layer, Meshes, norm, Rgb, sub, SUPER, V3 } from "./geo";
import { fbm, hash01, Rng } from "./rng";

/** Samples per side of the height grid, their spacing, and the grid's first coordinate. */
export const FIELD_N = 513;
export const FIELD_CELL = 4;
export const FIELD_MIN = -1024;
/** The playable square is `±HALF`. */
export const HALF = 940;

export interface Obstacle {
  x: number;
  z: number;
  r: number;
  kind: number;
}

export interface Muster {
  x: number;
  z: number;
  yaw: number;
  spacing: number;
  cols: number;
  rows: number;
  /** 0 swords, 1 halberds, 2 greatswords, 3 mixed. */
  mix: number;
  captain: number;
}

export interface Stage {
  start: [number, number, number];
  demon: [number, number, number];
  gate: [number, number];
  town: [number, number, number];
  half: number;
  musters: Muster[];
}

export interface Field {
  n: number;
  cell: number;
  min: number;
  hMin: number;
  hScale: number;
  /** Heights as stored (`hMin + q × hScale`), and as metres. */
  q: Uint16Array;
  h: Float32Array;
}

export interface Generated {
  meshes: Meshes;
  col: Collision;
  field: Field;
  obstacles: Obstacle[];
  stage: Stage;
  knights: number;
  counts: Record<string, number>;
}

const smooth = (a: number, b: number, x: number) => {
  const t = Math.max(0, Math.min(1, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
};

/** Where the dry stream bed runs: its x at a given z. */
function streamX(z: number): number {
  return 250 + 60 * Math.sin(z / 170) + 34 * Math.sin(z / 61 + 1.3);
}

/** Height in metres at a point, before it is stored. */
function terrain(x: number, z: number, seed: number): number {
  const r = Math.hypot(x, z);
  // Long swells, shallower where the armies meet; smaller rolls; ground grain.
  const open = 1 - 0.65 * (1 - smooth(120, 420, Math.abs(x))) * (1 - smooth(500, 760, Math.abs(z)));
  let h = (fbm(x / 520 + 3.1, z / 520 + 7.7, seed, 4) - 0.5) * 34 * open;
  h += (fbm(x / 130, z / 130, seed + 5, 3) - 0.5) * 5.5 * (0.4 + 0.6 * open);
  h += (fbm(x / 31, z / 31, seed + 9, 3) - 0.5) * 0.9;
  // Low rounded hills close the field.
  const rim = smooth(620, 1040, r + (fbm(x / 240, z / 240, seed + 13, 3) - 0.5) * 260);
  h += rim * rim * (58 + 40 * fbm(x / 300, z / 300, seed + 17, 3));
  // The rise the demon stands on.
  const dx = x;
  const dz = z + 384;
  h += 7.5 * Math.exp(-(dx * dx + dz * dz) / (2 * 80 * 80));
  // A dry stream bed down the east side.
  const sd = x - streamX(z);
  h -= 2.4 * Math.exp(-(sd * sd) / (2 * 9 * 9)) * (1 - rim);
  return h;
}

/** How much forest stands at a point: 0 open, 1 dense. */
function forest(x: number, z: number, seed: number): number {
  const wobble = (fbm(x / 210, z / 210, seed + 31, 3) - 0.5) * 240;
  const side = smooth(330, 520, Math.abs(x) + wobble * 0.6);
  const north = smooth(520, 700, -z + wobble);
  const south = smooth(700, 860, z + wobble * 0.5) * smooth(90, 240, Math.abs(x));
  const clump = smooth(0.66, 0.8, fbm(x / 90, z / 90, seed + 37, 3)) * 0.5;
  return Math.min(1, Math.max(side, north, south, clump));
}

/** True where the army forms up and fights: nothing stands there. */
function battleground(x: number, z: number): boolean {
  return Math.abs(x) < 232 && z > -470 && z < 640;
}

const GRASS: Rgb = [0.5, 0.52, 0.38];
const EARTH: Rgb = [0.47, 0.42, 0.35];
const MUD: Rgb = [0.33, 0.3, 0.27];
const GRAVEL: Rgb = [0.58, 0.58, 0.6];
const HEATH: Rgb = [0.36, 0.42, 0.34];
const STONE: Rgb = [0.5, 0.5, 0.52];
const TRUNK: Rgb = [0.3, 0.24, 0.2];
const DEAD: Rgb = [0.55, 0.52, 0.48];
const PEAK: Rgb = [0.24, 0.28, 0.36];
const SNOW: Rgb = [0.92, 0.94, 1.0];

const mix = (a: Rgb, b: Rgb, t: number): Rgb => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
const scale = (a: Rgb, k: number): Rgb => [a[0] * k, a[1] * k, a[2] * k];

class Builder {
  meshes = new Meshes();
  col = new Collision();
  field: Field;
  counts: Record<string, number> = {};

  constructor(readonly seed: number) {
    const n = FIELD_N;
    const raw = new Float32Array(n * n);
    let lo = Infinity;
    let hi = -Infinity;
    for (let j = 0; j < n; j++) {
      for (let i = 0; i < n; i++) {
        const v = terrain(FIELD_MIN + i * FIELD_CELL, FIELD_MIN + j * FIELD_CELL, seed);
        raw[j * n + i] = v;
        lo = Math.min(lo, v);
        hi = Math.max(hi, v);
      }
    }
    // Stored as 16 bits over the range: what is drawn and what is stood on are the stored heights.
    const hScale = (hi - lo) / 65535;
    const q = new Uint16Array(n * n);
    const h = new Float32Array(n * n);
    for (let k = 0; k < n * n; k++) {
      q[k] = Math.round((raw[k] - lo) / hScale);
      h[k] = Math.fround(lo) + q[k] * Math.fround(hScale);
    }
    this.field = { n, cell: FIELD_CELL, min: FIELD_MIN, hMin: Math.fround(lo), hScale: Math.fround(hScale), q, h };
  }

  at(i: number, j: number): number {
    const n = this.field.n;
    return this.field.h[Math.max(0, Math.min(n - 1, j)) * n + Math.max(0, Math.min(n - 1, i))];
  }

  /** Height at any point, off the same two triangles per square the simulation uses. */
  height(x: number, z: number): number {
    const f = this.field;
    const fx = Math.max(0, Math.min(f.n - 1.001, (x - f.min) / f.cell));
    const fz = Math.max(0, Math.min(f.n - 1.001, (z - f.min) / f.cell));
    const i = Math.floor(fx);
    const j = Math.floor(fz);
    const u = fx - i;
    const v = fz - j;
    const h00 = this.at(i, j);
    const h10 = this.at(i + 1, j);
    const h01 = this.at(i, j + 1);
    const h11 = this.at(i + 1, j + 1);
    return u >= v ? h00 + (h10 - h00) * u + (h11 - h10) * v : h00 + (h11 - h01) * u + (h01 - h00) * v;
  }

  normal(i: number, j: number): V3 {
    return norm([this.at(i - 1, j) - this.at(i + 1, j), 2 * FIELD_CELL, this.at(i, j - 1) - this.at(i, j + 1)]);
  }

  /** The ground's colour at a grid point. */
  tint(i: number, j: number): Rgb {
    const x = FIELD_MIN + i * FIELD_CELL;
    const z = FIELD_MIN + j * FIELD_CELL;
    const n = this.normal(i, j);
    const patch = fbm(x / 70, z / 70, this.seed + 41, 4);
    let c = mix(GRASS, EARTH, smooth(0.42, 0.62, patch));
    c = mix(c, HEATH, smooth(0.25, 0.7, forest(x, z, this.seed)));
    // Where the army has marched and stood, the ground is trodden to mud.
    const trodden = (1 - smooth(150, 235, Math.abs(x))) * smooth(-470, -380, z) * (1 - smooth(560, 660, z));
    c = mix(c, MUD, 0.7 * trodden * (0.5 + 0.5 * fbm(x / 23, z / 23, this.seed + 43, 3)));
    const bed = Math.exp(-((x - streamX(z)) ** 2) / (2 * 7 * 7));
    c = mix(c, GRAVEL, bed * 0.8);
    c = mix(c, STONE, smooth(0.86, 0.72, n[1]));
    return scale(c, 0.82 + 0.36 * hash01(i, j, this.seed + 47));
  }

  /** The ground of one cell at a step of `step` grid squares, with a skirt along its border. */
  ground(layer: Layer, ci: number, cj: number, size: number, step: number) {
    const geo = this.meshes.at(layer, FIELD_MIN + ci * FIELD_CELL + 1, FIELD_MIN + cj * FIELD_CELL + 1);
    const [vb, vt] = stripV(STRIP.GROUND);
    const span = FIELD_CELL * step;
    const perU = STRIP.GROUND.mPerU * step;
    const n = size / step;
    const base = geo.nv;
    for (let b = 0; b <= n; b++) {
      for (let a = 0; a <= n; a++) {
        const i = ci + a * step;
        const j = cj + b * step;
        const row = Math.floor(j / step);
        geo.vert([FIELD_MIN + i * FIELD_CELL, this.at(i, j), FIELD_MIN + j * FIELD_CELL], this.normal(i, j), (a * span) / perU, row % 2 === 0 ? vb : vt, this.tint(i, j));
      }
    }
    const idx: number[] = [];
    for (let b = 0; b < n; b++) {
      for (let a = 0; a < n; a++) {
        const p = base + b * (n + 1) + a;
        // The diagonal runs from the square's low corner to its high corner, as the simulation reads it.
        idx.push(p, p + n + 2, p + 1, p, p + n + 1, p + n + 2);
      }
    }
    // The skirt: the border dropped, so a coarser neighbour leaves no gap.
    const drop = 0.6 * step;
    const ring: number[] = [];
    for (let a = 0; a <= n; a++) ring.push(base + a);
    for (let b = 1; b <= n; b++) ring.push(base + b * (n + 1) + n);
    for (let a = n - 1; a >= 0; a--) ring.push(base + n * (n + 1) + a);
    for (let b = n - 1; b >= 1; b--) ring.push(base + b * (n + 1));
    const low: number[] = [];
    for (const r of ring) {
      const o = r * 11;
      const v = geo.v;
      low.push(geo.vert([v[o], v[o + 1] - drop, v[o + 2]], [v[o + 3], v[o + 4], v[o + 5]], v[o + 6], v[o + 7], [v[o + 8], v[o + 9], v[o + 10]]));
    }
    for (let k = 0; k < ring.length; k++) {
      const k1 = (k + 1) % ring.length;
      idx.push(ring[k], ring[k1], low[k1], ring[k], low[k1], low[k]);
    }
    // Append the indices.
    const need = geo.ni + idx.length;
    if (need > geo.i.length) {
      const next = new Uint32Array(Math.max(geo.i.length * 2, need));
      next.set(geo.i);
      geo.i = next;
    }
    geo.i.set(idx, geo.ni);
    geo.ni += idx.length;
  }

  count(name: string, n = 1) {
    this.counts[name] = (this.counts[name] ?? 0) + n;
  }

  /** A cone of `sides` triangles from a rim to an apex, its vertices shared so the boughs shade as one surface. */
  cone(layers: readonly Layer[], c: V3, rim: number, apex: number, radius: number, sides: number, strip: Strip, tint: Rgb, phase: number, jag = 0, droop = 0) {
    const [vb, vt] = stripV(strip);
    const slope = Math.atan2(radius, apex - rim);
    for (const l of layers) {
      const geo = this.meshes.at(l, c[0], c[2]);
      const first = geo.nv;
      for (let k = 0; k <= sides; k++) {
        const a = phase + (k / sides) * Math.PI * 2;
        const r = radius * (k % 2 === 0 ? 1 : 1 - jag);
        const n: V3 = [Math.cos(a) * Math.cos(slope), Math.sin(slope), Math.sin(a) * Math.cos(slope)];
        geo.vert([c[0] + Math.cos(a) * r, rim - (k % 2 === 0 ? droop : 0), c[2] + Math.sin(a) * r], n, (k / sides) * ((radius * 6.283) / strip.mPerU), vb, tint);
      }
      const top = geo.vert([c[0], apex, c[2]], [0, 1, 0], 0.5 * ((radius * 6.283) / strip.mPerU), vt, tint);
      // Seen from outside, the rim runs clockwise from above.
      for (let k = 0; k < sides; k++) geo.index(first + k + 1, first + k, top);
    }
  }

  /** A tube of `sides` between two rings: a trunk, a branch, a stump. */
  tube(layers: readonly Layer[], a: V3, b: V3, ra: number, rb: number, sides: number, strip: Strip, tint: Rgb, cap = false) {
    const axis = norm(sub(b, a));
    const ref: V3 = Math.abs(axis[1]) < 0.9 ? [0, 1, 0] : [1, 0, 0];
    const u = norm(cross(axis, ref));
    const w = cross(axis, u);
    const [vb, vt] = stripV(strip);
    const ring = (c: V3, r: number) => {
      const pts: V3[] = [];
      for (let k = 0; k < sides; k++) {
        const t = (k / sides) * Math.PI * 2;
        pts.push([c[0] + (u[0] * Math.cos(t) + w[0] * Math.sin(t)) * r, c[1] + (u[1] * Math.cos(t) + w[1] * Math.sin(t)) * r, c[2] + (u[2] * Math.cos(t) + w[2] * Math.sin(t)) * r]);
      }
      return pts;
    };
    const around = (Math.max(ra, rb) * 6.283) / strip.mPerU;
    for (const l of layers) {
      const geo = this.meshes.at(l, a[0], a[2]);
      const first = geo.nv;
      for (const [c, r, v] of [
        [a, ra, vb],
        [b, rb, vt],
      ] as const) {
        const pts = ring(c, r);
        for (let k = 0; k <= sides; k++) {
          const p = pts[k % sides];
          geo.vert(p, norm([p[0] - c[0], p[1] - c[1], p[2] - c[2]]), (k / sides) * around, v, tint);
        }
      }
      const n = sides + 1;
      for (let k = 0; k < sides; k++) geo.index(first + k, first + k + 1, first + n + k + 1, first + k, first + n + k + 1, first + n + k);
      if (cap) {
        const hi = ring(b, rb);
        for (let k = 1; k + 1 < sides; k++) geo.tri(hi[0], hi[k], hi[k + 1], [0, vt], [0.1, vt], [0.1, vb], scale(tint, 1.25), axis);
      }
    }
  }

  conifer(x: number, z: number, h: number, r: number, rng: Rng) {
    const y = this.height(x, z);
    // Trees she can walk up to get every level; the forest beyond, only the ones seen from afar.
    const reach = Math.max(Math.abs(x), Math.abs(z));
    const NEAR: Layer[] = reach < HALF + 50 ? [Layer.Near] : [];
    const MID: Layer[] = reach < HALF + 260 ? [Layer.Mid] : [];
    const tint = scale([0.17, 0.26, 0.21], rng.range(0.7, 1.15));
    const lean: V3 = [rng.range(-0.03, 0.03) * h, 0, rng.range(-0.03, 0.03) * h];
    const phase = rng.range(0, 6.28);
    this.tube(NEAR, [x, y - 0.3, z], [x + lean[0] * 0.35, y + h * 0.38, z + lean[2] * 0.35], r * 0.13, r * 0.07, 5, STRIP.BARK, TRUNK);
    const tiers = 5;
    for (let k = 0; k < tiers; k++) {
      const t = k / tiers;
      const c: V3 = [x + lean[0] * t, 0, z + lean[2] * t];
      this.cone(NEAR, c, y + h * (0.16 + 0.155 * k), y + h * (0.16 + 0.155 * k + 0.34), r * (1 - 0.17 * k), 8, STRIP.NEEDLE, scale(tint, 0.85 + 0.07 * k), phase + k * 0.9, 0.24, h * 0.03);
    }
    this.tube(MID, [x, y - 0.3, z], [x, y + h * 0.3, z], r * 0.12, r * 0.08, 3, STRIP.BARK, TRUNK);
    for (let k = 0; k < 3; k++) this.cone(MID, [x + lean[0] * k * 0.3, 0, z + lean[2] * k * 0.3], y + h * (0.17 + 0.26 * k), y + h * (0.17 + 0.26 * k + 0.42), r * (1 - 0.28 * k), 5, STRIP.NEEDLE, tint, phase + k, 0.18, 0);
    this.cone([Layer.Far], [x, 0, z], y + h * 0.12, y + h, r * 0.8, 4, STRIP.NEEDLE, scale(tint, 0.95), phase);
    // For the light bake: one cone casts the tree's shadow.
    const pts: V3[] = [];
    for (let k = 0; k < 6; k++) pts.push([x + Math.cos(k * 1.047) * r * 0.75, y + h * 0.2, z + Math.sin(k * 1.047) * r * 0.75]);
    for (let k = 0; k < 6; k++) this.col.tri(pts[(k + 1) % 6], pts[k], [x, y + h, z], 4);
    this.count("conifers");
  }

  deadTree(x: number, z: number, h: number, rng: Rng) {
    const y = this.height(x, z);
    const tint = scale(DEAD, rng.range(0.75, 1.1));
    const lean: V3 = [rng.range(-0.18, 0.18) * h, h, rng.range(-0.18, 0.18) * h];
    const at = (t: number, bow = 0): V3 => [x + lean[0] * t + bow, y + lean[1] * t, z + lean[2] * t];
    const r0 = h * 0.045;
    this.tube([Layer.Near], [x, y - 0.3, z], at(0.4), r0 * 1.25, r0 * 0.9, 6, STRIP.DEAD, tint);
    this.tube([Layer.Near], at(0.4), at(0.75), r0 * 0.9, r0 * 0.55, 6, STRIP.DEAD, tint);
    this.tube([Layer.Near], at(0.75), at(1.0, rng.range(-0.3, 0.3)), r0 * 0.55, r0 * 0.12, 5, STRIP.DEAD, tint, true);
    this.tube([Layer.Mid], [x, y - 0.3, z], at(1.0), r0 * 1.2, r0 * 0.2, 4, STRIP.DEAD, tint);
    this.tube([Layer.Far], [x, y, z], at(1.0), r0 * 1.3, r0 * 0.3, 3, STRIP.DEAD, tint);
    const boughs = rng.int(3, 5);
    for (let k = 0; k < boughs; k++) {
      const t = rng.range(0.35, 0.85);
      const a = rng.range(0, 6.28);
      const len = h * rng.range(0.18, 0.34) * (1.1 - t);
      const from = at(t);
      const to: V3 = [from[0] + Math.cos(a) * len, from[1] + len * rng.range(0.25, 0.8), from[2] + Math.sin(a) * len];
      this.tube([Layer.Near], from, to, r0 * 0.4 * (1.2 - t), r0 * 0.06, 4, STRIP.DEAD, tint, true);
      if (k < 2) this.tube([Layer.Mid], from, to, r0 * 0.4, r0 * 0.08, 3, STRIP.DEAD, tint);
    }
    this.col.quad([x - r0, y, z], [x + r0, y, z], [x + r0 + lean[0], y + h, z + lean[2]], [x - r0 + lean[0], y + h, z + lean[2]], 4);
    this.count("dead trees");
  }

  stump(x: number, z: number, r: number, h: number, rng: Rng) {
    const y = this.height(x, z);
    const tint = scale(TRUNK, rng.range(0.9, 1.3));
    this.tube([Layer.Near], [x, y - 0.2, z], [x + rng.range(-0.04, 0.04), y + h, z + rng.range(-0.04, 0.04)], r * 1.15, r, 7, STRIP.BARK, tint, true);
    this.tube([Layer.Mid], [x, y - 0.2, z], [x, y + h, z], r * 1.15, r, 4, STRIP.BARK, tint, true);
    this.count("stumps");
  }

  rock(x: number, z: number, r: number, rng: Rng) {
    const y = this.height(x, z) + r * 0.15;
    const tint = scale(STONE, rng.range(0.7, 1.1));
    // An octahedron whose corners are pushed about, split once for the near mesh.
    const squash = rng.range(0.5, 0.9);
    const corner = (d: V3): V3 => {
      const k = r * (0.75 + 0.5 * hash01(Math.round(d[0] * 7 + x * 3), Math.round(d[2] * 7 + z * 3 + d[1] * 11), this.seed + 53));
      return [x + d[0] * k, y + d[1] * k * squash, z + d[2] * k];
    };
    const dirs: V3[] = [
      [1, 0, 0],
      [0, 0, 1],
      [-1, 0, 0],
      [0, 0, -1],
    ];
    const top: V3 = [0, 1, 0];
    const [vb, vt] = stripV(STRIP.ROCK);
    const u = (r * 2) / STRIP.ROCK.mPerU;
    const face = (layers: readonly Layer[], a: V3, b: V3, c: V3, depth: number) => {
      if (depth === 0) {
        const pa = corner(a);
        const pb = corner(b);
        const pc = corner(c);
        for (const l of layers) this.meshes.at(l, x, z).tri(pa, pb, pc, [0, vb], [u, vb], [u / 2, vt], tint);
        return;
      }
      const m = (p: V3, q: V3): V3 => norm([p[0] + q[0], p[1] + q[1], p[2] + q[2]]);
      const ab = m(a, b);
      const bc = m(b, c);
      const ca = m(c, a);
      face(layers, a, ab, ca, depth - 1);
      face(layers, ab, b, bc, depth - 1);
      face(layers, ca, bc, c, depth - 1);
      face(layers, ab, bc, ca, depth - 1);
    };
    for (let k = 0; k < 4; k++) {
      const a = dirs[k];
      const b = dirs[(k + 1) % 4];
      face([Layer.Near], b, a, top, 1);
      face(r > 2 ? [Layer.Mid, Layer.Far] : [Layer.Mid], b, a, top, 0);
    }
    this.col.quad([x - r, y, z - r], [x + r, y, z - r], [x + r, y + r * squash, z + r], [x - r, y + r * squash, z + r], 3);
    this.count("rocks");
  }

  /** The far mountains and the hills under them: always drawn. */
  backdrop() {
    const geo = this.meshes.at(Layer.Backdrop, 0, 0);
    const [vb, vt] = stripV(STRIP.SNOW);
    const segs = 72;
    const ridge = (k: number, r: number, amp: number, seed: number): V3 => {
      const a = (k / segs) * Math.PI * 2;
      const h = amp * (0.35 + 0.65 * fbm(Math.cos(a) * 2.6 + 9, Math.sin(a) * 2.6 + 9, seed, 4)) * (0.75 + 0.5 * hash01(k, seed, 3));
      return [Math.cos(a) * r, h, Math.sin(a) * r];
    };
    for (let k = 0; k < segs; k++) {
      const k1 = (k + 1) % segs;
      // Far range: a foot, a shoulder where the snow line sits, a crest.
      const foot0: V3 = [Math.cos((k / segs) * 6.2832) * 2500, -40, Math.sin((k / segs) * 6.2832) * 2500];
      const foot1: V3 = [Math.cos((k1 / segs) * 6.2832) * 2500, -40, Math.sin((k1 / segs) * 6.2832) * 2500];
      const c0 = ridge(k, 3900, 1150, this.seed + 61);
      const c1 = ridge(k1, 3900, 1150, this.seed + 61);
      const s0: V3 = [c0[0] * 0.86, c0[1] * 0.55, c0[2] * 0.86];
      const s1: V3 = [c1[0] * 0.86, c1[1] * 0.55, c1[2] * 0.86];
      geo.quad(foot1, foot0, s0, s1, [((k % 4) + 1) / 4, (k % 4) / 4, vb, vt], PEAK);
      geo.quad(s1, s0, c0, c1, [((k % 4) + 1) / 4, (k % 4) / 4, vb, vt], SNOW);
      // The hills beyond the field's own rim.
      const h0 = ridge(k, 1900, 190, this.seed + 67);
      const h1 = ridge(k1, 1900, 190, this.seed + 67);
      const b0: V3 = [h0[0] * 0.62, 40, h0[2] * 0.62];
      const b1: V3 = [h1[0] * 0.62, 40, h1[2] * 0.62];
      geo.quad(b1, b0, h0, h1, [((k % 4) + 1) / 4, (k % 4) / 4, vb, vt], scale(HEATH, 0.8));
      geo.quad(h1, h0, [h0[0] * 1.35, -30, h0[2] * 1.35], [h1[0] * 1.35, -30, h1[2] * 1.35], [((k % 4) + 1) / 4, (k % 4) / 4, vb, vt], scale(HEATH, 0.6));
    }
  }
}

export function generate(seed: number): Generated {
  const b = new Builder(seed);
  const n = FIELD_N - 1;
  const per = CELL / FIELD_CELL;
  // ---- the ground: every 64 m cell at 4 m and at 8 m, every 256 m cell at 32 m
  for (let cj = 0; cj < n; cj += per) {
    for (let ci = 0; ci < n; ci += per) {
      b.ground(Layer.GroundNear, ci, cj, per, 1);
      b.ground(Layer.GroundMid, ci, cj, per, 2);
    }
  }
  const big = SUPER / FIELD_CELL;
  for (let cj = 0; cj < n; cj += big) for (let ci = 0; ci < n; ci += big) b.ground(Layer.GroundFar, ci, cj, big, 8);
  // For the light bake, the ground at 8 m.
  for (let j = 0; j < n; j += 2) {
    for (let i = 0; i < n; i += 2) {
      const p = (a: number, c: number): V3 => [FIELD_MIN + a * FIELD_CELL, b.at(a, c), FIELD_MIN + c * FIELD_CELL];
      b.col.quad(p(i, j + 2), p(i + 2, j + 2), p(i + 2, j), p(i, j), 0);
    }
  }
  b.backdrop();

  // ---- what stands on it
  const obstacles: Obstacle[] = [];
  const rng = new Rng(seed ^ 0x5eed);
  const reach = 1000;
  for (let gz = -reach; gz < reach; gz += 9) {
    for (let gx = -reach; gx < reach; gx += 9) {
      const r = rng.fork((gx + 2048) * 4096 + gz + 2048);
      const x = gx + r.range(0, 9);
      const z = gz + r.range(0, 9);
      if (battleground(x, z)) continue;
      const f = forest(x, z, seed);
      if (r.next() < f * 0.3) {
        const h = r.range(13, 24) * (0.7 + 0.5 * f);
        const rad = h * r.range(0.16, 0.22);
        b.conifer(x, z, h, rad, r);
        if (Math.abs(x) < HALF && Math.abs(z) < HALF) obstacles.push({ x, z, r: rad * 0.16 + 0.25, kind: 0 });
      } else if (f < 0.5 && f > 0.04 && r.next() < 0.06) {
        // The forest's edge, where the army came through: stumps, and trees that are dead and bare.
        if (r.chance(0.45)) {
          const h = r.range(6, 13);
          b.deadTree(x, z, h, r);
          if (Math.abs(x) < HALF && Math.abs(z) < HALF) obstacles.push({ x, z, r: h * 0.05 + 0.2, kind: 1 });
        } else {
          const rad = r.range(0.25, 0.5);
          b.stump(x, z, rad, r.range(0.4, 1.0), r);
          if (Math.abs(x) < HALF && Math.abs(z) < HALF) obstacles.push({ x, z, r: rad + 0.1, kind: 2 });
        }
      } else if (r.next() < 0.012 + 0.05 * Math.exp(-((x - streamX(z)) ** 2) / (2 * 20 * 20))) {
        const rad = r.range(0.5, 1.6) * (r.chance(0.12) ? 2.2 : 1);
        b.rock(x, z, rad, r);
        if (Math.abs(x) < HALF && Math.abs(z) < HALF) obstacles.push({ x, z, r: rad * 0.8, kind: 3 });
      }
    }
  }
  // A few bare trees on the open field, as in the long view over it.
  for (let k = 0; k < 26; k++) {
    const x = rng.range(-215, 215);
    const z = rng.range(-560, 620);
    // Only at the margins of the ground, clear of the ranks.
    if (Math.abs(x) < 196) continue;
    const h = rng.range(7, 12);
    b.deadTree(x, z, h, rng);
    obstacles.push({ x, z, r: h * 0.05 + 0.2, kind: 1 });
  }

  // ---- the army: cohorts in eleven files across the field, twelve deep, facing south
  const musters: Muster[] = [];
  const mr = new Rng(seed ^ 0xa11a);
  for (let row = 0; row < 12; row++) {
    for (let col = 0; col < 11; col++) {
      const x = (col - 5) * 38 + mr.range(-4, 4);
      const z = 392 - row * 62 + mr.range(-7, 7);
      const wide = mr.chance(0.3);
      musters.push({ x, z, yaw: Math.PI + mr.range(-0.05, 0.05), spacing: mr.range(1.75, 2.05), cols: wide ? 8 : 6, rows: wide ? 4 : 5, mix: mr.pick([0, 0, 1, 1, 2, 3, 3, 3]), captain: 1 });
    }
  }
  // The demon's guard: greatswords, in front of her.
  musters.push({ x: 0, z: -352, yaw: Math.PI, spacing: 2.1, cols: 9, rows: 3, mix: 2, captain: 1 });
  const stage: Stage = { start: [0, 520, 0], demon: [0, -384, Math.PI], gate: [0, 900], town: [0, 9000, 10], half: HALF, musters };
  const knights = musters.reduce((s, m) => s + m.cols * m.rows, 0);
  return { meshes: b.meshes, col: b.col, field: b.field, obstacles, stage, knights, counts: b.counts };
}
