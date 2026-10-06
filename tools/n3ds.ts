#!/usr/bin/env bun
// Pocket Requiem on Nintendo 3DS: build the Rust core and the C host into a
// .3dsx (devkitARM in PocketJS's pinned container), install and start it over
// PocketJS's paired LAN wire, steer and measure it.
//
//   bun tools/n3ds.ts build                     # dist/3ds/pocket-requiem.3dsx, the pack in its ROMFS
//   bun tools/n3ds.ts install [--no-build]      # build, send, start, wait for the game to report
//   bun tools/n3ds.ts status
//   bun tools/n3ds.ts ctl "auto=1 stats=1"      # Game::control words
//   bun tools/n3ds.ts capture [--out f.png] [--surface top|auxiliary]
//   bun tools/n3ds.ts bench [--seconds 60] [--install]   # autopilot frame timings → .pocket-build/validation/3ds/
//   bun tools/n3ds.ts look [--install] [--lead 14] [--ctl "auto=0"] [--frames 4] [--every 8] [--out DIR]
//                                               # one lease: install, run, steer, capture frames with their status
//   bun tools/n3ds.ts emu [--ctl "…"] [--seconds 6] [--out f.png] [--keep]
//                                               # the built .3dsx in Azahar (a window opens), with its own SD card and
//                                               # pairing key under .pocket-build/3ds/azahar: status and the upper screen
//
// `--host ADDRESS` (default 192.168.8.159, or POCKET_3DS_HOST). The console
// must run a Pocket Runtime build with the paired wire: this program itself
// once installed, or another Pocket Nexus .3dsx. Installs are .3dsx only.
// `--host 127.0.0.1` is the emulator `emu --keep` left running: status, ctl
// and capture reach it with its own key and do not claim the console.

import { $ } from "bun";
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { pocketRuntimeDeviceId } from "../vendor/pocketjs/contracts/spec/pocket-runtime-wire.ts";
import { withDeviceLease } from "../vendor/pocketjs/tools/device-lease.ts";
import { THREE_DS_DEV_HOST_ABI, THREE_DS_DEV_TARGET_ID } from "../vendor/pocketjs/tools/3ds-profile.ts";
import { runContainer } from "../vendor/pocketjs/tools/3ds-toolchain.ts";

const ROOT = resolve(import.meta.dir, "..");
const POCKETJS = join(ROOT, "vendor/pocketjs");
const DIR = join(ROOT, ".pocket-build/3ds");
const PACK = join(ROOT, ".pocket-build/stage/the-field.n3ds30.pack");
const ARTIFACT = join(ROOT, "dist/3ds/pocket-requiem.3dsx");
const NAME = "pocket-requiem.3dsx";
const RECEIPTS = join(ROOT, ".pocket-build/validation/3ds");
const argv = process.argv.slice(2);
const cmd = argv[0] ?? "";
const opt = (key: string, fallback: string) => {
  const at = argv.indexOf(key);
  return at < 0 ? fallback : (argv[at + 1] ?? fallback);
};
const host = cmd === "emu" ? "127.0.0.1" : opt("--host", process.env.POCKET_3DS_HOST ?? "192.168.8.159");
/** Azahar's home for `emu`: its settings, its SD card and the pairing key this tool wrote there. */
const EMULATOR = join(DIR, "azahar");
const emulated = host === "127.0.0.1";

// Loaded by path at run time: PocketJS's client module is checked by PocketJS's own compiler settings.
const clientModule: string = join(POCKETJS, "tools/3ds-runtime-client.ts");
const { discoverPocketRuntimes, parsePocketRuntimeToken, PocketRuntimeClient } = await import(clientModule);
type Client = any;

const sha = (path: string) => new Bun.CryptoHasher("sha256").update(readFileSync(path)).digest("hex");

async function build() {
  if (!existsSync(PACK)) throw new Error(`no pack at ${PACK}: run \`bun tools/requiem.ts cook --profile n3ds30\` first`);
  await $`rustup run nightly-2026-07-02 cargo build --release`.cwd(join(ROOT, "n3ds/core"));
  const romfs = join(DIR, "romfs");
  mkdirSync(romfs, { recursive: true });
  mkdirSync(join(DIR, "build"), { recursive: true });
  if (!existsSync(join(romfs, "world.pack")) || sha(join(romfs, "world.pack")) !== sha(PACK)) cpSync(PACK, join(romfs, "world.pack"));
  // The build's identity: the sources, the core library and the pack.
  const id = new Bun.CryptoHasher("sha256");
  for (const f of readdirSync(join(ROOT, "n3ds/src")).sort()) id.update(readFileSync(join(ROOT, "n3ds/src", f)));
  id.update(readFileSync(join(ROOT, "n3ds/core/target/armv6k-nintendo-3ds/release/librequiem_n3ds_core.a")));
  id.update(sha(PACK));
  const buildId = id.digest("hex").slice(0, 12);
  const header = `#define POCKETJS_HOST_ABI ${THREE_DS_DEV_HOST_ABI}\n#define POCKETJS_TARGET_ID "${THREE_DS_DEV_TARGET_ID}"\n#define REQUIEM_BUILD_ID "${buildId}"\n`;
  const config = join(DIR, "build/config.h");
  if (!existsSync(config) || readFileSync(config, "utf8") !== header) writeFileSync(config, header);
  // Compile a snapshot on the container's own filesystem: the shared mount can show a stale size for a file
  // that was just rewritten, on either side. The snapshot's name is new each build.
  const snapshot = `source-${buildId}-${Date.now()}.tar`;
  for (const f of readdirSync(DIR).filter((f) => f.startsWith("source-"))) rmSync(join(DIR, f));
  await $`tar --no-xattrs -cf ${join(DIR, snapshot)} n3ds/src n3ds/Makefile .pocket-build/3ds/build/config.h`.cwd(ROOT);
  await runContainer(
    `mkdir -p /tmp/source /tmp/build && tar -xf /requiem/.pocket-build/3ds/${snapshot} -C /tmp/source
cp /tmp/source/.pocket-build/3ds/build/config.h /tmp/build/config.h
make -f /tmp/source/n3ds/Makefile -j8 BUILD=/tmp/build SOURCE=/tmp/source/n3ds/src
cp /tmp/build/requiem.elf /tmp/build/requiem.map /requiem/.pocket-build/3ds/build/`,
    [{ hostPath: ROOT, containerPath: "/requiem" }],
    "/requiem",
    {},
    "Pocket Requiem build",
  );
  const bytes = readFileSync(ARTIFACT).length;
  if (bytes > 32 * 1024 * 1024) throw new Error(`the .3dsx is ${bytes} bytes; the wire installs at most 32 MiB`);
  mkdirSync(RECEIPTS, { recursive: true });
  const receipt = { target: "3ds", buildId, bytes, sha256: sha(ARTIFACT), packSha256: sha(PACK) };
  writeFileSync(join(RECEIPTS, "build.json"), JSON.stringify(receipt, null, 1) + "\n");
  console.log(`3ds: ${ARTIFACT} ${(bytes / 1e6).toFixed(1)} MB, build ${buildId}`);
  return receipt;
}

async function connect(): Promise<Client> {
  const keys = opt("--keys", emulated ? join(EMULATOR, "keys") : join(POCKETJS, ".pocket/3ds/devices"));
  let devices: any[] = await discoverPocketRuntimes({ addresses: [host] });
  for (let attempt = 0; attempt < 3 && !devices.some((d) => d.address === host); attempt++) {
    await Bun.sleep(400);
    devices = await discoverPocketRuntimes({ addresses: [host] });
  }
  const device = devices.find((d) => d.address === host);
  if (!device) throw new Error(`no Pocket Runtime answers at ${host}:8131`);
  let token: Uint8Array | undefined;
  for (const name of existsSync(keys) ? readdirSync(keys).filter((n) => n.endsWith(".key")) : []) {
    const t = parsePocketRuntimeToken(readFileSync(join(keys, name), "utf8"));
    if (pocketRuntimeDeviceId(t) === device.deviceId) token = t;
  }
  if (!token) throw new Error(`no pairing key in ${keys} matches the console; pass --keys DIR`);
  const client = new PocketRuntimeClient({ host, port: device.port, token, timeoutMs: 20000, heartbeatTimeoutMs: 30000 });
  client.on("ctrl", (m: any) => {
    if (m.t === "log" || m.t === "runtime.native") console.log(JSON.stringify(m));
  });
  try {
    await client.connect();
    return client;
  } catch (e) {
    client.close();
    throw e;
  }
}

async function status(c: Client, text = ""): Promise<any> {
  const reply = c.waitForCtrl((m: any) => m.t === "requiem.status", 8000);
  await c.sendCtrl({ t: "requiem.control", ...(text ? { text } : {}) });
  return await reply;
}

/** Connects, retrying: right after another client leaves, the first connect can time out. */
async function session<T>(f: (c: Client) => Promise<T>): Promise<T> {
  let last: unknown;
  for (let attempt = 0; attempt < 4; attempt++) {
    let c: Client | undefined;
    try {
      c = await connect();
      return await f(c);
    } catch (e) {
      last = e;
      await Bun.sleep(1200);
    } finally {
      c?.close();
    }
  }
  throw last;
}

/** Runs `f` holding the console's lease. A child process started with `lease.environment` shares it. */
async function device<T>(f: (lease: { environment: NodeJS.ProcessEnv }) => Promise<T>): Promise<T> {
  return await withDeviceLease(emulated ? "3ds:azahar" : "3ds:wire", f);
}

/** Sends the .3dsx and waits until the console reports this build, inside one lease. */
async function install(lease: { environment: NodeJS.ProcessEnv }, receipt: { buildId: string }) {
  await $`bun ${join(POCKETJS, "tools/3ds-dev.ts")} install --host ${host} --file ${ARTIFACT} --name ${NAME}`.cwd(POCKETJS).env(lease.environment as Record<string, string>);
  const end = Date.now() + 120_000;
  let last: any;
  while (Date.now() < end) {
    await Bun.sleep(1500);
    try {
      last = await session((c) => status(c));
      if (last.build === receipt.buildId && (last.stage === "running" || last.phase === "load-error")) break;
    } catch (e) {
      last = { error: String(e) };
    }
  }
  if (last?.build !== receipt.buildId) throw new Error(`the console did not come up with this build: ${JSON.stringify(last)}`);
  return last;
}

/**
 * Starts the built .3dsx in Azahar. The emulator has no headless mode and takes its whole user
 * directory from $HOME, so it runs with a home of its own: the user's settings and system files
 * copied, an empty SD card, and a pairing key so the dev wire listens on 127.0.0.1:8131.
 */
async function emulate() {
  const app = process.env.AZAHAR || "/Applications/Azahar.app";
  const binary = join(app, "Contents/MacOS/azahar");
  const source = join(homedir(), "Library/Application Support/Azahar");
  if (!existsSync(binary) || !existsSync(join(source, "config/qt-config.ini"))) throw new Error(`Azahar and its settings are needed (${app})`);
  if (!existsSync(ARTIFACT)) throw new Error("build first: bun tools/n3ds.ts build");
  const user = join(EMULATOR, "home/Library/Application Support/Azahar");
  // Only the emulator this tool started: its command line names this checkout's artifact.
  const stop = () => Bun.spawnSync(["pkill", "-9", "-f", `${binary} ${ARTIFACT}`]);
  stop();
  rmSync(EMULATOR, { recursive: true, force: true });
  mkdirSync(join(user, "config"), { recursive: true });
  mkdirSync(join(user, "sdmc/pocketjs/runtime"), { recursive: true });
  mkdirSync(join(EMULATOR, "keys"), { recursive: true });
  for (const directory of ["nand", "sysdata"]) if (existsSync(join(source, directory))) cpSync(join(source, directory), join(user, directory), { recursive: true });
  const settings = readFileSync(join(source, "config/qt-config.ini"), "utf8").replace(/^check_for_update_on_start=.*$/m, "check_for_update_on_start=false");
  writeFileSync(join(user, "config/qt-config.ini"), settings);
  const token = Buffer.from(Uint8Array.from({ length: 32 }, (_, i) => i * 7 + 3)).toString("hex");
  writeFileSync(join(user, "sdmc/pocketjs/runtime/dev.key"), token + "\n");
  writeFileSync(join(EMULATOR, "keys/azahar.key"), token + "\n");
  const log = join(EMULATOR, "console.log");
  await $`open -n -a ${app} --env HOME=${join(EMULATOR, "home")} --stdout ${log} --stderr ${log} --args ${ARTIFACT}`;
  try {
    const end = Date.now() + Number(opt("--timeout", "90")) * 1000;
    let last: any;
    while (Date.now() < end) {
      await Bun.sleep(1500);
      try {
        last = await session((c) => status(c));
        if (last.stage === "running" || last.phase === "load-error") break;
      } catch (e) {
        last = { error: String(e) };
      }
    }
    if (last?.stage !== "running") throw new Error(`the emulator did not reach the game: ${JSON.stringify(last)}`);
    const words = opt("--ctl", "");
    if (words) await session((c) => status(c, words));
    await Bun.sleep(Number(opt("--seconds", "6")) * 1000);
    const out = resolve(opt("--out", join(RECEIPTS, `emu-${Date.now()}.png`)));
    mkdirSync(resolve(out, ".."), { recursive: true });
    const [report, shot] = await session(async (c) => {
      const report = await status(c);
      const pending = c.waitForScreenshot();
      await c.sendCtrl({ t: "screenshot", surface: "top" });
      return [report, await pending];
    });
    await Bun.write(out, shot.png);
    console.log(JSON.stringify(report, null, 1));
    console.log(out);
  } finally {
    if (!argv.includes("--keep")) stop();
  }
}

switch (cmd) {
  case "build":
    await build();
    break;
  case "emu":
    await withDeviceLease("3ds:azahar", emulate);
    break;
  case "install": {
    const receipt = argv.includes("--no-build") ? JSON.parse(readFileSync(join(RECEIPTS, "build.json"), "utf8")) : await build();
    // One lease for the transfer and the wait: another checkout's tool cannot put its own program in between.
    await device(async (lease) => console.log(JSON.stringify(await install(lease, receipt), null, 1)));
    break;
  }
  case "look": {
    // One lease from start to end: install (with --install), let the autopilot run for --lead seconds, send
    // --ctl, then capture --frames upper screens --every seconds, each with the status beside it.
    const receipt = argv.includes("--no-build") || !argv.includes("--install") ? JSON.parse(readFileSync(join(RECEIPTS, "build.json"), "utf8")) : await build();
    const out = resolve(opt("--out", join(RECEIPTS, `look-${Date.now()}`)));
    mkdirSync(out, { recursive: true });
    await device(async (lease) => {
      if (argv.includes("--install")) await install(lease, receipt);
      await session((c) => status(c, "auto=1 reset=1 view=off"));
      await Bun.sleep(Number(opt("--lead", "0")) * 1000);
      const ctl = opt("--ctl", "");
      if (ctl) await session((c) => status(c, ctl));
      const rows: any[] = [];
      for (let i = 1; i <= Number(opt("--frames", "4")); i++) {
        await Bun.sleep(Number(opt("--every", "8")) * 1000);
        const { shot, s } = await session(async (c) => {
          const pending = c.waitForScreenshot();
          await c.sendCtrl({ t: "screenshot", surface: "top" });
          const shot = await pending;
          return { shot, s: await status(c) };
        });
        if (s.build !== receipt.buildId) throw new Error(`the console runs build ${s.build}, not ${receipt.buildId}`);
        await Bun.write(join(out, `s${i}.png`), shot.png);
        const row = { frames: s.frames, late: s.late, frameMs: s.frameMs, gpuMs: s.gpuMs, cpuMs: s.cpuMs, tris: s.tris, draws: s.draws, crowd: s.crowd, fx: s.fx, lodScale: s.settings?.lodScale, hp: s.player.hp, kos: s.player.kos };
        rows.push(row);
        console.log(JSON.stringify(row));
      }
      writeFileSync(join(out, "status.json"), JSON.stringify({ build: receipt.buildId, rows }, null, 1));
      console.log(out);
    });
    break;
  }
  case "status":
    await device(async () => console.log(JSON.stringify(await session((c) => status(c)), null, 1)));
    break;
  case "ctl":
    await device(async () => console.log(JSON.stringify(await session((c) => status(c, argv[1] ?? "")), null, 1)));
    break;
  case "capture":
    await device(async () => {
      const out = resolve(opt("--out", join(RECEIPTS, `capture-${Date.now()}.png`)));
      mkdirSync(resolve(out, ".."), { recursive: true });
      const shot = await session(async (c) => {
        const pending = c.waitForScreenshot();
        await c.sendCtrl({ t: "screenshot", surface: opt("--surface", "top") });
        return await pending;
      });
      await Bun.write(out, shot.png);
      console.log(out);
    });
    break;
  case "bench":
    await device(async (lease) => {
      const seconds = Number(opt("--seconds", "60"));
      const extra = opt("--ctl", "");
      const build = JSON.parse(readFileSync(join(RECEIPTS, "build.json"), "utf8"));
      // With --install the transfer and the measurement share the lease.
      if (argv.includes("--install")) await install(lease, build);
      await session(async (c) => {
        const first = await status(c, `auto=1 reset=1 view=off ${extra}`);
        if (first.build !== build.buildId) throw new Error(`the console runs build ${first.build}, not ${build.buildId}`);
        await Bun.sleep(3000);
        const start = Date.now();
        const begin = await status(c);
        const samples: any[] = [];
        while (Date.now() - start < seconds * 1000) {
          // Each sample costs the console a late frame or so: answering takes it a few milliseconds.
          await Bun.sleep(5000);
          const s = await status(c);
          samples.push({ t: (Date.now() - start) / 1000, frameMs: s.frameMs, worstMs: s.worstMs, late: s.late, frames: s.frames, cpuMs: s.cpuMs, gpuMs: s.gpuMs, draws: s.draws, tris: s.tris, crowd: s.crowd, fx: s.fx, lodScale: s.settings?.lodScale, player: s.player });
        }
        const last = samples.at(-1)!;
        const frames = last.frames - begin.frames;
        const late = last.late - begin.late;
        const avg = samples.reduce((n, s) => n + s.frameMs, 0) / samples.length;
        const summary = {
          seconds,
          frames,
          lateFrames: late,
          lateShare: late / Math.max(frames, 1),
          averageFrameMs: avg,
          fps: (frames / (last.t * 1000 - 0)) * 1000,
          worstFrameMs: Math.max(...samples.map((s) => s.worstMs)),
          maxGpuMs: Math.max(...samples.map((s) => s.gpuMs)),
          maxTriangles: Math.max(...samples.map((s) => s.tris)),
          maxDraws: Math.max(...samples.map((s) => s.draws)),
          knightsInView: { least: Math.min(...samples.map((s) => s.crowd.shown + s.crowd.far)), mean: Math.round(samples.reduce((n, s) => n + s.crowd.shown + s.crowd.far, 0) / samples.length), most: Math.max(...samples.map((s) => s.crowd.shown + s.crowd.far)) },
          knightMeshes: { mean: Math.round(samples.reduce((n, s) => n + s.crowd.shown, 0) / samples.length), most: Math.max(...samples.map((s) => s.crowd.shown)) },
          cpuMs: { sim: samples.reduce((n, s) => n + s.cpuMs.sim, 0) / samples.length, build: samples.reduce((n, s) => n + s.cpuMs.build, 0) / samples.length },
          leastLodScale: Math.min(...samples.map((s) => s.lodScale ?? 1)),
          bindingsUndone: last.player.kos,
          control: extra,
        };
        const dir = join(RECEIPTS, `bench-${new Date().toISOString().replace(/[:.]/g, "-")}`);
        mkdirSync(dir, { recursive: true });
        writeFileSync(join(dir, "device.json"), JSON.stringify({ summary, identity: { device: `3ds:${host}`, ...build, new3ds: begin.new3ds }, samples }, null, 1));
        console.log(join(dir, "device.json"));
        console.log(JSON.stringify(summary, null, 1));
      });
    });
    break;
  default:
    console.log("usage: bun tools/n3ds.ts <build|install|status|ctl|capture|bench|look|emu> [--host ADDRESS]");
    process.exit(cmd ? 1 : 0);
}
