// Interface quads in the screen's pixels, origin top left (vita/shaders/hud_v.cg,
// hud_f.cg): glyph coverage, or the atlas's solid block, × vertex colour.

struct Screen {
  size: vec4<f32>,
}
@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var font: texture_2d<f32>;
@group(0) @binding(2) var font_sampler: sampler;

struct HudIn {
  @location(0) position: vec2<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) color: vec4<f32>,
}

struct HudOut {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) color: vec4<f32>,
}

@vertex
fn quad(in: HudIn) -> HudOut {
  var out: HudOut;
  out.position = vec4<f32>(in.position.x / screen.size.x * 2.0 - 1.0, 1.0 - in.position.y / screen.size.y * 2.0, 0.0, 1.0);
  out.uv = in.uv;
  out.color = in.color;
  return out;
}

@fragment
fn ink(in: HudOut) -> @location(0) vec4<f32> {
  let a = textureSample(font, font_sampler, in.uv).r;
  return vec4<f32>(in.color.rgb, in.color.a * a);
}
