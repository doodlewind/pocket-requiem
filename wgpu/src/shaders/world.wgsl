// The static world (vita/shaders/world_v.cg, world_lit_v.cg, world_f.cg).
// Positions arrive as 16 bits over the mesh's bounds; the bounds are one
// record a mesh, read at the rate of instances. Atlas texel × baked light,
// then haze. `lit` is the mesh within reach of a spell's light: the bake
// stores tint × light, encoded, and a light that adds `d` to a baked light
// of about LIGHT_REF multiplies the encoded colour by sqrt(1 + d / LIGHT_REF).

@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

const LIGHT_GAIN = vec3<f32>(4.0, 2.8, 1.43);

struct WorldIn {
  @location(0) position: vec4<u32>,
  @location(1) uv: vec2<i32>,
  @location(2) color: vec4<f32>,
  @location(3) least: vec3<f32>,
  @location(4) size: vec3<f32>,
}

struct WorldOut {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) color: vec4<f32>,
}

fn place(in: WorldIn) -> vec3<f32> {
  return vec3<f32>(in.position.xyz) / 65535.0 * in.size + in.least;
}

@vertex
fn plain(in: WorldIn) -> WorldOut {
  var out: WorldOut;
  let clip = vec4<f32>(place(in), 1.0) * g.vp;
  out.position = clip;
  out.uv = vec2<f32>(in.uv) / 32767.0 * vec2<f32>(g.consts.y, 1.0);
  out.color = vec4<f32>(in.color.rgb * g.consts.z, haze(clip.w));
  return out;
}

@vertex
fn lit(in: WorldIn) -> WorldOut {
  var out: WorldOut;
  let world = place(in);
  let clip = vec4<f32>(world, 1.0) * g.vp;
  out.position = clip;
  out.uv = vec2<f32>(in.uv) / 32767.0 * vec2<f32>(g.consts.y, 1.0);
  var glow = vec3<f32>(0.0);
  for (var k = 0; k < 4; k++) {
    let d = g.spells[k * 2].xyz - world;
    let att = max(1.0 - dot(d, d) * g.spells[k * 2].w, 0.0);
    glow += g.spells[k * 2 + 1].xyz * (att * att);
  }
  let gain = sqrt(vec3<f32>(1.0) + glow * LIGHT_GAIN);
  out.color = vec4<f32>(in.color.rgb * g.consts.z * gain, haze(clip.w));
  return out;
}

@fragment
fn shade(in: WorldOut) -> @location(0) vec4<f32> {
  let c = textureSample(atlas, atlas_sampler, in.uv).rgb * in.color.rgb;
  return vec4<f32>(mix(c, g.fog.rgb, in.color.a), 1.0);
}
