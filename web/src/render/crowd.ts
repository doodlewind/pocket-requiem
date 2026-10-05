// The army in three.js, drawn the way the devices draw it: no skeleton at
// play time. Each kind of knight has its clips baked into frames (every
// vertex placed for every stored frame, in a texture); a knight is an
// instance that names two frames and a blend between them.

import * as THREE from "three";
import { SkinModel, SKIN_STRIDE } from "../model/sdf";
import { DRAW_BYTES, KNIGHT_FRAMES } from "../sim/abi.gen";
import { CrowdView as CrowdData, Sim } from "../sim/sim";
import { LIGHT_GLSL, lightUniforms } from "./light";

const CAPACITY = 4096;
const TEX_W = 2048;

const VERT = /* glsl */ `
precision highp float;
precision highp int;
uniform sampler2D uPos;
uniform sampler2D uNrm;
uniform int uVerts;
in vec3 color;
in vec4 iPosYaw;
in vec4 iAnim;
in float iFlash;
out vec3 vColor;
out vec3 vNormal;
out vec3 vWorld;
out float vFlash;

vec3 frame(sampler2D t, float f) {
  int idx = int(f) * uVerts + gl_VertexID;
  return texelFetch(t, ivec2(idx % ${TEX_W}, idx / ${TEX_W}), 0).xyz;
}

void main() {
  vec3 p = mix(frame(uPos, iAnim.x), frame(uPos, iAnim.y), iAnim.z) * iAnim.w;
  vec3 n = normalize(mix(frame(uNrm, iAnim.x), frame(uNrm, iAnim.y), iAnim.z));
  // Heading: yaw 0 faces -Z, positive turns left.
  float c = cos(iPosYaw.w);
  float s = sin(iPosYaw.w);
  vec3 world = vec3(p.x * c + p.z * s, p.y, -p.x * s + p.z * c) + iPosYaw.xyz;
  vNormal = vec3(n.x * c + n.z * s, n.y, -n.x * s + n.z * c);
  vWorld = world;
  vColor = color;
  vFlash = iFlash;
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
}
`;

const FRAG = /* glsl */ `
precision highp float;
${LIGHT_GLSL}
in vec3 vColor;
in vec3 vNormal;
in vec3 vWorld;
in float vFlash;


void main() {
  vec3 n = normalize(vNormal);
  vec3 toEye = normalize(cameraPosition - vWorld);
  float ndl = max(dot(n, uMoonDir), 0.0);
  vec3 light = uMoon * ndl + hemisphere(n) + castLight(vWorld, n);
  // Plate: a highlight off the moon and a rim against the sky.
  vec3 h = normalize(uMoonDir + toEye);
  float metal = smoothstep(0.25, 0.5, max(vColor.r, max(vColor.g, vColor.b)));
  float spec = pow(max(dot(n, h), 0.0), 36.0) * metal;
  float rim = pow(1.0 - max(dot(n, toEye), 0.0), 3.0) * (0.35 + 0.65 * max(dot(n, uMoonDir) * 0.5 + 0.5, 0.0));
  vec3 c = vColor * light + uMoon * (spec * 0.9 + rim * 0.22 * metal) + uSky * rim * 0.5;
  c += vec3(0.8, 0.9, 1.0) * vFlash * 0.7;
  c = haze(c, length(cameraPosition - vWorld));
  gl_FragColor = vec4(c, 1.0);
  #include <colorspace_fragment>
}
`;

interface Batch {
  mesh: THREE.Mesh;
  posYaw: Float32Array;
  anim: Float32Array;
  flash: Float32Array;
  count: number;
  triangles: number;
}

const toLinear = (c: number) => Math.pow(c, 2.2);

export class CrowdView {
  readonly group = new THREE.Group();
  private batches: Batch[][] = [];
  triangles = 0;
  shown = 0;

  /** `models[kind][lod]`; a knight nearer than `lodNear` metres draws lod 0. */
  constructor(
    sim: Sim,
    models: SkinModel[][],
    private lodNear: number,
  ) {
    models.forEach((lods, kind) => {
      this.batches.push(lods.map((m) => this.bake(sim, kind + 1, m)));
    });
  }

  private bake(sim: Sim, figure: number, model: SkinModel): Batch {
    const verts = model.v.length / SKIN_STRIDE;
    const texels = verts * KNIGHT_FRAMES;
    const h = Math.ceil(texels / TEX_W);
    const pos = new Float32Array(TEX_W * h * 4);
    const nrm = new Float32Array(TEX_W * h * 4);
    const v = model.v;
    for (let f = 0; f < KNIGHT_FRAMES; f++) {
      const m = sim.knightFrame(figure, f);
      for (let i = 0; i < verts; i++) {
        const o = i * SKIN_STRIDE;
        const a = v[o + 9] * 12;
        const b = v[o + 10] * 12;
        const wa = v[o + 11];
        const wb = 1 - wa;
        const at = (f * verts + i) * 4;
        for (let c = 0; c < 3; c++) {
          pos[at + c] = (m[a + c] * v[o] + m[a + 3 + c] * v[o + 1] + m[a + 6 + c] * v[o + 2] + m[a + 9 + c]) * wa + (m[b + c] * v[o] + m[b + 3 + c] * v[o + 1] + m[b + 6 + c] * v[o + 2] + m[b + 9 + c]) * wb;
          nrm[at + c] = (m[a + c] * v[o + 3] + m[a + 3 + c] * v[o + 4] + m[a + 6 + c] * v[o + 5]) * wa + (m[b + c] * v[o + 3] + m[b + 3 + c] * v[o + 4] + m[b + 6 + c] * v[o + 5]) * wb;
        }
      }
    }
    const tex = (data: Float32Array) => {
      const t = new THREE.DataTexture(data, TEX_W, h, THREE.RGBAFormat, THREE.FloatType);
      t.minFilter = t.magFilter = THREE.NearestFilter;
      t.needsUpdate = true;
      return t;
    };
    const g = new THREE.InstancedBufferGeometry();
    const col = new Float32Array(verts * 3);
    const dummy = new Float32Array(verts * 3);
    for (let i = 0; i < verts; i++) for (let c = 0; c < 3; c++) col[i * 3 + c] = toLinear(v[i * SKIN_STRIDE + 6 + c]);
    g.setAttribute("position", new THREE.BufferAttribute(dummy, 3));
    g.setAttribute("color", new THREE.BufferAttribute(col, 3));
    g.setIndex(new THREE.BufferAttribute(model.i, 1));
    const posYaw = new Float32Array(CAPACITY * 4);
    const anim = new Float32Array(CAPACITY * 4);
    const flash = new Float32Array(CAPACITY);
    g.setAttribute("iPosYaw", new THREE.InstancedBufferAttribute(posYaw, 4).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute("iAnim", new THREE.InstancedBufferAttribute(anim, 4).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute("iFlash", new THREE.InstancedBufferAttribute(flash, 1).setUsage(THREE.DynamicDrawUsage));
    const material = new THREE.ShaderMaterial({ vertexShader: VERT, fragmentShader: FRAG, uniforms: { ...lightUniforms, uPos: { value: tex(pos) }, uNrm: { value: tex(nrm) }, uVerts: { value: verts } } });
    const mesh = new THREE.Mesh(g, material);
    mesh.frustumCulled = false;
    mesh.matrixAutoUpdate = false;
    this.group.add(mesh);
    return { mesh, posYaw, anim, flash, count: 0, triangles: model.i.length / 3 };
  }

  /** Fills the instances from the simulation's draw list. */
  update(view: CrowdData) {
    for (const lods of this.batches) for (const b of lods) b.count = 0;
    const near2 = this.lodNear * this.lodNear;
    const words = DRAW_BYTES / 4;
    for (let i = 0; i < view.count; i++) {
      const f = i * words;
      const kind = view.u8[i * DRAW_BYTES + 36];
      const lods = this.batches[Math.min(kind, this.batches.length - 1)];
      const b = lods[view.f[f + 7] < near2 ? 0 : lods.length - 1];
      if (b.count >= CAPACITY) continue;
      const k = b.count++;
      b.posYaw.set([view.f[f], view.f[f + 1], view.f[f + 2], view.f[f + 3]], k * 4);
      b.anim.set([view.u16[i * (DRAW_BYTES / 2) + 8], view.u16[i * (DRAW_BYTES / 2) + 9], view.f[f + 5], view.f[f + 6]], k * 4);
      b.flash[k] = view.f[f + 8];
    }
    this.triangles = 0;
    this.shown = view.count;
    for (const lods of this.batches) {
      for (const b of lods) {
        const g = b.mesh.geometry as THREE.InstancedBufferGeometry;
        g.instanceCount = b.count;
        b.mesh.visible = b.count > 0;
        for (const name of ["iPosYaw", "iAnim", "iFlash"]) (g.getAttribute(name) as THREE.InstancedBufferAttribute).needsUpdate = true;
        this.triangles += b.count * b.triangles;
      }
    }
  }
}
