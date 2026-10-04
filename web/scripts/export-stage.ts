// Exports the stage for the compiler: StageIR, a directory of float geometry
// and source images with nothing baked, quantized or compressed.
//
//   bun web/scripts/export-stage.ts [--seed 2026] [--out .pocket-build/stage/ir]
//
// Files: meshes.bin (render buckets), models.bin (the mage, and each kind of
// knight at four mesh densities), atlas.rgba, collision.bin (the triangles
// the light bake casts rays against), stage.rqsw (the simulation's world),
// scene.json (light, air, LOD distances) and manifest.json, written last,
// with the sha256 of each.

import { mkdir } from "node:fs/promises";
import { join, resolve } from "node:path";
import { buildMage } from "../src/model/mage";
import { buildKnight, KnightDetail } from "../src/model/knight";
import { SKIN_STRIDE, SkinModel } from "../src/model/sdf";
import { skyColor } from "../src/render/sky";
import { FIGURE, KNIGHT_FRAMES } from "../src/sim/abi.gen";
import { Sim } from "../src/sim/sim";
import { ATLAS_H, ATLAS_W, paintAtlas, STRIP } from "../src/world/atlas";
import { CELL, Geo, Layer, STRIDE, SUPER } from "../src/world/geo";
import { SCENE } from "../src/world/scene";
import { generate } from "../src/world/stage";
import { writeWorldFile } from "../src/world/worldfile";

const args = process.argv.slice(2);
const arg = (name: string, dflt: string) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : dflt;
};
const seed = Number(arg("seed", "2026"));
const out = resolve(arg("out", join(import.meta.dir, "../../.pocket-build/stage/ir")));

/** A knight's levels of detail, nearest first. A device profile says at what distance each takes over. */
const KNIGHT_LODS: KnightDetail[] = [
  { cell: 0.026, trims: true },
  { cell: 0.046, inflate: 0.004 },
  { cell: 0.085, inflate: 0.016 },
  { cell: 0.16, inflate: 0.034 },
];

function meshBytes(header: number[], entries: { head: number[]; geo: Geo }[]): Uint8Array {
  let size = header.length * 4;
  for (const e of entries) size += e.head.length * 4 + 8 + e.geo.nv * STRIDE * 4 + e.geo.ni * 4;
  const bytes = new Uint8Array(size);
  const view = new DataView(bytes.buffer);
  let at = 0;
  const i32 = (v: number) => {
    view.setInt32(at, v, true);
    at += 4;
  };
  header.forEach(i32);
  for (const e of entries) {
    e.head.forEach(i32);
    i32(e.geo.nv);
    i32(e.geo.ni);
    new Float32Array(bytes.buffer, at, e.geo.nv * STRIDE).set(e.geo.vertices());
    at += e.geo.nv * STRIDE * 4;
    new Uint32Array(bytes.buffer, at, e.geo.ni).set(e.geo.indices());
    at += e.geo.ni * 4;
  }
  return bytes;
}

/** Skinned models: id, vertex count, index count, then 12 floats per vertex and `u32` indices. */
function modelBytes(models: { id: number; model: SkinModel }[]): Uint8Array {
  let size = 12;
  for (const m of models) size += 12 + m.model.v.byteLength + m.model.i.byteLength;
  const bytes = new Uint8Array(size);
  const view = new DataView(bytes.buffer);
  view.setUint32(0, 0x444d5152, true);
  view.setUint32(4, 1, true);
  view.setUint32(8, models.length, true);
  let at = 12;
  for (const m of models) {
    view.setInt32(at, m.id, true);
    view.setInt32(at + 4, m.model.v.length / SKIN_STRIDE, true);
    view.setInt32(at + 8, m.model.i.length, true);
    at += 12;
    bytes.set(new Uint8Array(m.model.v.buffer, m.model.v.byteOffset, m.model.v.byteLength), at);
    at += m.model.v.byteLength;
    bytes.set(new Uint8Array(m.model.i.buffer, m.model.i.byteOffset, m.model.i.byteLength), at);
    at += m.model.i.byteLength;
  }
  return bytes;
}

function collisionBytes(v: number[], i: number[], k: number[]): Uint8Array {
  const bytes = new Uint8Array(12 + v.length * 4 + i.length * 4 + ((k.length + 3) & ~3));
  const view = new DataView(bytes.buffer);
  view.setUint32(0, 0x4c435152, true);
  view.setUint32(4, v.length / 3, true);
  view.setUint32(8, k.length, true);
  new Float32Array(bytes.buffer, 12, v.length).set(v);
  new Uint32Array(bytes.buffer, 12 + v.length * 4, i.length).set(i);
  bytes.set(k, 12 + v.length * 4 + i.length * 4);
  return bytes;
}

const t0 = performance.now();
const gen = generate(seed);
const buckets = gen.meshes.sorted().filter((b) => b.geo.ni > 0);
// Models are built on the simulation's bind poses. The mage is model 0; a knight of kind k at level l is model 100 k + l.
const wasm = await Bun.file(join(import.meta.dir, "../public/sim/requiem_sim.wasm")).arrayBuffer();
const sim = await Sim.load(wasm, null, 0);
const models: { id: number; model: SkinModel }[] = [{ id: 0, model: buildMage(sim.bind(FIGURE.MAGE)) }];
const knightStats: Record<string, number[]> = {};
for (const kind of [FIGURE.KNIGHT_SWORD, FIGURE.KNIGHT_HALBERD, FIGURE.KNIGHT_GREAT]) {
  knightStats[kind] = [];
  KNIGHT_LODS.forEach((detail, lod) => {
    const model = buildKnight(kind, sim.bind(kind), detail);
    models.push({ id: kind * 100 + lod, model });
    knightStats[kind].push(model.i.length / 3);
  });
}
const files: Record<string, Uint8Array> = {
  "meshes.bin": meshBytes(
    [0x52495152, 1, buckets.length],
    buckets.map((b) => ({ head: [b.layer, b.cx, b.cz], geo: b.geo })),
  ),
  "models.bin": modelBytes(models),
  "atlas.rgba": new Uint8Array(paintAtlas(seed).buffer),
  "collision.bin": collisionBytes(gen.col.v, gen.col.i, gen.col.k),
  "stage.rqsw": writeWorldFile(gen),
};

// Strip boundaries let the compiler filter mips inside each strip.
const edges = new Set<number>([0, ATLAS_H]);
for (const s of Object.values(STRIP)) {
  edges.add(s.y);
  edges.add(s.y + s.h);
}
// The sky as a table: elevation by azimuth.
const sky: number[][] = [];
for (let e = -2; e <= 16; e++) {
  for (let a = 0; a < 24; a++) {
    const el = (e / 16) * (Math.PI / 2);
    const az = (a / 24) * Math.PI * 2;
    sky.push(skyColor([Math.cos(el) * Math.cos(az), Math.sin(el), Math.cos(el) * Math.sin(az)]));
  }
}
const layerCount = (l: Layer) => buckets.filter((b) => b.layer === l).length;
const scene = {
  seed,
  ...SCENE,
  cell: CELL,
  superCell: SUPER,
  atlas: { width: ATLAS_W, height: ATLAS_H, stripEdges: [...edges].sort((a, b) => a - b) },
  skyTable: { elevations: [-2, 16], azimuths: 24, colors: sky },
  stage: { knights: gen.knights, cohorts: gen.stage.musters.length, obstacles: gen.obstacles.length, start: gen.stage.start, demon: gen.stage.demon, ...gen.counts },
  crowd: { kinds: 3, lods: KNIGHT_LODS.length, frames: KNIGHT_FRAMES, triangles: knightStats },
  source: {
    buckets: { near: layerCount(Layer.Near), mid: layerCount(Layer.Mid), far: layerCount(Layer.Far), backdrop: layerCount(Layer.Backdrop) },
    triangles: [Layer.Near, Layer.Mid, Layer.Far, Layer.Backdrop].map((l) => gen.meshes.triangles(l)),
    collisionTriangles: gen.col.k.length,
  },
};
files["scene.json"] = new TextEncoder().encode(JSON.stringify(scene, null, 1));

await mkdir(out, { recursive: true });
const manifest = { version: 1, name: "the-field", seed, files: [] as { path: string; bytes: number; sha256: string }[] };
for (const [path, bytes] of Object.entries(files)) {
  await Bun.write(join(out, path), bytes);
  manifest.files.push({ path, bytes: bytes.length, sha256: new Bun.CryptoHasher("sha256").update(bytes).digest("hex") });
}
await Bun.write(join(out, "manifest.json"), JSON.stringify(manifest, null, 1));
console.log(`stage IR: ${out}  seed ${seed}  ${buckets.length} buckets  ${(Object.values(files).reduce((n, b) => n + b.length, 0) / 1e6).toFixed(1)} MB  ${(performance.now() - t0).toFixed(0)} ms`);
console.log(`knights: ${gen.knights} in ${gen.stage.musters.length} cohorts; triangles per level ${JSON.stringify(knightStats)}; mage ${models[0].model.i.length / 3}`);
console.log(`ground and props: near ${gen.meshes.triangles(Layer.Near)} mid ${gen.meshes.triangles(Layer.Mid)} far ${gen.meshes.triangles(Layer.Far)} backdrop ${gen.meshes.triangles(Layer.Backdrop)}; ${JSON.stringify(gen.counts)}`);
