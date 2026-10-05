// The stage's light as GLSL, for the reference's own materials (the crowd,
// the mage, the effects): the moon, a hemisphere of sky and ground, and the
// four strongest lights the spells cast. The devices compute the same sum.

import * as THREE from "three";
import { LIGHTS, SNAP } from "../sim/abi.gen";
import { SCENE } from "../world/scene";

export const lightUniforms = {
  uMoonDir: { value: new THREE.Vector3(...SCENE.sunDir) },
  uMoon: { value: new THREE.Vector3(...SCENE.sun) },
  uSky: { value: new THREE.Vector3(...SCENE.sky) },
  uBounce: { value: new THREE.Vector3(...SCENE.bounce) },
  uFogColor: { value: new THREE.Vector3(...SCENE.fog) },
  uFogDensity: { value: SCENE.fogDensity },
  uLightPos: { value: Array.from({ length: LIGHTS }, () => new THREE.Vector4(0, 0, 0, 1)) },
  uLightColor: { value: Array.from({ length: LIGHTS }, () => new THREE.Vector3()) },
};

/** Copies the snapshot's lights into the shared uniforms. */
export function setLights(s: Float32Array) {
  for (let k = 0; k < LIGHTS; k++) {
    const o = SNAP.LIGHTS + k * 8;
    lightUniforms.uLightPos.value[k].set(s[o], s[o + 1], s[o + 2], Math.max(s[o + 3], 0.01));
    lightUniforms.uLightColor.value[k].set(s[o + 4], s[o + 5], s[o + 6]);
  }
}

export const LIGHT_GLSL = /* glsl */ `
uniform vec3 uMoonDir;
uniform vec3 uMoon;
uniform vec3 uSky;
uniform vec3 uBounce;
uniform vec3 uFogColor;
uniform float uFogDensity;
uniform vec4 uLightPos[${LIGHTS}];
uniform vec3 uLightColor[${LIGHTS}];

/** What the spells cast on a point with normal n. */
vec3 castLight(vec3 world, vec3 n) {
  vec3 sum = vec3(0.0);
  for (int i = 0; i < ${LIGHTS}; i++) {
    vec3 d = uLightPos[i].xyz - world;
    float r = uLightPos[i].w;
    float att = max(1.0 - dot(d, d) / (r * r), 0.0);
    sum += uLightColor[i] * (att * att * (0.3 + 0.7 * max(dot(n, normalize(d)), 0.0)));
  }
  return sum;
}
vec3 hemisphere(vec3 n) {
  return mix(uBounce, uSky, 0.5 + 0.5 * n.y);
}
vec3 haze(vec3 color, float depth) {
  float d = depth * uFogDensity;
  return mix(color, uFogColor, 1.0 - exp(-d * d));
}
`;
