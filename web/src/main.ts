// Pocket Requiem, three.js reference.
//
//   /                      play (keyboard, mouse or a gamepad)
//   /?auto                 the autopilot plays
//   /?shot&ticks=N[&auto]  run N ticks, draw one frame, then set the title to `shot-ready {...}`
//   /?view=px,py,pz,tx,ty,tz,fov   a fixed camera (with `shot`)
//   /?chase=dx,dy,dz,fov   a camera at that offset from the mage, looking at her
//   /?test=N               a flat stage with N cohorts instead of the field
//   /?model=...            a model preview (see preview.ts)

import * as THREE from "three";
import { playTitle } from "../../vendor/pocketjs/engine/pocket3d/crates/pocket3d-title/web/pocket3d-title.js";
import { Hud } from "./game/hud";
import { Input } from "./game/input";
import { buildDemon } from "./model/demon";
import { buildMage } from "./model/mage";
import { buildKnight } from "./model/knight";
import { characterMaterial } from "./render/character";
import { CrowdView } from "./render/crowd";
import { FxView } from "./render/fx";
import { lightUniforms, setLights } from "./render/light";
import { Skinned } from "./render/skinned";
import { makeSky } from "./render/sky";
import { atlasTexture, WorldView } from "./render/worldview";
import { EV, FIGURE, LIGHTS, SNAP } from "./sim/abi.gen";
import { Sim } from "./sim/sim";
import { paintAtlas } from "./world/atlas";
import { SCENE } from "./world/scene";
import { generate } from "./world/stage";
import { writeWorldFile } from "./world/worldfile";

const q = new URLSearchParams(location.search);
const canvas = document.querySelector<HTMLCanvasElement>("#view")!;
if (q.has("model")) {
  const { preview } = await import("./preview");
  await preview(q, canvas);
  // The preview owns the page.
  await new Promise(() => {});
}
const seed = Number(q.get("seed") ?? 2026);
const shot = q.has("shot");
const auto = q.has("auto");
const fixed = q.get("view")?.split(",").map(Number);
const chase = q.get("chase")?.split(",").map(Number);
/** `demon=dx,dy,dz,fov`: a camera at that offset from the demon, looking at her. */
const atDemon = q.get("demon")?.split(",").map(Number);
const test = Number(q.get("test") ?? 0);

// The Pocket3D title card covers the page while the stage is generated. A capture run skips it.
const title = shot ? Promise.resolve() : playTitle();
const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, preserveDrawingBuffer: shot });
renderer.outputColorSpace = THREE.SRGBColorSpace;
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;

const t0 = performance.now();
const scene = new THREE.Scene();
scene.fog = new THREE.FogExp2(new THREE.Color().setRGB(...SCENE.fog), SCENE.fogDensity);
let sim: Sim;
let view: WorldView | null = null;
let counts: Record<string, number> = {};
let demonAt: [number, number, number] | null = null;
if (test > 0) {
  sim = await Sim.load(await fetch("/sim/requiem_sim.wasm"), null, test);
  const ground = new THREE.Mesh(new THREE.PlaneGeometry(800, 800), new THREE.MeshLambertMaterial({ color: new THREE.Color().setRGB(0.09, 0.09, 0.07) }));
  ground.rotation.x = -Math.PI / 2;
  ground.receiveShadow = true;
  scene.add(ground);
} else {
  const gen = generate(seed);
  counts = { ...gen.counts, knights: gen.knights, obstacles: gen.obstacles.length };
  sim = await Sim.load(await fetch("/sim/requiem_sim.wasm"), writeWorldFile(gen));
  demonAt = [gen.stage.demon[0], sim.height(gen.stage.demon[0], gen.stage.demon[1]), gen.stage.demon[1]];
  view = new WorldView(gen.meshes, atlasTexture(paintAtlas(seed)));
  scene.add(view.group);
}

// The mage, and the three kinds of knight at two levels of detail.
const mage = new Skinned(buildMage(sim.bind(FIGURE.MAGE)), characterMaterial());
scene.add(mage.mesh);
const demon = new Skinned(buildDemon(sim.bind(FIGURE.DEMON)), characterMaterial());
scene.add(demon.mesh);
const kinds = [FIGURE.KNIGHT_SWORD, FIGURE.KNIGHT_HALBERD, FIGURE.KNIGHT_GREAT];
const crowd = new CrowdView(
  sim,
  kinds.map((k) => [buildKnight(k, sim.bind(k), { cell: 0.03, trims: true }), buildKnight(k, sim.bind(k), { cell: 0.075, inflate: 0.012 })]),
  Number(q.get("lod") ?? 24),
);
scene.add(crowd.group);
const fx = new FxView(sim);
scene.add(fx.group);
const sky = makeSky();
scene.add(sky);
const buildMs = performance.now() - t0;

const moon = new THREE.DirectionalLight(new THREE.Color().setRGB(...SCENE.sun), Math.PI);
moon.castShadow = true;
moon.shadow.mapSize.set(4096, 4096);
moon.shadow.bias = -0.0004;
moon.shadow.normalBias = 0.2;
const sc = moon.shadow.camera;
sc.left = sc.bottom = -120;
sc.right = sc.top = 120;
sc.near = 1;
sc.far = 900;
scene.add(moon, moon.target);
scene.add(new THREE.HemisphereLight(new THREE.Color().setRGB(...SCENE.sky), new THREE.Color().setRGB(...SCENE.bounce), Math.PI));
// What the spells cast on the ground.
const cast = Array.from({ length: LIGHTS }, () => {
  const l = new THREE.PointLight(0xffffff, 0, 10, 1.2);
  scene.add(l);
  return l;
});

const camera = new THREE.PerspectiveCamera(58, 16 / 9, SCENE.clip.near, SCENE.clip.far);
const hud = new Hud(document.querySelector("#hud")!);
const input = new Input(canvas);

function resize() {
  const w = shot ? Number(q.get("w") ?? 960) : innerWidth;
  const h = shot ? Number(q.get("h") ?? 544) : innerHeight;
  renderer.setPixelRatio(shot ? 1 : Math.min(devicePixelRatio, 2));
  renderer.setSize(w, h, !shot);
  camera.aspect = w / h;
}
window.addEventListener("resize", resize);
resize();

const frustum = new THREE.Frustum();
const pv = new THREE.Matrix4();
const moonDir = new THREE.Vector3(...SCENE.sunDir);
const planes = new Float32Array(24);
let stats = { draws: 0, triangles: 0 };

function events(s: Float32Array) {
  const e = s[SNAP.EVENTS];
  if (e & EV.READY) hud.say("MANA FULL", 1.4);
  if (e & EV.UNSEAL) hud.say("THE BINDING COMES UNDONE", 2.2);
  if (e & EV.FALLEN) hud.say("FALLEN", 2.5);
  if (e & EV.WON) hud.say("EVERY BINDING IS UNDONE.", 6);
}

function draw() {
  const s = sim.snapshot();
  if (fixed && fixed.length >= 6) {
    camera.position.set(fixed[0], fixed[1], fixed[2]);
    camera.up.set(0, 1, 0);
    camera.lookAt(fixed[3], fixed[4], fixed[5]);
    camera.fov = fixed[6] ?? 58;
  } else if (atDemon && atDemon.length >= 3 && demonAt) {
    camera.position.set(demonAt[0] + atDemon[0], demonAt[1] + atDemon[1], demonAt[2] + atDemon[2]);
    camera.up.set(0, 1, 0);
    camera.lookAt(demonAt[0], demonAt[1] + 1.0, demonAt[2]);
    camera.fov = atDemon[3] ?? 45;
  } else if (chase && chase.length >= 3) {
    camera.position.set(s[SNAP.POS] + chase[0], s[SNAP.POS + 1] + chase[1], s[SNAP.POS + 2] + chase[2]);
    camera.up.set(0, 1, 0);
    camera.lookAt(s[SNAP.POS], s[SNAP.POS + 1] + 0.9, s[SNAP.POS + 2]);
    camera.fov = chase[3] ?? 45;
  } else {
    const shake = s[SNAP.CAM_SHAKE] * 0.22;
    const t = s[SNAP.TICK];
    camera.position.set(s[SNAP.CAM_POS] + Math.sin(t * 1.7) * shake, s[SNAP.CAM_POS + 1] + Math.sin(t * 2.3) * shake, s[SNAP.CAM_POS + 2] + Math.cos(t * 1.9) * shake);
    camera.up.set(0, 1, 0);
    camera.lookAt(camera.position.x + s[SNAP.CAM_LOOK], camera.position.y + s[SNAP.CAM_LOOK + 1], camera.position.z + s[SNAP.CAM_LOOK + 2]);
    camera.fov = s[SNAP.CAM_FOV];
  }
  camera.updateProjectionMatrix();
  camera.updateMatrixWorld();
  pv.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
  frustum.setFromProjectionMatrix(pv);
  stats = view ? view.select(camera.position, frustum) : { draws: 0, triangles: 0 };
  // Left, right, bottom, top; the crowd ignores the near and far planes.
  frustum.planes.forEach((p, k) => planes.set([p.normal.x, p.normal.y, p.normal.z, p.constant], k * 4));
  crowd.update(sim.crowd(planes, [camera.position.x, camera.position.y, camera.position.z], Number(q.get("far") ?? 420)));
  mage.skin(s.subarray(SNAP.SKIN));
  demon.skin(sim.demon());
  setLights(s);
  for (let k = 0; k < LIGHTS; k++) {
    const o = SNAP.LIGHTS + k * 8;
    cast[k].position.set(s[o], s[o + 1], s[o + 2]);
    cast[k].distance = s[o + 3];
    cast[k].color.setRGB(s[o + 4], s[o + 5], s[o + 6]);
    cast[k].intensity = s[o + 7] > 0 ? 6 : 0;
  }
  fx.update(s, camera);
  sky.position.copy(camera.position);
  // The shadow map follows the mage in steps, so its texels do not swim.
  const step = 240 / 4096;
  const cx = Math.round(s[SNAP.POS] / step / 32) * step * 32;
  const cz = Math.round(s[SNAP.POS + 2] / step / 32) * step * 32;
  moon.target.position.set(cx, s[SNAP.POS + 1], cz);
  moon.position.copy(moon.target.position).addScaledVector(moonDir, 400);
  renderer.render(scene, camera);
  hud.update(s);
  events(s);
}

/** `stick=lx,ly[,buttons]`: a held stick (and buttons) in shot mode. */
const stick = q.get("stick")?.split(",").map(Number);
/** `press=tick:button,...`: buttons pressed for one tick each in shot mode. */
const presses = new Map<number, number>((q.get("press") ?? "").split(",").filter(Boolean).map((p) => p.split(":").map(Number) as [number, number]));
let ticks = 0;

function tick() {
  if (auto) sim.tickAuto();
  else if (shot) sim.tick((stick?.[2] ?? 0) | (presses.get(ticks) ?? 0), stick?.[0] ?? 0, stick?.[1] ?? 0, 0, 0);
  else {
    const p = input.read();
    sim.tick(p.buttons, p.lx, p.ly, p.rx, p.ry);
  }
  ticks++;
}

/** Sound starts on the first key, click or pad press: browsers require a gesture. */
function startAudio() {
  const ctx = new AudioContext();
  const node = ctx.createScriptProcessor(1024, 0, 2);
  node.onaudioprocess = (e) => {
    const pcm = sim.audio(1024, ctx.sampleRate);
    const l = e.outputBuffer.getChannelData(0);
    const r = e.outputBuffer.getChannelData(1);
    for (let i = 0; i < 1024; i++) {
      l[i] = pcm[i * 2] / 32768;
      r[i] = pcm[i * 2 + 1] / 32768;
    }
  };
  node.connect(ctx.destination);
}

void lightUniforms;
if (shot) {
  document.body.classList.add("shot");
  const n = Number(q.get("ticks") ?? 1);
  for (let i = 0; i < n; i++) tick();
  draw();
  draw();
  const s = sim.snapshot();
  document.title = `shot-ready ${JSON.stringify({ ...stats, calls: renderer.info.render.calls, buildMs: Math.round(buildMs), pos: [...s.subarray(SNAP.POS, SNAP.POS + 3)].map((v) => Math.round(v)), kos: s[SNAP.KOS], free: s[SNAP.FREE], crowd: crowd.shown, crowdTris: crowd.triangles, ...counts })}`;
} else {
  for (const type of ["keydown", "pointerdown"]) window.addEventListener(type, startAudio, { once: true });
  // the game's clock starts when the title card has ended
  await title;
  let last = performance.now();
  let acc = 0;
  const frame = (now: number) => {
    const dt = Math.min((now - last) / 1000, 0.1);
    last = now;
    acc += dt;
    while (acc >= 1 / 60) {
      tick();
      acc -= 1 / 60;
    }
    draw();
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}
