// Effects in three.js. For now: a mark at each live effect and each bolt.

import * as THREE from "three";
import { BOLT_BYTES, BOLTS, FX_BYTES, FX_LIFE, FX_SLOTS, SNAP } from "../sim/abi.gen";
import { Sim } from "../sim/sim";

export class FxView {
  readonly group = new THREE.Group();
  private points: THREE.Points;
  private pos: Float32Array;
  private col: Float32Array;

  constructor(private sim: Sim) {
    const n = FX_SLOTS + BOLTS;
    this.pos = new Float32Array(n * 3);
    this.col = new Float32Array(n * 3);
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.BufferAttribute(this.pos, 3));
    g.setAttribute("color", new THREE.BufferAttribute(this.col, 3));
    this.points = new THREE.Points(g, new THREE.PointsMaterial({ size: 0.9, vertexColors: true, transparent: true, blending: THREE.AdditiveBlending, depthWrite: false, fog: false }));
    this.points.frustumCulled = false;
    this.group.add(this.points);
  }

  update(s: Float32Array, _camera: THREE.Camera) {
    const tick = s[SNAP.TICK];
    const fx = this.sim.fx();
    let n = 0;
    for (let k = 0; k < FX_SLOTS; k++) {
      const o = k * FX_BYTES;
      const kind = fx.getUint8(o + 36);
      if (kind === 0) continue;
      const age = (tick - fx.getUint32(o + 28, true)) / FX_LIFE[kind];
      if (age < 0 || age >= 1) continue;
      this.pos.set([fx.getFloat32(o, true), fx.getFloat32(o + 4, true), fx.getFloat32(o + 8, true)], n * 3);
      const f = 1 - age;
      this.col.set([0.6 * f, 0.8 * f, 1.0 * f], n * 3);
      n++;
    }
    const bolts = this.sim.bolts();
    for (let k = 0; k < BOLTS; k++) {
      const o = k * BOLT_BYTES;
      if (bolts.getUint32(o + 40, true) === 0) continue;
      this.pos.set([bolts.getFloat32(o, true), bolts.getFloat32(o + 4, true), bolts.getFloat32(o + 8, true)], n * 3);
      this.col.set([1, 1, 1], n * 3);
      n++;
    }
    const g = this.points.geometry;
    g.setDrawRange(0, n);
    g.getAttribute("position").needsUpdate = true;
    g.getAttribute("color").needsUpdate = true;
  }
}
