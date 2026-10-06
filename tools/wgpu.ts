#!/usr/bin/env bun
// Pocket Requiem drawn with wgpu (wgpu/): in a browser tab over WebGPU, and on
// this machine, where a frame goes to a file.
//
//   bun tools/wgpu.ts cook                         the packs for it: the PS Vita's (profiles/vita30.json), and the
//                                                  3DS's for its map of the field → .pocket-build/stage/the-field.map
//   bun tools/wgpu.ts build                        wasm32 + wasm-bindgen + the page + PocketJS's player → .pocket-build/wgpu/site
//   bun tools/wgpu.ts serve [--port 8802]          the site and the pack, with byte ranges
//   bun tools/wgpu.ts dist [--piece 1]             the directory a static host serves → .pocket-build/wgpu/dist:
//                                                  the page, the module under its build's name, and the pack
//                                                  cut into pieces of that many MiB with their manifest (one piece
//                                                  a read of wgpu/src/pack.rs)
//   bun tools/wgpu.ts serve --dist                 that directory as such a host serves it: no byte ranges
//   bun tools/wgpu.ts shot [--out f.png] [--shape vita] [--size WxH] [--frames 240] [--words "view=… auto=0"]
//                                                  one frame on this machine's GPU (Metal) → a PNG and the status
//   bun tools/wgpu.ts check [--headed] [--seconds 5] [--dist]   the page in Chrome, driven by keys and pointer:
//                                                  each device from the title card into the fight, the pad taken
//                                                  from the autopilot, the army closed round the mage, another
//                                                  device picked in the fight, what a frame costs
//                                                  → .pocket-build/validation/web/
//
// Every command takes [--pack PATH] (default: .pocket-build/stage/the-field.vita30.pack).
// The packs, the site and the captures stay under the ignored .pocket-build/.

import { $ } from "bun";
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { gzipSync } from "node:zlib";
import { join, resolve } from "node:path";
import { POCKET3D_ICON } from "../vendor/pocketjs/tools/pocket3d-icon.ts";
import { cutPack, POCKET3D_WEB, stagePocket3dWeb } from "../vendor/pocketjs/tools/pocket3d-web.ts";

const ROOT = resolve(import.meta.dir, "..");
const CRATE = join(ROOT, "wgpu");
const BUILD = join(ROOT, ".pocket-build/wgpu");
const SITE = join(BUILD, "site");
const DIST = join(BUILD, "dist");
export const STAGE = join(ROOT, ".pocket-build/stage");
/** The field from above for the 3DS's lower screen: the `MAPT` section of the 3DS's pack, as it is. */
export const MAP = join(STAGE, "the-field.map");
// What the host a build is deployed to allows (Pocket Studio's site deployments): the size of a file, the
// files and the bytes of a deployment, and the top-level names it keeps for itself.
const HOST = { file: 32 << 20, files: 4000, bytes: 1 << 30, reserved: ["play", "runtime"] };

const [command, ...rest] = process.argv.slice(2);
const option = (flag: string, fallback = "") => {
  const i = rest.indexOf(flag);
  return i >= 0 && rest[i + 1] ? rest[i + 1]! : fallback;
};
export const PACK = join(STAGE, "the-field.vita30.pack");
const pack = resolve(option("--pack", PACK));

function needPack() {
  for (const file of [pack, MAP]) if (!existsSync(file)) throw new Error(`${file} is missing: bun tools/wgpu.ts cook`);
}

/** A section of a pack file (crates/requiem-pack): its bytes, or null when the pack has none of that tag. */
function section(file: string, tag: string): Uint8Array | null {
  const bytes = readFileSync(file);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (bytes.subarray(0, 4).toString("latin1") !== "RQPK") throw new Error(`${file} is not a pack`);
  for (let i = 0; i < view.getUint32(8, true); i++) {
    const at = 16 + i * 16;
    if (bytes.subarray(at, at + 4).toString("latin1") === tag) return bytes.subarray(view.getUint32(at + 4, true), view.getUint32(at + 4, true) + view.getUint32(at + 8, true));
  }
  return null;
}

/** The packs a tab reads: the PS Vita's whole, and the map out of the 3DS's. */
async function cook() {
  await $`bun tools/requiem.ts cook --profile vita30`.cwd(ROOT);
  await $`bun tools/requiem.ts cook --profile n3ds30 --no-export`.cwd(ROOT);
  const map = section(join(STAGE, "the-field.n3ds30.pack"), "MAPT");
  if (!map) throw new Error("the 3DS's pack has no map");
  writeFileSync(MAP, map);
  return { pack: PACK, bytes: statSync(PACK).size, map: MAP, mapBytes: map.length };
}

/** The wasm module, its JavaScript side and the page, as one directory a static server can serve. */
async function build() {
  // The crate and the command line tool write two halves of one interface: their versions must be the same.
  const lock = readFileSync(join(CRATE, "Cargo.lock"), "utf8").match(/name = "wasm-bindgen"\nversion = "([^"]+)"/)?.[1];
  const tool = (await $`wasm-bindgen --version`.text()).trim().split(" ")[1];
  if (lock !== tool) throw new Error(`wasm-bindgen ${tool} is installed and wgpu/Cargo.lock has ${lock}: cargo install wasm-bindgen-cli --version ${lock}`);
  await $`cargo build --release --lib --target wasm32-unknown-unknown`.cwd(CRATE);
  rmSync(SITE, { recursive: true, force: true });
  mkdirSync(join(SITE, "pkg"), { recursive: true });
  await $`wasm-bindgen --target web --no-typescript --out-dir ${join(SITE, "pkg")} ${join(CRATE, "target/wasm32-unknown-unknown/release/requiem_wgpu.wasm")}`;
  for (const file of ["index.html", "main.js"]) cpSync(join(CRATE, "page", file), join(SITE, file));
  // What the page loads from PocketJS's browser kernel (vendor/pocketjs/devices/web/pocket-web-wgpu), as
  // PocketJS stages it. This game draws its readouts itself and starts no guest: the realm, the UI core and
  // the module that opens them are left out of the site.
  await stagePocket3dWeb(SITE);
  for (const file of [...POCKET3D_WEB.realm, POCKET3D_WEB.core, "pocket3d-interface.js"]) rmSync(join(SITE, file), { force: true });
  cpSync(POCKET3D_ICON.ios2x, join(SITE, "icon.png"));
  if (existsSync(MAP)) cpSync(MAP, join(SITE, "stage.map"));

  const sizes: Record<string, { bytes: number; gzip: number }> = {};
  for (const file of files(SITE).sort()) {
    const bytes = readFileSync(join(SITE, file));
    sizes[file] = { bytes: bytes.length, gzip: gzipSync(bytes, { level: 9 }).length };
  }
  writeFileSync(join(BUILD, "site.json"), JSON.stringify({ wasmBindgen: tool, sizes }, null, 1));
  return sizes;
}

const sha256 = (bytes: Uint8Array | string) => new Bun.CryptoHasher("sha256").update(bytes).digest("hex");

/** Every file under a directory, as paths from it. */
function files(directory: string, under = ""): string[] {
  return readdirSync(join(directory, under), { withFileTypes: true }).flatMap((entry) => (entry.isDirectory() ? files(directory, join(under, entry.name)) : [join(under, entry.name)]));
}

/**
 * The directory a static host serves, for a host that limits a file's size and keeps a file for ten minutes
 * in a browser's cache. Only the page is asked for again at every visit, so everything it names has a name of
 * its own contents: the module, its scripts and the map under `app/<build>/`, the pack's manifest by the
 * pack's hash, a piece by its own. A deployment of new code leaves the pack's files as they are.
 */
async function dist(pieceBytes: number) {
  needPack();
  await build();
  rmSync(DIST, { recursive: true, force: true });
  // (everything of the site but the page and its icon: the module, the scripts, the map)
  const app = files(SITE).filter((file) => !["index.html", "icon.png"].includes(file)).sort().map((file) => [file, readFileSync(join(SITE, file))] as const);
  const id = sha256(Buffer.concat(app.flatMap(([file, bytes]) => [Buffer.from(file), bytes]))).slice(0, 12);
  for (const [file, bytes] of app) {
    mkdirSync(join(DIST, "app", id, file, ".."), { recursive: true });
    writeFileSync(join(DIST, "app", id, file), bytes);
  }
  // The pack in pieces of one size, each named by its hash, and the manifest that lists them
  // (pocket_web_wgpu::source::Manifest), named by the pack's.
  const cut = cutPack(pack, join(DIST, "pack"), pieceBytes);
  const manifest = `pack/${cut.manifest}`;
  // The page names its build and its pack.
  let page = readFileSync(join(SITE, "index.html"), "utf8");
  const named: [string, string][] = [
    [`<meta name="pocket-pack" content="stage.pack">`, `<meta name="pocket-pack" content="${manifest}">`],
    [`<meta name="pocket-map" content="stage.map">`, `<meta name="pocket-map" content="app/${id}/stage.map">`],
    [`src="main.js"`, `src="app/${id}/main.js"`],
    [`href="pocket3d-stage.css"`, `href="app/${id}/pocket3d-stage.css"`],
    [`href="pocket3d-player.css"`, `href="app/${id}/pocket3d-player.css"`],
  ];
  for (const [from, to] of named) {
    if (!page.includes(from)) throw new Error(`wgpu/page/index.html has no ${from}`);
    page = page.replace(from, to);
  }
  // The game in Pocket Studio, for the player's door to it: the project this checkout is registered as
  // (`pocket-studio register` wrote .pocket-studio.json, which Git ignores). A checkout that is not
  // registered deploys a page whose door is the Studio's front one.
  const link = join(ROOT, ".pocket-studio.json");
  const project = existsSync(link) ? (JSON.parse(readFileSync(link, "utf8")) as { kind?: string; server?: string; app?: string }) : null;
  const registered = project?.kind === "site" && /^[A-Za-z0-9_-]+$/.test(project.app ?? "") && /^https:\/\/[A-Za-z0-9.-]+$/.test(project.server ?? "") ? project : null;
  if (registered) page = page.replace(`<meta name="pocket-pack"`, `<meta name="pocket-app" content="${registered.app}">\n<meta name="pocket-studio" content="${registered.server}">\n<meta name="pocket-pack"`);
  writeFileSync(join(DIST, "index.html"), page);
  cpSync(join(SITE, "icon.png"), join(DIST, "icon.png"));

  // What was written is what the host takes.
  const all = files(DIST).map((file) => ({ file, bytes: statSync(join(DIST, file)).size }));
  const total = all.reduce((sum, f) => sum + f.bytes, 0);
  const largest = all.reduce((a, b) => (b.bytes > a.bytes ? b : a));
  const part = (prefix: string) => all.filter((f) => f.file.startsWith(prefix)).reduce((sum, f) => ({ files: sum.files + 1, bytes: sum.bytes + f.bytes }), { files: 0, bytes: 0 });
  const refused = [
    ...all.filter((f) => f.bytes > HOST.file).map((f) => `${f.file} is ${f.bytes} bytes (a file is at most ${HOST.file})`),
    ...(all.length > HOST.files ? [`${all.length} files (at most ${HOST.files})`] : []),
    ...(total > HOST.bytes ? [`${total} bytes (at most ${HOST.bytes})`] : []),
    ...readdirSync(DIST).filter((name) => HOST.reserved.includes(name)).map((name) => `${name}/ is the host's own`),
  ];
  if (refused.length) throw new Error(`the host would refuse the directory: ${refused.join("; ")}`);
  const report = { directory: DIST, files: all.length, bytes: total, largest, build: id, studio: registered ? { app: registered.app, server: registered.server } : null, page: part("index.html"), app: part("app/"), pack: { ...part("pack/"), manifest, pieces: cut.pieces.length, piece: pieceBytes, sha256: cut.sha256 } };
  writeFileSync(join(BUILD, "dist.json"), JSON.stringify(report, null, 1));
  return report;
}

const TYPES: Record<string, string> = { html: "text/html; charset=utf-8", js: "text/javascript; charset=utf-8", css: "text/css; charset=utf-8", wasm: "application/wasm", json: "application/json", png: "image/png", webp: "image/webp", woff2: "font/woff2", txt: "text/plain; charset=utf-8" };

/**
 * What a game's host of Pocket Studio answers at `/app.json`, for the server here: the player reads the
 * game's name and its packages from it, and the address it tells that it opened (`opened`, which the
 * server here answers itself and counts).
 */
const OPENED = "/api/events/player";
const opens: { query: string; mode: string }[] = [];
const APP = { kind: "site", id: "local", slug: null, url: null, title: "Pocket Requiem", author: "local", tagline: "One mage against a headless army, on a field at night.", verified: true, status: "published", packages: [] };

/** The deployable directory as its host serves it: whole files, the page asked for again at every visit. */
function serveDist(port: number) {
  return Bun.serve({
    port,
    hostname: "127.0.0.1",
    fetch(request) {
      const path = decodeURIComponent(new URL(request.url).pathname);
      // (this host names no address for it: a page that asks is heard, and the check says so)
      if (path === OPENED) opens.push({ query: new URL(request.url).search, mode: request.headers.get("sec-fetch-mode") ?? "" });
      const file = join(DIST, path === "/" ? "index.html" : path);
      if (!file.startsWith(DIST) || !existsSync(file) || !statSync(file).isFile()) return new Response("not found", { status: 404 });
      const type = file.split(".").pop()!;
      return new Response(Bun.file(file), { headers: { "Content-Type": TYPES[type] ?? "application/octet-stream", "Cache-Control": type === "html" ? "no-cache" : "public, max-age=600" } });
    },
  });
}

/** The site and the pack. The pack is answered a range at a time, as a tab asks for it. */
function serve(port: number) {
  const size = statSync(pack).size;
  return Bun.serve({
    port,
    hostname: "127.0.0.1",
    fetch(request) {
      const path = decodeURIComponent(new URL(request.url).pathname);
      if (path === "/app.json") return Response.json({ ...APP, opened: new URL(OPENED, request.url).href }, { headers: { "Cache-Control": "no-store" } });
      if (path === OPENED) {
        opens.push({ query: new URL(request.url).search, mode: request.headers.get("sec-fetch-mode") ?? "" });
        return new Response(null, { status: 204 });
      }
      if (path === "/stage.pack") {
        const head = { "Accept-Ranges": "bytes", "Content-Type": "application/octet-stream", "Cache-Control": "no-store" };
        const range = request.headers.get("range")?.match(/^bytes=(\d+)-(\d*)$/);
        if (!range) return new Response(Bun.file(pack), { headers: head });
        const from = Number(range[1]);
        const to = Math.min(range[2] ? Number(range[2]) : size - 1, size - 1);
        if (from > to) return new Response(null, { status: 416, headers: { "Content-Range": `bytes */${size}` } });
        return new Response(Bun.file(pack).slice(from, to + 1), { status: 206, headers: { ...head, "Content-Range": `bytes ${from}-${to}/${size}`, "Content-Length": String(to - from + 1) } });
      }
      const file = join(SITE, path === "/" ? "index.html" : path);
      if (!file.startsWith(SITE) || !existsSync(file) || !statSync(file).isFile()) return new Response("not found", { status: 404 });
      return new Response(Bun.file(file), { headers: { "Content-Type": TYPES[file.split(".").pop()!] ?? "application/octet-stream", "Cache-Control": "no-store" } });
    },
  });
}

/** The capture binary, built. */
export async function shotBinary() {
  await $`cargo build --release --bin requiem-shot`.cwd(CRATE).quiet();
  return join(CRATE, "target/release/requiem-shot");
}

/** One frame on this machine's GPU. Returns the status the run printed. */
async function shot(out: string, extra: string[]) {
  const binary = await shotBinary();
  mkdirSync(resolve(out, ".."), { recursive: true });
  const run = await $`${binary} --pack ${pack} --map ${MAP} --out ${out} ${extra}`.quiet();
  return JSON.parse(run.stderr.toString().trim().split("\n").at(-1)!);
}

const stamp = () => new Date().toISOString().replace(/[:.]/g, "-");
const validation = (run: string) => {
  const directory = join(ROOT, ".pocket-build/validation/web", run);
  mkdirSync(directory, { recursive: true });
  return directory;
};

if (!import.meta.main) {
  // (tools/listing.ts imports the paths and the capture binary)
} else if (command === "cook") {
  console.log(JSON.stringify(await cook(), null, 1));
} else if (command === "build") {
  needPack();
  console.log(JSON.stringify(await build(), null, 1));
} else if (command === "dist") {
  console.log(JSON.stringify(await dist(Math.round(Number(option("--piece", "1")) * (1 << 20))), null, 1));
} else if (command === "serve" && rest.includes("--dist")) {
  if (!existsSync(join(DIST, "index.html"))) await dist(1 << 20);
  const server = serveDist(Number(option("--port", "8802")));
  console.log(`http://127.0.0.1:${server.port}/   (${DIST})`);
} else if (command === "serve") {
  needPack();
  if (!existsSync(join(SITE, "pkg/requiem_wgpu_bg.wasm"))) await build();
  const server = serve(Number(option("--port", "8802")));
  console.log(`http://127.0.0.1:${server.port}/   (${pack})`);
} else if (command === "shot") {
  needPack();
  const out = resolve(option("--out", join(validation(`shot-${stamp()}`), "frame.png")));
  const passed = ["--shape", "--size", "--samples", "--hz", "--frames", "--words", "--lower", "--status"].flatMap((flag) => (option(flag) ? [flag, option(flag)] : []));
  console.log(JSON.stringify(await shot(out, passed), null, 1));
  console.log(out);
} else if (command === "check") {
  const { check } = await import("./wgpu-check.ts");
  needPack();
  // (--dist: the deployable directory, served whole files only, with the pack in pieces)
  const deployed = rest.includes("--dist") ? await dist(Math.round(Number(option("--piece", "1")) * (1 << 20))) : null;
  const sizes = deployed ? JSON.parse(readFileSync(join(BUILD, "site.json"), "utf8")).sizes : await build();
  const server = deployed ? serveDist(0) : serve(0);
  const directory = validation(`check-${deployed ? "dist-" : ""}${stamp()}`);
  try {
    const report = await check({ origin: `http://127.0.0.1:${server.port}`, directory, seconds: Number(option("--seconds", "5")), headed: rest.includes("--headed"), deployed: !!deployed, opens });
    writeFileSync(join(directory, "report.json"), JSON.stringify({ ...report, pack, deployed, sizes }, null, 1));
    console.log(JSON.stringify(report, null, 1));
    console.log(directory);
  } finally {
    server.stop(true);
  }
} else {
  console.log("usage: bun tools/wgpu.ts <cook|build|serve [--dist]|dist|shot|check [--dist]>");
  process.exit(command ? 1 : 0);
}
