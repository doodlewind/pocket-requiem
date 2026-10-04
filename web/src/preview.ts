// Model preview: `/?model=mage|knight1|knight2|knight3|demon&poses=...`
// draws one model in one or several poses, side by side, for checking shapes
// and motion.
//
//   poses   comma list: `b` bind pose, `i:<tick>` at ease, `r:<phase>:<speed>` running,
//           `h` hovering, `g` guarding, `m<move>:<tick>` a move, `f<frame>` a knight's stored frame
//   angle, pitch, dist, look, fov   the camera; angle 0 looks at the figure's front
//   night=1   the stage's light instead of a studio's

import * as THREE from "three";
import { buildDemon } from "./model/demon";
import { buildMage } from "./model/mage";
import { buildKnight } from "./model/knight";
import { SkinModel } from "./model/sdf";
import { Skinned } from "./render/skinned";
import { BONES, FIGURE, SHOW } from "./sim/abi.gen";
import { Sim } from "./sim/sim";

export async function preview(q: URLSearchParams, canvas: HTMLCanvasElement) {
  const sim = await Sim.load(await fetch("/sim/requiem_sim.wasm"), null, 1);
  const name = q.get("model")!;
  const t0 = performance.now();
  const cells = q.get("cells")?.split(",").map(Number) as [number, number] | undefined;
  let model: SkinModel;
  let kind = 0;
  let size = 1.55;
  if (name.startsWith("knight")) {
    kind = Number(name.slice(6)) || 1;
    model = buildKnight(kind, sim.bind(kind), { cell: cells?.[0] ?? 0.024, inflate: Number(q.get("inflate") ?? 0), trims: !q.has("plain") });
    size = 1.75;
  } else if (name === "demon") {
    kind = FIGURE.DEMON;
    model = buildDemon(sim.bind(FIGURE.DEMON), cells);
  } else {
    kind = FIGURE.MAGE;
    model = buildMage(sim.bind(FIGURE.MAGE), cells);
  }
  const ms = performance.now() - t0;
  const w = Number(q.get("w") ?? 960);
  const h = Number(q.get("h") ?? 544);
  const night = q.has("night");
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, preserveDrawingBuffer: true });
  renderer.setPixelRatio(1);
  renderer.setSize(w, h, false);
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.autoClear = false;
  const scene = new THREE.Scene();
  const back = new THREE.Color(night ? 0x16347c : 0x9fb4c8);
  const mat = new THREE.MeshLambertMaterial({ vertexColors: true });
  const skinned = new Skinned(model, mat);
  scene.add(skinned.mesh);
  const ground = new THREE.Mesh(new THREE.CircleGeometry(size * 2.4, 48), new THREE.MeshLambertMaterial({ color: night ? 0x1f2933 : 0x8c8f8a }));
  ground.rotation.x = -Math.PI / 2;
  ground.receiveShadow = true;
  scene.add(ground);
  const sun = new THREE.DirectionalLight(night ? 0xa9c4ff : 0xfff0d8, night ? 2.0 : 2.6);
  sun.position.set(-size * 2, size * 3, -size * 2.5);
  sun.castShadow = true;
  sun.shadow.mapSize.set(2048, 2048);
  const sc = sun.shadow.camera;
  sc.left = sc.bottom = -size * 1.8;
  sc.right = sc.top = size * 1.8;
  sc.far = size * 10;
  scene.add(sun, night ? new THREE.HemisphereLight(0x3a57a8, 0x141c30, 2.2) : new THREE.HemisphereLight(0xbcd0f0, 0x70645a, 1.6));
  const angle = (Number(q.get("angle") ?? 30) * Math.PI) / 180;
  const pitch = (Number(q.get("pitch") ?? 8) * Math.PI) / 180;
  const dist = Number(q.get("dist") ?? 2.6) * size;
  const look = Number(q.get("look") ?? 0.55) * size;
  const poses = (q.get("poses") ?? "b").split(",");
  const cols = Number(q.get("cols") ?? Math.min(poses.length, 6));
  const rows = Math.ceil(poses.length / cols);
  const tw = Math.floor(w / cols);
  const th = Math.floor(h / rows);
  const camera = new THREE.PerspectiveCamera(Number(q.get("fov") ?? 32), tw / th, 0.02 * size, 100 * size);
  // Angle 0 looks at the figure's front (it faces -Z).
  camera.position.set(Math.sin(angle) * Math.cos(pitch) * dist, look + Math.sin(pitch) * dist, -Math.cos(angle) * Math.cos(pitch) * dist);
  camera.lookAt(0, look, 0);
  const id = new Float32Array(BONES * 12);
  for (let b = 0; b < BONES; b++) id[b * 12] = id[b * 12 + 4] = id[b * 12 + 8] = 1;
  renderer.setClearColor(back);
  renderer.clear();
  renderer.setScissorTest(true);
  poses.forEach((full, k) => {
    // `<pose>@<angle>` turns the camera for that tile.
    const [spec, turn] = full.split("@");
    if (turn !== undefined) {
      const t = (Number(turn) * Math.PI) / 180;
      camera.position.set(Math.sin(t) * Math.cos(pitch) * dist, look + Math.sin(pitch) * dist, -Math.cos(t) * Math.cos(pitch) * dist);
      camera.lookAt(0, look, 0);
    }
    const [head, a, b] = spec.split(":");
    let mats: Float32Array = id;
    if (head === "i") mats = sim.magePose(SHOW.FREE, 0, Number(a ?? 0));
    else if (head === "r") mats = sim.magePose(SHOW.FREE, 0, 0, Number(a ?? 0), Number(b ?? 7));
    else if (head === "h") mats = sim.magePose(SHOW.FREE, 0, 0, 0, 14, 1);
    else if (head === "g") mats = sim.magePose(SHOW.GUARD, 0, 0);
    else if (head === "x") mats = sim.magePose(SHOW.HIT, 0, Number(a ?? 4));
    else if (head === "d") mats = sim.magePose(SHOW.DOWN, 0, Number(a ?? 40));
    else if (head.startsWith("m")) mats = sim.magePose(SHOW.MOVE, Number(head.slice(1)), Number(a ?? 0));
    else if (head.startsWith("f")) mats = sim.knightFrame(kind, Number(head.slice(1)));
    skinned.skin(mats);
    const x = (k % cols) * tw;
    const y = h - (Math.floor(k / cols) + 1) * th;
    renderer.setViewport(x, y, tw, th);
    renderer.setScissor(x, y, tw, th);
    renderer.render(scene, camera);
  });
  document.body.classList.add("shot");
  document.title = `shot-ready ${JSON.stringify({ model: name, vertices: model.v.length / 12, triangles: model.i.length / 3, buildMs: Math.round(ms) })}`;
}
