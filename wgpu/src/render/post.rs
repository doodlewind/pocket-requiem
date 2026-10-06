//! Post-processing (`vita/src/post.rs`). The scene renders into a target of
//! its own; a quarter-size chain extracts what is bright, blurs it and smears
//! it toward the moon; one full-screen pass composes and grades the frame
//! onto the screen. Every tap's texture coordinate comes from the vertex
//! stage, so no fragment program computes one.

use pocket_web_wgpu::gpu::Gpu;
use pocket_web_wgpu::wgpu;
use requiem_sim::math::V3;

use super::{sampler_entry, texture_entry, uniform_entry, written, Targets, SCENE};
use crate::mat::Mat4;

#[derive(Clone, Copy)]
pub struct Look {
    pub bloom: bool,
    pub rays: bool,
    pub speed: bool,
    pub threshold: f32,
    pub bloom_gain: f32,
    pub rays_gain: f32,
    pub vignette: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub warm: f32,
    pub cool: f32,
}

impl Look {
    /// The night: what a spell lights blooms, the moon throws shafts, the shadows lean blue.
    pub const DEFAULT: Look = Look { bloom: true, rays: true, speed: true, threshold: 0.66, bloom_gain: 0.95, rays_gain: 0.55, vignette: 0.2, contrast: 1.06, saturation: 1.12, warm: 0.02, cool: 0.1 };
    /// The scene as it was drawn: a handheld's frame, which has no chain.
    pub const PLAIN: Look = Look { bloom: false, rays: false, speed: false, threshold: 1.0, bloom_gain: 0.0, rays_gain: 0.0, vignette: 0.0, contrast: 1.0, saturation: 1.0, warm: 0.0, cool: 0.0 };
}

/// One pass's uniforms (`shaders/post.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Pass {
    taps: [f32; 32],
    a: [f32; 4],
    b: [f32; 4],
}

/// Bytes from one pass's uniforms to the next in their buffer.
const STEP: usize = 256;
const BRIGHT: u32 = 0;
const ACROSS: u32 = 1;
const DOWN: u32 = 2;
const SHAFTS: u32 = 3;
const COMPOSE: u32 = 4;

/// Eight identity taps.
fn taps() -> [f32; 32] {
    let mut t = [0.0; 32];
    for k in 0..8 {
        t[k * 4] = 1.0;
        t[k * 4 + 1] = 1.0;
    }
    t
}

pub struct Post {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    bright: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    shafts: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    /// The textures each pass reads: the scene, the first quarter target, the second, and all three for the last.
    from_scene: wgpu::BindGroup,
    from_a: wgpu::BindGroup,
    from_b: wgpu::BindGroup,
    all: wgpu::BindGroup,
}

impl Post {
    pub fn new(gpu: &Gpu, screen: wgpu::TextureFormat, targets: &Targets) -> Post {
        let device = &gpu.device;
        let size = core::mem::size_of::<Pass>() as u64;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("post"), entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT, size, true), texture_entry(1), texture_entry(2), texture_entry(3), sampler_entry(4)] });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("post"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let uniforms = written(gpu, "post", STEP * 5, wgpu::BufferUsages::UNIFORM);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("post"), source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/post.wgsl").into()) });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("post"), bind_group_layouts: &[&layout], push_constant_ranges: &[] });
        let program = |fragment: &str, format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("whole"), compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(fragment), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let (bright, blur, shafts, composite) = (program("bright", SCENE), program("blur", SCENE), program("rays", SCENE), program("compose", screen));
        let [from_scene, from_a, from_b, all] = Self::groups(gpu, &layout, &sampler, &uniforms, targets);
        Post { layout, sampler, uniforms, bright, blur, shafts, composite, from_scene, from_a, from_b, all }
    }

    fn groups(gpu: &Gpu, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, uniforms: &wgpu::Buffer, t: &Targets) -> [wgpu::BindGroup; 4] {
        let group = |first: &wgpu::TextureView, second: &wgpu::TextureView, third: &wgpu::TextureView| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: super::part(uniforms, core::mem::size_of::<Pass>() as u64) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(first) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(second) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(third) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(sampler) },
                ],
            })
        };
        // (a pass that reads one texture is given it three times: its program samples the first)
        [group(t.scene(), t.scene(), t.scene()), group(&t.a, &t.a, &t.a), group(&t.b, &t.b, &t.b), group(t.scene(), &t.a, &t.rays)]
    }

    /// The targets were made again, at another size.
    pub fn retarget(&mut self, gpu: &Gpu, targets: &Targets) {
        [self.from_scene, self.from_a, self.from_b, self.all] = Self::groups(gpu, &self.layout, &self.sampler, &self.uniforms, targets);
    }

    /// Writes every pass's uniforms for a frame of `w × h` pixels. Returns whether the frame has shafts: the
    /// moon is in front of the eye and near the frame. `fast` is 0 at rest and 1 at full speed.
    #[allow(clippy::too_many_arguments)]
    pub fn plan(&mut self, gpu: &Gpu, look: &Look, vp: &Mat4, eye: V3, sun: V3, fast: f32, w: u32, h: u32) -> bool {
        let (qw, qh) = ((w / 4).max(1) as f32, (h / 4).max(1) as f32);
        let mut passes = [Pass { taps: taps(), a: [0.0; 4], b: [0.0; 4] }; 5];
        // Bright: four taps one source texel off the centre, each a bilinear 2 x 2.
        let (tx, ty) = (1.0 / w as f32, 1.0 / h as f32);
        for (k, (sx, sy)) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].into_iter().enumerate() {
            passes[BRIGHT as usize].taps[k * 4 + 2] = sx * tx;
            passes[BRIGHT as usize].taps[k * 4 + 3] = sy * ty;
        }
        passes[BRIGHT as usize].a = [look.threshold, 1.0 / (1.0 - look.threshold).max(0.05), 0.0, 0.0];
        // Blur: across, then down.
        for (pass, (dx, dy)) in [(ACROSS, (1.0 / qw, 0.0)), (DOWN, (0.0, 1.0 / qh))] {
            for (k, o) in [0.0f32, 1.3846, -1.3846, 3.2308, -3.2308].into_iter().enumerate() {
                passes[pass as usize].taps[k * 4 + 2] = o * dx;
                passes[pass as usize].taps[k * 4 + 3] = o * dy;
            }
        }
        // Shafts, when the moon is in front of the eye and near the frame.
        let mut lit = false;
        if look.bloom && look.rays {
            let p = eye + sun * 1000.0;
            let pw = vp[12] * p.x + vp[13] * p.y + vp[14] * p.z + vp[15];
            if pw > 1.0 {
                let u = (vp[0] * p.x + vp[1] * p.y + vp[2] * p.z + vp[3]) / pw * 0.5 + 0.5;
                let v = 0.5 - (vp[4] * p.x + vp[5] * p.y + vp[6] * p.z + vp[7]) / pw * 0.5;
                if (-0.4..1.4).contains(&u) && (-0.4..1.4).contains(&v) {
                    // Each tap is the pixel moved a step toward the moon: uv' = moon + (uv - moon) * s.
                    let t = &mut passes[SHAFTS as usize].taps;
                    for k in 0..8 {
                        let s = 1.0 - k as f32 * 0.062;
                        t[k * 4] = s;
                        t[k * 4 + 1] = s;
                        t[k * 4 + 2] = u * (1.0 - s);
                        t[k * 4 + 3] = v * (1.0 - s);
                    }
                    lit = true;
                }
            }
        }
        // The last pass. Tap 1: the screen as -1..1. Taps 2..4: the scene pulled toward the centre.
        let fast = if look.bloom && look.speed && fast > 0.02 { fast } else { 0.0 };
        let c = &mut passes[COMPOSE as usize];
        c.taps[4..8].copy_from_slice(&[2.0, 2.0, -1.0, -1.0]);
        for (k, s) in [(2usize, 0.985f32), (3, 0.97), (4, 0.955)] {
            let s = 1.0 - (1.0 - s) * fast;
            c.taps[k * 4..k * 4 + 4].copy_from_slice(&[s, s, 0.5 * (1.0 - s), 0.5 * (1.0 - s)]);
        }
        c.a = [if look.bloom { look.bloom_gain } else { 0.0 }, if lit { look.rays_gain } else { 0.0 }, look.vignette, fast * 1.6];
        c.b = [look.contrast, look.saturation, look.warm, look.cool];
        let mut bytes = vec![0u8; STEP * 5];
        for (k, pass) in passes.iter().enumerate() {
            bytes[k * STEP..k * STEP + core::mem::size_of::<Pass>()].copy_from_slice(bytemuck::bytes_of(pass));
        }
        gpu.queue.write_buffer(&self.uniforms, 0, &bytes);
        lit
    }

    /// The quarter-size chain, after the scene's pass: bright, blur across, blur down, shafts.
    pub fn chain(&self, encoder: &mut wgpu::CommandEncoder, targets: &Targets, look: &Look, lit: bool) {
        if !look.bloom {
            return;
        }
        let mut run = |target: &wgpu::TextureView, program: &wgpu::RenderPipeline, group: &wgpu::BindGroup, index: u32| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: target, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(program);
            pass.set_bind_group(0, group, &[index * STEP as u32]);
            pass.draw(0..3, 0..1);
        };
        run(&targets.a, &self.bright, &self.from_scene, BRIGHT);
        run(&targets.b, &self.blur, &self.from_a, ACROSS);
        run(&targets.a, &self.blur, &self.from_b, DOWN);
        if lit {
            run(&targets.rays, &self.shafts, &self.from_a, SHAFTS);
        }
    }

    /// Composes the frame onto the open screen pass.
    pub fn compose(&self, pass: &mut wgpu::RenderPass) {
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &self.all, &[COMPOSE * STEP as u32]);
        pass.draw(0..3, 0..1);
    }
}
