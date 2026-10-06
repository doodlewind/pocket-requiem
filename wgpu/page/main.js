// Pocket Requiem in a browser tab: the wgpu renderer (../src, built to pkg/),
// which runs the game's own simulation and draws its readouts itself, as the
// handhelds do. The page is the Pocket3D player of PocketJS's browser kernel
// (vendor/pocketjs/devices/web/pocket-web-wgpu, staged beside this file): the
// bar, the device's shell with its keys, the way to Pocket Studio. This file
// says what the game is and draws into the player's canvas.
//
// The page shows one of the handhelds the game runs on: its shell, its
// screens, its buttons. The Pocket3D title card plays first; the pack is read
// while it plays and after, and the canvas says how much of it has arrived.
//
// The address chooses the device and the pack:
//
//   ?device=vita|psp|3ds        the handheld (without it: a PS Vita)
//   ?size=960x544 &hz=30        the first device's screen, changed
//   ?pack=URL                   the pack (the PS Vita's): its file, on a server that answers byte ranges,
//                               or the manifest (.json) of one cut into pieces. Without it, the page's
//                               own (<meta name="pocket-pack">)
//   ?words=auto=0+view=…        what a development host would send the run (App::control)
//   ?sound=off                  no sound
//
// `window.pocketRequiem` is the running game, for a console and for tools/wgpu.ts.
import { playTitle } from "./pocket3d-title.js";
import { frames, hasWebGPU, titleCard } from "./pocket3d-shell.js";
import { createPlayer } from "./pocket3d-player.js";
import init, { Requiem, shapes } from "./pkg/requiem_wgpu.js";

// The handhelds: each one's screen is the renderer's shape of the same name. `sticks` says what turns the
// eye (a second stick, or the direction pad), `glyphs` what stands on the face buttons, `lower` is a second
// screen. `note` is what the player says beside the device's name: how this picture differs from the one
// that device's own build draws (README, "On the PSP and the 3DS").
const DEVICES = [
  { id: "vita", label: "PS Vita", sticks: 2, glyphs: "playstation", note: { en: "This page draws the PS Vita build's pack with the PS Vita's passes, at its 30 frames a second.", ja: "このページは PS Vita 版のパックを、PS Vita の描画パスで、同じ毎秒 30 フレームで描いています。" } },
  { id: "psp", label: "PSP", sticks: 1, glyphs: "playstation", note: { en: "On a PSP most knights near the eye are figures of prisms, the far ranks are two quads a knight and the edges are hard. This page draws the PS Vita build's army at the PSP's size, without the PS Vita's glow.", ja: "PSP では視点の近くの騎士の多くが角柱を組んだ姿になり、遠くの隊列は騎士 1 人を 2 枚の四角形で描き、輪郭はぎざぎざです。このページは PS Vita 版の軍勢を PSP の画面サイズで、PS Vita の光のにじみを付けずに描いています。" } },
  { id: "3ds", label: "Nintendo 3DS", sticks: 1, glyphs: "letters", lower: [320, 240], note: { en: "On a 3DS the knights are coarser from 44 metres, the far ranks are two quads a knight and the edges are hard. This page draws the PS Vita build's army at the 3DS's size, without the PS Vita's glow.", ja: "3DS では 44 メートルより先の騎士が粗くなり、遠くの隊列は騎士 1 人を 2 枚の四角形で描き、輪郭はぎざぎざです。このページは PS Vita 版の軍勢を 3DS の画面サイズで、PS Vita の光のにじみを付けずに描いています。" } },
];

const query = new URLSearchParams(location.search);
const number = (name) => Math.max(0, Number.parseInt(query.get(name) ?? "0", 10) || 0);
const message = (error) => String(error?.message ?? error);
const named = (name) => new URL(document.querySelector(`meta[name="${name}"]`).content, location.href).href;
const started = performance.now();

let device = DEVICES.find((d) => d.id === query.get("device")) ?? DEVICES[0];
let present = () => {};
const player = createPlayer({
  title: "Pocket Requiem",
  tagline: { en: "One mage against a headless army, on a field at night.", ja: "夜の荒れ野で、魔法使いがひとり、首のない軍勢に立ち向かう。" },
  devices: DEVICES,
  device: device.id,
  // (the targets the game has a package for)
  runsOn: ["psp", "vita", "3ds"],
  pick: (id) => present(DEVICES.find((d) => d.id === id)),
});
const { canvas, stage, controls } = player;
const say = (text) => player.say(text);

// Sound: the simulation's own synthesizer, rendered a little ahead of the clock and queued as buffers. A
// browser lets a page sound only after a key or a pointer went down on it.
function createSound(requiem) {
  if (query.get("sound") === "off" || !(window.AudioContext ?? window.webkitAudioContext)) return () => {};
  let context = null;
  let until = 0;
  const wake = () => {
    context ??= new (window.AudioContext ?? window.webkitAudioContext)();
    context.resume();
  };
  for (const type of ["keydown", "pointerdown"]) addEventListener(type, wake, { capture: true });
  return () => {
    if (context?.state !== "running") return;
    const now = context.currentTime;
    // (the tab was hidden, or the queue ran dry: start again from now)
    if (until < now + 0.02) until = now + 0.04;
    const count = Math.min(Math.floor((now + 0.16 - until) * context.sampleRate), 8192);
    if (count < 256) return;
    const pairs = requiem.sound(count, context.sampleRate);
    const buffer = context.createBuffer(2, count, context.sampleRate);
    const [left, right] = [buffer.getChannelData(0), buffer.getChannelData(1)];
    for (let i = 0; i < count; i++) {
      left[i] = pairs[i * 2];
      right[i] = pairs[i * 2 + 1];
    }
    const source = context.createBufferSource();
    source.buffer = buffer;
    source.connect(context.destination);
    source.start(until);
    until += count / context.sampleRate;
  };
}

// The 3DS's lower screen: the field from above with its marks at the left, the fight in numbers beside it.
function drawLower(requiem, context, map) {
  const pixels = requiem.lower();
  if (pixels.length === map.data.length) {
    map.data.set(pixels);
    context.putImageData(map, 0, 0);
  }
  const n = JSON.parse(requiem.numbers());
  context.fillStyle = "#000";
  context.fillRect(map.width, 0, context.canvas.width - map.width, context.canvas.height);
  if (!n) return;
  context.fillStyle = "#fff";
  context.font = "13px ui-monospace, Menlo, Consolas, monospace";
  context.textBaseline = "top";
  const lines = ["POCKET", "REQUIEM", "", `HP ${Math.round(n.hp * 100)}%`, `MANA ${Math.round(n.mana * 100)}%`, `${n.kos} / ${n.goal}`, `${n.standing} stand`, "", n.won ? "UNDONE" : n.auto ? "AUTOPILOT" : "MANUAL"];
  lines.forEach((line, i) => context.fillText(line, map.width + 2, 4 + i * 16, context.canvas.width - map.width - 4));
}

async function start() {
  // The card is the first picture of every launch, and covers the page while the pack is read.
  const title = titleCard(playTitle);
  if (!hasWebGPU()) {
    await title;
    say({ en: "This browser has no WebGPU, which Pocket Requiem draws with.", ja: "このブラウザは WebGPU に対応していません。Pocket Requiem の描画には WebGPU が必要です。" });
    return;
  }
  await init();
  const all = JSON.parse(shapes());

  // The shell, on the first device's screen. It draws before there is a game: the canvas says what is read.
  const [width, height] = (query.get("size") ?? "").split("x").map((n) => Number.parseInt(n, 10) || 0);
  const first = all.find((s) => s.name === device.id);
  stage.show({ device: device.id, width: width || first.width, height: height || first.height, lower: device.lower });
  const requiem = await Requiem.open(canvas, first.name);
  let shape = JSON.parse(requiem.reshape(first.name, canvas.width, canvas.height, 0, number("hz")));
  // (milliseconds from the page's start: the pack arrived, the first frame, the first of the game)
  const report = { requiem, device: () => device.id, shape: () => shape, firstFrame: 0, firstGame: 0, frames: 0, failure: "" };
  window.pocketRequiem = report;

  requiem.read(new URL(query.get("pack") ?? named("pocket-pack"), location.href).href);
  if (query.get("words")) requiem.control(query.get("words"));
  // The field from above, for a device with a second screen: read beside the pack.
  fetch(named("pocket-map")).then((answer) => (answer.ok ? answer.arrayBuffer() : Promise.reject(new Error(`the map: status ${answer.status}`)))).then((bytes) => requiem.map(new Uint8Array(bytes))).catch((error) => {
    report.mapFailure = message(error);
  });

  // A device on the page: its screens and its controls.
  let map = null;
  present = (next, sized) => {
    device = next;
    const to = sized ?? all.find((s) => s.name === next.id);
    player.show(next.id, { width: to.width, height: to.height, lower: next.lower, sticks: next.sticks, glyphs: next.glyphs });
    shape = JSON.parse(requiem.reshape(to.name, to.width, to.height, 0, to.hz));
    map = next.lower ? new ImageData(240, 240) : null;
  };
  present(device, { ...shape });

  const sound = createSound(requiem);
  const frame = (now) => {
    const held = controls.read();
    requiem.frame(now, held.buttons, held.left[0], held.left[1], held.right[0], held.right[1]);
    controls.next();
    report.frames++;
    report.firstFrame ||= performance.now() - started;
    if (!requiem.runs()) return;
    if (!report.firstGame) {
      report.firstGame = performance.now() - started;
      // (the game is on the screen: the player may read what it kept back)
      player.ready();
    }
    sound();
    // The map six times a second, as on the device.
    if (map && report.frames % 5 === 0) drawLower(requiem, stage.lower, map);
  };

  await title;
  canvas.hidden = false;
  stage.fit();
  const loop = frames(() => shape.hz, (now) => {
    try {
      frame(now);
    } catch (error) {
      // A frame the canvas had no texture for is skipped; anything else stops the game and says why.
      report.failure = message(error);
      if (!/Outdated|Lost|Timeout/.test(report.failure)) {
        loop.stop();
        say(report.failure);
      }
    }
  });

  // For a console and for tools/wgpu.ts.
  // Another device while the game runs: `pocketRequiem.present("psp")`.
  report.present = (id) => present(DEVICES.find((d) => d.id === id));
  // The frame as the canvases hold it, one pixel of the game to a pixel: PNGs as data URLs.
  report.capture = () => {
    frame(performance.now());
    return { upper: canvas.toDataURL("image/png"), lower: stage.second.hidden ? null : stage.second.toDataURL("image/png") };
  };
  // Milliseconds a frame costs the processor and the GPU together, over `count` frames made without
  // waiting for the display.
  report.burst = async (count = 300) => {
    const gpu = canvas.getContext("webgpu").getConfiguration().device;
    const from = performance.now();
    for (let i = 0; i < count; i++) frame(from + ((i + 1) * 1000) / shape.hz);
    await gpu.queue.onSubmittedWorkDone();
    return (performance.now() - from) / count;
  };
}

start().catch((error) => {
  window.pocketRequiem = { failure: message(error) };
  say(window.pocketRequiem.failure);
});
