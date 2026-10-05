// The night sky: a gradient from the horizon to the zenith with a glow
// around the moon, sparse stars, and the moon's disc drawn large. The device
// draws the same function per vertex of a coarse dome.

import * as THREE from "three";
import { hash01 } from "../world/rng";
import { SCENE } from "../world/scene";

/** Sky radiance (linear RGB) toward the unit direction `d`. */
export function skyColor(d: readonly [number, number, number]): [number, number, number] {
  const h = Math.max(d[1], 0);
  const k = 1 - Math.pow(1 - h, 2.4);
  const s = Math.max(d[0] * SCENE.sunDir[0] + d[1] * SCENE.sunDir[1] + d[2] * SCENE.sunDir[2], 0);
  const glow = 0.1 * Math.pow(s, 5) + 0.5 * Math.pow(s, 60);
  const below = Math.min(Math.max(-d[1] * 6, 0), 1);
  const out: [number, number, number] = [0, 0, 0];
  for (let i = 0; i < 3; i++) {
    const sky = SCENE.horizon[i] + (SCENE.zenith[i] - SCENE.horizon[i]) * k + SCENE.glow[i] * glow;
    out[i] = sky + (SCENE.fog[i] - sky) * below;
  }
  return out;
}

/** The stars: unit directions and a brightness each, the same list on every machine. */
export function stars(count: number): Float32Array {
  const out = new Float32Array(count * 4);
  for (let i = 0; i < count; i++) {
    const u = hash01(i, 1, 7);
    const v = hash01(i, 2, 7);
    const y = 0.06 + 0.94 * u;
    const r = Math.sqrt(1 - y * y);
    const a = v * Math.PI * 2;
    out.set([Math.cos(a) * r, y, Math.sin(a) * r, 0.25 + 0.75 * hash01(i, 3, 7) ** 3], i * 4);
  }
  return out;
}

export function makeSky(): THREE.Group {
  const group = new THREE.Group();
  const geo = new THREE.SphereGeometry(1, 48, 24);
  const pos = geo.getAttribute("position");
  const col = new Float32Array(pos.count * 3);
  for (let i = 0; i < pos.count; i++) col.set(skyColor([pos.getX(i), pos.getY(i), pos.getZ(i)]), i * 3);
  geo.setAttribute("color", new THREE.BufferAttribute(col, 3));
  const dome = new THREE.Mesh(geo, new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.BackSide, depthWrite: false, depthTest: false, fog: false }));
  dome.renderOrder = -10;
  dome.frustumCulled = false;
  dome.scale.setScalar(1000);
  group.add(dome);

  const list = stars(700);
  const sp = new Float32Array(700 * 3);
  const sc = new Float32Array(700 * 3);
  for (let i = 0; i < 700; i++) {
    sp.set([list[i * 4] * 990, list[i * 4 + 1] * 990, list[i * 4 + 2] * 990], i * 3);
    const b = list[i * 4 + 3];
    sc.set([0.75 * b, 0.85 * b, b], i * 3);
  }
  const sg = new THREE.BufferGeometry();
  sg.setAttribute("position", new THREE.BufferAttribute(sp, 3));
  sg.setAttribute("color", new THREE.BufferAttribute(sc, 3));
  const points = new THREE.Points(sg, new THREE.PointsMaterial({ size: 2.2, sizeAttenuation: false, vertexColors: true, depthWrite: false, depthTest: false, fog: false, transparent: true, blending: THREE.AdditiveBlending }));
  points.renderOrder = -9;
  points.frustumCulled = false;
  group.add(points);

  // The moon: a disc and a halo, facing the eye.
  const d = new THREE.Vector3(...SCENE.sunDir);
  const disc = (radius: number, color: THREE.Color, opacity: number, soft: boolean) => {
    const g = new THREE.CircleGeometry(radius * 980, 40);
    if (soft) {
      const n = g.getAttribute("position").count;
      const c = new Float32Array(n * 4);
      for (let i = 0; i < n; i++) c.set([color.r, color.g, color.b, i === 0 ? opacity : 0], i * 4);
      g.setAttribute("color", new THREE.BufferAttribute(c, 4));
    }
    const m = new THREE.Mesh(g, soft ? new THREE.MeshBasicMaterial({ vertexColors: true, transparent: true, depthWrite: false, depthTest: false, fog: false, blending: THREE.AdditiveBlending }) : new THREE.MeshBasicMaterial({ color, depthWrite: false, depthTest: false, fog: false }));
    m.position.copy(d).multiplyScalar(980);
    m.lookAt(0, 0, 0);
    m.renderOrder = soft ? -8 : -7;
    m.frustumCulled = false;
    group.add(m);
  };
  disc(SCENE.moonRadius * 3.2, new THREE.Color().setRGB(0.2, 0.34, 0.6), 0.9, true);
  disc(SCENE.moonRadius, new THREE.Color().setRGB(...SCENE.moon), 1, false);
  // Seas on the disc, so it reads as a moon.
  for (const [x, y, r, k] of [
    [-0.3, 0.25, 0.26, 0.86],
    [0.2, 0.3, 0.2, 0.9],
    [0.05, -0.2, 0.3, 0.88],
    [-0.4, -0.3, 0.14, 0.9],
    [0.42, -0.1, 0.12, 0.92],
  ]) {
    const g = new THREE.CircleGeometry(SCENE.moonRadius * r * 980, 20);
    const m = new THREE.Mesh(g, new THREE.MeshBasicMaterial({ color: new THREE.Color().setRGB(SCENE.moon[0] * k * 0.92, SCENE.moon[1] * k * 0.94, SCENE.moon[2] * k), depthWrite: false, depthTest: false, fog: false }));
    const right = new THREE.Vector3().crossVectors(d, new THREE.Vector3(0, 1, 0)).normalize();
    const up = new THREE.Vector3().crossVectors(right, d);
    m.position.copy(d).multiplyScalar(979).addScaledVector(right, x * SCENE.moonRadius * 980).addScaledVector(up, y * SCENE.moonRadius * 980);
    m.lookAt(0, 0, 0);
    m.renderOrder = -6;
    m.frustumCulled = false;
    group.add(m);
  }
  return group;
}
