// Skinned models: the mage and the demon (vita/shaders/skin_v.cg). Two bones
// per vertex; `bones` holds three rows per bone, taking a bind-pose position
// to world space. The light is the look of the animation at night: a lit
// tone and a shaded tone that meet at a narrow edge, a rim of moonlight, and
// what the spells cast.

struct Bones {
  rows: array<vec4<f32>, 84>,
}
@group(1) @binding(0) var<uniform> bones: Bones;

struct SkinIn {
  @location(0) position: vec3<f32>,
  @location(1) normal: vec4<f32>,
  @location(2) color: vec4<f32>,
  @location(3) bones: vec2<u32>,
  @location(4) weights: vec2<f32>,
}

struct SkinOut {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
  @location(1) fog: f32,
}

@vertex
fn skin(in: SkinIn) -> SkinOut {
  var out: SkinOut;
  let p = vec4<f32>(in.position, 1.0);
  var world = vec3<f32>(0.0);
  var n = vec3<f32>(0.0);
  for (var k = 0; k < 2; k++) {
    let j = in.bones[k] * 3u;
    let w = in.weights[k];
    let r0 = bones.rows[j];
    let r1 = bones.rows[j + 1u];
    let r2 = bones.rows[j + 2u];
    world += vec3<f32>(dot(r0, p), dot(r1, p), dot(r2, p)) * w;
    n += vec3<f32>(dot(r0.xyz, in.normal.xyz), dot(r1.xyz, in.normal.xyz), dot(r2.xyz, in.normal.xyz)) * w;
  }
  n = normalize(n);
  let clip = vec4<f32>(world, 1.0) * g.vp;
  out.position = clip;

  let to_eye = normalize(g.eye.xyz - world);
  let facing = dot(n, g.light[0].xyz);
  let lit = smoothstep(-0.04, 0.1, facing) * g.light[0].w;
  var light = g.light[2].xyz * 2.1 + g.light[3].xyz + g.light[1].xyz * (1.15 * lit);
  for (var i = 0; i < 4; i++) {
    let d = g.spells[i * 2].xyz - world;
    let att = max(1.0 - dot(d, d) * g.spells[i * 2].w, 0.0);
    light += g.spells[i * 2 + 1].xyz * (att * att * (0.36 + 0.84 * max(dot(n, normalize(d)), 0.0)));
  }
  let rim = pow(1.0 - max(dot(n, to_eye), 0.0), 3.5) * smoothstep(-0.2, 0.5, facing);
  let gleam = encode(g.light[1].xyz * (rim * 0.55));
  out.color = vec4<f32>(in.color.rgb * encode(light) + gleam * 0.7, 1.0);
  out.fog = haze(clip.w);
  return out;
}

@fragment
fn tint(in: SkinOut) -> @location(0) vec4<f32> {
  return vec4<f32>(mix(in.color.rgb, g.fog.rgb, in.fog), in.color.a);
}
