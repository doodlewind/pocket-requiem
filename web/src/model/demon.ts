// The demon: lilac hair in a rounded bob with two tasselled braids, two
// ringed horns that curve up, a dark bodice over a white skirt with a magenta
// tail at the hips, black gloves to the upper arm and striped boots to the
// thigh. In her left hand, raised, the scales: a gold beam with two pans, a
// pale flame on each.
//
// Authored like the mage (mage.ts), on the same bone list.

import { BONE } from "../sim/abi.gen";
import { Body, ellipse, MeshOut, Rgb, Rigid, SkinModel, Trim, V3 } from "./sdf";

const SKIN: Rgb = [0.98, 0.92, 0.9];
const HAIR: Rgb = [0.74, 0.6, 0.82];
const HAIR_SHADE: Rgb = [0.6, 0.47, 0.72];
const HORN: Rgb = [0.88, 0.86, 0.8];
const HORN_RING: Rgb = [0.66, 0.64, 0.62];
const BODICE: Rgb = [0.14, 0.14, 0.22];
const SWIRL: Rgb = [0.94, 0.94, 0.97];
const SKIRT: Rgb = [0.95, 0.94, 0.95];
const TAIL: Rgb = [0.62, 0.2, 0.46];
const GLOVE: Rgb = [0.09, 0.09, 0.12];
const BOOT: Rgb = [0.16, 0.15, 0.22];
const STRIPE: Rgb = [0.5, 0.42, 0.62];
const GOLD: Rgb = [0.86, 0.7, 0.36];
const GOLD_LIGHT: Rgb = [0.95, 0.85, 0.55];
const FLAME: Rgb = [0.78, 0.97, 0.98];
const EYE: Rgb = [0.2, 0.24, 0.5];
const LASH: Rgb = [0.2, 0.14, 0.22];

export function buildDemon(bind: Float32Array, cells: [number, number] = [0.016, 0.0095]): SkinModel {
  const b = new Body(bind, 1, 0.03);
  const B = BONE;

  // ---- trunk: a close bodice, bare shoulders
  b.ell(B.PELVIS, [0, 0.0, 0.004], [0.112, 0.094, 0.088], BODICE);
  b.cone(B.SPINE, [0, -0.03, 0.002], [0, 0.1, 0.0], 0.088, 0.094, BODICE, { k: 0.03, bone2: B.CHEST });
  b.ell(B.CHEST, [0, 0.065, -0.004], [0.106, 0.11, 0.084], BODICE, { k: 0.08 });
  b.ell(B.CHEST, [0, 0.14, 0.0], [0.108, 0.07, 0.078], SKIN, { k: 0.05 });
  for (const [s, c, u] of [
    [-1, B.CLAV_L, B.ARM_UL],
    [1, B.CLAV_R, B.ARM_UR],
  ] as const) {
    b.cone(c, [s * 0.02, 0.0, 0.0], [s * 0.12, -0.004, 0.0], 0.046, 0.04, SKIN, { k: 0.03, bone2: u });
  }
  b.cone(B.NECK, [0, -0.02, 0.004], [0, 0.07, 0.0], 0.036, 0.033, SKIN, { k: 0.015, bone2: B.HEAD });

  // ---- arms: bare to a white wrap, then black gloves
  for (const [u, l, h, s] of [
    [B.ARM_UL, B.ARM_LL, B.HAND_L, -1],
    [B.ARM_UR, B.ARM_LR, B.HAND_R, 1],
  ] as const) {
    b.cone(u, [0, -0.01, 0], [0, -0.235, 0], 0.034, 0.03, SKIN, { k: 0.03, bone2: l });
    b.cone(u, [0, -0.07, 0], [0, -0.12, 0], 0.036, 0.035, SWIRL, { k: 0.004 });
    b.cone(u, [0, -0.13, 0], [0, -0.235, 0], 0.034, 0.031, GLOVE, { k: 0.006, bone2: l });
    b.ell(l, [0, -0.004, 0.005], [0.03, 0.031, 0.032], GLOVE, { k: 0.02 });
    b.cone(l, [0, 0.0, 0], [0, -0.22, 0], 0.03, 0.022, GLOVE, { k: 0.02, bone2: h });
    b.ell(h, [0, -0.034, 0], [0.023, 0.035, 0.017], GLOVE, { k: 0.01 });
    b.ell(h, [0, -0.06, -0.012], [0.024, 0.02, 0.023], GLOVE, { k: 0.01 });
    b.cone(h, [-s * 0.022, -0.026, -0.01], [-s * 0.015, -0.056, -0.028], 0.0095, 0.0085, GLOVE, { k: 0.006 });
  }

  // ---- legs: boots to the thigh, striped
  for (const [u, l, f, s] of [
    [B.LEG_UL, B.LEG_LL, B.FOOT_L, -1],
    [B.LEG_UR, B.LEG_LR, B.FOOT_R, 1],
  ] as const) {
    b.ell(B.PELVIS, [s * 0.064, -0.05, 0.028], [0.068, 0.08, 0.072], SKIN, { k: 0.03 });
    b.cone(u, [0, 0.0, 0], [0, -0.39, 0], 0.062, 0.044, SKIN, { k: 0.035, bone2: l, sigma: 0.026 });
    b.cone(u, [0, -0.14, 0], [0, -0.39, 0], 0.057, 0.046, BOOT, { k: 0.006, bone2: l, sigma: 0.026 });
    b.ell(l, [0, 0.0, -0.01], [0.042, 0.043, 0.044], BOOT, { k: 0.02 });
    b.cone(l, [0, 0, 0], [0, -0.375, 0], 0.043, 0.03, BOOT, { k: 0.03, bone2: f, sigma: 0.026 });
    b.ell(l, [0, -0.12, 0.018], [0.043, 0.095, 0.046], BOOT, { k: 0.025 });
    b.ell(f, [0, -0.014, 0.016], [0.035, 0.038, 0.04], BOOT, { k: 0.02 });
    b.box(f, [0, -0.03, -0.06], [0.034, 0.022, 0.08], 0.02, BOOT, { k: 0.02 });
    for (const y of [-0.18, -0.25, -0.32]) b.paint(u, [0, y, 0], [0.1, 0.012, 0.1], STRIPE);
    for (const y of [-0.08, -0.16, -0.24]) b.paint(l, [0, y, 0], [0.1, 0.012, 0.1], STRIPE);
  }

  // ---- head: rounder than the mage's, with a small chin
  b.ell(B.HEAD, [0, 0.114, -0.002], [0.095, 0.112, 0.103], SKIN, { k: 0.02 });
  b.cone(B.HEAD, [0, 0.062, -0.028], [0, 0.012, -0.06], 0.064, 0.026, SKIN, { k: 0.04 });
  b.ell(B.HEAD, [0, 0.06, -0.048], [0.06, 0.035, 0.046], SKIN, { k: 0.03 });
  // The bob: a cap, full at the sides, cut level at the jaw.
  b.ell(B.HEAD, [0, 0.14, 0.014], [0.108, 0.104, 0.114], HAIR, { k: 0.006 });
  for (const s of [-1, 1]) {
    b.ell(B.HEAD, [s * 0.086, 0.078, 0.012], [0.058, 0.092, 0.078], HAIR, { k: 0.03 });
    // The horns: from the temples outward, then up and in, ringed.
    const horn: V3[] = [
      [s * 0.07, 0.19, 0.0],
      [s * 0.16, 0.22, 0.0],
      [s * 0.2, 0.3, 0.0],
      [s * 0.16, 0.38, 0.0],
      [s * 0.09, 0.4, 0.0],
    ];
    for (let k = 0; k + 1 < horn.length; k++) b.cone(B.HEAD, horn[k], horn[k + 1], 0.034 - k * 0.006, 0.028 - k * 0.006, HORN, { k: 0.006 });
    for (let k = 0; k < 7; k++) {
      const t = (k + 0.5) / 7;
      const i = Math.min(Math.floor(t * 4), 3);
      const u = t * 4 - i;
      const p: V3 = [horn[i][0] + (horn[i + 1][0] - horn[i][0]) * u, horn[i][1] + (horn[i + 1][1] - horn[i][1]) * u, 0];
      b.paintEll(B.HEAD, p, [0.045, 0.006, 0.045], HORN_RING, { rot: [0, 0, s * (0.2 + t * 2.4)] });
    }
  }
  // The braids: on the tail bones, hanging in front of the shoulders.
  for (const [t0, t1, t2] of [
    [B.TAIL_L0, B.TAIL_L1, B.TAIL_L2],
    [B.TAIL_R0, B.TAIL_R1, B.TAIL_R2],
  ] as const) {
    b.cone(t0, [0, 0.0, 0], [0, -0.24, 0], 0.026, 0.03, HAIR, { k: 0.01, bone2: t1, sigma: 0.05 });
    b.cone(t1, [0, 0.0, 0], [0, -0.24, 0], 0.03, 0.028, HAIR_SHADE, { k: 0.01, bone2: t2, sigma: 0.05 });
    b.cone(t2, [0, 0.0, 0], [0, -0.2, 0], 0.03, 0.012, HAIR, { k: 0.01, sigma: 0.05 });
    b.paint(t2, [0, -0.02, 0], [0.06, 0.01, 0.06], GOLD);
  }

  const out = new MeshOut();
  const neck = b.joint(B.HEAD)[1] - 0.01;
  b.mesh(out, cells[0], { yMax: neck + cells[0] });
  b.mesh(out, cells[1], { yMin: neck - cells[1] });

  // ---- the skirt: white and ruffled, short; the tail of magenta cloth over the hips
  const skirt = new Body(bind, 1, 0.05);
  const pelvis = b.joint(B.PELVIS);
  for (const [s, bone] of [
    [-1, B.SKIRT_L],
    [1, B.SKIRT_R],
  ] as const) {
    skirt.cone(B.PELVIS, [s * 0.024, 0.07, 0.004], [s * 0.07, -0.22, 0.004], 0.09, 0.17, SKIRT, { k: 0.04, bone2: bone });
    skirt.cone(B.PELVIS, [s * 0.05, 0.075, 0.04], [s * 0.1, -0.3, 0.09], 0.07, 0.1, TAIL, { k: 0.02, bone2: bone });
  }
  skirt.mesh(out, cells[0], { yMin: pelvis[1] - 0.2 });

  const t = new Trim(out, b);
  const ts = new Trim(out, skirt);
  const chest = b.joint(B.CHEST);
  const spine = b.joint(B.SPINE);
  // The skirt's ruffled hem, the gold buttons' band at the hips, the choker and the gorget.
  ts.cord(ts.loop([pelvis[0], pelvis[1] - 0.192, pelvis[2] + 0.004], [0, 1, 0], 0.5, 44, 0.004, (a) => 0.012 * Math.sin(a * 16)), 0.011, SKIRT, true, 5, 0.9);
  ts.cord(ts.loop([spine[0], spine[1] - 0.02, spine[2]], [0, 1, 0], 0.3, 28, 0.002), 0.01, GOLD, true, 5, 0.5);
  t.cord(t.loop(b.at(B.NECK, [0, 0.03, 0.002]), [0, 1, 0], 0.12, 20, 0.001), 0.006, BODICE, true, 4, 0.5);
  t.cord(t.loop(b.at(B.NECK, [0, -0.012, 0.002]), [0, 1, 0], 0.14, 22, 0.002, (a) => -0.02 * Math.max(0, -Math.sin(a))), 0.008, GOLD, true, 5, 0.6);
  // The white swirls on the bodice: two spirals turned toward each other.
  const front = chest[2] - 0.25;
  for (const s of [-1, 1]) {
    const path: V3[] = [];
    for (let k = 0; k <= 22; k++) {
      const a = (k / 22) * Math.PI * 2.6;
      const r = 0.012 + 0.03 * (1 - k / 22);
      path.push(b.snap([s * (0.045 - Math.cos(a) * r), chest[1] + 0.06 + Math.sin(a) * r, front], [0, 0, 1], 0.003, 0.4));
    }
    t.cord(path, 0.0052, SWIRL, false, 4, 0.5);
    const down: V3[] = [];
    for (let k = 0; k <= 8; k++) down.push(b.snap([s * (0.03 - 0.02 * (k / 8)), chest[1] + 0.02 - 0.14 * (k / 8), front], [0, 0, 1], 0.003, 0.4));
    t.cord(down, 0.0045, SWIRL, false, 4, 0.5);
  }

  face(out, b);
  scales(out, b);
  return out.model();
}

function face(out: MeshOut, b: Body) {
  const r = new Rigid(out, b, BONE.HEAD);
  const into: V3 = [0, 0, 1];
  const z = -0.17;
  for (const s of [-1, 1]) {
    const cx = s * 0.041;
    const cy = 0.09;
    r.decal(ellipse(cx, cy, 0.021, 0.017, z, 16), into, 0.001, [0.98, 0.98, 0.99]);
    r.decal(ellipse(cx, cy, 0.0136, 0.0156, z, 16), into, 0.0016, EYE);
    r.decal(ellipse(cx, cy + 0.001, 0.006, 0.0078, z, 10), into, 0.0022, [0.05, 0.06, 0.16]);
    r.decal(ellipse(cx - 0.006, cy + 0.006, 0.0036, 0.0036, z, 8), into, 0.003, [1, 1, 1]);
    const n = 10;
    const curve = (k: number, rx: number, ry: number, up: number): V3 => {
      const a = (k / n) * Math.PI;
      return [cx - s * Math.cos(a) * rx, cy + Math.sin(a) * ry + up, z];
    };
    for (let k = 0; k < n; k++) r.decal([curve(k + 1, 0.024, 0.016, 0.004), curve(k + 1, 0.0215, 0.0118, 0.0018), curve(k, 0.0215, 0.0118, 0.0018), curve(k, 0.024, 0.016, 0.004)], into, 0.0034, LASH);
  }
  // A small smile.
  const mouth: V3[] = [];
  for (let k = 0; k <= 6; k++) mouth.push([-0.014 + (0.028 * k) / 6, 0.04 - 0.004 * Math.sin((k / 6) * Math.PI), z]);
  for (let k = 0; k < 6; k++) r.decal([mouth[k], mouth[k + 1], [mouth[k + 1][0], mouth[k + 1][1] + 0.0018, z], [mouth[k][0], mouth[k][1] + 0.0018, z]], into, 0.0016, [0.7, 0.4, 0.45]);
  // The fringe, cut straight.
  for (let k = -3; k <= 3; k++) {
    const x = k * 0.026;
    r.tube(
      [
        [x * 0.5, 0.236, -0.02],
        [x * 0.9, 0.19, -0.09],
        [x, 0.135, -0.108],
      ],
      [0.018, 0.022, 0.016],
      k % 2 === 0 ? HAIR : HAIR_SHADE,
      6,
      { axis: [0, 0, 1], factor: 0.4 },
    );
  }
}

/** The scales, hanging from the left hand: a beam, two pans on cords, a heart above the pivot, a flame on each pan. */
function scales(out: MeshOut, b: Body) {
  const h = new Rigid(out, b, BONE.HAND_L);
  // The hand's frame: -Y down the limb. With the arm raised the hand points up, so the scales hang toward +Y of the hand.
  const pivot: V3 = [0, -0.07, -0.02];
  const down = (d: number): V3 => [pivot[0], pivot[1] + d, pivot[2]];
  h.tube([pivot, down(0.1)], [0.006, 0.006], GOLD, 5);
  const beamY = 0.1;
  h.tube(
    [
      [-0.2, pivot[1] + beamY, pivot[2]],
      [0.2, pivot[1] + beamY, pivot[2]],
    ],
    [0.008, 0.008],
    GOLD_LIGHT,
    6,
  );
  // The heart above the pivot: two lobes and a point.
  for (const s of [-1, 1]) h.tube([down(beamY - 0.02), [s * 0.022, pivot[1] + beamY - 0.05, pivot[2]]], [0.012, 0.016], GOLD_LIGHT, 6);
  for (const s of [-1, 1]) {
    const x = s * 0.2;
    const panY = pivot[1] + beamY + 0.2;
    for (const c of [-0.05, 0.05]) h.tube([[x, pivot[1] + beamY, pivot[2]], [x + c, panY, pivot[2]]], [0.002, 0.002], GOLD, 3);
    // The pan: a shallow bowl.
    h.tube(
      [
        [x, panY, pivot[2]],
        [x, panY + 0.012, pivot[2]],
        [x, panY + 0.03, pivot[2]],
      ],
      [0.07, 0.06, 0.012],
      GOLD,
      10,
      null,
      GOLD_LIGHT,
    );
    // The flame on it.
    h.tube(
      [
        [x, panY - 0.004, pivot[2]],
        [x, panY - 0.03, pivot[2]],
        [x + s * 0.006, panY - 0.075, pivot[2]],
      ],
      [0.026, 0.03, 0.002],
      FLAME,
      7,
    );
  }
}
