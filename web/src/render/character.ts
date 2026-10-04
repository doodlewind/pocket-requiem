// The mage's material: the look of the animation at night. Two tones split by
// the moon's direction, a rim of moonlight, and whatever the spells cast.

import * as THREE from "three";
import { LIGHT_GLSL, lightUniforms } from "./light";

const VERT = /* glsl */ `
in vec3 color;
out vec3 vColor;
out vec3 vNormal;
out vec3 vWorld;
void main() {
  vColor = color;
  vNormal = normal;
  vWorld = position;
  gl_Position = projectionMatrix * viewMatrix * vec4(position, 1.0);
}
`;

const FRAG = /* glsl */ `
precision highp float;
${LIGHT_GLSL}
uniform float uGlow;
in vec3 vColor;
in vec3 vNormal;
in vec3 vWorld;

void main() {
  vec3 n = normalize(vNormal);
  vec3 toEye = normalize(cameraPosition - vWorld);
  // The lit side and the shaded side meet at a narrow edge, as drawn.
  float lit = smoothstep(-0.04, 0.1, dot(n, uMoonDir));
  vec3 shade = uSky * 2.1 + uBounce;
  vec3 light = mix(shade, shade + uMoon * 1.15, lit) + castLight(vWorld, n) * 1.2;
  float rim = pow(1.0 - max(dot(n, toEye), 0.0), 3.5) * smoothstep(-0.2, 0.5, dot(n, uMoonDir));
  vec3 c = vColor * light + uMoon * rim * 0.55;
  c += vColor * uGlow;
  c = haze(c, length(cameraPosition - vWorld));
  gl_FragColor = vec4(c, 1.0);
  #include <colorspace_fragment>
}
`;

export function characterMaterial(): THREE.ShaderMaterial {
  return new THREE.ShaderMaterial({ vertexShader: VERT, fragmentShader: FRAG, uniforms: { ...lightUniforms, uGlow: { value: 0 } } });
}
