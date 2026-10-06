// Vertex-coloured geometry lit on the CPU (vita/shaders/color_v.cg): the sky's
// dome, the stars and the moon, centred on the eye and without haze, and the
// soft shadows on the ground. `tint` is the fragment program of every figure:
// the vertex's colour, then haze.

struct ColorIn {
  @location(0) position: vec3<f32>,
  @location(1) color: vec4<f32>,
}

struct ColorOut {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
  @location(1) fog: f32,
}

@vertex
fn sky(in: ColorIn) -> ColorOut {
  var out: ColorOut;
  out.position = vec4<f32>(in.position, 1.0) * g.sky_vp;
  out.color = in.color;
  out.fog = 0.0;
  return out;
}

@vertex
fn ground(in: ColorIn) -> ColorOut {
  var out: ColorOut;
  let clip = vec4<f32>(in.position, 1.0) * g.vp;
  out.position = clip;
  out.color = in.color;
  out.fog = haze(clip.w);
  return out;
}

@fragment
fn tint(in: ColorOut) -> @location(0) vec4<f32> {
  return vec4<f32>(mix(in.color.rgb, g.fog.rgb, in.fog), in.color.a);
}
