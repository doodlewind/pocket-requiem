// Post-processing (vita/shaders/post_v.cg and the fragment programs beside
// it). Every texture coordinate a fragment program reads is an affine
// function of the screen position, so it is computed in the vertex stage:
// taps[k] holds a scale (xy) and an offset (zw) per tap. A blur shifts the
// taps by texels; a radial effect scales them about a point.
//
// bright: a.x the threshold, a.y the slope above it.
// compose: a = x bloom gain, y shaft gain, z vignette, w speed blur;
//          b = x contrast, y saturation, z warmth of the highlights, w coolness of the shadows.

struct Pass {
  taps: array<vec4<f32>, 8>,
  a: vec4<f32>,
  b: vec4<f32>,
}
@group(0) @binding(0) var<uniform> pass_: Pass;
@group(0) @binding(1) var first: texture_2d<f32>;
@group(0) @binding(2) var second: texture_2d<f32>;
@group(0) @binding(3) var third: texture_2d<f32>;
@group(0) @binding(4) var smoothed: sampler;

struct PostOut {
  @builtin(position) position: vec4<f32>,
  @location(0) uv0: vec2<f32>,
  @location(1) uv1: vec2<f32>,
  @location(2) uv2: vec2<f32>,
  @location(3) uv3: vec2<f32>,
  @location(4) uv4: vec2<f32>,
  @location(5) uv5: vec2<f32>,
  @location(6) uv6: vec2<f32>,
  @location(7) uv7: vec2<f32>,
}

// One triangle over the whole target.
@vertex
fn whole(@builtin(vertex_index) index: u32) -> PostOut {
  var out: PostOut;
  let p = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
  out.position = vec4<f32>(p, 0.0, 1.0);
  let uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
  out.uv0 = uv * pass_.taps[0].xy + pass_.taps[0].zw;
  out.uv1 = uv * pass_.taps[1].xy + pass_.taps[1].zw;
  out.uv2 = uv * pass_.taps[2].xy + pass_.taps[2].zw;
  out.uv3 = uv * pass_.taps[3].xy + pass_.taps[3].zw;
  out.uv4 = uv * pass_.taps[4].xy + pass_.taps[4].zw;
  out.uv5 = uv * pass_.taps[5].xy + pass_.taps[5].zw;
  out.uv6 = uv * pass_.taps[6].xy + pass_.taps[6].zw;
  out.uv7 = uv * pass_.taps[7].xy + pass_.taps[7].zw;
  return out;
}

fn texel(uv: vec2<f32>) -> vec3<f32> {
  return textureSample(first, smoothed, uv).rgb;
}

// Quarter-size copy of the scene keeping what is bright: four bilinear taps cover a 4 x 4 block.
@fragment
fn bright(in: PostOut) -> @location(0) vec4<f32> {
  let c = (texel(in.uv0) + texel(in.uv1) + texel(in.uv2) + texel(in.uv3)) * 0.25;
  let l = dot(c, vec3<f32>(0.30, 0.59, 0.11));
  let k = saturate((l - pass_.a.x) * pass_.a.y);
  return vec4<f32>(c * k, 1.0);
}

// One direction of a Gaussian blur: five bilinear taps stand for nine texels.
@fragment
fn blur(in: PostOut) -> @location(0) vec4<f32> {
  let c = texel(in.uv0) * 0.2270 + (texel(in.uv1) + texel(in.uv2)) * 0.3162 + (texel(in.uv3) + texel(in.uv4)) * 0.0703;
  return vec4<f32>(c, 1.0);
}

// Light shafts: the bright image smeared toward the moon. The eight taps step from the pixel toward the
// moon's place on the screen, fading.
@fragment
fn rays(in: PostOut) -> @location(0) vec4<f32> {
  let c = texel(in.uv0) * 0.20 + texel(in.uv1) * 0.17 + texel(in.uv2) * 0.15 + texel(in.uv3) * 0.13 + texel(in.uv4) * 0.11 + texel(in.uv5) * 0.09 + texel(in.uv6) * 0.08 + texel(in.uv7) * 0.07;
  return vec4<f32>(c, 1.0);
}

// The frame as shown: the scene, plus bloom and light shafts, graded. Taps: 0 the scene; 1 the screen as
// -1..1 for the vignette; 2..4 the scene scaled about the centre, for the blur at speed.
@fragment
fn compose(in: PostOut) -> @location(0) vec4<f32> {
  var c = texel(in.uv0);
  let edge = dot(in.uv1, in.uv1);
  // Streak toward the centre, more at the rim: the middle of the frame stays sharp.
  let smear = (c + texel(in.uv2) + texel(in.uv3) + texel(in.uv4)) * 0.25;
  c = mix(c, smear, saturate(edge * pass_.a.w));
  let lit = textureSample(second, smoothed, in.uv0).rgb * pass_.a.x + textureSample(third, smoothed, in.uv0).rgb * pass_.a.y;
  // Screen: light adds without clipping what is already bright.
  c = c + lit * (vec3<f32>(1.0) - c);
  let l = dot(c, vec3<f32>(0.30, 0.59, 0.11));
  c = mix(vec3<f32>(l), c, pass_.b.y);
  c = (c - 0.5) * pass_.b.x + 0.5;
  // Split tone: warm where it is light, cool where it is dark.
  let warm = vec3<f32>(1.0 + pass_.b.z, 1.0, 1.0 - pass_.b.z);
  let cool = vec3<f32>(1.0 - pass_.b.w, 1.0, 1.0 + pass_.b.w);
  c = c * mix(cool, warm, saturate(l * 1.4));
  c = c * (1.0 - edge * pass_.a.z);
  return vec4<f32>(c, 1.0);
}
