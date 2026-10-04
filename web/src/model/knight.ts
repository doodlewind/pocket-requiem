// The army's knights: plate armour over the bodies of soldiers who fell in
// the north, each with no head. Three kinds share the frame and differ in
// their harness and their weapon: a longsword, a halberd, a greatsword.
//
// The plates are primitives of one blended field (sdf.ts), joined with a
// narrow blend so every plate keeps its edge. The steel is pitted and has
// rusted in patches. Where the neck was, the collar opens on darkness.
//
// Everything is authored in bone frames of the simulation's bind pose: +Y up
// the bone, -Z forward, +X to the figure's right; a limb runs down its -Y.
// A weapon lies along the prop bone's +Y, the right hand at its origin.

import { BONE, FIGURE } from "../sim/abi.gen";
import { Body, MeshOut, Rgb, Rigid, SkinModel, Trim, V3 } from "./sdf";

const STEEL: Rgb = [0.52, 0.55, 0.62];
const STEEL_DARK: Rgb = [0.34, 0.37, 0.45];
const STEEL_LIGHT: Rgb = [0.68, 0.71, 0.78];
const IRON: Rgb = [0.24, 0.26, 0.32];
const RUST: Rgb = [0.38, 0.27, 0.23];
const MAIL: Rgb = [0.2, 0.22, 0.27];
const CLOTH: Rgb = [0.17, 0.16, 0.2];
const LEATHER: Rgb = [0.24, 0.18, 0.15];
const VOID: Rgb = [0.02, 0.025, 0.045];
const BRASS: Rgb = [0.55, 0.45, 0.28];
const WOOD: Rgb = [0.3, 0.22, 0.17];
const EDGE: Rgb = [0.78, 0.82, 0.9];

/** What a level of detail keeps. */
export interface KnightDetail {
  /** Mesh cell size in metres. */
  cell: number;
  /** Outward offset of the surface, which keeps thin parts whole at a coarse cell. */
  inflate?: number;
  /** Rims and straps as cords. */
  trims?: boolean;
}

/** A deterministic scatter for the rust: `n` numbers in [0, 1) from a seed. */
function scatter(seed: number, n: number): number[] {
  const out: number[] = [];
  let s = (seed * 2654435761) >>> 0;
  for (let k = 0; k < n; k++) {
    s ^= s << 13;
    s >>>= 0;
    s ^= s >>> 17;
    s ^= s << 5;
    s >>>= 0;
    out.push(s / 4294967296);
  }
  return out;
}

export function buildKnight(kind: number, bind: Float32Array, detail: KnightDetail = { cell: 0.024, trims: true }): SkinModel {
  const b = new Body(bind, 1, 0.04);
  const B = BONE;
  const heavy = kind === FIGURE.KNIGHT_GREAT;
  const light = kind === FIGURE.KNIGHT_HALBERD;
  const plate = heavy ? STEEL_DARK : STEEL;
  const under = light ? MAIL : IRON;
  const j = 0.008;

  // ---- trunk: a wide flat-planed cuirass over a narrow waist
  b.ell(B.PELVIS, [0, 0.0, 0.004], [0.15, 0.115, 0.118], under);
  b.cone(B.SPINE, [0, -0.02, 0.002], [0, 0.1, 0.0], 0.132, 0.15, under, { k: 0.02, bone2: B.CHEST });
  // The plackart over the belly, and the breastplate with a keel down its middle.
  b.box(B.SPINE, [0, 0.05, -0.012], [0.135, 0.07, 0.115], 0.06, plate, { k: 0.01 });
  b.box(B.CHEST, [0, 0.105, -0.012], [0.19, 0.125, 0.135], 0.085, plate, { k: 0.012 });
  b.ell(B.CHEST, [0, 0.085, -0.125], [0.05, 0.13, 0.05], plate, { k: 0.04 });
  // The backplate.
  b.box(B.CHEST, [0, 0.11, 0.045], [0.175, 0.12, 0.085], 0.07, STEEL_DARK, { k: 0.012 });
  // Mail shows at the armpits and under the arms.
  b.paint(B.CHEST, [0.2, 0.1, 0], [0.03, 0.08, 0.2], MAIL);
  b.paint(B.CHEST, [-0.2, 0.1, 0], [0.03, 0.08, 0.2], MAIL);
  // The fauld: lames under the waist, then a tasset over each thigh (a long mail skirt for the halberdier).
  if (light) {
    b.cone(B.PELVIS, [0, 0.07, 0.004], [0, -0.24, 0.004], 0.15, 0.2, MAIL, { k: 0.012 });
    b.paint(B.PELVIS, [0, -0.25, 0], [0.4, 0.012, 0.4], CLOTH);
  } else {
    b.cone(B.PELVIS, [0, 0.075, 0.004], [0, 0.03, 0.004], 0.15, 0.166, plate, { k: j });
    b.cone(B.PELVIS, [0, 0.03, 0.004], [0, -0.02, 0.004], 0.164, 0.18, plate, { k: 0.004 });
    b.cone(B.PELVIS, [0, -0.02, 0.004], [0, -0.07, 0.004], 0.178, 0.192, plate, { k: 0.004 });
    b.paint(B.PELVIS, [0, 0.031, 0], [0.4, 0.005, 0.4], IRON);
    b.paint(B.PELVIS, [0, -0.019, 0], [0.4, 0.005, 0.4], IRON);
    b.paint(B.PELVIS, [0, -0.08, 0], [0.4, 0.012, 0.4], MAIL);
  }
  // The belt.
  b.paint(B.SPINE, [0, -0.02, 0], [0.4, 0.016, 0.4], LEATHER);

  // ---- the collar: a gorget standing round where the neck was, cut level, open on the dark inside
  b.cone(B.NECK, [0, -0.11, 0.0], [0, -0.01, 0.0], 0.155, 0.118, plate, { k: 0.012 });
  b.cone(B.NECK, [0, -0.02, 0.0], [0, 0.03, 0.0], 0.118, 0.126, plate, { k: 0.004 });
  b.carve(B.NECK, [0, 0.142, 0.0], [0.4, 0.1, 0.4]);
  b.carve(B.NECK, [0, 0.04, 0.0], [0.086, 0.075, 0.086], { k: 0.004 });
  b.paintEll(B.NECK, [0, 0.03, 0.0], [0.094, 0.085, 0.094], VOID);

  // ---- shoulders and arms
  for (const [c, u, l, h, s] of [
    [B.CLAV_L, B.ARM_UL, B.ARM_LL, B.HAND_L, -1],
    [B.CLAV_R, B.ARM_UR, B.ARM_LR, B.HAND_R, 1],
  ] as const) {
    const sx = heavy ? 0.185 : 0.165;
    b.cone(c, [s * 0.04, 0.0, 0], [s * (sx - 0.02), -0.004, 0], 0.08, 0.07, under, { k: 0.02, bone2: u });
    // Pauldron: a cap over the shoulder and lames down the arm.
    if (heavy) {
      b.ell(c, [s * (sx - 0.015), 0.03, 0.0], [0.13, 0.08, 0.125], plate, { k: j });
      b.ell(u, [s * 0.014, -0.06, 0.0], [0.095, 0.06, 0.1], plate, { k: 0.004 });
      b.ell(u, [s * 0.014, -0.115, 0.0], [0.084, 0.05, 0.09], plate, { k: 0.004 });
      // A flange standing up beside the collar.
      b.box(c, [s * 0.12, 0.1, 0.0], [0.012, 0.06, 0.085], 0.01, plate, { k: 0.008, rot: [0, 0, -s * 0.4] });
    } else if (light) {
      b.ell(c, [s * (sx - 0.02), 0.02, 0.0], [0.098, 0.062, 0.098], plate, { k: j });
      b.ell(u, [s * 0.008, -0.055, 0.0], [0.076, 0.045, 0.08], plate, { k: 0.004 });
    } else {
      b.ell(c, [s * (sx - 0.018), 0.025, 0.0], [0.114, 0.07, 0.11], plate, { k: j });
      b.ell(u, [s * 0.01, -0.058, 0.0], [0.086, 0.052, 0.09], plate, { k: 0.004 });
      b.ell(u, [s * 0.01, -0.105, 0.0], [0.076, 0.044, 0.08], plate, { k: 0.004 });
    }
    // Upper arm in mail, a plate on its outside; the elbow's cop and its wing; the vambrace.
    b.cone(u, [0, -0.03, 0], [0, -0.295, 0], 0.052, 0.046, MAIL, { k: 0.012, bone2: l, sigma: 0.035 });
    if (!light) b.ell(u, [s * 0.016, -0.19, 0.0], [0.048, 0.085, 0.055], plate, { k: 0.006 });
    b.ell(l, [0, 0.0, 0.014], [0.054, 0.052, 0.062], plate, { k: 0.006 });
    b.ell(l, [s * 0.05, 0.0, 0.02], [0.018, 0.05, 0.05], plate, { k: 0.008 });
    b.cone(l, [0, -0.045, 0], [0, -0.25, 0], 0.055, 0.044, plate, { k: 0.006, bone2: h, sigma: 0.035 });
    b.cone(l, [0, -0.205, 0], [0, -0.262, 0], 0.05, 0.058, STEEL_DARK, { k: 0.004 });
    // Gauntlet: a closed fist.
    b.ell(h, [0, -0.045, 0], [0.042, 0.055, 0.035], STEEL_DARK, { k: 0.008 });
    b.ell(h, [0, -0.08, -0.016], [0.041, 0.033, 0.039], IRON, { k: 0.008 });
    b.cone(h, [-s * 0.035, -0.036, -0.014], [-s * 0.024, -0.078, -0.042], 0.015, 0.013, IRON, { k: 0.006 });
  }

  // ---- legs: full cuisses, pointed knee cops, greaves fuller at the calf, long sabatons
  for (const [u, l, f, s] of [
    [B.LEG_UL, B.LEG_LL, B.FOOT_L, -1],
    [B.LEG_UR, B.LEG_LR, B.FOOT_R, 1],
  ] as const) {
    b.ell(B.PELVIS, [s * 0.09, -0.07, 0.03], [0.098, 0.11, 0.1], under, { k: 0.02 });
    b.cone(u, [0, 0.0, 0], [0, -0.44, 0], 0.1, 0.07, light ? CLOTH : MAIL, { k: 0.014, bone2: l, sigma: 0.035 });
    if (!light) {
      b.ell(u, [s * 0.006, -0.2, -0.028], [0.098, 0.19, 0.082], plate, { k: 0.008 });
      // The tasset hanging from the fauld over the thigh.
      b.box(B.PELVIS, [s * 0.1, -0.13, -0.085], [0.07, 0.08, 0.014], 0.012, plate, { k: 0.006, bone2: u, rot: [-0.16, 0, 0] });
    }
    b.ell(l, [0, 0.0, -0.026], [0.07, 0.07, 0.08], plate, { k: 0.006 });
    b.cone(l, [0, 0.01, -0.07], [0, -0.01, -0.105], 0.04, 0.012, plate, { k: 0.01 });
    b.ell(l, [s * 0.064, 0.0, 0.0], [0.018, 0.055, 0.055], plate, { k: 0.008 });
    b.cone(l, [0, -0.05, 0], [0, -0.4, 0], 0.07, 0.052, plate, { k: 0.006, bone2: f, sigma: 0.035 });
    b.ell(l, [0, -0.15, 0.032], [0.064, 0.13, 0.066], plate, { k: 0.012 });
    b.ell(f, [0, -0.02, 0.02], [0.054, 0.05, 0.06], STEEL_DARK, { k: 0.01 });
    b.box(f, [0, -0.038, -0.09], [0.05, 0.028, 0.12], 0.026, STEEL_DARK, { k: 0.01 });
    b.cone(f, [0, -0.04, -0.19], [0, -0.05, -0.25], 0.03, 0.01, STEEL_DARK, { k: 0.012 });
    b.paint(f, [0, -0.066, -0.06], [0.1, 0.008, 0.3], IRON);
  }

  // ---- years in the open: rust in patches, darker pits
  const r = scatter(kind * 97 + 11, 96);
  const sites: [number, V3, number][] = [
    [B.CHEST, [0, 0.12, -0.16], 0.16],
    [B.CHEST, [0, 0.12, 0.14], 0.16],
    [B.SPINE, [0, 0.05, -0.13], 0.12],
    [B.PELVIS, [0, -0.03, -0.2], 0.16],
    [B.ARM_UL, [0, -0.12, 0], 0.08],
    [B.ARM_UR, [0, -0.12, 0], 0.08],
    [B.ARM_LL, [0, -0.14, 0], 0.07],
    [B.ARM_LR, [0, -0.14, 0], 0.07],
    [B.LEG_UL, [0, -0.2, -0.06], 0.1],
    [B.LEG_UR, [0, -0.2, -0.06], 0.1],
    [B.LEG_LL, [0, -0.2, 0], 0.08],
    [B.LEG_LR, [0, -0.2, 0], 0.08],
  ];
  let at = 0;
  for (const [bone, centre, spread] of sites) {
    for (let k = 0; k < 2; k++) {
      const p: V3 = [centre[0] + (r[at] - 0.5) * spread * 2, centre[1] + (r[at + 1] - 0.5) * spread * 2, centre[2] + (r[at + 2] - 0.5) * spread];
      const size = 0.02 + r[at + 3] * 0.04;
      b.paintEll(bone, p, [size * 1.4, size, size * 1.4], k === 0 ? RUST : IRON, { rot: [r[at] * 3, r[at + 1] * 3, r[at + 2] * 3] });
      at += 4;
    }
  }

  const out = new MeshOut();
  b.mesh(out, detail.cell, { inflate: detail.inflate ?? 0 });

  if (detail.trims) {
    const t = new Trim(out, b);
    // The rims of the collar, the gauntlets' cuffs and the greaves' tops.
    t.cord(t.loop(b.at(B.NECK, [0, 0.03, 0]), [0, 1, 0], 0.24, 22, 0.002), 0.009, heavy ? BRASS : STEEL_LIGHT, true, 5, 0.8);
    for (const l of [B.ARM_LL, B.ARM_LR]) t.cord(t.loop(b.at(l, [0, -0.255, 0]), b.dir(l, [0, 1, 0]), 0.12, 12, 0.002), 0.006, STEEL_LIGHT, true, 4, 0.6);
    for (const l of [B.LEG_LL, B.LEG_LR]) t.cord(t.loop(b.at(l, [0, -0.065, 0]), b.dir(l, [0, 1, 0]), 0.14, 12, 0.002), 0.006, STEEL_LIGHT, true, 4, 0.6);
  }

  weapon(out, b, kind, detail);
  return out.model();
}

function weapon(out: MeshOut, b: Body, kind: number, detail: KnightDetail) {
  const p = new Rigid(out, b, BONE.PROP);
  const fine = detail.cell < 0.05;
  const sides = fine ? 6 : 4;
  const blade = (from: number, to: number, width: number, thick: number) => {
    // Flat, with a ridge down the middle: a tube squashed across its width's normal.
    const n = fine ? 5 : 2;
    const path: V3[] = [];
    const radii: number[] = [];
    for (let k = 0; k <= n; k++) {
      const t = k / n;
      path.push([0, from + (to - from) * t, 0]);
      radii.push(width * (t > 0.86 ? 1 - ((t - 0.86) / 0.14) * 0.92 : 1 - t * 0.12));
    }
    p.tube(path, radii, EDGE, 4, { axis: [0, 0, 1], factor: thick }, STEEL_LIGHT);
  };
  if (kind === FIGURE.KNIGHT_HALBERD) {
    p.tube(
      [
        [0, -0.95, 0],
        [0, 1.2, 0],
      ],
      [0.019, 0.017],
      WOOD,
      sides,
    );
    // The axe blade on one side, a hook behind it, a spike above.
    const y = 1.1;
    p.poly(
      [
        [0.012, y - 0.13, 0.006],
        [0.2, y - 0.18, 0.004],
        [0.24, y, 0.004],
        [0.2, y + 0.18, 0.004],
        [0.012, y + 0.1, 0.006],
      ],
      EDGE,
    );
    p.poly(
      [
        [0.012, y + 0.1, -0.006],
        [0.2, y + 0.18, -0.004],
        [0.24, y, -0.004],
        [0.2, y - 0.18, -0.004],
        [0.012, y - 0.13, -0.006],
      ],
      STEEL_LIGHT,
    );
    p.poly(
      [
        [-0.012, y - 0.05, 0.005],
        [-0.012, y + 0.06, 0.005],
        [-0.15, y - 0.02, 0.003],
      ],
      STEEL,
    );
    p.poly(
      [
        [-0.012, y + 0.06, -0.005],
        [-0.012, y - 0.05, -0.005],
        [-0.15, y - 0.02, -0.003],
      ],
      STEEL,
    );
    p.tube(
      [
        [0, 1.2, 0],
        [0, 1.3, 0],
        [0, 1.52, 0],
      ],
      [0.022, 0.02, 0.002],
      STEEL_LIGHT,
      4,
    );
    p.tube(
      [
        [0, -0.95, 0],
        [0, -1.02, 0],
      ],
      [0.02, 0.006],
      IRON,
      4,
    );
    return;
  }
  const great = kind === FIGURE.KNIGHT_GREAT;
  const grip = great ? 0.3 : 0.2;
  const guard = great ? 0.14 : 0.1;
  const length = great ? 1.5 : 1.02;
  // Pommel, grip, guard, blade.
  p.tube(
    [
      [0, -grip + guard - 0.045, 0],
      [0, -grip + guard - 0.02, 0],
      [0, -grip + guard + 0.005, 0],
    ],
    [0.006, great ? 0.034 : 0.027, 0.014],
    BRASS,
    sides,
  );
  p.tube(
    [
      [0, -grip + guard, 0],
      [0, guard, 0],
    ],
    [0.016, 0.018],
    LEATHER,
    sides,
  );
  const half = great ? 0.17 : 0.115;
  p.box([-half, guard - 0.012, -0.014], [half, guard + 0.014, 0.014], great ? BRASS : STEEL_DARK);
  blade(guard + 0.014, length, great ? 0.05 : 0.032, great ? 0.16 : 0.18);
}

/**
 * A knight for the far ranks: boxes, each moving with one bone. `fine` gives
 * the limbs two parts each and the weapon its head; coarse is a trunk, two
 * legs and a line for the weapon, for knights a few pixels tall.
 */
export function buildKnightFar(kind: number, bind: Float32Array, fine: boolean): SkinModel {
  const b = new Body(bind);
  const out = new MeshOut();
  const B = BONE;
  const heavy = kind === FIGURE.KNIGHT_GREAT;
  const light = kind === FIGURE.KNIGHT_HALBERD;
  const plate = heavy ? STEEL_DARK : STEEL;
  /** A four-sided prism along a bone from `y0` to `y1`, `w` wide and `d` deep at each end; open-ended unless capped. */
  const prism = (bone: number, y0: number, y1: number, w0: number, d0: number, w1: number, d1: number, color: Rgb, cap: boolean, x = 0, z = 0) => {
    const corner = (y: number, w: number, d: number): V3[] => [
      [x - w, y, z - d],
      [x + w, y, z - d],
      [x + w, y, z + d],
      [x - w, y, z + d],
    ];
    const lo = corner(y0, w0, d0).map((p) => b.at(bone, p));
    const hi = corner(y1, w1, d1).map((p) => b.at(bone, p));
    const normals: V3[] = [
      [0, 0, -1],
      [1, 0, 0],
      [0, 0, 1],
      [-1, 0, 0],
    ];
    for (let k = 0; k < 4; k++) {
      const k1 = (k + 1) % 4;
      const n = b.dir(bone, normals[k]);
      const first = out.vertex(lo[k], n, color, bone);
      out.vertex(lo[k1], n, color, bone);
      out.vertex(hi[k1], n, color, bone);
      out.vertex(hi[k], n, color, bone);
      // Seen from outside, with +Y up the bone.
      out.i.push(first, first + 2, first + 1, first, first + 3, first + 2);
    }
    if (cap) {
      const n = b.dir(bone, [0, 1, 0]);
      const first = out.vertex(hi[0], n, VOID, bone);
      for (let k = 1; k < 4; k++) out.vertex(hi[k], n, VOID, bone);
      out.i.push(first, first + 2, first + 1, first, first + 3, first + 2);
    }
  };
  if (fine) {
    prism(B.PELVIS, -0.14, 0.1, 0.2, 0.15, 0.15, 0.12, light ? MAIL : plate, false);
    prism(B.SPINE, 0.0, 0.5, 0.15, 0.12, heavy ? 0.3 : 0.26, 0.15, plate, true);
    for (const [u, l, f] of [
      [B.LEG_UL, B.LEG_LL, B.FOOT_L],
      [B.LEG_UR, B.LEG_LR, B.FOOT_R],
    ]) {
      prism(u, -0.44, 0.02, 0.07, 0.075, 0.1, 0.1, light ? CLOTH : plate, false);
      prism(l, -0.43, 0.0, 0.055, 0.06, 0.075, 0.08, plate, false);
      prism(f, -0.07, -0.01, 0.055, 0.14, 0.055, 0.08, STEEL_DARK, true, 0, -0.06);
    }
    for (const [u, l] of [
      [B.ARM_UL, B.ARM_LL],
      [B.ARM_UR, B.ARM_LR],
    ]) {
      prism(u, -0.3, 0.05, 0.055, 0.06, heavy ? 0.12 : 0.1, 0.1, plate, true);
      prism(l, -0.34, 0.0, 0.05, 0.05, 0.055, 0.06, STEEL_DARK, false);
    }
  } else {
    prism(B.SPINE, -0.16, 0.5, 0.18, 0.13, 0.27, 0.15, plate, true);
    prism(B.LEG_UL, -0.86, 0.0, 0.06, 0.08, 0.1, 0.1, STEEL_DARK, false);
    prism(B.LEG_UR, -0.86, 0.0, 0.06, 0.08, 0.1, 0.1, STEEL_DARK, false);
    prism(B.ARM_UL, -0.6, 0.04, 0.05, 0.05, 0.09, 0.09, STEEL_DARK, false);
    prism(B.ARM_UR, -0.6, 0.04, 0.05, 0.05, 0.09, 0.09, STEEL_DARK, false);
  }
  // The weapon: a pale sliver, wider than life so it survives the distance.
  const reach = kind === FIGURE.KNIGHT_HALBERD ? [-0.9, 1.45] : heavy ? [-0.2, 1.5] : [-0.12, 1.02];
  const w = fine ? 0.022 : 0.03;
  prism(B.PROP, reach[0], reach[1], w, w, w * 0.6, w * 0.6, EDGE, false);
  if (fine && kind === FIGURE.KNIGHT_HALBERD) prism(B.PROP, 0.95, 1.25, 0.012, 0.012, 0.012, 0.012, EDGE, false, 0.12);
  return out.model();
}
