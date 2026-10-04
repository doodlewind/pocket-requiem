// Writes the simulation's world file (`RQSW`, see crates/requiem-sim/src/worldfile.rs).

import { Generated } from "./stage";

export function writeWorldFile(g: Generated): Uint8Array {
  const f = g.field;
  const heights = (f.n * f.n * 2 + 3) & ~3;
  const size = 28 + heights + 4 + g.obstacles.length * 16 + 16 + 12 + 12 + 8 + 4 + g.stage.musters.length * 24;
  const out = new Uint8Array(size);
  const view = new DataView(out.buffer);
  let at = 0;
  const u32 = (v: number) => {
    view.setUint32(at, v, true);
    at += 4;
  };
  const u16 = (v: number) => {
    view.setUint16(at, v, true);
    at += 2;
  };
  const f32 = (v: number) => {
    view.setFloat32(at, v, true);
    at += 4;
  };
  u32(0x57535152);
  u32(1);
  u32(f.n);
  f32(f.cell);
  f32(f.min);
  f32(f.hMin);
  f32(f.hScale);
  for (let k = 0; k < f.n * f.n; k++) u16(f.q[k]);
  at = (at + 3) & ~3;
  u32(g.obstacles.length);
  for (const o of g.obstacles) {
    f32(o.x);
    f32(o.z);
    f32(o.r);
    u32(o.kind);
  }
  const s = g.stage;
  s.town.forEach(f32);
  f32(s.half);
  s.start.forEach(f32);
  s.demon.forEach(f32);
  s.gate.forEach(f32);
  u32(s.musters.length);
  for (const m of s.musters) {
    f32(m.x);
    f32(m.z);
    f32(m.yaw);
    f32(m.spacing);
    u16(m.cols);
    u16(m.rows);
    view.setUint8(at++, m.mix);
    view.setUint8(at++, m.captain);
    u16(0);
  }
  if (at !== size) throw new Error(`world file: wrote ${at} of ${size} bytes`);
  return out;
}
