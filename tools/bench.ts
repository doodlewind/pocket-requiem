// Frame timings on the device while the autopilot fights.
//
//   bun tools/requiem.ts bench [--seconds 60] [--ctl '{"lodMid":300}'] [--share DIR]
//
// Samples the running process's status receipt once a second and writes the
// evidence to `.pocket-build/validation/vita/bench-<time>/device.json`. The
// identity (native build, pack hash) comes from what the device reports and
// must not change during the window.

import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { DeviceEvidence, type DeviceIdentity } from "../vendor/pocketjs/tools/device-evidence.ts";
import { ctl, status } from "./vita.ts";

const ROOT = resolve(import.meta.dir, "..");

interface Sample {
  t: number;
  frameMs: number;
  worstMs: number;
  late: number;
  frames: number;
  cpuMs: Record<string, number>;
  gpuMs: number | null;
  world: Record<string, number>;
  crowd: Record<string, any>;
  fx: Record<string, number>;
  mage: Record<string, number>;
  player: Record<string, unknown>;
}

function identity(s: any): DeviceIdentity {
  if (s?.engine?.stage !== "running") throw new Error(`the device is not running the game (stage ${s?.engine?.stage ?? "unknown"})`);
  return { device: `vita:${s.titleId}`, runtimeBuild: s.nativeBuild, assets: { "stage.pack": s.engine.pack.sha256 } };
}

export async function bench(argv: string[]) {
  const arg = (name: string, dflt: string) => {
    const i = argv.indexOf(name);
    return i >= 0 ? argv[i + 1] : dflt;
  };
  const seconds = Number(arg("--seconds", "60"));
  const extra = JSON.parse(arg("--ctl", "{}"));
  ctl(argv, JSON.stringify({ auto: true, reset: true, nonce: Date.now(), ...extra }));
  await Bun.sleep(2500);
  const first = status(argv);
  const evidence = new DeviceEvidence<Sample>(identity(first));
  const start = Date.now();
  let prev: any = first.engine;
  const samples: Sample[] = [];
  while (Date.now() - start < seconds * 1000) {
    await Bun.sleep(1000);
    const s = status(argv);
    const e = s.engine;
    if (e.frames === prev.frames) continue;
    const sample: Sample = { t: (Date.now() - start) / 1000, frameMs: e.frameMs, worstMs: e.worstMs, late: e.late, frames: e.frames, cpuMs: e.cpuMs, gpuMs: e.gpuMs, world: e.world, crowd: e.crowd, fx: e.fx, mage: e.mage, player: e.player };
    evidence.observe(identity(s), sample);
    samples.push(sample);
    prev = e;
  }
  if (samples.length < 2) throw new Error("no samples: is the USB host running and the game on screen?");
  const frames = samples.at(-1)!.frames - first.engine.frames;
  const late = samples.at(-1)!.late - first.engine.late;
  const avg = samples.reduce((n, s) => n + s.frameMs, 0) / samples.length;
  const worst = Math.max(...samples.map((s) => s.worstMs));
  const maxTris = Math.max(...samples.map((s) => s.world.tris + s.crowd.tris + s.mage.tris + s.fx.tris));
  const maxDraws = Math.max(...samples.map((s) => s.world.draws + s.crowd.draws + s.fx.draws + 1));
  const knights = samples.map((s) => s.crowd.shown as number);
  const cpu = (k: string) => samples.reduce((n, s) => n + s.cpuMs[k], 0) / samples.length;
  const summary = {
    seconds,
    frames,
    lateFrames: late,
    lateShare: late / Math.max(frames, 1),
    averageFrameMs: avg,
    worstFrameMs: worst,
    fps: 1000 / avg,
    maxTriangles: maxTris,
    maxDraws,
    knightsInView: { least: Math.min(...knights), mean: Math.round(knights.reduce((a, b) => a + b, 0) / knights.length), most: Math.max(...knights, samples.at(-1)!.crowd.mostShown) },
    cpuMs: { sim: cpu("sim"), crowd: cpu("crowd"), draw: cpu("draw") },
    undone: samples.at(-1)!.player.kos,
    control: extra,
  };
  const dir = resolve(ROOT, `.pocket-build/validation/vita/bench-${new Date().toISOString().replace(/[:.]/g, "-")}`);
  mkdirSync(dir, { recursive: true });
  writeFileSync(`${dir}/device.json`, JSON.stringify({ summary, ...evidence.receipt() }, null, 1));
  console.log(`${dir}/device.json`);
  console.log(JSON.stringify(summary, null, 1));
}
