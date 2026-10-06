// What every program of the scene reads, written once a frame. A matrix is
// stored row by row, so a program multiplies with the vector on the left.
//
// light: 0 the moon's direction (w: how much of it reaches a figure), 1 its colour, 2 sky, 3 ground bounce.
// spells: per light a spell casts, its place with 1 / radius² in w, then its colour.
// eye: the eye, with the haze density in w. fog: the haze's colour, encoded.
// cam: 0 the eye's right and the time in seconds, 1 its up, 2 the eye.
// consts: x metres a stored frame's full range stands for, y the stored u's scale, z the stored colour's.
struct Globals {
  vp: mat4x4<f32>,
  sky_vp: mat4x4<f32>,
  light: array<vec4<f32>, 4>,
  spells: array<vec4<f32>, 8>,
  eye: vec4<f32>,
  fog: vec4<f32>,
  cam: array<vec4<f32>, 3>,
  consts: vec4<f32>,
}
@group(0) @binding(0) var<uniform> g: Globals;

// Haze is a function of view depth, computed per vertex.
fn haze(depth: f32) -> f32 {
  let d = depth * g.eye.w;
  return 1.0 - exp(-d * d);
}

fn encode(light: vec3<f32>) -> vec3<f32> {
  return pow(max(light, vec3<f32>(0.0001)), vec3<f32>(0.4545));
}
