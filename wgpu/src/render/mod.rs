//! The renderer: the PS Vita's passes (`vita/src`) in wgpu, with its
//! programs in WGSL (`src/shaders`).
//!
//! A frame is the night sky, the baked field, the soft shadows, the army as
//! instanced blends of stored frames, the mage and the demon skinned on the
//! GPU, the effects, then the quarter-size chain (bright, blur, shafts) and
//! one pass that composes, grades and lays the interface over the picture.
//! The scene goes into a target of its own with several samples a pixel; the
//! last pass writes the screen.

mod crowd;
mod figures;
mod fx;
mod post;
mod world;

use pocket_web_wgpu::gpu::{Frame, Gpu, DEPTH};
use pocket_web_wgpu::wgpu::{self, util::DeviceExt};
use requiem_pack::{self as pack, Pack};
use requiem_sim::math::*;
use requiem_sim::Sim;
use serde_json::Value;

use crate::hud::Hud;
use crate::mat::{self, Mat4};
pub use post::Look;

/// The format of the scene's own target and of the quarter-size chain.
pub const SCENE: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const COMMON: &str = include_str!("../shaders/common.wgsl");

/// Scene constants from the pack's META.
pub struct Scene {
    pub sun_dir: V3,
    pub sun: [f32; 3],
    pub sky: [f32; 3],
    pub bounce: [f32; 3],
    pub fog: [f32; 3],
    pub fog_density: f32,
    pub horizon: [f32; 3],
    pub zenith: [f32; 3],
    pub glow: [f32; 3],
    pub moon: [f32; 3],
    pub moon_radius: f32,
    pub lod_near: f32,
    pub lod_mid: f32,
    pub clip_near: f32,
    pub clip_far: f32,
    pub cell: f32,
    pub super_cell: f32,
    pub crowd_reach: Vec<f32>,
    /// Triangles of knights a frame may draw.
    pub crowd_budget: usize,
    /// Linear 0..2 to sRGB bytes.
    lut: Vec<u8>,
}

fn arr3(v: &Value) -> [f32; 3] {
    let f = |i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    [f(0), f(1), f(2)]
}

impl Scene {
    pub fn from_meta(meta: &Value) -> Scene {
        let s = &meta["scene"];
        let d = arr3(&s["sunDir"]);
        let num = |v: &Value, d: f32| v.as_f64().map(|x| x as f32).unwrap_or(d);
        let lut = (0..1024).map(|i| ((i as f32 / 511.5).powf(1.0 / 2.2).min(1.0) * 255.0 + 0.5) as u8).collect();
        Scene {
            sun_dir: v3(d[0], d[1], d[2]).norm(),
            sun: arr3(&s["sun"]),
            sky: arr3(&s["sky"]),
            bounce: arr3(&s["bounce"]),
            fog: arr3(&s["fog"]),
            fog_density: num(&s["fogDensity"], 0.002),
            horizon: arr3(&s["horizon"]),
            zenith: arr3(&s["zenith"]),
            glow: arr3(&s["glow"]),
            moon: arr3(&s["moon"]),
            moon_radius: num(&s["moonRadius"], 0.1),
            lod_near: num(&s["lod"]["near"], 150.0),
            lod_mid: num(&s["lod"]["mid"], 520.0),
            clip_near: num(&s["clip"]["near"], 0.35),
            clip_far: num(&s["clip"]["far"], 6000.0),
            cell: num(&s["cell"], 64.0),
            super_cell: num(&s["superCell"], 256.0),
            crowd_reach: meta["crowd"]["reach"].as_array().map(|a| a.iter().map(|v| num(v, 100.0)).collect()).unwrap_or_default(),
            crowd_budget: meta["crowd"]["budget"].as_u64().unwrap_or(220_000) as usize,
            lut,
        }
    }

    #[inline]
    pub fn encode(&self, lin: f32) -> u8 {
        self.lut[((lin * 511.5) as usize).min(1023)]
    }

    /// sRGB haze colour, as the programs take it.
    pub fn fog_srgb(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| self.fog[i].powf(1.0 / 2.2))
    }

    /// The light table of the lit programs: the moon's direction and how much of it arrives, its colour, sky, bounce.
    pub fn light(&self, vis: f32) -> [f32; 16] {
        [self.sun_dir.x, self.sun_dir.y, self.sun_dir.z, vis, self.sun[0], self.sun[1], self.sun[2], 0.0, self.sky[0], self.sky[1], self.sky[2], 0.0, self.bounce[0], self.bounce[1], self.bounce[2], 0.0]
    }

    /// Sky radiance toward `d` (`web/src/render/sky.ts`).
    pub fn sky_color(&self, d: V3) -> [f32; 3] {
        let h = max(d.y, 0.0);
        let k = 1.0 - (1.0 - h).powf(2.4);
        let s = max(d.dot(self.sun_dir), 0.0);
        let glow = 0.1 * s.powf(5.0) + 0.5 * s.powf(60.0);
        let below = saturate(-d.y * 6.0);
        [0, 1, 2].map(|i| {
            let sky = self.horizon[i] + (self.zenith[i] - self.horizon[i]) * k + self.glow[i] * glow;
            sky + (self.fog[i] - sky) * below
        })
    }
}

/// What every program of the scene reads (`shaders/common.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    vp: Mat4,
    sky_vp: Mat4,
    light: [f32; 16],
    spells: [f32; 32],
    eye: [f32; 4],
    fog: [f32; 4],
    cam: [f32; 12],
    consts: [f32; 4],
}

/// Where the eye is for a frame.
#[derive(Clone, Copy)]
pub struct Eye {
    pub pos: V3,
    pub look: V3,
    /// Vertical field of view in degrees.
    pub fov: f32,
}

/// What a frame draws of the scene, as a development host switches it.
#[derive(Clone, Copy)]
pub struct Parts {
    pub world: bool,
    pub crowd: bool,
    pub mage: bool,
    pub fx: bool,
    pub lod_near: f32,
    pub lod_mid: f32,
    /// Scales the distances at which the army's levels of detail hand over.
    pub crowd_scale: f32,
    /// Triangles of knights a frame may draw.
    pub crowd_budget: usize,
    /// The first level of detail drawn with the far program.
    pub far_from: usize,
}

/// What a frame drew.
#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub world: world::Stats,
    pub crowd: crowd::Stats,
    pub fx: fx::Stats,
    pub figures: u32,
}

impl Stats {
    pub fn tris(&self) -> u32 {
        self.world.tris + self.crowd.tris + self.fx.tris + self.figures
    }
    pub fn draws(&self) -> u32 {
        self.world.draws + self.crowd.draws + self.fx.draws
    }
}

/// The targets a frame is drawn into before the screen: the scene with its samples and its depth, and the
/// quarter-size chain.
struct Targets {
    width: u32,
    height: u32,
    /// The view the scene's pass draws into, and the one its samples are resolved into (none with one sample).
    colour: wgpu::TextureView,
    resolve: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
    a: wgpu::TextureView,
    b: wgpu::TextureView,
    rays: wgpu::TextureView,
}

fn texture(gpu: &Gpu, label: &str, format: wgpu::TextureFormat, width: u32, height: u32, samples: u32, sampled: bool) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | if sampled { wgpu::TextureUsages::TEXTURE_BINDING } else { wgpu::TextureUsages::empty() },
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl Targets {
    fn new(gpu: &Gpu, width: u32, height: u32, samples: u32) -> Targets {
        let scene = texture(gpu, "scene", SCENE, width, height, 1, true);
        let (colour, resolve) = if samples > 1 { (texture(gpu, "scene samples", SCENE, width, height, samples, false), Some(scene)) } else { (scene, None) };
        let (qw, qh) = ((width / 4).max(1), (height / 4).max(1));
        Targets {
            width,
            height,
            colour,
            resolve,
            depth: texture(gpu, "scene depth", DEPTH, width, height, samples, false),
            a: texture(gpu, "bright a", SCENE, qw, qh, 1, true),
            b: texture(gpu, "bright b", SCENE, qw, qh, 1, true),
            rays: texture(gpu, "shafts", SCENE, qw, qh, 1, true),
        }
    }

    /// The scene as a later pass samples it.
    fn scene(&self) -> &wgpu::TextureView {
        self.resolve.as_ref().unwrap_or(&self.colour)
    }
}

/// How a program's fragments meet what is there.
#[derive(Clone, Copy, PartialEq)]
enum Blend {
    Opaque,
    /// Colour × its alpha over the rest.
    Alpha,
    /// Colour already × its alpha, over the rest.
    Premultiplied,
    Additive,
}

/// How a program's fragments meet the depth buffer.
#[derive(Clone, Copy, PartialEq)]
enum Depth {
    /// Tested and written, back faces left out: the solid world.
    Solid,
    /// Tested, not written, both faces.
    Tested,
    /// Not tested, not written, both faces: the sky.
    Over,
    /// The target has no depth buffer.
    None,
}

struct Program<'a> {
    label: &'a str,
    module: &'a wgpu::ShaderModule,
    vertex: &'a str,
    fragment: &'a str,
    groups: &'a [&'a wgpu::BindGroupLayout],
    buffers: &'a [wgpu::VertexBufferLayout<'a>],
    format: wgpu::TextureFormat,
    samples: u32,
    depth: Depth,
    blend: Blend,
}

fn pipeline(gpu: &Gpu, p: &Program) -> wgpu::RenderPipeline {
    use wgpu::{BlendComponent, BlendFactor as F, BlendOperation as Op};
    let layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(p.label), bind_group_layouts: p.groups, push_constant_ranges: &[] });
    let over = |src: F| BlendComponent { src_factor: src, dst_factor: F::OneMinusSrcAlpha, operation: Op::Add };
    let blend = match p.blend {
        Blend::Opaque => None,
        Blend::Alpha => Some(wgpu::BlendState { color: over(F::SrcAlpha), alpha: over(F::One) }),
        Blend::Premultiplied => Some(wgpu::BlendState { color: over(F::One), alpha: over(F::One) }),
        Blend::Additive => Some(wgpu::BlendState { color: BlendComponent { src_factor: F::One, dst_factor: F::One, operation: Op::Add }, alpha: BlendComponent { src_factor: F::One, dst_factor: F::One, operation: Op::Add } }),
    };
    gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(p.label),
        layout: Some(&layout),
        vertex: wgpu::VertexState { module: p.module, entry_point: Some(p.vertex), compilation_options: Default::default(), buffers: p.buffers },
        fragment: Some(wgpu::FragmentState { module: p.module, entry_point: Some(p.fragment), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: p.format, blend, write_mask: wgpu::ColorWrites::ALL })] }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: (p.depth == Depth::Solid).then_some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: (p.depth != Depth::None).then(|| wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: p.depth == Depth::Solid,
            depth_compare: if p.depth == Depth::Over { wgpu::CompareFunction::Always } else { wgpu::CompareFunction::LessEqual },
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: p.samples, mask: !0, alpha_to_coverage_enabled: false },
        multiview: None,
        cache: None,
    })
}

fn module(gpu: &Gpu, label: &str, source: &str) -> wgpu::ShaderModule {
    gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(format!("{COMMON}\n{source}").into()) })
}

/// A buffer holding `bytes`, padded to whole words.
fn buffer(gpu: &Gpu, label: &str, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    let mut padded;
    let contents = if bytes.len() % 4 == 0 && !bytes.is_empty() {
        bytes
    } else {
        padded = bytes.to_vec();
        padded.resize((bytes.len() + 4) & !3, 0);
        &padded[..]
    };
    gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage })
}

/// A buffer of `size` bytes a frame writes into.
fn written(gpu: &Gpu, label: &str, size: usize, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: ((size + 3) & !3) as u64, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages, size: u64, dynamic: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: dynamic, min_binding_size: wgpu::BufferSize::new(size) }, count: None }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None }
}

fn part(buffer: &wgpu::Buffer, size: u64) -> wgpu::BindingResource<'_> {
    wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer, offset: 0, size: wgpu::BufferSize::new(size) })
}

/// A texture of `w × h` texels with the levels given, largest first.
fn picture(gpu: &Gpu, label: &str, format: wgpu::TextureFormat, w: u32, h: u32, levels: &[Vec<u8>]) -> wgpu::TextureView {
    let texel = if format == wgpu::TextureFormat::R8Unorm { 1 } else { 4 };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, texels) in levels.iter().enumerate() {
        let (lw, lh) = ((w >> level).max(1), (h >> level).max(1));
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            texels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw * texel), rows_per_image: None },
            wgpu::Extent3d { width: lw, height: lh, depth_or_array_layers: 1 },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// One level of 8-bit texels halved: each texel the mean of four.
pub(crate) fn halve(from: &[u8], w: usize, h: usize) -> Vec<u8> {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let at = |x: usize, y: usize| from[y.min(h - 1) * w + x.min(w - 1)] as u32;
    (0..nw * nh).map(|i| ((at(i % nw * 2, i / nw * 2) + at(i % nw * 2 + 1, i / nw * 2) + at(i % nw * 2, i / nw * 2 + 1) + at(i % nw * 2 + 1, i / nw * 2 + 1) + 2) / 4) as u8).collect()
}

/// The light the spells cast, as the lit programs take them: place and 1 / radius², then colour.
fn spell_lights(sim: &Sim, eye: V3) -> [f32; 32] {
    use requiem_sim::fx::{Light, LIGHTS};
    let mut lights = [Light::default(); LIGHTS];
    let n = sim.fx.lights(sim.tick, eye, &mut lights);
    let mut out = [0.0f32; 32];
    for (k, l) in lights.iter().enumerate().take(n) {
        let o = k * 8;
        out[o..o + 4].copy_from_slice(&[l.pos.x, l.pos.y, l.pos.z, 1.0 / (l.radius * l.radius).max(0.01)]);
        out[o + 4..o + 8].copy_from_slice(&[l.color[0] * l.power, l.color[1] * l.power, l.color[2] * l.power, 0.0]);
    }
    for k in n..LIGHTS {
        out[k * 8 + 3] = 1.0;
    }
    out
}

pub struct Renderer {
    pub scene: Scene,
    samples: u32,
    targets: Targets,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    world: world::World,
    crowd: crowd::Crowd,
    figures: figures::Figures,
    fx: fx::Fx,
    post: post::Post,
    hud: crate::hud::Painter,
    /// Bytes of the pack held on the GPU.
    pub bytes: usize,
}

impl Renderer {
    /// Everything of the pack on the GPU, and the programs, for a screen of `width × height` pixels of
    /// `format`, the scene drawn with `samples` samples a pixel.
    pub fn new(gpu: &Gpu, p: &Pack, format: wgpu::TextureFormat, width: u32, height: u32, samples: u32) -> Result<Renderer, String> {
        let meta: Value = serde_json::from_slice(p.section(pack::META)?).map_err(|e| e.to_string())?;
        let scene = Scene::from_meta(&meta);
        let globals_layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("globals"), entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT, core::mem::size_of::<Globals>() as u64, false)] });
        let globals = written(gpu, "globals", core::mem::size_of::<Globals>(), wgpu::BufferUsages::UNIFORM);
        let globals_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("globals"), layout: &globals_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }] });
        let targets = Targets::new(gpu, width, height, samples);
        let world = world::World::load(gpu, p, &scene, &globals_layout, samples)?;
        let crowd = crowd::Crowd::load(gpu, p, &scene, &globals_layout, samples)?;
        let figures = figures::Figures::load(gpu, p, &scene, &globals_layout, samples)?;
        let fx = fx::Fx::load(gpu, p, &globals_layout, samples)?;
        let post = post::Post::new(gpu, format, &targets);
        let hud = crate::hud::Painter::load(gpu, p, format)?;
        let bytes = world.bytes + crowd.bytes + figures.bytes + fx.bytes;
        Ok(Renderer { scene, samples, targets, globals, globals_group, world, crowd, figures, fx, post, hud, bytes })
    }

    /// Another screen from the next frame on.
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        self.targets = Targets::new(gpu, width, height, self.samples);
        self.post.retarget(gpu, &self.targets);
    }

    /// The glyphs an interface batch is laid out with.
    pub fn font(&self) -> std::rc::Rc<crate::hud::Font> {
        self.hud.font.clone()
    }

    /// One frame of the game onto `frame`: the scene from `eye`, the chain `look` asks for, `hud` over it.
    /// `fast` is 0 at rest and 1 at full speed, for the smear toward the centre.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, gpu: &Gpu, frame: &Frame, sim: &Sim, eye: &Eye, parts: &Parts, look: &Look, fast: f32, hud: &Hud) -> Stats {
        let mut stats = Stats::default();
        let (w, h) = (self.targets.width, self.targets.height);
        let vp = mat::mul(&mat::perspective(eye.fov, w as f32 / h as f32, self.scene.clip_near, self.scene.clip_far), &mat::view(eye.pos, eye.look, 0.0));
        let planes = mat::planes(&vp);
        let spells = spell_lights(sim, eye.pos);
        let right = eye.look.cross(V3::UP).norm_or(v3(1.0, 0.0, 0.0));
        let up = right.cross(eye.look);
        let fog = self.scene.fog_srgb();
        let globals = Globals {
            vp,
            sky_vp: mat::translated(&vp, eye.pos),
            light: self.scene.light(1.0),
            spells,
            eye: [eye.pos.x, eye.pos.y, eye.pos.z, self.scene.fog_density],
            fog: [fog[0], fog[1], fog[2], 0.0],
            cam: [right.x, right.y, right.z, sim.tick as f32 / 60.0, up.x, up.y, up.z, 0.0, eye.pos.x, eye.pos.y, eye.pos.z, 0.0],
            consts: [self.crowd.scale, pack::UV_SCALE, pack::COLOR_SCALE, 0.0],
        };
        gpu.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        // What the frame draws, chosen and written before any pass is opened.
        if parts.world {
            self.world.choose(&planes, eye.pos, parts.lod_near, parts.lod_mid, &spells);
        }
        let shadows = self.figures.shadows(gpu, sim, eye.pos);
        if parts.crowd {
            self.crowd.budget = parts.crowd_budget;
            self.crowd.choose(gpu, sim, &planes, eye.pos, parts.crowd_scale);
        }
        let demon = parts.mage && self.figures.pose(gpu, sim, &planes, eye.pos);
        if parts.fx {
            self.fx.choose(gpu, sim);
        }
        let lit = self.post.plan(gpu, look, &vp, eye.pos, self.scene.sun_dir, fast, w, h);
        self.hud.write(gpu, hud, w, h);

        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets.colour,
                    resolve_target: self.targets.resolve.as_ref(),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: if self.targets.resolve.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store } },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &self.targets.depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            self.figures.draw_sky(&mut pass);
            if parts.world {
                stats.world = self.world.draw(&mut pass);
            }
            self.figures.draw_shadows(&mut pass, shadows);
            if parts.crowd {
                stats.crowd = self.crowd.draw(&mut pass, parts.far_from);
            }
            if parts.mage {
                stats.figures = self.figures.draw_figures(&mut pass, demon);
            }
            if parts.fx {
                stats.fx = self.fx.draw(&mut pass);
            }
        }
        self.post.chain(&mut encoder, &self.targets, look, lit);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screen"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: frame.shown(), resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.post.compose(&mut pass);
            self.hud.draw(&mut pass);
        }
        gpu.queue.submit([encoder.finish()]);
        stats
    }
}

/// A screen with the interface alone, over the night's ground colour: what is shown while the pack is read.
pub struct Waiting {
    hud: crate::hud::Painter,
}

impl Waiting {
    pub fn new(gpu: &Gpu, font: &[u8], format: wgpu::TextureFormat) -> Result<Waiting, String> {
        Ok(Waiting { hud: crate::hud::Painter::from_font(gpu, font, format)? })
    }

    pub fn font(&self) -> std::rc::Rc<crate::hud::Font> {
        self.hud.font.clone()
    }

    pub fn draw(&mut self, gpu: &Gpu, frame: &Frame, width: u32, height: u32, hud: &Hud) {
        self.hud.write(gpu, hud, width, height);
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("waiting") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("waiting"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: frame.shown(), resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.047, g: 0.063, b: 0.078, a: 1.0 }), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.hud.draw(&mut pass);
        }
        gpu.queue.submit([encoder.finish()]);
    }
}
