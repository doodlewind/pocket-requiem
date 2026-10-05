#!/usr/bin/env bun
// The PS Vita LiveArea pictures: two captures of the web reference, written as
// the 8-bit indexed PNGs the VPK holds.
//
//   bun tools/livearea.ts            capture, then write vita/assets/sce_sys/livearea/contents/{bg,startup}.png
//   bun tools/livearea.ts --check    the committed pictures pass PocketJS's VPK asset rules
//
// Both pictures are tick 900 of the autopilot's fight on the seeded field. The
// reference renders each one larger than its file, the tool averages it down
// and reduces it to 256 colours. Needs the wasm simulation
// (`bun tools/requiem.ts sim`) and Chrome. The tool starts its own server on a
// free port and ends it, so the capture is of this checkout.
//
// The bubble's icon is not made here: it is Pocket3D's, from
// vendor/pocketjs/engine/pocket3d/icon (tools/vita.ts passes it to the packager).

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { medianCut } from "../vendor/pocketjs/tools/median-cut.ts";
import { decodePNG, encodeIndexedPNG } from "../vendor/pocketjs/tools/png.ts";
import { POCKET3D_ICON } from "../vendor/pocketjs/tools/pocket3d-icon.ts";
import { resolveVitaPackageAssets } from "../vendor/pocketjs/tools/vita-package.ts";
import { serveWeb, shot } from "./shot.ts";

const ROOT = resolve(import.meta.dir, "..");
const ASSETS = join(ROOT, "vita/assets");
const OUT = join(ASSETS, "sce_sys/livearea/contents");
const WORK = join(ROOT, ".pocket-build/livearea");
const TICKS = "900";

/** `scale` is how many rendered pixels each way become one pixel of the file. */
const PICTURES = [
  // Behind the gate: the game's own camera.
  { file: "bg.png", width: 840, height: 500, scale: 2, query: "" },
  // The gate: a camera 7.5 m from the mage, looking at her.
  { file: "startup.png", width: 280, height: 158, scale: 4, query: "chase=-4,2.2,-6,42" },
] as const;

/** The mean of each `scale` x `scale` block. */
function average(rgba: Uint8Array, width: number, height: number, scale: number): Uint8Array {
  const out = new Uint8Array(width * height * 4);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    for (let c = 0; c < 3; c++) {
      let sum = 0;
      for (let v = 0; v < scale; v++) for (let u = 0; u < scale; u++) sum += rgba[((y * scale + v) * width * scale + x * scale + u) * 4 + c]!;
      out[(y * width + x) * 4 + c] = Math.round(sum / (scale * scale));
    }
    out[(y * width + x) * 4 + 3] = 255;
  }
  return out;
}

/** Reduces RGBA to at most 256 colours: a palette of r, g, b triples and one index per pixel. */
function index(rgba: Uint8Array, pixels: number): { indices: Uint8Array; palette: Uint8Array } {
  const key = (i: number) => (rgba[i * 4]! << 16) | (rgba[i * 4 + 1]! << 8) | rgba[i * 4 + 2]!;
  const counts = new Map<number, number>();
  for (let i = 0; i < pixels; i++) counts.set(key(i), (counts.get(key(i)) ?? 0) + 1);
  const colours = [...counts].map(([k, count]) => ({ r: k >> 16, g: (k >> 8) & 255, b: k & 255, count }));
  const palette: number[] = [];
  const slot = new Map<number, number>();
  for (const box of medianCut(colours, 256)) {
    const total = box.reduce((sum, colour) => sum + colour.count, 0);
    for (const c of ["r", "g", "b"] as const) palette.push(Math.round(box.reduce((sum, colour) => sum + colour[c] * colour.count, 0) / total));
    for (const colour of box) slot.set((colour.r << 16) | (colour.g << 8) | colour.b, palette.length / 3 - 1);
  }
  const indices = new Uint8Array(pixels);
  for (let i = 0; i < pixels; i++) indices[i] = slot.get(key(i))!;
  return { indices, palette: Uint8Array.from(palette) };
}

/** Throws unless the packager accepts the asset tree with the Pocket3D icon. */
export function check(): void {
  resolveVitaPackageAssets({ applicationAssets: ASSETS, icon: POCKET3D_ICON.vita });
}

if (import.meta.main) {
  if (!process.argv.includes("--check")) {
    const probe = Bun.listen({ hostname: "127.0.0.1", port: 0, socket: { data() {} } });
    const port = probe.port;
    probe.stop(true);
    const server = await serveWeb(port);
    if (server === undefined) throw new Error(`port ${port} already answers: not this checkout's server`);
    try {
      mkdirSync(WORK, { recursive: true });
      for (const p of PICTURES) {
        const raw = join(WORK, p.file);
        await shot(["--out", raw, "--w", String(p.width * p.scale), "--h", String(p.height * p.scale), "--auto", "--ticks", TICKS, "--port", String(port), ...(p.query ? ["--query", p.query] : [])]);
        const capture = decodePNG(readFileSync(raw));
        if (capture.w !== p.width * p.scale || capture.h !== p.height * p.scale) throw new Error(`${raw} is ${capture.w}x${capture.h}`);
        const { indices, palette } = index(average(capture.rgba, p.width, p.height, p.scale), p.width * p.height);
        const png = encodeIndexedPNG(indices, palette, p.width, p.height);
        writeFileSync(join(OUT, p.file), png);
        console.log(`livearea: ${p.file} ${p.width}x${p.height}, ${palette.length / 3} colours, ${png.length} bytes`);
      }
    } finally {
      // The server leads its own process group (shot.ts starts it detached).
      process.kill(-server, "SIGTERM");
    }
  }
  check();
  console.log("livearea: the packager accepts vita/assets with the Pocket3D icon");
}
