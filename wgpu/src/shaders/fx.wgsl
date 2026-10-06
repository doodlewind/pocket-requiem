// Effects (vita/shaders/fx_*.cg). A layer of an effect is a fixed template of
// vertices and eight rows of constants; an instance is a place and an age, a
// direction and one number. Every vertex is placed from the age: nothing is
// simulated. The formulas are those of web/src/render/fx.ts. An effect's
// texel is how bright it is; its colour and strength come from the layer.

struct Layer {
  p: array<vec4<f32>, 8>,
}
@group(1) @binding(0) var<uniform> layer: Layer;
@group(1) @binding(1) var glyphs: texture_2d<f32>;
@group(1) @binding(2) var glyph_sampler: sampler;

struct FxIn {
  @location(0) a: vec4<f32>,
  @location(1) b: vec4<f32>,
  @location(2) c: vec4<f32>,
  @location(3) pos_age: vec4<f32>,
  @location(4) dir_a: vec4<f32>,
}

struct FxOut {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
  @location(1) uv: vec2<f32>,
}

struct Axes {
  first: vec3<f32>,
  second: vec3<f32>,
}

// The effect's frame: right and up across its direction.
fn across(f: vec3<f32>) -> Axes {
  var out: Axes;
  out.first = normalize(cross(f, vec3<f32>(0.0, 1.0, 0.0)) + vec3<f32>(0.0001, 0.0, 0.0));
  out.second = cross(out.first, f);
  return out;
}

// Flat on the ground: forward and right.
fn on_ground(f: vec3<f32>) -> Axes {
  var out: Axes;
  out.first = normalize(vec3<f32>(f.x, 0.0, f.z) + vec3<f32>(0.0, 0.0, -0.0001));
  out.second = vec3<f32>(-out.first.z, 0.0, out.first.x);
  return out;
}

// A power of what is left of a life: never of exactly nothing, which a GPU may answer with no number.
fn fade(left: f32, power: f32) -> f32 {
  return pow(max(left, 0.00001), power);
}

// Hermite between two edges given in either order.
fn ramp(e0: f32, e1: f32, x: f32) -> f32 {
  let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
  return t * t * (3.0 - 2.0 * t);
}

@vertex
fn particles(in: FxIn) -> FxOut {
  var out: FxOut;
  let a = mix(1.0, in.dir_a.w, layer.p[6].x);
  let tsec = (in.pos_age.w - in.b.x * layer.p[1].x) * layer.p[1].w;
  let pl = mix(layer.p[1].y, layer.p[1].z, in.b.y);
  let t = tsec / pl;
  let alive = step(0.0, t) * step(t, 1.0);
  let v0 = mix(layer.p[0].x, layer.p[0].y, in.a.w) * a;
  let travel = layer.p[6].y * a + v0 * tsec * (1.0 - 0.5 * layer.p[0].w * t);
  let ax = across(in.dir_a.xyz);
  let d = ax.first * in.a.x + ax.second * in.a.y + in.dir_a.xyz * in.a.z;
  var world = in.pos_age.xyz + d * travel;
  world.y += (layer.p[6].z - 0.5 * layer.p[0].z * tsec) * tsec;
  let size = mix(layer.p[2].x, layer.p[2].y, t) * (1.0 - layer.p[2].z * in.b.z) * alive * a;
  let vel = d * (v0 * (1.0 - layer.p[0].w * t)) + vec3<f32>(0.0, layer.p[6].z - layer.p[0].z * tsec, 0.0);
  let sp = length(vel);
  let axis = vel / max(sp, 0.001);
  let side = normalize(cross(axis, g.cam[2].xyz - world) + vec3<f32>(0.0, 0.0001, 0.0));
  let along = side * (in.c.x * size) + axis * (in.c.y * size * (1.0 + layer.p[2].w * sp));
  let facing = g.cam[0].xyz * (in.c.x * size) + g.cam[1].xyz * (in.c.y * size);
  world += mix(facing, along, step(0.0005, layer.p[2].w));
  out.position = vec4<f32>(world, 1.0) * g.vp;
  var c = mix(layer.p[3], layer.p[4], t);
  c.a *= min(t / max(layer.p[7].x, 0.001), 1.0) * fade(1.0 - t, layer.p[6].w) * alive;
  out.color = c;
  out.uv = mix(layer.p[5].xy, layer.p[5].zw, in.c.xy * 0.5 + 0.5);
  return out;
}

@vertex
fn ring(in: FxIn) -> FxOut {
  var out: FxOut;
  let a = mix(1.0, abs(in.dir_a.w), layer.p[2].w);
  let sgn = mix(1.0, sign(in.dir_a.w), layer.p[6].z);
  let t = in.pos_age.w;
  let te = fade(t, layer.p[1].w);
  let rad = mix(mix(layer.p[0].x, layer.p[0].y, te), mix(layer.p[0].z, layer.p[0].w, te), in.a.y) * a;
  let th = (in.a.x * layer.p[1].x + layer.p[1].y + layer.p[1].z * t) * sgn;
  let ax = across(in.dir_a.xyz);
  let fl = on_ground(in.dir_a.xyz);
  // ground: forward and right on the ground. facing: across the direction. swing: forward and a tilted right.
  let mode = layer.p[6].x;
  let is_facing = step(0.5, mode) * step(mode, 1.5);
  let is_swing = step(1.5, mode);
  let ax_u = mix(fl.first, ax.first, is_facing);
  let tilted = fl.second * cos(layer.p[6].y) + vec3<f32>(0.0, sin(layer.p[6].y) * sgn, 0.0);
  let ax_v = mix(mix(fl.second, ax.second, is_facing), tilted, is_swing);
  let world = in.pos_age.xyz + vec3<f32>(0.0, layer.p[2].z, 0.0) + ax_u * (cos(th) * rad) + ax_v * (sin(th) * rad);
  out.position = vec4<f32>(world, 1.0) * g.vp;
  var c = mix(layer.p[3], layer.p[4], t);
  let along = in.a.x * 0.5 + 0.5;
  c.a *= fade(1.0 - t, layer.p[2].y) * mix(1.0, sin(along * 3.14159), layer.p[7].w);
  out.color = c;
  out.uv = mix(layer.p[5].xy, layer.p[5].zw, vec2<f32>(along, in.a.y));
  return out;
}

@vertex
fn ribbon(in: FxIn) -> FxOut {
  var out: FxOut;
  let a = mix(1.0, in.dir_a.w, layer.p[1].w);
  let t = in.pos_age.w;
  let len = mix(layer.p[0].x, layer.p[0].y, min(t / max(layer.p[2].x, 0.001), 1.0)) * a;
  // Upright strips stand on the point; the others follow the direction, fanned about the vertical.
  var f = mix(in.dir_a.xyz, vec3<f32>(0.0, 1.0, 0.0), layer.p[6].y);
  let fa = in.b.x * layer.p[1].z;
  let fc = cos(fa);
  let fs = sin(fa);
  f = vec3<f32>(f.x * fc + f.z * fs, f.y, f.z * fc - f.x * fs);
  let ax = across(f);
  let beat = floor(g.cam[0].w * layer.p[1].y + in.b.y * 7.0);
  let o = in.a.zw * cos(beat * 2.4 + in.a.x * 9.0 + in.b.y * 20.0);
  let pinned = sin(in.a.x * 3.14159);
  var world = in.pos_age.xyz + vec3<f32>(0.0, layer.p[6].x, 0.0) + f * (in.a.x * len) + (ax.first * o.x + ax.second * o.y) * (layer.p[1].x * len * pinned);
  let side = normalize(cross(f, world - g.cam[2].xyz) + vec3<f32>(0.0, 0.0001, 0.0));
  let taper = mix(layer.p[2].z, 1.0, ramp(0.0, 0.15, in.a.x)) * mix(layer.p[2].w, 1.0, ramp(1.0, 0.8, in.a.x));
  world += side * (in.a.y * mix(layer.p[0].z, layer.p[0].w, t) * a * taper);
  out.position = vec4<f32>(world, 1.0) * g.vp;
  var c = mix(layer.p[3], layer.p[4], t);
  c.a *= fade(1.0 - t, layer.p[2].y);
  out.color = c;
  out.uv = mix(layer.p[5].xy, layer.p[5].zw, vec2<f32>(in.a.x, in.a.y * 0.5 + 0.5));
  return out;
}

@vertex
fn shell(in: FxIn) -> FxOut {
  var out: FxOut;
  let a = mix(1.0, in.dir_a.w, layer.p[2].w);
  let t = in.pos_age.w;
  let te = fade(t, layer.p[2].x);
  let rad = mix(layer.p[0].x, layer.p[0].y, te) * a;
  let hgt = mix(layer.p[0].z, layer.p[0].w, te) * a;
  let ang = layer.p[1].x + layer.p[1].y * t;
  let cs = cos(ang);
  let sn = sin(ang);
  let q = vec3<f32>(in.a.x * cs - in.a.z * sn, in.a.y, in.a.x * sn + in.a.z * cs);
  let nq = vec3<f32>(in.b.x * cs - in.b.z * sn, in.b.y, in.b.x * sn + in.b.z * cs);
  let ax = across(in.dir_a.xyz);
  let fl = on_ground(in.dir_a.xyz);
  // ground: the shell's axis is up and its -z is ahead. facing: its axis is the direction.
  let facing = step(0.5, layer.p[6].x);
  let x = mix(fl.second, ax.first, facing);
  let y = mix(vec3<f32>(0.0, 1.0, 0.0), in.dir_a.xyz, facing);
  let z = mix(-fl.first, ax.second, facing);
  let world = in.pos_age.xyz + x * (q.x * rad) + y * (q.y * hgt + layer.p[2].z) + z * (q.z * rad) + in.dir_a.xyz * layer.p[6].y;
  let nw = x * nq.x + y * nq.y + z * nq.z;
  out.position = vec4<f32>(world, 1.0) * g.vp;
  let edge = fade(1.0 - abs(dot(normalize(g.cam[2].xyz - world), nw)), layer.p[1].z);
  var c = mix(layer.p[3], layer.p[4], t);
  c.a *= fade(1.0 - t, layer.p[2].y) * (layer.p[6].z + layer.p[1].w * edge);
  out.color = c;
  out.uv = mix(layer.p[5].xy, layer.p[5].zw, in.c.xy);
  return out;
}

@fragment
fn glow(in: FxOut) -> @location(0) vec4<f32> {
  let k = textureSample(glyphs, glyph_sampler, in.uv).r * in.color.a;
  return vec4<f32>(in.color.rgb * k, k);
}
