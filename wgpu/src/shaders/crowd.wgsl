// The army (vita/shaders/crowd_v.cg). A knight has no bones here: the
// compiler placed every vertex at every stored frame of every clip. A draw
// binds two frames as two buffers and one record per knight; the program
// blends the frames by that knight's blend, turns the result to its heading
// and sets it at its place. Every knight showing the same two frames is in
// the same draw. `near` is the first levels of detail; `far` the far ranks:
// the moon, the sky, and the one strongest light a spell casts.

struct CrowdIn {
  @location(0) pos_a: vec4<i32>,
  @location(1) nrm_a: vec4<f32>,
  @location(2) pos_b: vec4<i32>,
  @location(3) nrm_b: vec4<f32>,
  @location(4) color: vec4<f32>,
  @location(5) place: vec3<f32>,
  @location(6) turn: vec2<i32>,
  @location(7) misc: vec4<f32>,
}

struct CrowdOut {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
  @location(1) fog: f32,
}

struct Placed {
  world: vec3<f32>,
  normal: vec3<f32>,
}

fn placed(in: CrowdIn) -> Placed {
  let t = in.misc.x;
  let a = vec3<f32>(in.pos_a.xyz) / 32767.0;
  let b = vec3<f32>(in.pos_b.xyz) / 32767.0;
  let p = mix(a, b, t) * (g.consts.x * (1.0 + in.misc.z));
  let n = normalize(mix(in.nrm_a.xyz, in.nrm_b.xyz, t));
  let s = f32(in.turn.x) / 32767.0;
  let c = f32(in.turn.y) / 32767.0;
  var out: Placed;
  out.world = vec3<f32>(p.x * c + p.z * s, p.y, p.z * c - p.x * s) + in.place;
  out.normal = vec3<f32>(n.x * c + n.z * s, n.y, n.z * c - n.x * s);
  return out;
}

@vertex
fn near(in: CrowdIn) -> CrowdOut {
  var out: CrowdOut;
  let at = placed(in);
  let world = at.world;
  let wn = at.normal;
  let clip = vec4<f32>(world, 1.0) * g.vp;
  out.position = clip;
  let facing = dot(wn, g.light[0].xyz);
  var light = g.light[1].xyz * max(facing, 0.0) + mix(g.light[3].xyz, g.light[2].xyz, 0.5 + 0.5 * wn.y);
  let to_eye = normalize(g.eye.xyz - world);
  for (var k = 0; k < 4; k++) {
    let d = g.spells[k * 2].xyz - world;
    let att = max(1.0 - dot(d, d) * g.spells[k * 2].w, 0.0);
    light += g.spells[k * 2 + 1].xyz * (att * att * (0.3 + 0.7 * max(dot(wn, normalize(d)), 0.0)));
  }
  // Plate: a highlight off the moon, and a rim against the sky.
  let h = normalize(g.light[0].xyz + to_eye);
  let spec = pow(max(dot(wn, h), 0.0), 36.0) * in.color.a;
  let rim = pow(1.0 - max(dot(wn, to_eye), 0.0), 3.0) * (0.675 + 0.325 * facing);
  let shine = g.light[1].xyz * (spec * 0.9 + rim * 0.22 * in.color.a) + g.light[2].xyz * (rim * 0.5);
  out.color = vec4<f32>(in.color.rgb * encode(light) + encode(shine) * 0.8 + vec3<f32>(0.55, 0.62, 0.7) * in.misc.y, 1.0);
  out.fog = haze(clip.w);
  return out;
}

@vertex
fn far(in: CrowdIn) -> CrowdOut {
  var out: CrowdOut;
  let at = placed(in);
  let world = at.world;
  let wn = at.normal;
  let clip = vec4<f32>(world, 1.0) * g.vp;
  out.position = clip;
  let facing = dot(wn, g.light[0].xyz);
  var light = g.light[1].xyz * max(facing, 0.0) + mix(g.light[3].xyz, g.light[2].xyz, 0.5 + 0.5 * wn.y);
  let d0 = g.spells[0].xyz - world;
  let att0 = max(1.0 - dot(d0, d0) * g.spells[0].w, 0.0);
  light += g.spells[1].xyz * (att0 * att0);
  out.color = vec4<f32>(in.color.rgb * sqrt(light) + vec3<f32>(0.55, 0.62, 0.7) * in.misc.y, 1.0);
  out.fog = haze(clip.w);
  return out;
}

@fragment
fn tint(in: CrowdOut) -> @location(0) vec4<f32> {
  return vec4<f32>(mix(in.color.rgb, g.fog.rgb, in.fog), in.color.a);
}
