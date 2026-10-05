// The mage: a slight elf in a white jacket and skirt with gold trim, a short
// cape over the shoulders, a striped shirt at the chest, black tights and
// brown boots; silver hair in two long tails, and a staff whose crescent head
// holds a red orb.
//
// The body and the clothes are one blended field (sdf.ts). The trims are
// cords laid on that field, the stripes and the face are flat shapes pressed
// onto it, and the hair's locks, the ears' jewels and the staff are separate
// crisp pieces. Everything is authored in bone frames of the simulation's
// bind pose: +Y up the bone, -Z forward, +X to the figure's right; a limb
// runs down its -Y.

import { BONE, STAFF_BUTT, STAFF_HEAD } from "../sim/abi.gen";
import { Body, ellipse, MeshOut, Rgb, Rigid, SkinModel, Trim, V3 } from "./sdf";

const SKIN: Rgb = [1.0, 0.935, 0.905];
const SKIN_SHADE: Rgb = [0.95, 0.84, 0.81];
const HAIR: Rgb = [0.93, 0.94, 0.98];
const HAIR_SHADE: Rgb = [0.8, 0.82, 0.92];
const WHITE: Rgb = [0.96, 0.955, 0.93];
const WHITE_SHADE: Rgb = [0.86, 0.865, 0.88];
const GOLD: Rgb = [0.84, 0.67, 0.34];
const GOLD_LIGHT: Rgb = [0.93, 0.82, 0.52];
const INK: Rgb = [0.11, 0.11, 0.13];
const TIGHTS: Rgb = [0.14, 0.15, 0.21];
const BOOT: Rgb = [0.47, 0.31, 0.21];
const BOOT_DARK: Rgb = [0.31, 0.2, 0.14];
const SOLE: Rgb = [0.14, 0.1, 0.09];
const RED: Rgb = [0.8, 0.1, 0.13];
const SHAFT: Rgb = [0.56, 0.15, 0.14];
const EYE: Rgb = [0.34, 0.66, 0.58];
const EYE_DARK: Rgb = [0.14, 0.34, 0.33];
const EYE_LIGHT: Rgb = [0.62, 0.88, 0.76];
const LASH: Rgb = [0.16, 0.14, 0.17];
const BROW: Rgb = [0.66, 0.66, 0.72];

/** `cells` are the mesh cell sizes of the body and of the head, in metres; a coarser pair is a handheld's model. */
export function buildMage(bind: Float32Array, cells: [number, number] = [0.012, 0.0075]): SkinModel {
  const b = new Body(bind, 1, 0.03);
  const B = BONE;

  // ---- trunk: slim, the jacket closed to a belt; what the skirt hides is dark
  b.ell(B.PELVIS, [0, 0.0, 0.004], [0.108, 0.092, 0.086], TIGHTS);
  b.cone(B.SPINE, [0, -0.03, 0.002], [0, 0.1, 0.0], 0.09, 0.096, WHITE, { k: 0.03, bone2: B.CHEST });
  b.ell(B.CHEST, [0, 0.07, 0.0], [0.108, 0.115, 0.082], WHITE, { k: 0.09 });
  for (const [s, c, u] of [
    [-1, B.CLAV_L, B.ARM_UL],
    [1, B.CLAV_R, B.ARM_UR],
  ] as const) {
    b.cone(c, [s * 0.02, 0.0, 0.002], [s * 0.118, -0.004, 0.002], 0.05, 0.042, WHITE, { k: 0.03, bone2: u });
  }
  // The neck, and the collar that stands round it.
  b.cone(B.NECK, [0, -0.02, 0.004], [0, 0.07, 0.0], 0.036, 0.033, SKIN, { k: 0.015, bone2: B.HEAD });
  b.cone(B.NECK, [0, -0.04, 0.006], [0, 0.034, 0.004], 0.056, 0.043, WHITE, { k: 0.012 });

  // ---- arms: sleeves that widen to a cuff, small hands
  for (const [u, l, h, s] of [
    [B.ARM_UL, B.ARM_LL, B.HAND_L, -1],
    [B.ARM_UR, B.ARM_LR, B.HAND_R, 1],
  ] as const) {
    b.cone(u, [0, -0.01, 0], [0, -0.225, 0], 0.036, 0.031, WHITE, { k: 0.03, bone2: l });
    b.ell(l, [0, -0.004, 0.005], [0.031, 0.032, 0.033], WHITE, { k: 0.02 });
    b.cone(l, [0, 0.0, 0], [0, -0.16, 0], 0.031, 0.04, WHITE, { k: 0.02 });
    b.cone(l, [0, -0.15, 0], [0, -0.19, 0], 0.043, 0.046, WHITE, { k: 0.006 });
    // The wrist, inside the cuff.
    b.cone(l, [0, -0.17, 0], [0, -0.214, 0], 0.018, 0.017, SKIN, { k: 0.004, bone2: h });
    b.paint(l, [0, -0.2, 0], [0.1, 0.008, 0.1], WHITE_SHADE);
    // The hand: a palm closed round a grip, the thumb across.
    b.ell(h, [0, -0.032, 0], [0.023, 0.034, 0.017], SKIN, { k: 0.01 });
    b.ell(h, [0, -0.058, -0.012], [0.024, 0.02, 0.023], SKIN, { k: 0.01 });
    b.cone(h, [-s * 0.022, -0.026, -0.01], [-s * 0.015, -0.056, -0.028], 0.0095, 0.0085, SKIN, { k: 0.006 });
  }

  // ---- legs: dark tights into boots that end below the knee
  for (const [u, l, f, s] of [
    [B.LEG_UL, B.LEG_LL, B.FOOT_L, -1],
    [B.LEG_UR, B.LEG_LR, B.FOOT_R, 1],
  ] as const) {
    b.ell(B.PELVIS, [s * 0.062, -0.05, 0.028], [0.066, 0.078, 0.07], TIGHTS, { k: 0.03 });
    b.cone(u, [0, 0.0, 0], [0, -0.365, 0], 0.06, 0.042, TIGHTS, { k: 0.035, bone2: l, sigma: 0.026 });
    b.ell(u, [s * 0.004, -0.14, -0.012], [0.056, 0.14, 0.054], TIGHTS, { k: 0.03, sigma: 0.026 });
    b.ell(l, [0, 0.0, -0.01], [0.04, 0.041, 0.042], TIGHTS, { k: 0.02 });
    b.cone(l, [0, 0, 0], [0, -0.345, 0], 0.041, 0.029, TIGHTS, { k: 0.03, bone2: f, sigma: 0.026 });
    b.ell(l, [0, -0.11, 0.018], [0.042, 0.09, 0.045], TIGHTS, { k: 0.025 });
    // The boot: its cuff folds over at the calf.
    b.cone(l, [0, -0.15, 0.003], [0, -0.345, 0], 0.049, 0.038, BOOT, { k: 0.008, bone2: f });
    b.cone(l, [0, -0.135, 0.003], [0, -0.166, 0.003], 0.057, 0.055, BOOT_DARK, { k: 0.004 });
    b.ell(f, [0, -0.014, 0.016], [0.036, 0.04, 0.042], BOOT, { k: 0.02 });
    b.box(f, [0, -0.032, -0.06], [0.037, 0.024, 0.083], 0.022, BOOT, { k: 0.02 });
    b.paint(f, [0, -0.058, -0.04], [0.08, 0.009, 0.2], SOLE);
  }

  // ---- head: a round skull, a small pointed chin, long ears that lie back
  b.ell(B.HEAD, [0, 0.114, -0.002], [0.094, 0.114, 0.104], SKIN, { k: 0.02 });
  b.cone(B.HEAD, [0, 0.062, -0.028], [0, 0.011, -0.062], 0.064, 0.024, SKIN, { k: 0.04 });
  b.ell(B.HEAD, [0, 0.06, -0.048], [0.058, 0.034, 0.046], SKIN, { k: 0.03 });
  for (const s of [-1, 1]) {
    b.ell(B.HEAD, [s * 0.128, 0.098, 0.02], [0.066, 0.014, 0.027], SKIN, { k: 0.008, rot: [0, -s * 0.34, s * 0.3] });
    b.paintEll(B.HEAD, [s * 0.112, 0.094, 0.006], [0.03, 0.007, 0.012], SKIN_SHADE, { rot: [0, -s * 0.34, s * 0.3] });
  }
  // The scalp, and the hair gathered to where the tails are tied.
  b.ell(B.HEAD, [0, 0.134, 0.012], [0.103, 0.106, 0.11], HAIR, { k: 0.006 });
  for (const [s, t0, t1, t2] of [
    [-1, B.TAIL_L0, B.TAIL_L1, B.TAIL_L2],
    [1, B.TAIL_R0, B.TAIL_R1, B.TAIL_R2],
  ] as const) {
    b.ell(B.HEAD, [s * 0.098, 0.176, 0.05], [0.034, 0.032, 0.036], HAIR, { k: 0.02 });
    // The tail: full below the tie, tapering to its end.
    b.cone(t0, [0, 0.0, 0], [0, -0.22, 0], 0.028, 0.042, HAIR, { k: 0.01, bone2: t1, sigma: 0.05 });
    b.cone(t1, [0, 0.0, 0], [0, -0.22, 0], 0.042, 0.036, HAIR, { k: 0.01, bone2: t2, sigma: 0.05 });
    b.cone(t2, [0, 0.0, 0], [0, -0.2, 0], 0.036, 0.008, HAIR, { k: 0.01, sigma: 0.05 });
    b.paint(t0, [0, -0.02, 0], [0.06, 0.012, 0.06], GOLD);
  }

  const out = new MeshOut();
  const neck = b.joint(B.HEAD)[1] - 0.01;
  b.mesh(out, cells[0], { yMax: neck + cells[0] });
  b.mesh(out, cells[1], { yMin: neck - cells[1] });

  // ---- the short cape: its own surface over the shoulders and the top of the arms, cut level at its edge
  const cape = new Body(bind, 1, 0.04);
  const chest = b.joint(B.CHEST);
  const edge = chest[1] + 0.07;
  cape.ell(B.CHEST, [0, 0.122, 0.006], [0.172, 0.09, 0.114], WHITE);
  for (const [s, c, u] of [
    [-1, B.CLAV_L, B.ARM_UL],
    [1, B.CLAV_R, B.ARM_UR],
  ] as const) {
    cape.cone(c, [s * 0.04, -0.004, 0.004], [s * 0.118, -0.006, 0.004], 0.062, 0.056, WHITE, { k: 0.08, bone2: u });
    cape.cone(u, [0, 0.01, 0.004], [0, -0.14, 0.004], 0.052, 0.064, WHITE, { k: 0.08 });
  }
  cape.mesh(out, cells[0], { yMin: edge });

  // ---- the skirt: its own surface too, flaring from the belt to a level hem
  const skirt = new Body(bind, 1, 0.05);
  const pelvis = b.joint(B.PELVIS);
  const hem = pelvis[1] - 0.262;
  for (const [s, bone] of [
    [-1, B.SKIRT_L],
    [1, B.SKIRT_R],
  ] as const) {
    skirt.cone(B.PELVIS, [s * 0.024, 0.085, 0.004], [s * 0.066, -0.3, 0.004], 0.086, 0.15, WHITE, { k: 0.04, bone2: bone });
  }
  skirt.mesh(out, cells[0], { yMin: hem });

  trims(out, b, cape, skirt, edge, hem);
  face(out, b);
  hair(out, b);
  staff(out, b);
  return out.model();
}

/** The gold edges of the cape, the cuffs and the hem; the collar and the belt; the striped shirt; the jewels. */
function trims(out: MeshOut, b: Body, cape: Body, skirt: Body, edge: number, hem: number) {
  const t = new Trim(out, b);
  const tc = new Trim(out, cape);
  const ts = new Trim(out, skirt);
  const B = BONE;
  const pelvis = b.joint(B.PELVIS);
  const chest = b.joint(B.CHEST);
  const spine = b.joint(B.SPINE);
  // Hem of the skirt: a band on the rim and a thin line above it.
  ts.cord(ts.loop([pelvis[0], hem + 0.006, pelvis[2] + 0.004], [0, 1, 0], 0.5, 44, 0.002), 0.0085, GOLD, true, 6, 0.7);
  ts.cord(ts.loop([pelvis[0], hem + 0.034, pelvis[2] + 0.004], [0, 1, 0], 0.5, 44, 0.002), 0.0035, GOLD_LIGHT, true, 4, 0.6);
  // Edge of the cape.
  tc.cord(tc.loop([chest[0], edge + 0.006, chest[2] + 0.008], [0, 1, 0], 0.5, 52, 0.002), 0.008, GOLD, true, 6, 0.7);
  tc.cord(tc.loop([chest[0], edge + 0.03, chest[2] + 0.008], [0, 1, 0], 0.5, 52, 0.002), 0.003, GOLD_LIGHT, true, 4, 0.6);
  // Cuffs.
  for (const l of [B.ARM_LL, B.ARM_LR]) {
    t.cord(t.loop(b.at(l, [0, -0.184, 0]), b.dir(l, [0, 1, 0]), 0.1, 16, 0.002), 0.0055, GOLD, true, 5, 0.6);
  }
  // Boot tops.
  for (const l of [B.LEG_LL, B.LEG_LR]) {
    t.cord(t.loop(b.at(l, [0, -0.15, 0.003]), b.dir(l, [0, 1, 0]), 0.075, 16, 0.002), 0.0038, GOLD, true, 4, 0.6);
  }
  // The collar: a dark band between two gold lines.
  t.cord(t.loop(b.at(B.NECK, [0, 0.018, 0.004]), [0, 1, 0], 0.12, 22, 0.001), 0.0085, INK, true, 5, 0.45);
  t.cord(t.loop(b.at(B.NECK, [0, 0.031, 0.004]), [0, 1, 0], 0.12, 22, 0.002), 0.0032, GOLD, true, 4, 0.6);
  t.cord(t.loop(b.at(B.NECK, [0, 0.005, 0.004]), [0, 1, 0], 0.12, 22, 0.002), 0.0032, GOLD, true, 4, 0.6);
  // The belt, over the top of the skirt.
  ts.cord(ts.loop([spine[0], spine[1] - 0.012, spine[2]], [0, 1, 0], 0.3, 32, 0.002), 0.016, INK, true, 6, 0.35);

  // The shirt between the edges of the jacket: black and white stripes from the collar to the belt,
  // on the cape above its edge and on the jacket below.
  const front = chest[2] - 0.25;
  const half = 0.03;
  const stripes = 7;
  const panel = (trim: Trim, body: Body, y0: number, y1: number, rows: number) => {
    // White under the stripes, so they read on the cape as on the shirt.
    for (let k = 0; k < stripes; k++) {
      const x0 = -half + (2 * half * k) / stripes;
      const x1 = -half + (2 * half * (k + 1)) / stripes;
      for (let r = 0; r < rows; r++) {
        const a = y0 + ((y1 - y0) * r) / rows;
        const c = y0 + ((y1 - y0) * (r + 1)) / rows;
        trim.decal(
          [
            [x1, a, front],
            [x0, a, front],
            [x0, c, front],
            [x1, c, front],
          ],
          [0, 0, 1],
          0.0016,
          k % 2 === 0 ? INK : [0.97, 0.97, 0.96],
        );
      }
    }
    // The jacket's gold edges, either side of the shirt.
    for (const s of [-1, 1]) {
      const path: V3[] = [];
      for (let k = 0; k <= rows * 2; k++) path.push(body.snap([s * (half + 0.006), y0 + ((y1 - y0) * k) / (rows * 2), front], [0, 0, 1], 0.003, 0.4));
      trim.cord(path, 0.0052, GOLD, false, 5, 0.6);
    }
  };
  panel(tc, cape, edge + 0.004, chest[1] + 0.2, 5);
  panel(t, b, spine[1] + 0.004, edge + 0.002, 4);

  // A strap with a red jewel on each shoulder of the cape.
  for (const s of [-1, 1]) {
    const at = cape.snap([s * 0.088, chest[1] + 0.15, front], [0, 0, 1], 0.004, 0.4);
    const r = new Rigid(out, cape, B.CHEST);
    const local: V3 = [at[0] - chest[0], at[1] - chest[1], at[2] - chest[2]];
    r.tube(
      [
        [local[0], local[1], local[2] + 0.004],
        [local[0], local[1], local[2] - 0.01],
      ],
      [0.018, 0.014],
      GOLD,
      10,
      null,
      GOLD_LIGHT,
    );
    r.tube(
      [
        [local[0], local[1], local[2] - 0.006],
        [local[0], local[1], local[2] - 0.017],
      ],
      [0.011, 0.006],
      RED,
      10,
      null,
      [1.0, 0.45, 0.45],
    );
    // The strap down to the cape's edge.
    const down: V3[] = [];
    for (let k = 0; k <= 5; k++) down.push(cape.snap([s * 0.088, chest[1] + 0.15 - (chest[1] + 0.146 - edge) * (k / 5), front], [0, 0, 1], 0.003, 0.4));
    tc.cord(down, 0.007, GOLD, false, 5, 0.5);
  }
  // The belt's buckle.
  const belt = skirt.snap([0, spine[1] - 0.012, front], [0, 0, 1], 0.004, 0.4);
  const rb = new Rigid(out, b, B.SPINE);
  rb.box([belt[0] - spine[0] - 0.017, belt[1] - spine[1] - 0.013, belt[2] - spine[2] - 0.006], [belt[0] - spine[0] + 0.017, belt[1] - spine[1] + 0.013, belt[2] - spine[2] + 0.004], GOLD);
}

function face(out: MeshOut, b: Body) {
  const r = new Rigid(out, b, BONE.HEAD);
  const into: V3 = [0, 0, 1];
  const z = -0.17;
  for (const s of [-1, 1]) {
    const cx = s * 0.042;
    const cy = 0.089;
    // The white, a dark rim, the iris, the lighter lower half, the pupil, two highlights.
    r.decal(ellipse(cx, cy, 0.0228, 0.0182, z, 18), into, 0.001, [0.985, 0.985, 0.995]);
    r.decal(ellipse(cx, cy + 0.0008, 0.0158, 0.0178, z, 18), into, 0.0014, EYE_DARK);
    r.decal(ellipse(cx, cy + 0.0002, 0.0142, 0.0162, z, 18), into, 0.0018, EYE);
    r.decal(ellipse(cx, cy - 0.0072, 0.0104, 0.0072, z, 14), into, 0.0022, EYE_LIGHT);
    r.decal(ellipse(cx, cy + 0.002, 0.0062, 0.0084, z, 12), into, 0.0026, [0.05, 0.11, 0.12]);
    r.decal(ellipse(cx - 0.0066, cy + 0.0066, 0.0042, 0.0042, z, 10), into, 0.0032, [1, 1, 1]);
    r.decal(ellipse(cx + 0.0058, cy - 0.0072, 0.002, 0.002, z, 6), into, 0.0032, [1, 1, 1]);
    // The upper lid: a heavy line low over the iris, falling toward the temple.
    const n = 12;
    const curve = (k: number, rx: number, ry: number, up: number): V3 => {
      const a = (k / n) * Math.PI;
      const droop = k > n * 0.55 ? ((k - n * 0.55) / (n * 0.45)) ** 1.5 * 0.0055 : 0;
      return [cx - s * Math.cos(a) * rx, cy + Math.sin(a) * ry + up - droop, z];
    };
    for (let k = 0; k < n; k++) {
      const quad: V3[] = [curve(k + 1, 0.0256, 0.0118, 0.0048), curve(k + 1, 0.0232, 0.0074, 0.0016), curve(k, 0.0232, 0.0074, 0.0016), curve(k, 0.0256, 0.0118, 0.0048)];
      r.decal(s < 0 ? quad : [quad[3], quad[2], quad[1], quad[0]], into, 0.0036, LASH);
    }
    // The skin of the lid over the top of the eye, so the gaze is half closed.
    for (let k = 0; k < n; k++) {
      const quad: V3[] = [curve(k + 1, 0.0236, 0.0078, 0.0016), curve(k + 1, 0.0236, 0.023, 0.001), curve(k, 0.0236, 0.023, 0.001), curve(k, 0.0236, 0.0078, 0.0016)];
      r.decal(s < 0 ? [quad[3], quad[2], quad[1], quad[0]] : quad, into, 0.0034, SKIN);
    }
    // A flick at the temple end.
    const tip = curve(n, 0.0256, 0.0118, 0.0048);
    const flick: V3[] = [
      [tip[0] - s * 0.003, tip[1] + 0.0025, z],
      [tip[0] + s * 0.0075, tip[1] + 0.003, z],
      [tip[0] - s * 0.002, tip[1] + 0.0068, z],
    ];
    r.decal(s < 0 ? [flick[0], flick[2], flick[1]] : flick, into, 0.0036, LASH);
    // The lower lid: a thin line under the outer half.
    const lower: V3[] = [
      [cx + s * 0.003, cy - 0.0186, z],
      [cx + s * 0.0215, cy - 0.012, z],
      [cx + s * 0.0215, cy - 0.0106, z],
      [cx + s * 0.003, cy - 0.0174, z],
    ];
    r.decal(s < 0 ? lower : [lower[1], lower[0], lower[3], lower[2]], into, 0.0028, [0.55, 0.45, 0.48]);
    // The brow: thin and level, a little raised at the temple.
    const b0 = cx - s * 0.02;
    const b1 = cx + s * 0.024;
    const brow: V3[] = [
      [b0, cy + 0.035, z],
      [b1, cy + 0.0372, z],
      [b1, cy + 0.039, z],
      [b0, cy + 0.0376, z],
    ];
    r.decal(s < 0 ? [brow[1], brow[0], brow[3], brow[2]] : brow, into, 0.002, BROW);
  }
  // The mouth: a short level line.
  r.decal(
    [
      [0.0075, 0.0375, z],
      [-0.0075, 0.0375, z],
      [-0.006, 0.039, z],
      [0.006, 0.039, z],
    ],
    into,
    0.0014,
    [0.74, 0.47, 0.47],
  );
  // The earrings: a gold stud and a red drop under each ear.
  for (const s of [-1, 1]) {
    const x = s * 0.101;
    r.tube(
      [
        [x, 0.068, -0.004],
        [x, 0.06, -0.004],
      ],
      [0.0045, 0.0035],
      GOLD_LIGHT,
      6,
    );
    r.tube(
      [
        [x, 0.058, -0.004],
        [x, 0.044, -0.004],
        [x, 0.028, -0.004],
        [x, 0.016, -0.004],
      ],
      [0.0018, 0.0042, 0.0058, 0.0012],
      RED,
      6,
    );
  }
}

function hair(out: MeshOut, b: Body) {
  const r = new Rigid(out, b, BONE.HEAD);
  /** A lock from `from` to `end`, bowed by `bend`, flattened across `axis`; `taper` near 1 ends in a point. */
  const lock = (from: V3, end: V3, bend: V3, width: number, axis: V3, color: Rgb = HAIR, taper = 0.9, flat = 0.36) => {
    const path: V3[] = [];
    const radii: number[] = [];
    const steps = 8;
    for (let k = 0; k <= steps; k++) {
      const t = k / steps;
      const m = 2 * t * (1 - t);
      path.push([from[0] + (end[0] - from[0]) * t + bend[0] * m, from[1] + (end[1] - from[1]) * t + bend[1] * m, from[2] + (end[2] - from[2]) * t + bend[2] * m]);
      radii.push(width * (0.45 + 0.8 * Math.sin(Math.min(t * 1.4, 1) * Math.PI * 0.5)) * (1 - t ** 2.4 * taper));
    }
    r.tube(path, radii, color, 7, { axis, factor: flat });
  };
  // The fringe: parted in the middle and swept to each side, short over the brow, longer toward the temples.
  for (const s of [-1, 1]) {
    lock([s * 0.004, 0.235, -0.02], [s * 0.014, 0.134, -0.108], [s * 0.004, 0.035, -0.07], 0.022, [0, 0, 1], HAIR_SHADE, 0.85);
    lock([s * 0.01, 0.236, -0.016], [s * 0.036, 0.122, -0.106], [s * 0.016, 0.04, -0.07], 0.03, [0, 0, 1]);
    lock([s * 0.022, 0.236, -0.01], [s * 0.062, 0.112, -0.094], [s * 0.026, 0.045, -0.07], 0.032, [0.35 * s, 0, 0.94]);
    lock([s * 0.036, 0.232, -0.004], [s * 0.084, 0.098, -0.07], [s * 0.034, 0.05, -0.06], 0.032, [0.6 * s, 0, 0.8]);
    // A thin strand down between the eyes.
    lock([s * 0.002, 0.232, -0.03], [s * 0.006, 0.105, -0.111], [0, 0.02, -0.062], 0.008, [0, 0, 1], HAIR_SHADE, 0.95);
    // The long locks in front of the ears, down past the jaw to the collar.
    lock([s * 0.052, 0.226, 0.002], [s * 0.094, -0.035, -0.05], [s * 0.056, 0.03, -0.05], 0.026, [1, 0, 0], HAIR, 0.92);
    lock([s * 0.062, 0.218, 0.012], [s * 0.101, 0.02, -0.026], [s * 0.05, 0.03, -0.032], 0.022, [1, 0, 0], HAIR_SHADE, 0.92);
    // The side of the head, combed back to the tie.
    lock([s * 0.05, 0.228, -0.006], [s * 0.1, 0.18, 0.044], [s * 0.03, 0.012, -0.012], 0.034, [0.7 * s, 0.7, 0], HAIR, 0.3, 0.3);
    lock([s * 0.08, 0.13, 0.03], [s * 0.1, 0.176, 0.05], [s * 0.022, 0.0, 0.012], 0.03, [s, 0, 0], HAIR_SHADE, 0.2, 0.3);
    lock([s * 0.03, 0.2, 0.098], [s * 0.094, 0.18, 0.062], [s * 0.01, 0.014, 0.02], 0.032, [0, 0.3, 0.95], HAIR, 0.2, 0.3);
  }
  // The back of the head, combed up from the nape.
  for (const x of [-0.06, -0.024, 0.024, 0.06]) lock([x * 0.6, 0.226, 0.03], [x, 0.036, 0.09], [x * 0.5, 0.05, 0.09], 0.034, [0, 0, 1], Math.abs(x) > 0.03 ? HAIR : HAIR_SHADE, 0.6, 0.3);
}

/** The staff, along the prop bone's +Y: a red shaft, gold fittings, a crescent that holds a red orb. */
function staff(out: MeshOut, b: Body) {
  const p = new Rigid(out, b, BONE.PROP);
  const head = STAFF_HEAD;
  // Shaft, with a gold cap at the butt and a collar under the head.
  p.tube(
    [
      [0, STAFF_BUTT, 0],
      [0, STAFF_BUTT + 0.05, 0],
    ],
    [0.012, 0.017],
    GOLD,
    8,
    null,
    GOLD_LIGHT,
  );
  p.tube(
    [
      [0, STAFF_BUTT + 0.05, 0],
      [0, -0.2, 0],
      [0, head - 0.23, 0],
    ],
    [0.0135, 0.0145, 0.0135],
    SHAFT,
    8,
  );
  p.tube(
    [
      [0, head - 0.23, 0],
      [0, head - 0.2, 0],
      [0, head - 0.16, 0],
    ],
    [0.018, 0.023, 0.016],
    GOLD,
    8,
    null,
    GOLD_LIGHT,
  );
  // The crescent: an arc open to one side, thick in the middle, pointed at both ends. It lies in the prop's XY plane.
  const arc = (r0: number, from: number, to: number, thick: number, n: number, cx: number, cy: number) => {
    const path: V3[] = [];
    const radii: number[] = [];
    for (let k = 0; k <= n; k++) {
      const t = k / n;
      const a = from + (to - from) * t;
      path.push([cx + Math.cos(a) * r0, cy + Math.sin(a) * r0, 0]);
      radii.push(thick * (0.14 + 0.86 * Math.sin(Math.PI * Math.min(Math.max(t, 0.02), 0.98)) ** 0.7));
    }
    p.tube(path, radii, GOLD_LIGHT, 8, { axis: [0, 0, 1], factor: 0.55 });
  };
  // The stem into the crescent.
  p.tube(
    [
      [0, head - 0.16, 0],
      [0, head - 0.108, 0],
    ],
    [0.014, 0.02],
    GOLD_LIGHT,
    8,
  );
  arc(0.112, -Math.PI * 0.62, Math.PI * 0.8, 0.026, 24, 0, head);
  // A smaller horn inside it, on the open side.
  arc(0.07, Math.PI * 0.16, Math.PI * 0.88, 0.013, 12, -0.014, head + 0.012);
  // The orb.
  const orb: V3[] = [];
  const orbR: number[] = [];
  for (let k = 0; k <= 10; k++) {
    const a = (k / 10) * Math.PI;
    orb.push([0, head - Math.cos(a) * 0.047, 0]);
    orbR.push(Math.max(Math.sin(a) * 0.047, 0.002));
  }
  p.tube(orb, orbR, RED, 14, null, [1.0, 0.5, 0.5]);
  // The ribbon tied under the head.
  p.tube(
    [
      [0.016, head - 0.19, 0.0],
      [0.054, head - 0.24, 0.01],
      [0.062, head - 0.34, 0.014],
    ],
    [0.008, 0.012, 0.003],
    RED,
    5,
    { axis: [0, 0, 1], factor: 0.25 },
  );
  p.tube(
    [
      [0.014, head - 0.19, -0.004],
      [0.038, head - 0.27, -0.012],
      [0.032, head - 0.38, -0.016],
    ],
    [0.008, 0.011, 0.003],
    RED,
    5,
    { axis: [0, 0, 1], factor: 0.25 },
  );
}
