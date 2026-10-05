// Effects in three.js, drawn the way the devices draw them: each layer of an
// effect is a fixed vertex template and eight rows of constants (fx/ir.ts);
// every live instance of the effect is one record (place, age, direction,
// number), and the vertex program places each vertex from the age alone.

import * as THREE from "three";
import { FX_ATLAS, paintFxAtlas } from "../fx/atlas";
import { BOLT_FX, compileEffects } from "../fx/effects";
import { Lowered, TEMPLATE_STRIDE } from "../fx/ir";
import { BOLT_BYTES, BOLTS, FX_BYTES, FX_LIFE, FX_SLOTS, SNAP } from "../sim/abi.gen";
import { Sim } from "../sim/sim";

const CAPACITY = 96;

const HEAD = /* glsl */ `
precision highp float;
uniform vec4 uP[8];
uniform vec3 uCamRight;
uniform vec3 uCamUp;
uniform float uTime;
in vec4 aA;
in vec4 aB;
in vec4 aC;
in vec4 iPosAge;
in vec4 iDirA;
out vec4 vColor;
out vec2 vUv;

/** The effect's frame: forward along its direction, right, and up across both. */
void frame(vec3 f, out vec3 r, out vec3 u) {
  r = normalize(cross(f, vec3(0.0, 1.0, 0.0)) + vec3(1e-4, 0.0, 0.0));
  u = cross(r, f);
}
/** Flat on the ground: forward and right. */
void flatFrame(vec3 f, out vec3 fh, out vec3 rh) {
  fh = normalize(vec3(f.x, 0.0, f.z) + vec3(0.0, 0.0, -1e-4));
  rh = vec3(-fh.z, 0.0, fh.x);
}
`;

const PARTICLES = /* glsl */ `
void main() {
  float a = mix(1.0, iDirA.w, uP[6].x);
  float tsec = (iPosAge.w - aB.x * uP[1].x) * uP[1].w;
  float pl = mix(uP[1].y, uP[1].z, aB.y);
  float t = tsec / pl;
  float alive = step(0.0, t) * step(t, 1.0);
  float v0 = mix(uP[0].x, uP[0].y, aA.w) * a;
  float travel = uP[6].y * a + v0 * tsec * (1.0 - 0.5 * uP[0].w * t);
  vec3 r; vec3 u;
  frame(iDirA.xyz, r, u);
  vec3 d = r * aA.x + u * aA.y + iDirA.xyz * aA.z;
  vec3 world = iPosAge.xyz + d * travel;
  world.y += (uP[6].z - 0.5 * uP[0].z * tsec) * tsec;
  float size = mix(uP[2].x, uP[2].y, t) * (1.0 - uP[2].z * aB.z) * alive * a;
  vec3 vel = d * (v0 * (1.0 - uP[0].w * t)) + vec3(0.0, uP[6].z - uP[0].z * tsec, 0.0);
  float sp = length(vel);
  vec3 axis = vel / max(sp, 1e-3);
  vec3 side = normalize(cross(axis, cameraPosition - world) + vec3(0.0, 1e-4, 0.0));
  vec3 along = side * (aC.x * size) + axis * (aC.y * size * (1.0 + uP[2].w * sp));
  vec3 facing = uCamRight * (aC.x * size) + uCamUp * (aC.y * size);
  world += mix(facing, along, step(0.0005, uP[2].w));
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
  vec4 c = mix(uP[3], uP[4], t);
  c.a *= min(t / max(uP[7].x, 1e-3), 1.0) * pow(max(1.0 - t, 0.0), uP[6].w) * alive;
  vColor = c;
  vUv = mix(uP[5].xy, uP[5].zw, aC.xy * 0.5 + 0.5);
}
`;

const RING = /* glsl */ `
void main() {
  float a = mix(1.0, abs(iDirA.w), uP[2].w);
  float sgn = mix(1.0, sign(iDirA.w), uP[6].z);
  float t = iPosAge.w;
  float te = pow(t, uP[1].w);
  float rad = mix(mix(uP[0].x, uP[0].y, te), mix(uP[0].z, uP[0].w, te), aA.y) * a;
  float th = (aA.x * uP[1].x + uP[1].y + uP[1].z * t) * sgn;
  vec3 r; vec3 u; vec3 fh; vec3 rh;
  frame(iDirA.xyz, r, u);
  flatFrame(iDirA.xyz, fh, rh);
  float mode = uP[6].x;
  // ground: forward and right on the ground. facing: across the direction. swing: forward and a tilted right.
  vec3 axU = mode > 0.5 && mode < 1.5 ? r : fh;
  vec3 axV = mode < 0.5 ? rh : (mode < 1.5 ? u : rh * cos(uP[6].y) + vec3(0.0, sin(uP[6].y) * sgn, 0.0));
  vec3 world = iPosAge.xyz + vec3(0.0, uP[2].z, 0.0) + axU * (cos(th) * rad) + axV * (sin(th) * rad);
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
  vec4 c = mix(uP[3], uP[4], t);
  float along = aA.x * 0.5 + 0.5;
  c.a *= pow(max(1.0 - t, 0.0), uP[2].y) * mix(1.0, sin(along * 3.14159), uP[7].w);
  vColor = c;
  vUv = mix(uP[5].xy, uP[5].zw, vec2(along, aA.y));
}
`;

const RIBBON = /* glsl */ `
void main() {
  float a = mix(1.0, iDirA.w, uP[1].w);
  float t = iPosAge.w;
  float len = mix(uP[0].x, uP[0].y, min(t / max(uP[2].x, 1e-3), 1.0)) * a;
  // Upright strips stand on the point; the others follow the direction, fanned about the vertical.
  vec3 f = mix(iDirA.xyz, vec3(0.0, 1.0, 0.0), uP[6].y);
  float fa = aB.x * uP[1].z;
  f = vec3(f.x * cos(fa) + f.z * sin(fa), f.y, f.z * cos(fa) - f.x * sin(fa));
  vec3 r; vec3 u;
  frame(f, r, u);
  float beat = floor(uTime * uP[1].y + aB.y * 7.0);
  vec2 o = aA.zw * cos(beat * 2.4 + aA.x * 9.0 + aB.y * 20.0);
  float pinned = sin(aA.x * 3.14159);
  vec3 world = iPosAge.xyz + vec3(0.0, uP[6].x, 0.0) + f * (aA.x * len) + (r * o.x + u * o.y) * (uP[1].x * len * pinned);
  vec3 side = normalize(cross(f, world - cameraPosition) + vec3(0.0, 1e-4, 0.0));
  float taper = mix(uP[2].z, 1.0, smoothstep(0.0, 0.15, aA.x)) * mix(uP[2].w, 1.0, smoothstep(1.0, 0.8, aA.x));
  world += side * (aA.y * mix(uP[0].z, uP[0].w, t) * a * taper);
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
  vec4 c = mix(uP[3], uP[4], t);
  c.a *= pow(max(1.0 - t, 0.0), uP[2].y);
  vColor = c;
  vUv = mix(uP[5].xy, uP[5].zw, vec2(aA.x, aA.y * 0.5 + 0.5));
}
`;

const SHELL = /* glsl */ `
void main() {
  float a = mix(1.0, iDirA.w, uP[2].w);
  float t = iPosAge.w;
  float te = pow(t, uP[2].x);
  float rad = mix(uP[0].x, uP[0].y, te) * a;
  float hgt = mix(uP[0].z, uP[0].w, te) * a;
  float ang = uP[1].x + uP[1].y * t;
  float cs = cos(ang);
  float sn = sin(ang);
  vec3 q = vec3(aA.x * cs - aA.z * sn, aA.y, aA.x * sn + aA.z * cs);
  vec3 nq = vec3(aB.x * cs - aB.z * sn, aB.y, aB.x * sn + aB.z * cs);
  vec3 r; vec3 u; vec3 fh; vec3 rh;
  frame(iDirA.xyz, r, u);
  flatFrame(iDirA.xyz, fh, rh);
  // ground: the shell's axis is up and its -z is ahead. facing: its axis is the direction.
  bool facing = uP[6].x > 0.5;
  vec3 X = facing ? r : rh;
  vec3 Y = facing ? iDirA.xyz : vec3(0.0, 1.0, 0.0);
  vec3 Z = facing ? u : -fh;
  vec3 world = iPosAge.xyz + X * (q.x * rad) + Y * (q.y * hgt + uP[2].z) + Z * (q.z * rad) + iDirA.xyz * uP[6].y;
  vec3 nw = X * nq.x + Y * nq.y + Z * nq.z;
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
  float edge = pow(1.0 - abs(dot(normalize(cameraPosition - world), nw)), uP[1].z);
  vec4 c = mix(uP[3], uP[4], t);
  c.a *= pow(max(1.0 - t, 0.0), uP[2].y) * (uP[6].z + uP[1].w * edge);
  vColor = c;
  vUv = mix(uP[5].xy, uP[5].zw, aC.xy);
}
`;

const FRAG = /* glsl */ `
precision highp float;
uniform sampler2D uTex;
in vec4 vColor;
in vec2 vUv;
void main() {
  float k = texture(uTex, vUv).r * vColor.a;
  gl_FragColor = vec4(vColor.rgb * k, k);
  #include <colorspace_fragment>
}
`;

const PROGRAMS = [PARTICLES, RING, RIBBON, SHELL];

interface Kind {
  posAge: Float32Array;
  dirA: Float32Array;
  attrs: [THREE.InstancedBufferAttribute, THREE.InstancedBufferAttribute];
  meshes: THREE.Mesh[];
  count: number;
}

export class FxView {
  readonly group = new THREE.Group();
  private kinds: Kind[] = [];
  private shared = { uCamRight: { value: new THREE.Vector3() }, uCamUp: { value: new THREE.Vector3() }, uTime: { value: 0 } };
  drawn = 0;

  constructor(
    private sim: Sim,
    seed = 2026,
  ) {
    const tex = new THREE.DataTexture(paintFxAtlas(seed), FX_ATLAS, FX_ATLAS, THREE.RedFormat);
    tex.magFilter = THREE.LinearFilter;
    tex.minFilter = THREE.LinearFilter;
    tex.needsUpdate = true;
    const compiled = compileEffects();
    compiled.effects.forEach((layers) => this.kinds.push(this.build(layers, tex)));
  }

  private build(layers: Lowered[], tex: THREE.Texture): Kind {
    const posAge = new Float32Array(CAPACITY * 4);
    const dirA = new Float32Array(CAPACITY * 4);
    const attrs: Kind["attrs"] = [new THREE.InstancedBufferAttribute(posAge, 4).setUsage(THREE.DynamicDrawUsage), new THREE.InstancedBufferAttribute(dirA, 4).setUsage(THREE.DynamicDrawUsage)];
    const meshes = layers.map((l) => {
      const g = new THREE.InstancedBufferGeometry();
      const n = l.vertices.length / TEMPLATE_STRIDE;
      const stream = new THREE.InterleavedBuffer(l.vertices, TEMPLATE_STRIDE);
      g.setAttribute("position", new THREE.BufferAttribute(new Float32Array(n * 3), 3));
      g.setAttribute("aA", new THREE.InterleavedBufferAttribute(stream, 4, 0));
      g.setAttribute("aB", new THREE.InterleavedBufferAttribute(stream, 4, 4));
      g.setAttribute("aC", new THREE.InterleavedBufferAttribute(stream, 4, 8));
      g.setAttribute("iPosAge", attrs[0]);
      g.setAttribute("iDirA", attrs[1]);
      g.setIndex(new THREE.BufferAttribute(l.indices, 1));
      const rows = Array.from({ length: 8 }, (_, k) => new THREE.Vector4(l.rows[k * 4], l.rows[k * 4 + 1], l.rows[k * 4 + 2], l.rows[k * 4 + 3]));
      const material = new THREE.ShaderMaterial({
        vertexShader: HEAD + PROGRAMS[l.program],
        fragmentShader: FRAG,
        uniforms: { ...this.shared, uP: { value: rows }, uTex: { value: tex } },
        transparent: true,
        depthWrite: false,
        side: THREE.DoubleSide,
        blending: THREE.CustomBlending,
        blendSrc: THREE.OneFactor,
        // Add: light on what is behind. Over: cover it by the layer's alpha.
        blendDst: l.blend === 0 ? THREE.OneFactor : THREE.OneMinusSrcAlphaFactor,
      });
      const mesh = new THREE.Mesh(g, material);
      mesh.frustumCulled = false;
      mesh.matrixAutoUpdate = false;
      mesh.renderOrder = l.blend === 0 ? 20 : 10;
      this.group.add(mesh);
      return mesh;
    });
    return { posAge, dirA, attrs, meshes, count: 0 };
  }

  update(s: Float32Array, camera: THREE.Camera) {
    const tick = s[SNAP.TICK];
    this.shared.uTime.value = tick / 60;
    const e = camera.matrixWorld.elements;
    this.shared.uCamRight.value.set(e[0], e[1], e[2]);
    this.shared.uCamUp.value.set(e[4], e[5], e[6]);
    for (const k of this.kinds) k.count = 0;
    const fx = this.sim.fx();
    for (let slot = 0; slot < FX_SLOTS; slot++) {
      const o = slot * FX_BYTES;
      const kind = fx.getUint8(o + 36);
      if (kind === 0) continue;
      const age = (tick - fx.getUint32(o + 28, true)) / FX_LIFE[kind];
      const k = this.kinds[kind];
      if (age < 0 || age >= 1 || k.count >= CAPACITY) continue;
      const at = k.count++ * 4;
      k.posAge.set([fx.getFloat32(o, true), fx.getFloat32(o + 4, true), fx.getFloat32(o + 8, true), age], at);
      k.dirA.set([fx.getFloat32(o + 12, true), fx.getFloat32(o + 16, true), fx.getFloat32(o + 20, true), fx.getFloat32(o + 24, true)], at);
    }
    // The bolts in flight: one instance each, pointing back along its path.
    const bolts = this.sim.bolts();
    const bk = this.kinds[BOLT_FX];
    for (let b = 0; b < BOLTS; b++) {
      const o = b * BOLT_BYTES;
      if (bolts.getUint32(o + 40, true) === 0) continue;
      const v = [bolts.getFloat32(o + 12, true), bolts.getFloat32(o + 16, true), bolts.getFloat32(o + 20, true)];
      const l = Math.hypot(v[0], v[1], v[2]) || 1;
      const at = bk.count++ * 4;
      bk.posAge.set([bolts.getFloat32(o, true), bolts.getFloat32(o + 4, true), bolts.getFloat32(o + 8, true), 0.3], at);
      bk.dirA.set([-v[0] / l, -v[1] / l, -v[2] / l, 1], at);
    }
    this.drawn = 0;
    for (const k of this.kinds) {
      k.attrs[0].needsUpdate = true;
      k.attrs[1].needsUpdate = true;
      for (const m of k.meshes) {
        (m.geometry as THREE.InstancedBufferGeometry).instanceCount = k.count;
        m.visible = k.count > 0;
      }
      this.drawn += k.count;
    }
  }
}
