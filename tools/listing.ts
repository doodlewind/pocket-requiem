#!/usr/bin/env bun
// The game's listing on Pocket Studio: its words (listing/listing.json) and
// the clips, stills and share picture they name, recorded from the game.
//
//   bun tools/listing.ts [--only first-contact.mp4,card.jpg]   record → dist/listing/
//   bun tools/listing.ts --check                                the words and the takes against the limits, nothing recorded
//   bun tools/listing.ts --upload                               then `pocket-studio listing dist/listing`, run here,
//                                                               where `pocket-studio register` wrote .pocket-studio.json
//                                                               (POCKET_STUDIO_CLI names the command when it is not on PATH)
//
// Every picture is drawn by the wgpu renderer on this machine's GPU
// (wgpu/src/bin/shot.rs: the browser version's renderer, reading the PS
// Vita's pack) from a run of the simulation with nothing held on the pad: the
// autopilot plays, or the mage stands still, and words hold the eye. A run
// is frames of a thirtieth of a second, two ticks each, so a clip has no
// dropped frame and a second recording makes the same pictures. ffmpeg
// encodes them.
//
// No media file goes to Git: dist/ is ignored.

import { $ } from "bun";
import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { MAP, PACK, shotBinary } from "./wgpu.ts";

const ROOT = resolve(import.meta.dir, "..");
const OUT = join(ROOT, "dist/listing");
const WORDS = join(ROOT, "listing/listing.json");
const FPS = 30;

/** An eye at a frame of a take: where it is, what it looks at, its field of view in degrees. */
type Eye = [frame: number, px: number, py: number, pz: number, tx: number, ty: number, tz: number, fov: number];

interface Take {
  /** Words handed in before the first frame (`App::control` of wgpu/src/app.rs). */
  words: string;
  /** Words handed in before a frame of the run. */
  at?: [frame: number, words: string][];
  /** The eye, moved from one of these to the next with an ease at both ends. */
  path?: Eye[];
  /** A clip: the run's frames from `from` to `to`. A still: the frame `to`. */
  from?: number;
  to: number;
  /** A clip's poster: a frame of the run. */
  poster?: number;
  size?: [number, number];
}

// What the autopilot does is the simulation's: the same frames at every run of one revision. The frames
// below were chosen by looking at that run (`--sheet` tiles one for choosing again).
const READOUTS = "hint=0";
const TAKES: Record<string, Take> = {
  // From the run toward the first cohorts, through the first strikes and spells, to the first undoing.
  "first-contact.mp4": { words: READOUTS, from: 150, to: 750, poster: 414 },
  // Fire, the beam, the pillar, the dome.
  "spells.mp4": { words: READOUTS, from: 1050, to: 1500, poster: 1087 },
  // The mage stands where the fight starts; the eye rises over the first rank's middle cohort and goes north
  // above the ones behind it, toward the moon.
  "the-army.mp4": { words: "auto=0 hud=0", from: 0, to: 360, poster: 110, path: [[0, 10, 3, 432, 2, 1.5, 394, 50], [360, 0, 8, 372, -10, 4, 280, 50]] },
  "circle.jpg": { words: READOUTS, to: 414 },
  "fire.jpg": { words: READOUTS, to: 1087 },
  "undoing.jpg": { words: READOUTS, to: 1443 },
  // (the autopilot kills too fast to show a press of knights: she stands still from 14 s on)
  "press.jpg": { words: READOUTS, at: [[420, "auto=0"]], to: 1290 },
  "card.jpg": { words: "hud=0", to: 414, size: [1200, 630] },
};

/** The limits of a listing (the contract between the Studio and the game repositories, version 1). */
const LIMITS = { tagline: 120, paragraphs: [1, 6], paragraph: 600, media: [2, 12], caption: 140, video: 12 << 20, image: 2 << 20, card: 1 << 20, seconds: [6, 30], all: 96 << 20 };
const FROM = ["browser", "psp", "vita", "3ds", "ipod-touch", "android"];

interface Media {
  kind: "video" | "image";
  file: string;
  poster?: string;
  width: number;
  height: number;
  seconds?: number;
  from: string;
  caption: string;
}
interface Listing {
  tagline: string;
  description: string[];
  media: Media[];
  card: string;
}

/** The words, checked against the limits and against the takes. */
function words(): Listing {
  const listing = JSON.parse(readFileSync(WORDS, "utf8")) as Listing;
  const wrong: string[] = [];
  const within = (n: number, [least, most]: number[]) => n >= least! && n <= most!;
  if (!listing.tagline || listing.tagline.length > LIMITS.tagline) wrong.push(`the tagline has ${listing.tagline?.length ?? 0} characters (1 to ${LIMITS.tagline})`);
  if (!within(listing.description.length, LIMITS.paragraphs)) wrong.push(`${listing.description.length} paragraphs`);
  listing.description.forEach((p, i) => p.length > LIMITS.paragraph && wrong.push(`paragraph ${i + 1} has ${p.length} characters (at most ${LIMITS.paragraph})`));
  if (!within(listing.media.length, LIMITS.media)) wrong.push(`${listing.media.length} media entries`);
  const name = /^[a-z0-9-]+\.(mp4|jpg|webp|png)$/;
  for (const m of listing.media) {
    const take = TAKES[m.file];
    if (!name.test(m.file) || !take) wrong.push(`${m.file}: no take of that name`);
    if (m.caption.length > LIMITS.caption) wrong.push(`${m.file}: the caption has ${m.caption.length} characters (at most ${LIMITS.caption})`);
    if (!FROM.includes(m.from)) wrong.push(`${m.file}: from "${m.from}"`);
    const [w, h] = take?.size ?? [960, 544];
    if (m.width !== w || m.height !== h) wrong.push(`${m.file}: the take is ${w} x ${h}`);
    if (m.width / m.height < 4 / 3 || m.width / m.height > 2) wrong.push(`${m.file}: not between 4:3 and 2:1`);
    if (m.kind === "video") {
      const seconds = take ? (take.to - (take.from ?? 0)) / FPS : 0;
      if (!m.file.endsWith(".mp4") || !m.poster || !name.test(m.poster) || take?.poster === undefined) wrong.push(`${m.file}: a clip is an .mp4 with a poster`);
      if (m.seconds !== seconds || !within(seconds, LIMITS.seconds)) wrong.push(`${m.file}: the take is ${seconds} s`);
    } else if (m.kind !== "image" || m.file.endsWith(".mp4")) wrong.push(`${m.file}: kind "${m.kind}"`);
  }
  if (listing.media[0]?.kind !== "video") wrong.push("the lead is not a clip");
  const card = TAKES[listing.card];
  if (!card || card.size?.[0] !== 1200 || card.size?.[1] !== 630 || !listing.card.endsWith(".jpg")) wrong.push(`${listing.card}: the share picture is a JPEG of 1200 x 630`);
  if (wrong.length) throw new Error(`listing/listing.json: ${wrong.join("; ")}`);
  return listing;
}

const ease = (t: number) => t * t * (3 - 2 * t);

/** The arguments of the capture binary for a take run to frame `to`. */
function run(take: Take, to: number): string[] {
  const [w, h] = take.size ?? [960, 544];
  const args = ["--pack", PACK, "--map", MAP, "--shape", "vita", "--size", `${w}x${h}`, "--frames", String(to), "--words", take.words];
  for (const [frame, said] of take.at ?? []) args.push("--at", `${frame}:${said}`);
  const path = take.path ?? [];
  for (let frame = 0; path.length && frame < to; frame++) {
    const next = Math.max(1, path.findIndex((k) => k[0] > frame) < 0 ? path.length - 1 : path.findIndex((k) => k[0] > frame));
    const [a, b] = [path[next - 1]!, path[next]!];
    const t = ease(Math.min(1, Math.max(0, (frame - a[0]) / (b[0] - a[0]))));
    args.push("--at", `${frame}:view=${[1, 2, 3, 4, 5, 6, 7].map((i) => (a[i]! + (b[i]! - a[i]!) * t).toFixed(3)).join(",")}`);
  }
  return args;
}

/** A frame of a take as a JPEG. */
async function still(binary: string, take: Take, frame: number, out: string) {
  const png = `${out}.png`;
  await $`${binary} ${run(take, frame)} --out ${png}`.quiet();
  await $`ffmpeg -v error -y -i ${png} -q:v 2 -pix_fmt yuvj420p ${out}`.quiet();
  rmSync(png);
}

/** A take's frames from `from` on as an H.264 clip without sound, faded in and out so that its loop has no cut. */
async function clip(binary: string, take: Take, out: string) {
  const [w, h] = take.size ?? [960, 544];
  const seconds = (take.to - take.from!) / FPS;
  const fade = `fade=t=in:st=0:d=0.3,fade=t=out:st=${(seconds - 0.3).toFixed(2)}:d=0.3`;
  const shot = Bun.spawn([binary, ...run(take, take.to), "--from", String(take.from), "--film", "-"], { stdout: "pipe", stderr: "ignore" });
  const encode = Bun.spawn(
    ["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", `${w}x${h}`, "-r", String(FPS), "-i", "-", "-vf", fade, "-an", "-c:v", "libx264", "-profile:v", "high", "-pix_fmt", "yuv420p", "-preset", "slow", "-crf", "20", "-maxrate", "4M", "-bufsize", "8M", "-movflags", "+faststart", out],
    { stdin: shot.stdout, stdout: "inherit", stderr: "inherit" },
  );
  if ((await shot.exited) !== 0 || (await encode.exited) !== 0) throw new Error(`${out}: the recording failed`);
}

const flag = (name: string) => process.argv.includes(name);
const option = (name: string) => {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : undefined;
};

const listing = words();
if (flag("--check")) {
  console.log(`listing/listing.json: ${listing.media.length} media entries and the share picture, within the limits`);
  process.exit(0);
}
if (!existsSync(PACK) || !existsSync(MAP)) throw new Error("the packs are missing: bun tools/wgpu.ts cook");
const binary = await shotBinary();

if (option("--sheet")) {
  // A run of the autopilot as tiles a second and a half apart, for choosing frames: --sheet out.png [--frames 5400].
  const frames = Number(option("--frames") ?? 5400);
  const shot = Bun.spawn([binary, ...run({ words: READOUTS, to: frames }, frames), "--film", "-"], { stdout: "pipe", stderr: "ignore" });
  const tile = Bun.spawn(["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", "960x544", "-r", String(FPS), "-i", "-", "-vf", `select=not(mod(n\\,45)),scale=192:109,tile=12x${Math.ceil(frames / 45 / 12)}`, "-frames:v", "1", resolve(option("--sheet")!)], { stdin: shot.stdout, stdout: "inherit", stderr: "inherit" });
  process.exit((await tile.exited) || (await shot.exited));
}

const only = option("--only")?.split(",");
if (!only) rmSync(OUT, { recursive: true, force: true });
mkdirSync(OUT, { recursive: true });
const wanted = (file: string) => !only || only.includes(file);
for (const m of listing.media) {
  const take = TAKES[m.file]!;
  if (!wanted(m.file)) continue;
  console.log(`listing: ${m.file}`);
  if (m.kind === "video") {
    await clip(binary, take, join(OUT, m.file));
    await still(binary, take, take.poster!, join(OUT, m.poster!));
  } else {
    await still(binary, take, take.to, join(OUT, m.file));
  }
}
if (wanted(listing.card)) {
  console.log(`listing: ${listing.card}`);
  await still(binary, TAKES[listing.card]!, TAKES[listing.card]!.to, join(OUT, listing.card));
}
writeFileSync(join(OUT, "listing.json"), `${JSON.stringify(listing, null, 2)}\n`);

// What was written is what the Studio takes.
const report: Record<string, number> = {};
const over: string[] = [];
for (const [file, most] of [...listing.media.flatMap((m) => [[m.file, m.kind === "video" ? LIMITS.video : LIMITS.image], ...(m.poster ? [[m.poster, LIMITS.image]] : [])]), [listing.card, LIMITS.card]] as [string, number][]) {
  if (!existsSync(join(OUT, file))) {
    over.push(`${file} was not recorded`);
    continue;
  }
  report[file] = statSync(join(OUT, file)).size;
  if (report[file]! > most) over.push(`${file} is ${report[file]} bytes (at most ${most})`);
}
const total = Object.values(report).reduce((a, b) => a + b, 0);
if (total > LIMITS.all) over.push(`${total} bytes in all (at most ${LIMITS.all})`);
console.log(JSON.stringify({ directory: OUT, bytes: total, files: report }, null, 1));
if (over.length) throw new Error(`dist/listing: ${over.join("; ")}`);

if (flag("--upload")) {
  if (!existsSync(join(ROOT, ".pocket-studio.json"))) throw new Error('this checkout is not registered with Pocket Studio: run `pocket-studio register --title "Pocket Requiem"` here');
  const cli = process.env.POCKET_STUDIO_CLI?.trim().split(/\s+/) ?? (Bun.which("pocket-studio") ? ["pocket-studio"] : null);
  if (!cli) throw new Error("no pocket-studio on PATH: install it from the Studio, or set POCKET_STUDIO_CLI to its command");
  const command = [...cli, "listing", OUT];
  console.log(`$ ${command.join(" ")}`);
  const code = await Bun.spawn(command, { cwd: ROOT, stdin: "ignore", stdout: "inherit", stderr: "inherit" }).exited;
  if (code !== 0) throw new Error(`pocket-studio listing exited ${code}`);
}
